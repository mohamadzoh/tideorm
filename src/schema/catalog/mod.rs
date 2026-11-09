//! What a connected database's catalog says about its tables: the one place
//! TideORM reads it, for schema sync, migrations, the schema writer and
//! [`Database::catalog`].

use crate::config::DatabaseType;
use crate::database::Database;
use crate::error::{Error, ErrorContext, Result};
use crate::internal::{
    ConnectionTrait, QueryResult, TryGetable, Value, build_statement_with_values, translate_error,
};

mod mysql;
mod postgres;
mod sqlite;

/// A column of a table or view, as [`Catalog::columns`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    /// The column's name.
    pub name: String,
    /// Its type as the database writes it: `character varying(255)` on
    /// PostgreSQL, `int unsigned` on MySQL, the declared type on SQLite.
    pub data_type: String,
    /// Whether it accepts NULL.
    pub nullable: bool,
    /// Its default expression, as the catalog writes it.
    pub default: Option<String>,
    /// Whether it is part of the primary key.
    pub primary_key: bool,
    /// Whether a unique index other than the primary key holds this column
    /// alone.
    pub unique: bool,
    /// Whether it is part of a foreign key.
    pub foreign_key: bool,
    /// What else the catalog says about it: MySQL's `auto_increment`,
    /// PostgreSQL's identity, a generated column's expression.
    pub extra: Option<String>,
}

/// An index of a table, as [`Catalog::indexes`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexInfo {
    /// The index's name.
    pub name: String,
    /// Its key columns in order. PostgreSQL writes an expression key as its
    /// expression; MySQL and SQLite leave one out.
    pub columns: Vec<String>,
    /// Whether it is unique.
    pub unique: bool,
    /// Whether it is the primary key's.
    pub primary: bool,
    /// Whether every key part is a whole column: not when one is an
    /// expression or, on MySQL and MariaDB, a column's prefix.
    pub whole_columns: bool,
}

/// One column of a foreign key, as [`Catalog::foreign_keys`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKeyInfo {
    /// The referencing column.
    pub column: String,
    /// The referenced table, written as [`Catalog::tables`] writes it.
    pub references_table: String,
    /// The referenced column.
    pub references_column: String,
}

/// Reads a [`Database`]'s catalog: its tables, views, sequences and enum
/// types, and each table's columns, indexes and foreign keys.
///
/// It reads the connection's current schema on PostgreSQL, its database on
/// MySQL and MariaDB, and SQLite's main database. On PostgreSQL a name in
/// another schema is written `schema.name`, and a table passed that way is
/// read from that schema; schemas named `pg_*` and `information_schema`, and
/// what an extension installed, are left out of the listings, since
/// PostgreSQL drops those only with the extension. Inside a transaction the
/// catalog is read on it, so it sees the tables the transaction created.
#[derive(Debug, Clone, Copy)]
pub struct Catalog<'a> {
    db: &'a Database,
}

impl Database {
    /// This database's catalog.
    ///
    /// ```no_run
    /// # async fn example(db: &tideorm::Database) -> tideorm::Result<()> {
    /// for table in db.catalog().tables().await? {
    ///     let columns = db.catalog().columns(&table).await?;
    ///     println!("{table}: {} columns", columns.len());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn catalog(&self) -> Catalog<'_> {
        Catalog { db: self }
    }
}

