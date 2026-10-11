use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::config::DatabaseType;
use crate::error::{Error, Result};
use crate::internal::sql_safety::quote_ident;
use crate::internal::{
    Backend, ConnectionTrait, OrmConnection, QueryResult, TryGetable, Value,
    build_statement_with_values, translate_error,
};
use crate::model::IndexDefinition;

use super::{ColumnSchema, SchemaGenerator, TableSchema, TableSchemaBuilder};

mod mysql_objects;
mod postgres_dependencies;
mod postgres_privileges;
use postgres_dependencies::postgres_dependencies;

/// Schema writer for auto-generating schema files
pub struct SchemaWriter;

impl SchemaWriter {
    /// Write the connected database's schema to `path` as SQL.
    ///
    /// MySQL and MariaDB retain server-authored table, view, routine, trigger
    /// and event definitions. Replay their script with the `mysql` or `mariadb`
    /// client on one connection: it uses `DELIMITER` for stored program bodies
    /// and temporarily restores their SQL modes, collations and event time zones.
    /// Restoring requires ALTER permission on the destination database: its
    /// default collation is restored from the source, while each stored program
    /// is created under its own historical database collation.
    /// On PostgreSQL every base table is read back from
    /// the catalog - its columns, primary key and secondary indexes - and
    /// rendered by [`SchemaGenerator`]. What a table's columns and indexes
    /// cannot describe comes after the tables as the catalog reports it: a
    /// full-text, expression, partial or prefix index, or one with `INCLUDE`
    /// columns. PostgreSQL CHECKs without routine dependencies are declared
    /// before inheriting tables are created. Routine-backed checks, foreign
    /// keys and exclusion constraints follow all tables.
    /// PostgreSQL tables, views, functions and referenced types follow their
    /// catalog dependencies. Object ownership, grantor chains, column grants and
    /// default privileges are preserved. Domain checks, column defaults, rules
    /// and triggers follow the tables; their enabled states are retained.
    /// Partition column overrides, replica identities and table storage options
    /// also survive replay.
    /// Schemas, extensions and collations precede these objects. Unsupported types, custom aggregates, operators and text-search objects fail
    /// the export instead of producing a file that cannot recreate them.
    ///
    /// SQLite keeps the statement that created each table, view, index and
    /// trigger, so its file is those statements, generated columns,
    /// collations and `AUTOINCREMENT` included. Its FTS5 shadow tables are
    /// left out; the virtual table recreates them.
    ///
    /// This exports schema, not row data, current sequence values, database
    /// accounts or replication subscriptions. Referenced roles/definers must
    /// exist, and the restoring connection needs permission to restore their
    /// ownership and grants. Keep the schema stable while exporting. MySQL
    /// references to the selected source database are rebound to the destination;
    /// explicit external-database references and dynamic SQL strings retain
    /// their original text.
    pub async fn write_schema<P: AsRef<Path>>(path: P) -> Result<()> {
        let db = crate::require_db()?;
        let db_type = db.backend();
        let conn = db.__internal_connection()?;

        let Catalog {
            prelude,
            tables,
            definitions,
            verbatim,
            constraints,
        } = match db_type {
            DatabaseType::Postgres => introspect_postgres(&conn).await?,
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                introspect_mysql(&conn, db_type == DatabaseType::MariaDB).await?
            }
            DatabaseType::SQLite => introspect_sqlite(&conn).await?,
        };

        let mut generator = SchemaGenerator::new(db_type);
        for table in tables {
            generator.add_table(table);
        }

        let mut text = String::new();
        for statement in prelude {
            text.push_str(statement.trim_end().trim_end_matches(';'));
            text.push_str(";\n");
        }
        text.push_str(&generator.generate());
        for statement in definitions.into_iter().chain(verbatim).chain(constraints) {
            if statement.starts_with("--") {
                text.push_str(&statement);
                text.push('\n');
            } else {
                text.push_str(statement.trim_end().trim_end_matches(';'));
                text.push_str(";\n");
            }
        }

        fs::write(path.as_ref(), text)
            .map_err(|e| Error::internal(format!("Failed to write schema file: {}", e)))?;

        Ok(())
    }
}

/// What a catalog read found: the tables the generator renders, the
/// statements written as the catalog reports them, or a comment naming what
/// could not be exported, and the `ALTER TABLE .. ADD CONSTRAINT` statements
/// that follow them.
#[derive(Default)]
struct Catalog {
    prelude: Vec<String>,
    tables: Vec<TableSchema>,
    definitions: Vec<String>,
    verbatim: Vec<String>,
    constraints: Vec<String>,
}

/// One column as the catalog reports it.
pub(super) struct CatalogColumn {
    pub(super) name: String,
    pub(super) sql_type: String,
    pub(super) nullable: bool,
    pub(super) default: Option<String>,
    /// Whether the column draws its values from a sequence or counter.
    pub(super) auto_increment: bool,
}

