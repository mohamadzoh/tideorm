//! One PostgreSQL table's definition, columns and per-table statements.

use std::fmt::Write as _;

use super::*;

/// One table's statements. Its `CREATE TABLE`, owner and `NOT NULL`
/// constraints are returned, to be ordered with the objects they depend on;
/// its indexes and column defaults join the catalog's verbatim statements and
/// its constraints the catalog's, or `foreign_keys` and `routine_checks` for
/// after every table exists.
pub(super) async fn postgres_table(
    conn: &impl ConnectionTrait,
    row: &QueryResult,
    catalog: &mut Catalog,
    foreign_keys: &mut Vec<String>,
    routine_checks: &mut Vec<Vec<String>>,
) -> Result<Vec<String>> {
    let pg = DatabaseType::Postgres;
    let partition_key: Option<String> = get(row, "partition_key")?;
    let partition_bound: Option<String> = get(row, "partition_bound")?;
    let partition_parent: Option<String> = get(row, "partition_parent")?;
    let inheritance_parents: Option<String> = get(row, "inheritance_parents")?;
    let table_schema: String = get(row, "table_schema")?;
    let table_name: String = get(row, "table_name")?;
    let params = || vec![table_schema.clone().into(), table_name.clone().into()];
    let (columns, partition_columns) =
        postgres_columns(conn, params(), partition_parent.is_some()).await?;

    // Keep primary-key column order. Every secondary index is exported
    // verbatim: even a simple B-tree can carry ordering, collation or an
    // operator class which a column-name list cannot represent.
    for mut definition in statements(
        conn,
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
        &["definition"],
    )
    .await?
    {
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
    let table = quote_qualified(DatabaseType::Postgres, Some(&table_schema), &table_name);
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
        let _ = write!(definition, " INHERITS ({parents})");
    }
    if let Some(key) = partition_key {
        let _ = write!(definition, " PARTITION BY {key}");
    }
    if get::<String>(row, "persistence")? == "u" {
        definition = definition.replacen("CREATE TABLE", "CREATE UNLOGGED TABLE", 1);
    }
    if let Some(options) = get::<Option<String>>(row, "storage_options")? {
        let _ = write!(definition, " WITH ({options})");
    }
    let mut definitions = vec![definition, get(row, "owner")?];
    routine_checks.push(deferred_routine_checks);
    catalog.constraints.extend(unvalidated_checks);
    if inheritance_parents.is_some() || partition_parent.is_some() {
        definitions.extend(statements(conn,
            "SELECT format('ALTER TABLE ONLY %I.%I ALTER COLUMN %I SET NOT NULL', ns.nspname, c.relname, a.attname) AS definition
                 FROM pg_catalog.pg_class c
                 JOIN pg_catalog.pg_namespace ns ON ns.oid = c.relnamespace
                 JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid
                 WHERE ns.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped
                   AND a.attnotnull
                 ORDER BY a.attnum", params(), &["definition"]).await?);
    }
    // ONLY avoids propagating defaults into children that deliberately
    // dropped or overrode them. Every effective default, including those
    // on inherited columns and partitions, is read from its own table.
    catalog.verbatim.extend(statements(conn,
        "SELECT format('ALTER TABLE ONLY %I.%I ALTER COLUMN %I SET DEFAULT %s', ns.nspname, c.relname, a.attname,
                           pg_catalog.pg_get_expr(d.adbin, d.adrelid)) AS definition
             FROM pg_catalog.pg_class c
             JOIN pg_catalog.pg_namespace ns ON ns.oid=c.relnamespace
             JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid
             JOIN pg_catalog.pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
             WHERE ns.nspname=$1 AND c.relname=$2 AND a.attnum>0 AND NOT a.attisdropped
               AND a.attgenerated='' AND a.attidentity=''
             ORDER BY a.attnum", params(), &["definition"]).await?);
    Ok(definitions)
}

/// The columns of the table `params` names, and for a partition the
/// `WITH OPTIONS` clauses of its generated columns.
async fn postgres_columns(
    conn: &impl ConnectionTrait,
    params: Vec<Value>,
    partition: bool,
) -> Result<(Vec<CatalogColumn>, Vec<String>)> {
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
        DatabaseType::Postgres,
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
        params,
    )
    .await?
    .iter()
    .map(|row| {
        let default: Option<String> = get(row, "column_default")?;
        let mut sql_type: String = get(row, "data_type")?;
        if let Some(collation) = get::<Option<String>>(row, "collation")? {
            let _ = write!(sql_type, " COLLATE {collation}");
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
            if partition {
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
    Ok((columns, partition_columns))
}
