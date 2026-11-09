use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::config::DatabaseType;
use crate::database::Database;
use crate::error::{Error, Result};
use crate::internal::sql_safety::{quote_ident, quote_qualified};
use crate::internal::{ConnectionTrait, OrmConnection, QueryResult, Value, translate_error};
use crate::model::IndexDefinition;

use super::catalog::{get, query};
use super::{ColumnSchema, SchemaGenerator, TableSchema, TableSchemaBuilder};

mod mysql_objects;
mod postgres;
mod postgres_dependencies;
mod postgres_privileges;
mod postgres_tables;

/// Schema writer for auto-generating schema files
pub struct SchemaWriter;

impl SchemaWriter {
    /// Write the global database's schema to `path` as SQL, as
    /// [`schema_sql`](Self::schema_sql) renders it.
    pub async fn write_schema<P: AsRef<Path>>(path: P) -> Result<()> {
        Self::write_schema_for(&crate::require_db()?, path).await
    }

    /// Write `db`'s schema to `path` as SQL, as [`schema_sql`](Self::schema_sql)
    /// renders it. Nothing is written when the export fails.
    pub async fn write_schema_for<P: AsRef<Path>>(db: &Database, path: P) -> Result<()> {
        let text = Self::schema_sql(db).await?;
        fs::write(path.as_ref(), text)
            .map_err(|e| Error::internal(format!("Failed to write schema file: {}", e)))
    }

    /// `db`'s schema as SQL, a script that recreates it.
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
    /// Schemas, extensions and collations precede these objects. Unsupported types, custom aggregates, operators, foreign tables/servers and text-search objects fail
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
    pub async fn schema_sql(db: &Database) -> Result<String> {
        let db_type = db.backend();
        let conn = db.__internal_connection()?;

        let Catalog {
            prelude,
            tables,
            definitions,
            verbatim,
            constraints,
        } = match db_type {
            DatabaseType::Postgres => postgres::introspect_postgres(&conn).await?,
            DatabaseType::MySQL | DatabaseType::MariaDB => mysql_objects::introspect(&conn).await?,
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

        Ok(text)
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

/// The statements a PostgreSQL catalog query formats: its `columns`, row by
/// row.
async fn statements(
    conn: &impl ConnectionTrait,
    sql: &str,
    params: Vec<Value>,
    columns: &[&str],
) -> Result<Vec<String>> {
    let mut statements = Vec::new();
    for row in query(conn, DatabaseType::Postgres, sql, params).await? {
        for column in columns {
            statements.push(get(&row, column)?);
        }
    }
    Ok(statements)
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
        DatabaseType::SQLite,
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
               AND tbl_name NOT IN (SELECT name FROM pragma_table_list WHERE schema = 'main' AND type = 'shadow')
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