/// Assemble a table from what the catalog reported.
///
/// `primary_key` is in key order, which can differ from column order, so it
/// is set on the table as given rather than collected from the columns. Only
/// key columns keep their auto-increment flag unless `counters_anywhere`:
/// MySQL requires the counter to be a key, where PostgreSQL's serial type
/// numbers any column.
pub(super) fn catalog_table(
    name: &str,
    schema_name: Option<&str>,
    columns: Vec<CatalogColumn>,
    primary_key: Vec<String>,
    indexes: Vec<IndexDefinition>,
    counters_anywhere: bool,
) -> TableSchema {
    let mut builder = TableSchemaBuilder::new(name);
    if let Some(schema_name) = schema_name {
        builder = builder.schema(schema_name);
    }

    for column in columns {
        let mut schema = ColumnSchema::new(column.name, column.sql_type);

        let is_key = primary_key.contains(&schema.name);
        if is_key {
            schema = schema.primary_key();
        }
        if column.auto_increment && (is_key || counters_anywhere) {
            schema = schema.auto_increment();
        }

        if !column.nullable {
            schema = schema.not_null();
        }

        if let Some(default) = column.default {
            schema = schema.default(default);
        }

        builder = builder.column(schema);
    }

    let mut table = builder.indexes(indexes).build();
    table.primary_keys = primary_key;
    table
}

/// Decode one catalog value.
///
/// A value that does not decode fails the export: read as empty, it would
/// silently drop a column's type, key or default from the written schema.
fn get<T: TryGetable>(row: &QueryResult, column: &str) -> Result<T> {
    row.try_get("", column).map_err(translate_error)
}

async fn query(
    conn: &impl ConnectionTrait,
    backend: Backend,
    sql: &str,
    params: Vec<Value>,
) -> Result<Vec<QueryResult>> {
    conn.query_all_raw(build_statement_with_values(backend, sql, params))
        .await
        .map_err(translate_error)
}

async fn introspect_postgres(conn: &OrmConnection) -> Result<Catalog> {
    use crate::internal::TransactionTrait;
    let txn = conn.begin().await.map_err(translate_error)?;
    txn.execute_unprepared("SET LOCAL search_path = pg_catalog")
        .await
        .map_err(translate_error)?;
    let catalog = introspect_postgres_catalog(&txn).await?;
    txn.commit().await.map_err(translate_error)?;
    Ok(catalog)
}