impl Catalog<'_> {
    /// The base tables, PostgreSQL's partitioned ones and MariaDB's
    /// system-versioned ones included, by name.
    pub async fn tables(&self) -> Result<Vec<String>> {
        self.names(|queries| Some(queries.tables)).await
    }

    /// The views, by name.
    pub async fn views(&self) -> Result<Vec<String>> {
        self.names(|queries| Some(queries.views)).await
    }

    /// The sequences, by name: PostgreSQL's and MariaDB's. MySQL and SQLite
    /// have none.
    pub async fn sequences(&self) -> Result<Vec<String>> {
        self.names(|queries| queries.sequences).await
    }

    /// PostgreSQL's enum types, by name. The other backends declare an enum
    /// on its column.
    pub async fn enum_types(&self) -> Result<Vec<String>> {
        self.names(|queries| queries.enum_types).await
    }

    /// The columns of the table or view `table`, in order; none when there is
    /// no such table.
    pub async fn columns(&self, table: &str) -> Result<Vec<ColumnInfo>> {
        let indexes = self.indexes(table).await?;
        let foreign_keys = self.foreign_keys(table).await?;
        self.rows(table, |queries| queries.columns)
            .await?
            .iter()
            .map(|row| {
                let name: String = get(row, "name")?;
                Ok(ColumnInfo {
                    data_type: get(row, "data_type")?,
                    nullable: flag(row, "nullable")?,
                    default: get(row, "default_value")?,
                    primary_key: flag(row, "primary_key")?,
                    unique: indexes.iter().any(|index| {
                        index.unique && !index.primary && index.columns == [name.as_str()]
                    }),
                    foreign_key: foreign_keys.iter().any(|key| key.column == name),
                    extra: get(row, "extra")?,
                    name,
                })
            })
            .collect()
    }

    /// The indexes of `table`, by name. SQLite has no index for an `INTEGER
    /// PRIMARY KEY`, which is the row id.
    pub async fn indexes(&self, table: &str) -> Result<Vec<IndexInfo>> {
        let connection = self.db.__get_connection()?;
        let executor = connection.executor();
        let backend = executor.get_database_backend();
        let (schema, name) = table_parts(backend, table);
        table_indexes(&executor, backend, schema, name).await
    }

    /// The columns of `table`'s foreign keys, key by key.
    pub async fn foreign_keys(&self, table: &str) -> Result<Vec<ForeignKeyInfo>> {
        let mut keys = Vec::new();
        for row in self.rows(table, |queries| queries.foreign_keys).await? {
            // SQLite names no column for a key to a parent without a primary
            // key, which it refuses to enforce.
            if let Some(references_column) = get(&row, "references_column")? {
                keys.push(ForeignKeyInfo {
                    column: get(&row, "column_name")?,
                    references_table: get(&row, "references_table")?,
                    references_column,
                });
            }
        }
        Ok(keys)
    }

    async fn names(&self, sql: fn(&Queries) -> Option<&'static str>) -> Result<Vec<String>> {
        let connection = self.db.__get_connection()?;
        let executor = connection.executor();
        let backend = executor.get_database_backend();
        let Some(sql) = sql(queries(backend)) else {
            return Ok(Vec::new());
        };
        query(&executor, backend, sql, Vec::new())
            .await?
            .iter()
            .map(|row| get(row, "name"))
            .collect()
    }

    async fn rows(
        &self,
        table: &str,
        sql: fn(&Queries) -> &'static str,
    ) -> Result<Vec<QueryResult>> {
        let connection = self.db.__get_connection()?;
        let executor = connection.executor();
        let backend = executor.get_database_backend();
        let (schema, name) = table_parts(backend, table);
        table_rows(&executor, backend, sql(queries(backend)), schema, name).await
    }
}

/// One backend's catalog statements. A listing binds nothing. A statement
/// about one table, and `index_table`, binds the schema (`NULL` for the
/// connection's own) and the name, SQLite's the name alone. Each names its
/// columns in lower case, as MySQL 8 does not.
struct Queries {
    tables: &'static str,
    views: &'static str,
    sequences: Option<&'static str>,
    enum_types: Option<&'static str>,
    /// Whether a table or view exists, as `present`.
    table_exists: &'static str,
    columns: &'static str,
    /// One row per key column, grouped by index, with `whole_column`.
    indexes: &'static str,
    foreign_keys: &'static str,
    /// The table that owns an index: PostgreSQL's and SQLite's index names
    /// are schema-wide, MySQL's per table.
    index_table: Option<&'static str>,
}

fn queries(backend: DatabaseType) -> &'static Queries {
    match backend {
        DatabaseType::Postgres => &postgres::QUERIES,
        DatabaseType::MySQL | DatabaseType::MariaDB => &mysql::QUERIES,
        DatabaseType::SQLite => &sqlite::QUERIES,
    }
}

/// `table`'s schema and name: PostgreSQL reads `schema.table`; the others
/// name a table alone.
fn table_parts(backend: DatabaseType, table: &str) -> (Option<&str>, &str) {
    match table.rsplit_once('.') {
        Some((schema, name)) if backend == DatabaseType::Postgres => (Some(schema), name),
        _ => (None, table),
    }
}

fn table_params(backend: DatabaseType, schema: Option<&str>, name: &str) -> Vec<Value> {
    match backend {
        DatabaseType::SQLite => vec![name.into()],
        _ => vec![schema.into(), name.into()],
    }
}

async fn table_rows(
    conn: &impl ConnectionTrait,
    backend: DatabaseType,
    sql: &str,
    schema: Option<&str>,
    table: &str,
) -> Result<Vec<QueryResult>> {
    query(conn, backend, sql, table_params(backend, schema, table))
        .await
        .map_err(|error| error.with_context(ErrorContext::new().table(table)))
}

/// Whether `table` exists in `schema`, or in the connection's current schema
/// (PostgreSQL) or database (MySQL, MariaDB) without one; SQLite reads its
/// main database, whose table names ignore case.
pub(crate) async fn table_exists(
    conn: &impl ConnectionTrait,
    backend: DatabaseType,
    schema: Option<&str>,
    table: &str,
) -> Result<bool> {
    let rows = table_rows(conn, backend, queries(backend).table_exists, schema, table).await?;
    present(&rows, table)
}

