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
    /// Every base table is read back from the catalog - its columns, primary
    /// key and secondary indexes - and rendered by [`SchemaGenerator`] for the
    /// backend the global connection actually talks to.
    pub async fn write_schema<P: AsRef<Path>>(path: P) -> Result<()> {
        let db = crate::require_db()?;
        let db_type = db.backend();
        let conn = db.__internal_connection()?;

        let tables = match db_type {
            DatabaseType::Postgres => introspect_postgres(&conn).await?,
            DatabaseType::MySQL | DatabaseType::MariaDB => introspect_mysql(&conn).await?,
            DatabaseType::SQLite => introspect_sqlite(&conn).await?,
        };

        let mut generator = SchemaGenerator::new(db_type);
        for table in tables {
            generator.add_table(table);
        }

        fs::write(path.as_ref(), generator.generate())
            .map_err(|e| Error::internal(format!("Failed to write schema file: {}", e)))?;

        Ok(())
    }
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
/// key columns keep their auto-increment flag.
pub(super) fn catalog_table(
    name: &str,
    schema_name: Option<&str>,
    columns: Vec<CatalogColumn>,
    primary_key: Vec<String>,
    indexes: Vec<IndexDefinition>,
) -> TableSchema {
    let mut builder = TableSchemaBuilder::new(name);
    if let Some(schema_name) = schema_name {
        builder = builder.schema(schema_name);
    }

    for column in columns {
        let mut schema = ColumnSchema::new(column.name, column.sql_type);

        if primary_key.contains(&schema.name) {
            schema = schema.primary_key();
            if column.auto_increment {
                schema = schema.auto_increment();
            }
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

async fn introspect_postgres(conn: &OrmConnection) -> Result<Vec<TableSchema>> {
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

    let mut tables = Vec::with_capacity(table_rows.len());

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
                    pg_catalog.pg_get_expr(d.adbin, d.adrelid) AS column_default
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
            // A serial column's default is its sequence, which the serial
            // type recreates.
            let auto_increment = default
                .as_deref()
                .is_some_and(|default| default.contains("nextval"));

            Ok(CatalogColumn {
                name: get(row, "column_name")?,
                sql_type: get(row, "data_type")?,
                nullable: get(row, "is_nullable")?,
                default: default.filter(|_| !auto_increment),
                auto_increment,
            })
        })
        .collect::<Result<Vec<_>>>()?;

        // One row per key column, in key order: `indkey` lists the columns
        // the way the index was declared, which `attnum` does not.
        let mut primary_key = Vec::new();
        let mut index_columns = Vec::new();
        for row in query(
            conn,
            pg,
            "SELECT i.relname AS index_name, ix.indisprimary AS is_primary,
                    ix.indisunique AS is_unique, a.attname AS column_name
             FROM pg_catalog.pg_index ix
             JOIN pg_catalog.pg_class t ON t.oid = ix.indrelid
             JOIN pg_catalog.pg_namespace ns ON ns.oid = t.relnamespace
             JOIN pg_catalog.pg_class i ON i.oid = ix.indexrelid
             CROSS JOIN LATERAL unnest(ix.indkey) WITH ORDINALITY AS k(attnum, position)
             JOIN pg_catalog.pg_attribute a ON a.attrelid = t.oid AND a.attnum = k.attnum
             WHERE ns.nspname = $1 AND t.relname = $2
             ORDER BY i.relname, k.position",
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

        tables.push(catalog_table(
            &table_name,
            Some(&table_schema),
            columns,
            primary_key,
            group_indexes(index_columns),
        ));
    }

    Ok(tables)
}

async fn introspect_mysql(conn: &OrmConnection) -> Result<Vec<TableSchema>> {
    let mysql = Backend::MySql;
    let database_row = query(conn, mysql, "SELECT DATABASE() AS db_name", Vec::new()).await?;
    let Some(db_name) = database_row
        .first()
        .map(|row| get::<Option<String>>(row, "db_name"))
        .transpose()?
        .flatten()
        .filter(|name| !name.is_empty())
    else {
        return Ok(Vec::new());
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

    let mut tables = Vec::with_capacity(table_rows.len());

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
                    extra AS extra
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

            Ok(CatalogColumn {
                name: get(row, "column_name")?,
                sql_type: get(row, "column_type")?,
                nullable: is_nullable == "YES",
                default: get(row, "column_default")?,
                auto_increment: extra.contains("auto_increment"),
            })
        })
        .collect::<Result<Vec<_>>>()?;

        // `non_unique` is an INT on MySQL and a BIGINT on MariaDB, so it is
        // cast to one width. A functional key part has no column name and
        // cannot be exported.
        let mut primary_key = Vec::new();
        let mut index_columns = Vec::new();
        for row in query(
            conn,
            mysql,
            "SELECT index_name AS index_name, CAST(non_unique AS SIGNED) AS non_unique,
                    column_name AS column_name
             FROM information_schema.statistics
             WHERE table_schema = ? AND table_name = ?
             ORDER BY index_name, seq_in_index",
            params(),
        )
        .await?
        {
            let Some(column) = get::<Option<String>>(&row, "column_name")? else {
                continue;
            };
            let index: String = get(&row, "index_name")?;

            if index == "PRIMARY" {
                primary_key.push(column);
            } else {
                index_columns.push(CatalogIndexColumn {
                    index,
                    unique: get::<i64>(&row, "non_unique")? == 0,
                    column,
                });
            }
        }

        tables.push(catalog_table(
            &table_name,
            None,
            columns,
            primary_key,
            group_indexes(index_columns),
        ));
    }

    Ok(tables)
}