async fn introspect_postgres_catalog(conn: &impl ConnectionTrait) -> Result<Catalog> {
    let pg = Backend::Postgres;
    let table_rows = query(
        conn,
        pg,
        "WITH RECURSIVE tables AS (
            SELECT c.oid, n.nspname, c.relname, c.relispartition, c.relowner, c.relpersistence, c.reloptions,
                   pg_catalog.pg_get_partkeydef(c.oid) AS partition_key,
                   pg_catalog.pg_get_expr(c.relpartbound, c.oid) AS partition_bound
            FROM pg_catalog.pg_class c
            JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
            WHERE c.relkind IN ('r', 'p')
              AND n.nspname NOT IN ('information_schema', 'pg_catalog')
              AND n.nspname !~ '^pg_'
              AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_class'::regclass AND d.objid=c.oid AND d.deptype='e')
         ), hierarchy AS (
            SELECT t.oid, 0 AS depth FROM tables t
            WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_inherits i WHERE i.inhrelid = t.oid)
            UNION ALL
            SELECT t.oid, h.depth + 1 FROM tables t
            JOIN pg_catalog.pg_inherits i ON i.inhrelid = t.oid
            JOIN hierarchy h ON h.oid = i.inhparent
         )
         SELECT t.oid::bigint AS table_id, t.nspname::text AS table_schema, t.relname::text AS table_name,
                t.partition_key, t.partition_bound, t.relpersistence::text AS persistence,
                CASE WHEN t.reloptions IS NULL THEN NULL ELSE array_to_string(t.reloptions, ', ') END AS storage_options,
                format('ALTER TABLE %I.%I OWNER TO %I', t.nspname, t.relname, pg_catalog.pg_get_userbyid(t.relowner)) AS owner,
                CASE WHEN t.relispartition THEN parents.names END AS partition_parent,
                CASE WHEN NOT t.relispartition THEN parents.names END AS inheritance_parents
         FROM tables t JOIN (SELECT oid, max(depth) AS depth FROM hierarchy GROUP BY oid) h USING(oid)
         LEFT JOIN LATERAL (
             SELECT string_agg(format('%I.%I', pn.nspname, p.relname), ', ' ORDER BY i.inhseqno) AS names
             FROM pg_catalog.pg_inherits i
             JOIN pg_catalog.pg_class p ON p.oid = i.inhparent
             JOIN pg_catalog.pg_namespace pn ON pn.oid = p.relnamespace
             WHERE i.inhrelid = t.oid
         ) parents ON true
         ORDER BY h.depth, t.nspname, t.relname",
        Vec::new(),
    )
    .await?;

    let mut objects = postgres_dependencies(conn).await?;
    let mut catalog = Catalog::default();

    let mut foreign_keys = Vec::new();
    let mut routine_checks = Vec::new();
    for row in table_rows {
        let table_id: i64 = get(&row, "table_id")?;
        let partition_key: Option<String> = get(&row, "partition_key")?;
        let partition_bound: Option<String> = get(&row, "partition_bound")?;
        let partition_parent: Option<String> = get(&row, "partition_parent")?;
        let inheritance_parents: Option<String> = get(&row, "inheritance_parents")?;
        let table_schema: String = get(&row, "table_schema")?;
        let table_name: String = get(&row, "table_name")?;
        let params = || vec![table_schema.clone().into(), table_name.clone().into()];

        // `information_schema.columns.data_type` is a *category*, not a
        // declared type: it reports `text[]` as "ARRAY", an enum as
        // "USER-DEFINED", `varchar(50)` as an unbounded "character varying"
        // and `numeric(10,2)` as a bare "numeric". Replaying that produces
        // invalid SQL, so read the exact declared type from pg_catalog
        // instead - `format_type` renders element type, length and precision
        // the way the column was declared. It already returns the canonical
        // spelling and quotes what needs quoting, so it is used verbatim:
        // upcasing it would turn a quoted mixed-case enum type such as
        // `"MyStatus"` into an identifier that does not exist.
        let mut partition_columns = Vec::new();
        let columns = query(
            conn,
            pg,
            "SELECT a.attname AS column_name,
                    pg_catalog.format_type(a.atttypid, a.atttypmod) AS data_type,
                    NOT a.attnotnull OR EXISTS (
                        SELECT 1 FROM pg_catalog.pg_constraint nn
                        WHERE nn.conrelid = a.attrelid AND nn.contype = 'n'
                          AND a.attnum = ANY(nn.conkey)
                    ) AS is_nullable,
                    (SELECT string_agg(format('CONSTRAINT %I NOT NULL%s', nn.conname,
                              CASE WHEN nn.connoinherit THEN ' NO INHERIT' ELSE '' END), ' ' ORDER BY nn.conname)
                     FROM pg_catalog.pg_constraint nn
                     WHERE nn.conrelid = a.attrelid AND nn.contype = 'n'
                       AND nn.conislocal AND a.attnum = ANY(nn.conkey)) AS not_null_definition,
                    pg_catalog.pg_get_expr(d.adbin, d.adrelid) AS column_default,
                    a.attidentity::text AS identity,
                    a.attgenerated::text AS generated,
                    CASE WHEN a.attcollation <> 0 THEN
                        format('%I.%I', cn.nspname, co.collname) END AS collation,
                    CASE WHEN a.attidentity <> '' THEN
                        format('GENERATED %s AS IDENTITY (SEQUENCE NAME %s START WITH %s INCREMENT BY %s MINVALUE %s MAXVALUE %s CACHE %s %s)',
                            CASE a.attidentity WHEN 'a' THEN 'ALWAYS' ELSE 'BY DEFAULT' END,
                            pg_catalog.pg_get_serial_sequence(format('%I.%I', ns.nspname, c.relname), a.attname),
                            seq.seqstart, seq.seqincrement, seq.seqmin, seq.seqmax, seq.seqcache,
                            CASE WHEN seq.seqcycle THEN 'CYCLE' ELSE 'NO CYCLE' END)
                    END AS identity_definition
             FROM pg_catalog.pg_attribute a
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid
             JOIN pg_catalog.pg_namespace ns ON ns.oid = c.relnamespace
             LEFT JOIN pg_catalog.pg_collation co ON co.oid = a.attcollation
             LEFT JOIN pg_catalog.pg_namespace cn ON cn.oid = co.collnamespace
             LEFT JOIN pg_catalog.pg_attrdef d
                 ON d.adrelid = a.attrelid AND d.adnum = a.attnum
             LEFT JOIN pg_catalog.pg_sequence seq ON a.attidentity <> '' AND seq.seqrelid =
                 pg_catalog.pg_get_serial_sequence(format('%I.%I', ns.nspname, c.relname), a.attname)::regclass
             WHERE ns.nspname = $1 AND c.relname = $2
             AND a.attnum > 0 AND NOT a.attisdropped AND (a.attislocal OR c.relispartition)
             ORDER BY a.attnum",
            params(),
        )
        .await?
        .iter()
        .map(|row| {
            let default: Option<String> = get(row, "column_default")?;
            let mut sql_type: String = get(row, "data_type")?;
            if let Some(collation) = get::<Option<String>>(row, "collation")? {
                sql_type.push_str(&format!(" COLLATE {collation}"));
            }
            // `pg_attrdef` also holds a generated column's expression, which
            // is no default: one may read the row's other columns.
            let generated: String = get(row, "generated")?;
            let generation = match generated.as_str() {
                "s" => Some("STORED"),
                "v" => Some("VIRTUAL"),
                _ => None,
            };
            if let (Some(kind), Some(expression)) = (generation, &default) {
                sql_type = format!("{sql_type} GENERATED ALWAYS AS ({expression}) {kind}");
                if partition_parent.is_some() {
                    partition_columns.push(format!("{} WITH OPTIONS GENERATED ALWAYS AS ({expression}) {kind}",
                        quote_ident(DatabaseType::Postgres, &get::<String>(row, "column_name")?)));
                }
            }
            if let Some(definition) = get::<Option<String>>(row, "identity_definition")? {
                sql_type.push(' ');
                sql_type.push_str(&definition);
            }
            if let Some(definition) = get::<Option<String>>(row, "not_null_definition")? {
                sql_type.push(' ');
                sql_type.push_str(&definition);
            }
            // Preserve nextval defaults verbatim: several columns may share
            // one sequence, with options unlike SERIAL's defaults.
            Ok(CatalogColumn {
                name: get(row, "column_name")?,
                sql_type,
                nullable: get(row, "is_nullable")?,
                // Restore ordinary defaults after the whole hierarchy exists.
                // Otherwise differing parent defaults can prevent CREATE INHERITS.
                default: None,
                auto_increment: false,
            })
        })
        .collect::<Result<Vec<_>>>()?;

        // Keep primary-key column order. Every secondary index is exported
        // verbatim: even a simple B-tree can carry ordering, collation or an
        // operator class which a column-name list cannot represent.
        for row in query(
            conn,
            pg,
            "SELECT pg_catalog.pg_get_indexdef(ix.indexrelid) AS definition
             FROM pg_catalog.pg_index ix
             JOIN pg_catalog.pg_class t ON t.oid = ix.indrelid
             JOIN pg_catalog.pg_namespace ns ON ns.oid = t.relnamespace
             JOIN pg_catalog.pg_class i ON i.oid = ix.indexrelid
             WHERE ns.nspname = $1 AND t.relname = $2 AND NOT ix.indisprimary
               AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_constraint c
                               WHERE c.conindid = ix.indexrelid AND c.contype IN ('p', 'u', 'x'))
               AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_inherits inh WHERE inh.inhrelid = ix.indexrelid)
             ORDER BY i.relname",
            params(),
        )
        .await?
        {
            let mut definition: String = get(&row, "definition")?;
            if partition_key.is_some() {
                // pg_get_indexdef uses ON ONLY for partitioned indexes. The
                // hierarchy already exists, so recurse to recreate leaf indexes.
                use crate::internal::sql_lexer::{Kind, significant};
                let mut tokens = significant(&definition);
                let mut only = None;
                while let Some(token) = tokens.next() {
                    if token.kind == Kind::Word && token.text.eq_ignore_ascii_case("ON") {
                        if let Some(token) = tokens.next()
                            && token.kind == Kind::Word && token.text.eq_ignore_ascii_case("ONLY")
                        {
                            only = Some(token.start..token.start + token.text.len());
                        }
                        break;
                    }
                }
                drop(tokens);
                if let Some(range) = only {
                    definition.replace_range(range, "");
                }
            }
            catalog.verbatim.push(definition);
        }

        // A partition's constraints come with its parent's, and an inherited
        // CHECK with the table it is inherited from.
        let table = format!(
            "{}.{}",
            quote_ident(DatabaseType::Postgres, &table_schema),
            quote_ident(DatabaseType::Postgres, &table_name)
        );
        let mut local_checks = Vec::new();
        let mut deferred_routine_checks = Vec::new();
        let mut unvalidated_checks = Vec::new();
        for row in query(
            conn,
            pg,
            "SELECT c.conname AS constraint_name, c.contype::text AS constraint_type,
                    pg_catalog.pg_get_constraintdef(c.oid) AS definition, c.convalidated,
                    EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_constraint'::regclass
                            AND d.objid=c.oid AND d.refclassid='pg_catalog.pg_proc'::regclass) AS needs_routine
             FROM pg_catalog.pg_constraint c
             JOIN pg_catalog.pg_class t ON t.oid = c.conrelid
             JOIN pg_catalog.pg_namespace ns ON ns.oid = t.relnamespace
             WHERE ns.nspname = $1 AND t.relname = $2
             AND c.contype IN ('p', 'u', 'c', 'x', 'f')
             AND c.conislocal AND c.conparentid = 0
             ORDER BY c.contype = 'f', c.conname",
            params(),
        )
        .await?
        {
            let name: String = get(&row, "constraint_name")?;
            let definition: String = get(&row, "definition")?;
            let statement = format!(
                "ALTER TABLE {table} ADD CONSTRAINT {} {definition}",
                quote_ident(DatabaseType::Postgres, &name)
            );
            match get::<String>(&row, "constraint_type")?.as_str() {
                "f" => foreign_keys.push(statement),
                "c" if get::<bool>(&row, "needs_routine")? => deferred_routine_checks.push(statement),
                "c" if !get::<bool>(&row, "convalidated")? => unvalidated_checks.push(statement),
                "c" if partition_parent.is_none() => local_checks.push(format!(
                    "CONSTRAINT {} {definition}",
                    quote_ident(DatabaseType::Postgres, &name)
                )),
                _ => catalog.constraints.push(statement),
            }
        }

        let mut definition = if let Some(ref parent) = partition_parent {
            format!(
                "CREATE TABLE {table} PARTITION OF {parent}{} {}",
                if partition_columns.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", partition_columns.join(", "))
                },
                partition_bound.ok_or_else(|| Error::internal("partition has no bound"))?
            )
        } else {
            let table = catalog_table(
                &table_name,
                Some(&table_schema),
                columns,
                Vec::new(),
                Vec::new(),
                true,
            );
            SchemaGenerator::new(DatabaseType::Postgres)
                .generate_create_table(&table)
                .trim_end()
                .trim_end_matches(';')
                .to_string()
        };
        // Parent CHECKs must exist when children are created. Only local
        // declarations go in the child; PostgreSQL supplies inherited ones.
        if !local_checks.is_empty() {
            let end = definition
                .rfind(')')
                .ok_or_else(|| Error::internal("table definition has no closing parenthesis"))?;
            let separator = if definition[..end].trim_end().ends_with('(') {
                ""
            } else {
                ", "
            };
            definition.insert_str(end, &format!("{separator}{}", local_checks.join(", ")));
        }
        if let Some(parents) = inheritance_parents.as_ref() {
            definition.push_str(&format!(" INHERITS ({parents})"));
        }
        if let Some(key) = partition_key {
            definition.push_str(&format!(" PARTITION BY {key}"));
        }
        if get::<String>(&row, "persistence")? == "u" {
            definition = definition.replacen("CREATE TABLE", "CREATE UNLOGGED TABLE", 1);
        }
        if let Some(options) = get::<Option<String>>(&row, "storage_options")? {
            definition.push_str(&format!(" WITH ({options})"));
        }
        catalog.definitions.push(definition);
        catalog.definitions.push(get(&row, "owner")?);
        routine_checks.push(deferred_routine_checks);
        catalog.constraints.extend(unvalidated_checks);
        if inheritance_parents.is_some() || partition_parent.is_some() {
            for row in query(conn, pg,
                "SELECT format('ALTER TABLE ONLY %I.%I ALTER COLUMN %I SET NOT NULL', ns.nspname, c.relname, a.attname) AS definition
                 FROM pg_catalog.pg_class c
                 JOIN pg_catalog.pg_namespace ns ON ns.oid = c.relnamespace
                 JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid
                 WHERE ns.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped
                   AND a.attnotnull
                 ORDER BY a.attnum", params()).await? {
                catalog.definitions.push(get(&row, "definition")?);
            }
        }
        // ONLY avoids propagating defaults into children that deliberately
        // dropped or overrode them. Every effective default, including those
        // on inherited columns and partitions, is read from its own table.
        for row in query(conn, pg,
            "SELECT format('ALTER TABLE ONLY %I.%I ALTER COLUMN %I SET DEFAULT %s', ns.nspname, c.relname, a.attname,
                           pg_catalog.pg_get_expr(d.adbin, d.adrelid)) AS definition
             FROM pg_catalog.pg_class c
             JOIN pg_catalog.pg_namespace ns ON ns.oid=c.relnamespace
             JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid
             JOIN pg_catalog.pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
             WHERE ns.nspname=$1 AND c.relname=$2 AND a.attnum>0 AND NOT a.attisdropped
               AND a.attgenerated='' AND a.attidentity=''
             ORDER BY a.attnum", params()).await? {
            catalog.verbatim.push(get(&row, "definition")?);
        }
        objects.nodes.insert(
            ("table".into(), table_id),
            std::mem::take(&mut catalog.definitions),
        );
    }
    catalog
        .verbatim
        .extend(std::mem::take(&mut objects.indexes));
    catalog
        .constraints
        .extend(std::mem::take(&mut objects.postlude));
    catalog.definitions = objects.ordered(conn).await?;
    // Add local child checks first, then let parent additions recurse and
    // merge them. This preserves local/inherited ownership of matching checks.
    for checks in routine_checks.into_iter().rev() {
        catalog.constraints.extend(checks);
    }
    for row in query(conn, pg,
        "SELECT format('ALTER SEQUENCE %I.%I OWNED BY %I.%I.%I', sn.nspname, s.relname, tn.nspname, t.relname, a.attname) AS definition
         FROM pg_catalog.pg_class s JOIN pg_catalog.pg_namespace sn ON sn.oid = s.relnamespace
         JOIN pg_catalog.pg_depend d ON d.classid = 'pg_catalog.pg_class'::regclass AND d.objid = s.oid
             AND d.refclassid = 'pg_catalog.pg_class'::regclass AND d.deptype = 'a'
         JOIN pg_catalog.pg_class t ON t.oid = d.refobjid
         JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace
         JOIN pg_catalog.pg_attribute a ON a.attrelid = t.oid AND a.attnum = d.refobjsubid
         WHERE s.relkind = 'S' AND sn.nspname <> 'information_schema' AND sn.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend e WHERE e.classid = 'pg_catalog.pg_class'::regclass AND e.objid = s.oid AND e.deptype = 'e')
         ORDER BY sn.nspname, s.relname", Vec::new()).await? {
        catalog.verbatim.push(get(&row, "definition")?);
    }
    // A NOT VALID CHECK is added after the hierarchy exists so descendants
    // keep its inherited ownership. A child created after such a check can
    // already have validated it; preserve that per-table state as well.
    for row in query(conn, pg,
        "SELECT format('ALTER TABLE %I.%I VALIDATE CONSTRAINT %I', n.nspname, t.relname, c.conname) AS definition
         FROM pg_catalog.pg_constraint c
         JOIN pg_catalog.pg_class t ON t.oid = c.conrelid
         JOIN pg_catalog.pg_namespace n ON n.oid = t.relnamespace
         WHERE c.contype = 'c' AND c.convalidated AND c.coninhcount > 0
           AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
         ORDER BY n.nspname, t.relname, c.conname", Vec::new()).await? {
        catalog.constraints.push(get(&row, "definition")?);
    }
    // Referenced UNIQUE/PRIMARY constraints must exist before any foreign key,
    // regardless of how the tables sort by name.
    catalog.constraints.extend(foreign_keys);

    for row in query(conn, pg,
        "SELECT format('CREATE POLICY %I ON %I.%I AS %s FOR %s TO %s%s%s',
             pol.polname, n.nspname, c.relname,
             CASE WHEN pol.polpermissive THEN 'PERMISSIVE' ELSE 'RESTRICTIVE' END,
             CASE pol.polcmd WHEN '*' THEN 'ALL' WHEN 'r' THEN 'SELECT' WHEN 'a' THEN 'INSERT' WHEN 'w' THEN 'UPDATE' WHEN 'd' THEN 'DELETE' END,
             (SELECT string_agg(CASE WHEN role_id = 0 THEN 'PUBLIC' ELSE quote_ident(pg_catalog.pg_get_userbyid(role_id)) END, ', ' ORDER BY ord)
              FROM unnest(pol.polroles) WITH ORDINALITY AS roles(role_id, ord)),
             CASE WHEN pol.polqual IS NULL THEN '' ELSE ' USING (' || pg_catalog.pg_get_expr(pol.polqual, pol.polrelid) || ')' END,
             CASE WHEN pol.polwithcheck IS NULL THEN '' ELSE ' WITH CHECK (' || pg_catalog.pg_get_expr(pol.polwithcheck, pol.polrelid) || ')' END) AS definition
         FROM pg_catalog.pg_policy pol
         JOIN pg_catalog.pg_class c ON c.oid = pol.polrelid
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
         ORDER BY n.nspname, c.relname, pol.polname", Vec::new()).await? {
        catalog.constraints.push(get(&row, "definition")?);
    }
    for row in query(conn, pg,
        "SELECT format('ALTER TABLE %I.%I %s ROW LEVEL SECURITY', n.nspname, c.relname, action) AS definition
         FROM pg_catalog.pg_class c
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
         CROSS JOIN LATERAL (SELECT 'ENABLE' AS action WHERE c.relrowsecurity
                             UNION ALL SELECT 'FORCE' WHERE c.relforcerowsecurity) flags
         WHERE c.relkind IN ('r', 'p') AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
         ORDER BY n.nspname, c.relname, action", Vec::new()).await? {
        catalog.constraints.push(get(&row, "definition")?);
    }

    // Restore user triggers after functions, tables, indexes and constraints.
    // Partition trigger clones are created by their parent's CREATE TRIGGER;
    // their individual enabled states still need to be restored.
    for row in query(conn, pg,
        "SELECT CASE WHEN tg.tgparentid = 0 THEN pg_catalog.pg_get_triggerdef(tg.oid, false) END AS definition,
                format('ALTER TABLE ONLY %I.%I %s TRIGGER %I', ns.nspname, c.relname,
                    CASE tg.tgenabled WHEN 'D' THEN 'DISABLE' WHEN 'R' THEN 'ENABLE REPLICA'
                         WHEN 'A' THEN 'ENABLE ALWAYS' ELSE 'ENABLE' END, tg.tgname) AS state
         FROM pg_catalog.pg_trigger tg
         JOIN pg_catalog.pg_class c ON c.oid = tg.tgrelid
         JOIN pg_catalog.pg_namespace ns ON ns.oid = c.relnamespace
         WHERE NOT tg.tgisinternal AND c.relkind IN ('r', 'p', 'v')
           AND ns.nspname <> 'information_schema' AND ns.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid = 'pg_catalog.pg_trigger'::regclass
                           AND d.objid = tg.oid AND d.deptype = 'e')
         ORDER BY tg.tgparentid <> 0, ns.nspname, c.relname, tg.tgname", Vec::new()).await? {
        if let Some(definition) = get::<Option<String>>(&row, "definition")? {
            catalog.constraints.push(definition);
        }
        catalog.constraints.push(get(&row, "state")?);
    }
    for row in query(conn, pg,
        "SELECT pg_catalog.pg_get_ruledef(r.oid, false) AS definition,
             format('ALTER TABLE %I.%I %s RULE %I', n.nspname, c.relname,
                 CASE r.ev_enabled WHEN 'D' THEN 'DISABLE' WHEN 'R' THEN 'ENABLE REPLICA'
                     WHEN 'A' THEN 'ENABLE ALWAYS' ELSE 'ENABLE' END, r.rulename) AS state
         FROM pg_catalog.pg_rewrite r JOIN pg_catalog.pg_class c ON c.oid=r.ev_class
         JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
         WHERE r.rulename <> '_RETURN' AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_rewrite'::regclass AND d.objid=r.oid AND d.deptype='e')
         ORDER BY n.nspname,c.relname,r.rulename", Vec::new()).await? {
        catalog.constraints.push(get(&row, "definition")?);
        catalog.constraints.push(get(&row, "state")?);
    }
    // Recursive CREATE INDEX chooses new names for descendant indexes. Locate
    // an inherited replica index by its root and owning table after replay,
    // including nested partitions and constraint-backed indexes.
    for row in query(conn, pg,
        "SELECT CASE WHEN root.name IS NOT NULL THEN
             format('DO %L', format('DECLARE replica_index text; BEGIN
                 WITH RECURSIVE family(oid) AS (
                     SELECT %L::regclass::oid
                     UNION ALL
                     SELECT inhrelid FROM pg_catalog.pg_inherits JOIN family ON inhparent=family.oid
                 )
                 SELECT idx.relname INTO STRICT replica_index FROM family
                 JOIN pg_catalog.pg_index pi ON pi.indexrelid=family.oid
                 JOIN pg_catalog.pg_class idx ON idx.oid=pi.indexrelid
                 WHERE pi.indrelid=%L::regclass;
                 EXECUTE %L || pg_catalog.quote_ident(replica_index);
             END', root.name, format('%I.%I', n.nspname,c.relname),
                 format('ALTER TABLE %I.%I REPLICA IDENTITY USING INDEX ', n.nspname,c.relname)))
             ELSE format('ALTER TABLE %I.%I REPLICA IDENTITY %s', n.nspname,c.relname,
                 CASE c.relreplident WHEN 'f' THEN 'FULL' WHEN 'n' THEN 'NOTHING' WHEN 'd' THEN 'DEFAULT'
                     WHEN 'i' THEN 'USING INDEX ' || quote_ident(i.relname) END) END AS definition
         FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
         LEFT JOIN pg_catalog.pg_index ix ON ix.indrelid=c.oid AND ix.indisreplident
         LEFT JOIN pg_catalog.pg_class i ON i.oid=ix.indexrelid
         LEFT JOIN LATERAL (
             WITH RECURSIVE ancestors(oid) AS (
                 SELECT i.oid
                 UNION ALL
                 SELECT inhparent FROM pg_catalog.pg_inherits JOIN ancestors ON inhrelid=ancestors.oid
             )
             SELECT format('%I.%I', ns.nspname, idx.relname) AS name
             FROM ancestors JOIN pg_catalog.pg_class idx ON idx.oid=ancestors.oid
             JOIN pg_catalog.pg_namespace ns ON ns.oid=idx.relnamespace
             WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_inherits WHERE inhrelid=idx.oid)
         ) root ON EXISTS (SELECT 1 FROM pg_catalog.pg_inherits WHERE inhrelid=i.oid)
         WHERE c.relkind IN ('r','p') AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_'
           AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid='pg_catalog.pg_class'::regclass AND d.objid=c.oid AND d.deptype='e')
         ORDER BY n.nspname,c.relname", Vec::new()).await? {
        catalog.constraints.push(get(&row, "definition")?);
    }
    catalog
        .constraints
        .extend(postgres_privileges::postgres_privileges(conn).await?);
    catalog
        .constraints
        .push("RESET check_function_bodies".into());
    Ok(catalog)
}