/// The column names of `table`, looked for where [`table_exists`] looks.
pub(crate) async fn column_names(
    conn: &impl ConnectionTrait,
    backend: DatabaseType,
    schema: Option<&str>,
    table: &str,
) -> Result<Vec<String>> {
    table_rows(conn, backend, queries(backend).columns, schema, table)
        .await?
        .iter()
        .map(|row| get(row, "name"))
        .collect()
}

/// The tables `table`'s foreign keys reference, one entry per key column,
/// written as [`Catalog::tables`] writes them.
pub(crate) async fn referenced_tables(
    conn: &impl ConnectionTrait,
    backend: DatabaseType,
    schema: Option<&str>,
    table: &str,
) -> Result<Vec<String>> {
    table_rows(conn, backend, queries(backend).foreign_keys, schema, table)
        .await?
        .iter()
        .map(|row| get(row, "references_table"))
        .collect()
}

/// The indexes of `table`, looked for where [`table_exists`] looks.
pub(crate) async fn table_indexes(
    conn: &impl ConnectionTrait,
    backend: DatabaseType,
    schema: Option<&str>,
    table: &str,
) -> Result<Vec<IndexInfo>> {
    let mut indexes: Vec<IndexInfo> = Vec::new();
    for row in table_rows(conn, backend, queries(backend).indexes, schema, table).await? {
        let name: String = get(&row, "name")?;
        let column: Option<String> = get(&row, "column_name")?;
        let whole = flag(&row, "whole_column")?;
        match indexes.last_mut() {
            Some(index) if index.name == name => {
                index.columns.extend(column);
                index.whole_columns &= whole;
            }
            _ => indexes.push(IndexInfo {
                name,
                columns: column.into_iter().collect(),
                unique: flag(&row, "is_unique")?,
                primary: flag(&row, "is_primary")?,
                whole_columns: whole,
            }),
        }
    }
    Ok(indexes)
}

/// Whether `table` has an index named `index` in the MySQL or MariaDB
/// database `schema`, or the connection's own without one: MySQL has no
/// `CREATE INDEX IF NOT EXISTS`, so a create checks first.
pub(crate) async fn mysql_index_exists(
    conn: &impl ConnectionTrait,
    schema: Option<&str>,
    table: &str,
    index: &str,
) -> Result<bool> {
    let params = vec![schema.into(), table.into(), index.into()];
    let rows = query(conn, DatabaseType::MySQL, mysql::INDEX_EXISTS, params)
        .await
        .map_err(|error| error.with_context(ErrorContext::new().table(table)))?;
    present(&rows, table)
}

/// The table that owns the index `index` in `schema`, or the connection's
/// current schema without one: `None` when there is no such index, and on
/// MySQL and MariaDB, whose index names are per table.
pub(crate) async fn index_table(
    conn: &impl ConnectionTrait,
    backend: DatabaseType,
    schema: Option<&str>,
    index: &str,
) -> Result<Option<String>> {
    let Some(sql) = queries(backend).index_table else {
        return Ok(None);
    };
    let rows = query(conn, backend, sql, table_params(backend, schema, index)).await?;
    rows.first().map(|row| get(row, "name")).transpose()
}

/// The answer of an existence probe. A failure to read it surfaces rather
/// than reading as "absent": that would make `force_sync` skip its `DROP` and
/// turn the whole run into a silent no-op.
fn present(rows: &[QueryResult], table: &str) -> Result<bool> {
    rows.first()
        .map_or(Ok(false), |row| flag(row, "present"))
        .map_err(|error| error.with_context(ErrorContext::new().table(table)))
}

/// A yes-or-no column. PostgreSQL answers with a boolean; MySQL and SQLite
/// with an integer whose width varies by server.
fn flag(row: &QueryResult, column: &str) -> Result<bool> {
    if let Ok(value) = row.try_get::<bool>(column) {
        return Ok(value);
    }
    if let Ok(value) = row.try_get::<i64>(column) {
        return Ok(value != 0);
    }
    if let Ok(value) = row.try_get::<u64>(column) {
        return Ok(value != 0);
    }
    if let Ok(value) = row.try_get::<i32>(column) {
        return Ok(value != 0);
    }
    Err(Error::query(format!(
        "Unable to decode the catalog's `{column}` answer"
    )))
}

/// Decode one catalog value.
///
/// A value that does not decode fails the read: read as empty, it would
/// silently drop a column's type, key or default.
pub(crate) fn get<T: TryGetable>(row: &QueryResult, column: &str) -> Result<T> {
    row.try_get(column).map_err(translate_error)
}

/// Run one catalog statement.
pub(crate) async fn query(
    conn: &impl ConnectionTrait,
    backend: DatabaseType,
    sql: &str,
    params: Vec<Value>,
) -> Result<Vec<QueryResult>> {
    conn.query_all_raw(build_statement_with_values(backend, sql, params))
        .await
        .map_err(translate_error)
}

#[cfg(test)]
#[path = "../../../tests/unit/schema_catalog_tests.rs"]
mod tests;
