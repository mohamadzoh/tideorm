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

/// Schema writer for auto-generating schema files
pub struct SchemaWriter;

impl SchemaWriter {
    /// Write the connected database's schema to `path` as SQL.
    ///
    /// On PostgreSQL, MySQL and MariaDB every base table is read back from
    /// the catalog - its columns, primary key and secondary indexes - and
    /// rendered by [`SchemaGenerator`]. What a table's columns and indexes
    /// cannot describe comes after the tables as the catalog reports it: a
    /// full-text, expression, partial or prefix index, or one with `INCLUDE`
    /// columns. The tables' `CHECK` and foreign key constraints (and
    /// PostgreSQL's `EXCLUDE` ones) come last, added with `ALTER TABLE` once
    /// every table they name exists.
    ///
    /// SQLite keeps the statement that created each table, view, index and
    /// trigger, so its file is those statements, generated columns,
    /// collations and `AUTOINCREMENT` included. Its FTS5 shadow tables are
    /// left out; the virtual table recreates them.
    pub async fn write_schema<P: AsRef<Path>>(path: P) -> Result<()> {
        let db = crate::require_db()?;
        let db_type = db.backend();
        let conn = db.__internal_connection()?;

        let Catalog {
            tables,
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

        let mut text = generator.generate();
        for statement in verbatim.into_iter().chain(constraints) {
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
    tables: Vec<TableSchema>,
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

/// One key column of an index, as the catalog reports it.
pub(super) struct CatalogIndexColumn {
    pub(super) index: String,
    pub(super) unique: bool,
    pub(super) column: String,
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

/// Group index key columns into index definitions.
///
/// Each index keeps the column order its rows arrive in, which every catalog
/// query below sorts by key position. The indexes themselves come out sorted
/// by name, so the same database always exports the same file.
pub(super) fn group_indexes(
    columns: impl IntoIterator<Item = CatalogIndexColumn>,
) -> Vec<IndexDefinition> {
    let mut indexes: BTreeMap<String, IndexDefinition> = BTreeMap::new();

    for CatalogIndexColumn {
        index,
        unique,
        column,
    } in columns
    {
        indexes
            .entry(index.clone())
            .or_insert_with(|| IndexDefinition::new(index, Vec::new(), unique))
            .columns
            .push(column);
    }

    indexes.into_values().collect()
}

/// Decode one catalog value.
///
/// A value that does not decode fails the export: read as empty, it would
/// silently drop a column's type, key or default from the written schema.
fn get<T: TryGetable>(row: &QueryResult, column: &str) -> Result<T> {
    row.try_get("", column).map_err(translate_error)
}

async fn query(
    conn: &OrmConnection,
    backend: Backend,
    sql: &str,
    params: Vec<Value>,
) -> Result<Vec<QueryResult>> {
    conn.query_all_raw(build_statement_with_values(backend, sql, params))
        .await
        .map_err(translate_error)
}

async fn introspect_postgres(conn: &OrmConnection) -> Result<Catalog> {
    let pg = Backend::Postgres;
    let table_rows = query(
        conn,
        pg,
        "SELECT table_schema::text AS table_schema, table_name::text AS table_name
         FROM information_schema.tables
         WHERE table_schema NOT IN ('information_schema', 'pg_catalog')
         AND table_schema NOT LIKE 'pg_toast%'
         AND table_schema NOT LIKE 'pg_temp_%'
         AND table_type = 'BASE TABLE'
         ORDER BY table_schema, table_name",
        Vec::new(),
    )
    .await?;

    let mut catalog = Catalog::default();

    for row in table_rows {
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
        let columns = query(
            conn,
            pg,
            "SELECT a.attname AS column_name,
                    pg_catalog.format_type(a.atttypid, a.atttypmod) AS data_type,
                    NOT a.attnotnull AS is_nullable,
                    pg_catalog.pg_get_expr(d.adbin, d.adrelid) AS column_default,
                    a.attidentity::text AS identity,
                    a.attgenerated::text AS generated
             FROM pg_catalog.pg_attribute a
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid
             JOIN pg_catalog.pg_namespace ns ON ns.oid = c.relnamespace
             LEFT JOIN pg_catalog.pg_attrdef d
                 ON d.adrelid = a.attrelid AND d.adnum = a.attnum
             WHERE ns.nspname = $1 AND c.relname = $2
             AND a.attnum > 0 AND NOT a.attisdropped
             ORDER BY a.attnum",
            params(),
        )
        .await?
        .iter()
        .map(|row| {
            let default: Option<String> = get(row, "column_default")?;
            let mut sql_type: String = get(row, "data_type")?;
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
            }
            // A serial column's default is its sequence, which the serial
            // type recreates, as it does an identity column's.
            let identity: String = get(row, "identity")?;
            let auto_increment = !identity.is_empty()
                || default
                    .as_deref()
                    .is_some_and(|default| default.contains("nextval"));

            Ok(CatalogColumn {
                name: get(row, "column_name")?,
                sql_type,
                nullable: get(row, "is_nullable")?,
                default: default.filter(|_| !auto_increment && generation.is_none()),
                auto_increment: auto_increment && generation.is_none(),
            })
        })
        .collect::<Result<Vec<_>>>()?;

        // One row per key column, in key order: `indkey` lists the columns
        // the way the index was declared, which `attnum` does not. An index
        // on an expression, a partial one or one not a B-tree cannot be told
        // by its columns, and is exported as `pg_get_indexdef` renders it; so
        // is one whose `INCLUDE` columns `indkey` lists after its keys, which
        // exported as keys would let rows repeat a unique key, and one whose
        // key `NULLS NOT DISTINCT` refuses a second NULL (read through
        // `to_jsonb`, since the column exists from PostgreSQL 15 on). A
        // primary key's `INCLUDE` columns are left out of it.
        const PLAIN_INDEX: &str = "ix.indexprs IS NULL AND ix.indpred IS NULL \
             AND am.amname = 'btree' AND ix.indnatts = ix.indnkeyatts \
             AND NOT COALESCE((to_jsonb(ix) ->> 'indnullsnotdistinct')::boolean, false)";
        let mut primary_key = Vec::new();
        let mut index_columns = Vec::new();
        for row in query(
            conn,
            pg,
            &format!(
                "SELECT i.relname AS index_name, ix.indisprimary AS is_primary,
                        ix.indisunique AS is_unique, a.attname AS column_name
                 FROM pg_catalog.pg_index ix
                 JOIN pg_catalog.pg_class t ON t.oid = ix.indrelid
                 JOIN pg_catalog.pg_namespace ns ON ns.oid = t.relnamespace
                 JOIN pg_catalog.pg_class i ON i.oid = ix.indexrelid
                 JOIN pg_catalog.pg_am am ON am.oid = i.relam
                 CROSS JOIN LATERAL unnest(ix.indkey) WITH ORDINALITY AS k(attnum, position)
                 JOIN pg_catalog.pg_attribute a ON a.attrelid = t.oid AND a.attnum = k.attnum
                 WHERE ns.nspname = $1 AND t.relname = $2
                 AND (ix.indisprimary OR ({PLAIN_INDEX})) AND k.position <= ix.indnkeyatts
                 ORDER BY i.relname, k.position"
            ),
            params(),
        )
        .await?
        {
            let column: String = get(&row, "column_name")?;
            if get(&row, "is_primary")? {
                primary_key.push(column);
            } else {
                index_columns.push(CatalogIndexColumn {
                    index: get(&row, "index_name")?,
                    unique: get(&row, "is_unique")?,
                    column,
                });
            }
        }

        for row in query(
            conn,
            pg,
            &format!(
                "SELECT pg_catalog.pg_get_indexdef(ix.indexrelid) AS definition
                 FROM pg_catalog.pg_index ix
                 JOIN pg_catalog.pg_class t ON t.oid = ix.indrelid
                 JOIN pg_catalog.pg_namespace ns ON ns.oid = t.relnamespace
                 JOIN pg_catalog.pg_class i ON i.oid = ix.indexrelid
                 JOIN pg_catalog.pg_am am ON am.oid = i.relam
                 WHERE ns.nspname = $1 AND t.relname = $2
                 AND NOT ix.indisprimary AND NOT ({PLAIN_INDEX})
                 ORDER BY i.relname"
            ),
            params(),
        )
        .await?
        {
            catalog.verbatim.push(get(&row, "definition")?);
        }

        // A partition's constraints come with its parent's, and an inherited
        // CHECK with the table it is inherited from.
        let table = format!(
            "{}.{}",
            quote_ident(DatabaseType::Postgres, &table_schema),
            quote_ident(DatabaseType::Postgres, &table_name)
        );
        for row in query(
            conn,
            pg,
            "SELECT c.conname AS constraint_name,
                    pg_catalog.pg_get_constraintdef(c.oid) AS definition
             FROM pg_catalog.pg_constraint c
             JOIN pg_catalog.pg_class t ON t.oid = c.conrelid
             JOIN pg_catalog.pg_namespace ns ON ns.oid = t.relnamespace
             WHERE ns.nspname = $1 AND t.relname = $2
             AND c.contype IN ('c', 'x', 'f') AND c.conislocal AND c.conparentid = 0
             ORDER BY c.contype = 'f', c.conname",
            params(),
        )
        .await?
        {
            let name: String = get(&row, "constraint_name")?;
            let definition: String = get(&row, "definition")?;
            catalog.constraints.push(format!(
                "ALTER TABLE {table} ADD CONSTRAINT {} {definition}",
                quote_ident(DatabaseType::Postgres, &name)
            ));
        }

        catalog.tables.push(catalog_table(
            &table_name,
            Some(&table_schema),
            columns,
            primary_key,
            group_indexes(index_columns),
            true,
        ));
    }

    Ok(catalog)
}

async fn introspect_mysql(conn: &OrmConnection, mariadb: bool) -> Result<Catalog> {
    let mysql = Backend::MySql;
    let database_row = query(conn, mysql, "SELECT DATABASE() AS db_name", Vec::new()).await?;
    let Some(db_name) = database_row
        .first()
        .map(|row| get::<Option<String>>(row, "db_name"))
        .transpose()?
        .flatten()
        .filter(|name| !name.is_empty())
    else {
        return Ok(Catalog::default());
    };

    // Every catalog column is aliased: MySQL 8 labels information_schema
    // columns in upper case unless told otherwise, while MariaDB keeps the
    // case the query spelled.
    let table_rows = query(
        conn,
        mysql,
        "SELECT table_name AS table_name FROM information_schema.tables
         WHERE table_schema = ? AND table_type = 'BASE TABLE'
         ORDER BY table_name",
        vec![db_name.clone().into()],
    )
    .await?;

    let mut catalog = Catalog::default();

    for row in table_rows {
        let table_name: String = get(&row, "table_name")?;
        let params = || vec![db_name.clone().into(), table_name.clone().into()];

        // `column_type` is the declared type, used verbatim: upcasing it would
        // rewrite the values of an ENUM or SET.
        let columns = query(
            conn,
            mysql,
            "SELECT column_name AS column_name, column_type AS column_type,
                    is_nullable AS is_nullable, column_default AS column_default,
                    extra AS extra, generation_expression AS generation_expression
             FROM information_schema.columns
             WHERE table_schema = ? AND table_name = ?
             ORDER BY ordinal_position",
            params(),
        )
        .await?
        .iter()
        .map(|row| {
            let is_nullable: String = get(row, "is_nullable")?;
            let extra: String = get(row, "extra")?;
            let mut sql_type: String = get(row, "column_type")?;
            let mut default =
                mysql_default(get(row, "column_default")?, &sql_type, &extra, mariadb);
            // `extra` names a generated column's kind; MySQL escapes the
            // quotes of its expression as it does an expression default's.
            let upper_extra = extra.to_ascii_uppercase();
            let generation = ["VIRTUAL", "STORED", "PERSISTENT"]
                .into_iter()
                .find(|kind| upper_extra.contains(&format!("{kind} GENERATED")));
            let expression: Option<String> = get(row, "generation_expression")?;
            // A generated column takes no `DEFAULT`, which MariaDB refuses
            // beside `GENERATED ALWAYS AS`; it reports one anyway, the text
            // `NULL`, where MySQL reports none.
            if let (Some(kind), Some(expression)) = (generation, expression) {
                let expression = if mariadb {
                    expression
                } else {
                    expression.replace("\\'", "'")
                };
                sql_type = format!("{sql_type} GENERATED ALWAYS AS ({expression}) {kind}");
                default = None;
            }

            Ok(CatalogColumn {
                name: get(row, "column_name")?,
                sql_type,
                nullable: is_nullable == "YES",
                default,
                auto_increment: extra.contains("auto_increment"),
            })
        })
        .collect::<Result<Vec<_>>>()?;

        // `non_unique` is an INT on MySQL and a BIGINT on MariaDB, so it is
        // cast to one width, as is the prefix length `sub_part`.
        let mut primary_key = Vec::new();
        let mut keys: BTreeMap<String, MysqlKey> = BTreeMap::new();
        for row in query(
            conn,
            mysql,
            "SELECT index_name AS index_name, CAST(non_unique AS SIGNED) AS non_unique,
                    column_name AS column_name, index_type AS index_type,
                    CAST(sub_part AS SIGNED) AS sub_part
             FROM information_schema.statistics
             WHERE table_schema = ? AND table_name = ?
             ORDER BY index_name, seq_in_index",
            params(),
        )
        .await?
        {
            let index: String = get(&row, "index_name")?;
            let column: Option<String> = get(&row, "column_name")?;
            if index == "PRIMARY" {
                primary_key.extend(column);
                continue;
            }
            let key = keys.entry(index).or_insert_with(|| MysqlKey {
                unique: false,
                kind: String::new(),
                parts: Vec::new(),
            });
            key.unique = get::<i64>(&row, "non_unique")? == 0;
            key.kind = get(&row, "index_type")?;
            let prefix: Option<i64> = get(&row, "sub_part")?;
            key.parts.push(column.map(|column| (column, prefix)));
        }

        // A plain key is described by its columns; a full-text, spatial or
        // prefix one is written out, and one with an expression part, which
        // the catalog does not spell, is named in a comment.
        let table = quote_ident(DatabaseType::MySQL, &table_name);
        let mut index_columns = Vec::new();
        for (index, key) in keys {
            let Some(parts) = key
                .parts
                .into_iter()
                .collect::<Option<Vec<(String, Option<i64>)>>>()
            else {
                catalog.verbatim.push(format!(
                    "-- index {} on {} has an expression key part and is not exported",
                    quote_ident(DatabaseType::MySQL, &index),
                    table
                ));
                continue;
            };
            let plain = matches!(key.kind.as_str(), "BTREE" | "HASH")
                && parts.iter().all(|(_, prefix)| prefix.is_none());
            if plain {
                index_columns.extend(parts.into_iter().map(|(column, _)| CatalogIndexColumn {
                    index: index.clone(),
                    unique: key.unique,
                    column,
                }));
                continue;
            }
            let kind = match key.kind.as_str() {
                "FULLTEXT" => "FULLTEXT ",
                "SPATIAL" => "SPATIAL ",
                _ if key.unique => "UNIQUE ",
                _ => "",
            };
            let columns: Vec<String> = parts
                .iter()
                .map(|(column, prefix)| {
                    let column = quote_ident(DatabaseType::MySQL, column);
                    match prefix {
                        Some(length) => format!("{column}({length})"),
                        None => column,
                    }
                })
                .collect();
            catalog.verbatim.push(format!(
                "CREATE {kind}INDEX {} ON {table} ({})",
                quote_ident(DatabaseType::MySQL, &index),
                columns.join(", ")
            ));
        }

        mysql_constraints(
            conn,
            &db_name,
            &table_name,
            mariadb,
            &mut catalog.constraints,
        )
        .await?;

        catalog.tables.push(catalog_table(
            &table_name,
            None,
            columns,
            primary_key,
            group_indexes(index_columns),
            false,
        ));
    }

    Ok(catalog)
}

/// The foreign key and `CHECK` constraints of one MySQL or MariaDB table, as
/// `ALTER TABLE .. ADD CONSTRAINT` statements.
///
/// MySQL reports a `CHECK` clause with its quotes escaped, as it does an
/// expression default, and whether it is enforced; MariaDB reports the
/// clause as written, including the `json_valid` check it gives a JSON
/// column, whose type it reports as `longtext`.
async fn mysql_constraints(
    conn: &OrmConnection,
    db_name: &str,
    table_name: &str,
    mariadb: bool,
    constraints: &mut Vec<String>,
) -> Result<()> {
    let mysql = Backend::MySql;
    let quote = |name: &str| quote_ident(DatabaseType::MySQL, name);
    let params = || vec![db_name.into(), table_name.into()];
    let table = quote(table_name);

    let check_sql = if mariadb {
        "SELECT constraint_name AS constraint_name, check_clause AS check_clause,
                'YES' AS enforced
         FROM information_schema.check_constraints
         WHERE constraint_schema = ? AND table_name = ?
         ORDER BY constraint_name"
    } else {
        "SELECT tc.constraint_name AS constraint_name, cc.check_clause AS check_clause,
                tc.enforced AS enforced
         FROM information_schema.table_constraints tc
         JOIN information_schema.check_constraints cc
             ON cc.constraint_schema = tc.constraint_schema
             AND cc.constraint_name = tc.constraint_name
         WHERE tc.table_schema = ? AND tc.table_name = ? AND tc.constraint_type = 'CHECK'
         ORDER BY tc.constraint_name"
    };
    for row in query(conn, mysql, check_sql, params()).await? {
        let name: String = get(&row, "constraint_name")?;
        let clause: String = get(&row, "check_clause")?;
        let enforced: String = get(&row, "enforced")?;
        let clause = if mariadb {
            clause
        } else {
            clause.replace("\\'", "'")
        };
        let enforcement = if enforced.eq_ignore_ascii_case("NO") {
            " NOT ENFORCED"
        } else {
            ""
        };
        constraints.push(format!(
            "ALTER TABLE {table} ADD CONSTRAINT {} CHECK ({clause}){enforcement}",
            quote(&name)
        ));
    }

    // One row per key column, in key order.
    let mut foreign_keys: Vec<(String, MysqlForeignKey)> = Vec::new();
    for row in query(
        conn,
        mysql,
        "SELECT rc.constraint_name AS constraint_name, kcu.column_name AS column_name,
                kcu.referenced_table_schema AS referenced_schema,
                kcu.referenced_table_name AS referenced_table,
                kcu.referenced_column_name AS referenced_column,
                rc.update_rule AS update_rule, rc.delete_rule AS delete_rule
         FROM information_schema.referential_constraints rc
         JOIN information_schema.key_column_usage kcu
             ON kcu.constraint_schema = rc.constraint_schema
             AND kcu.constraint_name = rc.constraint_name
             AND kcu.table_name = rc.table_name
         WHERE rc.constraint_schema = ? AND rc.table_name = ?
         ORDER BY rc.constraint_name, kcu.ordinal_position",
        params(),
    )
    .await?
    {
        let name: String = get(&row, "constraint_name")?;
        if foreign_keys.last().is_none_or(|(last, _)| *last != name) {
            let referenced_schema: String = get(&row, "referenced_schema")?;
            let referenced_table: String = get(&row, "referenced_table")?;
            let referenced = if referenced_schema == db_name {
                quote(&referenced_table)
            } else {
                format!("{}.{}", quote(&referenced_schema), quote(&referenced_table))
            };
            foreign_keys.push((
                name.clone(),
                MysqlForeignKey {
                    columns: Vec::new(),
                    referenced,
                    referenced_columns: Vec::new(),
                    on_delete: get(&row, "delete_rule")?,
                    on_update: get(&row, "update_rule")?,
                },
            ));
        }
        let (_, key) = foreign_keys.last_mut().expect("pushed above");
        key.columns
            .push(quote(&get::<String>(&row, "column_name")?));
        key.referenced_columns
            .push(quote(&get::<String>(&row, "referenced_column")?));
    }
    for (name, key) in foreign_keys {
        let mut statement = format!(
            "ALTER TABLE {table} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
            quote(&name),
            key.columns.join(", "),
            key.referenced,
            key.referenced_columns.join(", ")
        );
        for (event, rule) in [("DELETE", &key.on_delete), ("UPDATE", &key.on_update)] {
            let rule = rule.to_ascii_uppercase();
            if matches!(
                rule.as_str(),
                "CASCADE" | "SET NULL" | "SET DEFAULT" | "RESTRICT" | "NO ACTION"
            ) {
                statement.push_str(&format!(" ON {event} {rule}"));
            }
        }
        constraints.push(statement);
    }

    Ok(())
}

/// One MySQL foreign key, its columns and those it references quoted.
struct MysqlForeignKey {
    columns: Vec<String>,
    referenced: String,
    referenced_columns: Vec<String>,
    on_delete: String,
    on_update: String,
}

/// One MySQL index as `information_schema.statistics` lists it: each key
/// part is a column and its prefix length, or `None` for an expression.
struct MysqlKey {
    unique: bool,
    kind: String,
    parts: Vec<Option<(String, Option<i64>)>>,
}

/// A MySQL or MariaDB `column_default` as a `DEFAULT` clause writes it.
///
/// MariaDB reports the clause's own spelling. MySQL 8 reports a literal bare
/// (`draft` for `DEFAULT 'draft'`), and an expression, which `extra` marks
/// `DEFAULT_GENERATED`, without its parentheses and with its quotes escaped;
/// both are restored here. A `TIMESTAMP` or `DATETIME` column's
/// `CURRENT_TIMESTAMP` default stays bare, as MySQL takes it; the same text
/// as another column's literal is quoted like any other.
pub(super) fn mysql_default(
    default: Option<String>,
    sql_type: &str,
    extra: &str,
    mariadb: bool,
) -> Option<String> {
    let default = default?;
    if mariadb {
        return Some(default);
    }
    let lowered = sql_type.to_ascii_lowercase();
    let temporal = lowered.starts_with("timestamp") || lowered.starts_with("datetime");
    let current_timestamp = default
        .to_ascii_uppercase()
        .strip_prefix("CURRENT_TIMESTAMP")
        .is_some_and(|precision| {
            precision.is_empty()
                || precision
                    .strip_prefix('(')
                    .and_then(|digits| digits.strip_suffix(')'))
                    .is_some_and(|digits| digits.chars().all(|digit| digit.is_ascii_digit()))
        });
    if temporal && current_timestamp {
        return Some(default);
    }
    if extra.to_ascii_uppercase().contains("DEFAULT_GENERATED") {
        return Some(format!("({})", default.replace("\\'", "'")));
    }
    let numeric = [
        "tinyint",
        "smallint",
        "mediumint",
        "int",
        "bigint",
        "decimal",
        "numeric",
        "float",
        "double",
        "bit",
        "year",
    ]
    .iter()
    .any(|prefix| lowered.starts_with(prefix));
    Some(if numeric {
        default
    } else {
        format!(
            "'{}'",
            crate::internal::sql_safety::escape_sql_literal_for_db(DatabaseType::MySQL, &default)
        )
    })
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
               AND l.name NOT LIKE 'sqlite_%'
             UNION ALL
             SELECT CASE type WHEN 'view' THEN 2 WHEN 'index' THEN 3 ELSE 4 END, name, sql
             FROM sqlite_master
             WHERE type IN ('view', 'index', 'trigger')
               AND tbl_name NOT LIKE 'sqlite_%'
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