async fn introspect_mysql(conn: &OrmConnection, _mariadb: bool) -> Result<Catalog> {
    mysql_objects::introspect(conn).await
}

/// Move only foreign keys out of server-authored DDL so cyclic references
/// restore after every table exists, without changing session settings.
pub(super) fn mysql_defer_foreign_keys(ddl: &str, table: &str) -> Result<(String, Vec<String>)> {
    use crate::internal::sql_lexer::{Kind, quoted_len, significant};
    let mut pos = 0;
    let mut depth = 0;
    let mut start = None;
    let mut end = None;
    let mut boundary = 0;
    let mut parts = Vec::new();
    while pos < ddl.len() {
        let rest = &ddl[pos..];
        let ch = rest.chars().next().unwrap();
        if matches!(ch, '\'' | '"' | '`') {
            pos += quoted_len(rest, ch, ch != '`');
            continue;
        }
        match ch {
            '(' => {
                depth += 1;
                if start.is_none() {
                    start = Some(pos);
                    boundary = pos + 1;
                }
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    parts.push(&ddl[boundary..pos]);
                    end = Some(pos);
                    break;
                }
            }
            ',' if depth == 1 => {
                parts.push(&ddl[boundary..pos]);
                boundary = pos + 1;
            }
            _ => {}
        }
        pos += ch.len_utf8();
    }
    let (Some(start), Some(end)) = (start, end) else {
        return Err(Error::internal(
            "SHOW CREATE TABLE returned an invalid table definition",
        ));
    };
    let mut kept = Vec::new();
    let mut foreign_keys = Vec::new();
    for part in parts {
        let mut tokens = significant(part);
        let mut first = tokens.next();
        if first.is_some_and(|t| t.kind == Kind::Word && t.text.eq_ignore_ascii_case("CONSTRAINT"))
        {
            tokens.next(); // constraint name
            first = tokens.next();
        }
        if first.is_some_and(|t| t.kind == Kind::Word && t.text.eq_ignore_ascii_case("FOREIGN")) {
            foreign_keys.push(format!(
                "ALTER TABLE {} ADD {}",
                quote_ident(DatabaseType::MySQL, table),
                part.trim()
            ));
        } else {
            kept.push(part);
        }
    }
    if foreign_keys.is_empty() {
        return Ok((ddl.to_owned(), foreign_keys));
    }
    Ok((
        format!(
            "{}{}){}",
            &ddl[..start + 1],
            kept.join(","),
            &ddl[end + 1..]
        ),
        foreign_keys,
    ))
}