async fn introspect_sqlite(conn: &OrmConnection) -> Result<Vec<TableSchema>> {
    let sqlite = Backend::Sqlite;
    let table_rows = query(
        conn,
        sqlite,
        "SELECT name FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
         ORDER BY name",
        Vec::new(),
    )
    .await?;

    let mut tables = Vec::with_capacity(table_rows.len());

    for row in table_rows {
        let table_name: String = get(&row, "name")?;
        let quoted_table_name = quote_ident(DatabaseType::SQLite, &table_name);

        let mut primary_key: Vec<(i64, String)> = Vec::new();
        let mut columns = Vec::new();
        for row in query(
            conn,
            sqlite,
            &format!("PRAGMA table_info({})", quoted_table_name),
            Vec::new(),
        )
        .await?
        {
            let name: String = get(&row, "name")?;
            // `pk` is the column's 1-based position in the key, or 0.
            let key_position: i64 = get(&row, "pk")?;
            if key_position > 0 {
                primary_key.push((key_position, name.clone()));
            }

            columns.push(CatalogColumn {
                name,
                sql_type: get::<String>(&row, "type")?.to_uppercase(),
                nullable: get::<i64>(&row, "notnull")? == 0,
                default: get(&row, "dflt_value")?,
                // An INTEGER key is the rowid, which SQLite assigns on its own.
                auto_increment: false,
            });
        }
        primary_key.sort_unstable();

        let mut index_columns = Vec::new();
        for index_row in query(
            conn,
            sqlite,
            &format!("PRAGMA index_list({})", quoted_table_name),
            Vec::new(),
        )
        .await?
        {
            let origin: String = get(&index_row, "origin")?;
            if origin == "pk" {
                continue;
            }

            let index: String = get(&index_row, "name")?;
            let unique = get::<i64>(&index_row, "unique")? == 1;

            // `index_info` is already in key order. An expression key part
            // has no column name and cannot be exported.
            let mut key_columns = Vec::new();
            for info_row in query(
                conn,
                sqlite,
                &format!(
                    "PRAGMA index_info({})",
                    quote_ident(DatabaseType::SQLite, &index)
                ),
                Vec::new(),
            )
            .await?
            {
                if let Some(column) = get::<Option<String>>(&info_row, "name")? {
                    key_columns.push(column);
                }
            }

            let index = sqlite_index_export_name(&table_name, index, &key_columns);
            index_columns.extend(key_columns.into_iter().map(|column| CatalogIndexColumn {
                index: index.clone(),
                unique,
                column,
            }));
        }

        tables.push(catalog_table(
            &table_name,
            None,
            columns,
            primary_key.into_iter().map(|(_, name)| name).collect(),
            group_indexes(index_columns),
        ));
    }

    Ok(tables)
}

/// The name a SQLite index is exported under.
///
/// SQLite names the index behind a `UNIQUE` constraint `sqlite_autoindex_*`
/// and refuses to create an index with its reserved `sqlite_` prefix, so the
/// export uses the name `TableBuilder::unique_index` would have given it.
pub(super) fn sqlite_index_export_name(table: &str, index: String, columns: &[String]) -> String {
    if index.starts_with("sqlite_") {
        return format!("idx_{}_{}_unique", table, columns.join("_"));
    }

    index
}