/// SQLite keeps every table, view, index and trigger as the statement that
/// created it, rewritten by each `ALTER TABLE`, so the schema file carries
/// those statements. Rebuilt from `PRAGMA` metadata, a table lost what the
/// pragmas leave out: a generated column, a collation, `AUTOINCREMENT`, a
/// `CHECK` or foreign key constraint, `WITHOUT ROWID`.
async fn introspect_sqlite(conn: &OrmConnection) -> Result<Catalog> {
    // `pragma_table_list` tells a virtual table (an FTS5 index) and the
    // shadow tables it keeps its data in from an ordinary table. The virtual
    // table recreates its shadow tables, so they are left out. Tables come
    // first, then virtual tables and views, which can read them, then the
    // indexes (a key or `UNIQUE` constraint's is part of its table) and the
    // triggers, which keep an FTS5 index in step with its table.
    let statements = query(
        conn,
        Backend::Sqlite,
        "SELECT sql FROM (
             SELECT CASE l.type WHEN 'table' THEN 0 ELSE 1 END AS rank, l.name AS name, m.sql AS sql
             FROM pragma_table_list l
             JOIN sqlite_master m ON m.name = l.name AND m.type = 'table'
             WHERE l.schema = 'main' AND l.type IN ('table', 'virtual')
               AND l.name NOT GLOB 'sqlite_*'
             UNION ALL
             SELECT CASE type WHEN 'view' THEN 2 WHEN 'index' THEN 3 ELSE 4 END, name, sql
             FROM sqlite_master
             WHERE type IN ('view', 'index', 'trigger')
               AND tbl_name NOT GLOB 'sqlite_*'
               AND tbl_name NOT IN (SELECT name FROM pragma_table_list WHERE type = 'shadow')
         )
         WHERE sql IS NOT NULL
         ORDER BY rank, name",
        Vec::new(),
    )
    .await?;

    let mut catalog = Catalog::default();
    for row in statements {
        catalog.verbatim.push(get(&row, "sql")?);
    }
    Ok(catalog)
}
