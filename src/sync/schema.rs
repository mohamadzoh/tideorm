use crate::database::Database;
use crate::error::{Error, Result};
use crate::internal::sql_safety::quote_ident_for_backend;
use crate::internal::{
    Alias, Backend, ConnectionTrait, Expr, Index, MysqlQueryBuilder, OrmColumnDef, OrmConnection,
    PostgresQueryBuilder, QueryResult, SqliteQueryBuilder, Table, build_statement,
    build_statement_with_values,
};
use crate::migration::ColumnType;
use crate::schema::rust_type_to_column_type;
use crate::{tide_debug, tide_info, tide_warn};

use super::SyncRegistry;

/// Column definition for TideORM schema synchronization
#[derive(Debug, Clone)]
pub struct ColumnDef {
    /// Column name
    pub name: String,
    /// Column type (Rust type string, converted at sync time)
    pub col_type: String,
    /// Whether the column allows NULL values
    pub nullable: bool,
    /// Whether this is the primary key
    pub primary_key: bool,
    /// Whether this column auto-increments
    pub auto_increment: bool,
    /// Default value expression (if any)
    pub default: Option<String>,
}

impl ColumnDef {
    /// Create a new column definition
    pub fn new(name: impl Into<String>, col_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            col_type: col_type.into(),
            nullable: true,
            primary_key: false,
            auto_increment: false,
            default: None,
        }
    }

    /// Set as primary key
    pub fn primary_key(mut self) -> Self {
        self.primary_key = true;
        self.nullable = false;
        self
    }

    /// Set as auto-increment
    pub fn auto_increment(mut self) -> Self {
        self.auto_increment = true;
        self
    }

    /// Set as not nullable
    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }

    /// Set default value
    pub fn default(mut self, expr: impl Into<String>) -> Self {
        self.default = Some(expr.into());
        self
    }
}

/// Model schema definition for TideORM synchronization
#[derive(Debug, Clone)]
pub struct ModelSchema {
    /// Table name in the database
    pub table_name: String,
    /// Schema name (default: "public")
    pub schema_name: String,
    /// Column definitions
    pub columns: Vec<ColumnDef>,
    /// Primary key columns, in declaration order.
    pub primary_keys: Vec<String>,
}

impl ModelSchema {
    /// Create a new model schema
    pub fn new(table_name: impl Into<String>) -> Self {
        Self {
            table_name: table_name.into(),
            schema_name: "public".to_string(),
            columns: Vec::new(),
            primary_keys: Vec::new(),
        }
    }

    /// Set the schema name
    pub fn schema(mut self, schema: impl Into<String>) -> Self {
        self.schema_name = schema.into();
        self
    }

    /// Add a column definition
    pub fn column(mut self, col: ColumnDef) -> Self {
        self.columns.push(col);
        self
    }

    /// Add multiple columns
    pub fn columns(mut self, cols: Vec<ColumnDef>) -> Self {
        self.columns.extend(cols);
        self
    }

    /// Set the model primary keys.
    pub fn primary_keys(mut self, columns: Vec<String>) -> Self {
        self.primary_keys = columns;
        self
    }
}

pub(super) async fn sync_model_schemas(db: &Database, force_sync: bool) -> Result<()> {
    let models = SyncRegistry::get_all_schemas();
    let conn = db.__internal_connection()?;
    let backend = Backend::from(conn.get_database_backend());

    for model in models {
        let table_exists =
            check_table_exists(&conn, &model.schema_name, &model.table_name, backend).await?;

        if force_sync && table_exists {
            let quoted_table = quote_ident_for_backend(backend, &model.table_name);
            let drop_sql = match qualifying_schema(&model, backend) {
                Some(schema) => format!(
                    "DROP TABLE IF EXISTS {}.{} CASCADE",
                    quote_ident_for_backend(backend, schema),
                    quoted_table
                ),
                None => format!("DROP TABLE IF EXISTS {}", quoted_table),
            };

            let drop_stmt = build_statement(backend, drop_sql);
            conn.execute_raw(drop_stmt)
                .await
                .map_err(|error| Error::query(error.to_string()))?;

            tide_warn!("Dropped TideORM table: {}", model.table_name);
        }

        if !table_exists || force_sync {
            create_table_from_model_schema(&conn, &model, backend).await?;
            tide_info!("Created TideORM table: {}", model.table_name);
        } else {
            tide_debug!("TideORM table exists: {}", model.table_name);
            reconcile_existing_table(&conn, &model, backend).await?;
        }
    }

    Ok(())
}

/// The schema a model's DDL has to be qualified with, if any.
///
/// Only PostgreSQL has schemas that are independent of the connected database,
/// and it is the only backend whose existence probe filters on
/// `model.schema_name`. MySQL's "schema" *is* the database - `check_table_exists`
/// resolves it with `DATABASE()` - and SQLite has none, so qualifying there
/// would name a database that does not exist.
///
/// Every statement that names the table has to go through this, or `CREATE
/// TABLE` lands somewhere the existence probe and the force `DROP` never look.
fn qualifying_schema(model: &ModelSchema, backend: Backend) -> Option<&str> {
    match backend {
        Backend::Postgres if !model.schema_name.is_empty() => Some(&model.schema_name),
        _ => None,
    }
}

/// Bring an existing table in line with its model definition, additively.
///
/// Columns the model declares but the table lacks are added with
/// `ALTER TABLE ... ADD COLUMN`. Columns the table carries but the model no
/// longer declares are only reported - sync never drops a column, because that
/// destroys data. A column declared `NOT NULL` without a default is added as
/// nullable, since existing rows have nothing to backfill with; the remaining
/// mismatch is reported so it can be resolved with an explicit migration.
async fn reconcile_existing_table(
    conn: &OrmConnection,
    model: &ModelSchema,
    backend: Backend,
) -> Result<()> {
    let existing =
        fetch_existing_columns(conn, &model.schema_name, &model.table_name, backend).await?;

    if existing.is_empty() {
        return Err(Error::query(format!(
            "Unable to inspect columns of existing table '{}'; refusing to report a successful sync",
            model.table_name
        )));
    }

    let (missing, extra) = diff_columns(model, &existing);

    if !extra.is_empty() {
        tide_warn!(
            "TideORM table '{}' has column(s) the model no longer declares: {}. \
             Sync never drops columns - remove them with an explicit migration.",
            model.table_name,
            extra.join(", ")
        );
    }

    for column in missing {
        add_missing_column(conn, model, column, backend).await?;
        tide_info!(
            "Added column '{}' to TideORM table '{}'",
            column.name,
            model.table_name
        );
    }

    Ok(())
}

/// Split a model definition against the live column list.
///
/// Returns the model columns the table is missing, and the live columns the
/// model no longer declares. Names are compared case-insensitively because
/// backends fold unquoted identifiers differently.
fn diff_columns<'a>(
    model: &'a ModelSchema,
    existing: &'a [String],
) -> (Vec<&'a ColumnDef>, Vec<&'a str>) {
    let missing = model
        .columns
        .iter()
        .filter(|column| {
            !existing
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&column.name))
        })
        .collect();

    let extra = existing
        .iter()
        .map(String::as_str)
        .filter(|&name| {
            !model
                .columns
                .iter()
                .any(|column| column.name.eq_ignore_ascii_case(name))
        })
        .collect();

    (missing, extra)
}

async fn fetch_existing_columns(
    conn: &OrmConnection,
    schema: &str,
    table: &str,
    backend: Backend,
) -> Result<Vec<String>> {
    let statement = match backend {
        Backend::Postgres => build_statement_with_values(
            Backend::Postgres,
            "SELECT column_name FROM information_schema.columns WHERE table_schema = $1 AND table_name = $2",
            vec![schema.into(), table.into()],
        ),
        Backend::MySql => build_statement_with_values(
            Backend::MySql,
            "SELECT column_name FROM information_schema.columns WHERE table_schema = DATABASE() AND table_name = ?",
            vec![table.into()],
        ),
        Backend::Sqlite => build_statement_with_values(
            Backend::Sqlite,
            "SELECT name FROM pragma_table_info(?)",
            vec![table.into()],
        ),
    };

    let rows = conn
        .query_all_raw(statement)
        .await
        .map_err(|error| Error::query(error.to_string()))?;

    let mut columns = Vec::with_capacity(rows.len());
    for row in rows {
        let name: String = row.try_get_by_index(0).map_err(|error| {
            Error::query(format!(
                "Unable to read the column list of table '{}': {}",
                table, error
            ))
        })?;
        columns.push(name);
    }

    Ok(columns)
}

async fn add_missing_column(
    conn: &OrmConnection,
    model: &ModelSchema,
    col: &ColumnDef,
    backend: Backend,
) -> Result<()> {
    let mut column = OrmColumnDef::new(Alias::new(&col.name));
    let _ = apply_column_type(&mut column, &col.col_type, col.auto_increment, backend);

    if let Some(default) = &col.default {
        column.default(Expr::cust(default.clone()));
    }

    if !col.nullable && !col.auto_increment {
        if col.default.is_some() {
            column.not_null();
        } else {
            tide_warn!(
                "Column '{}' of table '{}' is declared NOT NULL without a default; \
                 adding it as nullable because existing rows cannot be backfilled. \
                 Use a migration to backfill and tighten the constraint.",
                col.name,
                model.table_name
            );
        }
    }

    let mut alter = Table::alter();
    match qualifying_schema(model, backend) {
        Some(schema) => {
            alter.table((Alias::new(schema), Alias::new(&model.table_name)));
        }
        None => {
            alter.table(Alias::new(&model.table_name));
        }
    }
    alter.add_column(&mut column);

    let sql = match backend {
        Backend::Postgres => alter.to_string(PostgresQueryBuilder),
        Backend::MySql => alter.to_string(MysqlQueryBuilder),
        Backend::Sqlite => alter.to_string(SqliteQueryBuilder),
    };

    let statement = build_statement(backend, sql);
    conn.execute_raw(statement).await.map_err(|error| {
        Error::query(format!(
            "Failed to add column '{}' to table '{}': {}",
            col.name, model.table_name, error
        ))
    })?;

    Ok(())
}

async fn check_table_exists(
    conn: &OrmConnection,
    schema: &str,
    table: &str,
    backend: Backend,
) -> Result<bool> {
    let statement = match backend {
        Backend::Postgres => build_statement_with_values(
            Backend::Postgres,
            "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_schema = $1 AND table_name = $2)",
            vec![schema.into(), table.into()],
        ),
        Backend::MySql => build_statement_with_values(
            Backend::MySql,
            "SELECT COUNT(*) > 0 FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = ?",
            vec![table.into()],
        ),
        Backend::Sqlite => build_statement_with_values(
            Backend::Sqlite,
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?",
            vec![table.into()],
        ),
    };

    let result = conn
        .query_one_raw(statement)
        .await
        .map_err(|error| Error::query(error.to_string()))?;

    match result {
        Some(row) => decode_table_exists(&row, table),
        None => Ok(false),
    }
}

/// Decode the single-column result of a table existence probe.
///
/// The three backends return three different SQL types for the same question:
/// PostgreSQL's `EXISTS` is a boolean, while MySQL evaluates `COUNT(*) > 0` to a
/// BIGINT and SQLite to a 64-bit integer - so pinning the width to `i32` makes
/// the decode fail on MySQL. A failure must surface rather than be read as
/// "table absent": swallowing it makes `force_sync` skip its `DROP` and turns
/// the whole run into a silent no-op.
fn decode_table_exists(row: &QueryResult, table: &str) -> Result<bool> {
    if let Ok(value) = row.try_get_by_index::<bool>(0) {
        return Ok(value);
    }

    if let Ok(value) = row.try_get_by_index::<i64>(0) {
        return Ok(value != 0);
    }

    if let Ok(value) = row.try_get_by_index::<u64>(0) {
        return Ok(value != 0);
    }

    if let Ok(value) = row.try_get_by_index::<i32>(0) {
        return Ok(value != 0);
    }

    Err(Error::query(format!(
        "Unable to decode the existence probe for table '{}'; refusing to \
         treat an unreadable answer as a missing table",
        table
    )))
}

async fn create_table_from_model_schema(
    conn: &OrmConnection,
    model: &ModelSchema,
    backend: Backend,
) -> Result<()> {
    let create_stmt = build_statement(backend, build_create_table_sql(model, backend));
    conn.execute_raw(create_stmt)
        .await
        .map_err(|error| Error::query(error.to_string()))?;

    Ok(())
}

/// Render the `CREATE TABLE` a model asks for.
///
/// Kept separate from execution so the rendered DDL - in particular the schema
/// qualification - can be asserted without a live database.
fn build_create_table_sql(model: &ModelSchema, backend: Backend) -> String {
    let mut table = Table::create();
    match qualifying_schema(model, backend) {
        Some(schema) => {
            table.table((Alias::new(schema), Alias::new(&model.table_name)));
        }
        None => {
            table.table(Alias::new(&model.table_name));
        }
    }
    let composite_primary_key = model.primary_keys.len() > 1;

    for col in &model.columns {
        let mut column = OrmColumnDef::new(Alias::new(&col.name));

        let can_auto_increment =
            apply_column_type(&mut column, &col.col_type, col.auto_increment, backend);

        if col.primary_key && !composite_primary_key {
            column.primary_key();
        }

        if col.auto_increment && can_auto_increment {
            column.auto_increment();
        }

        if (composite_primary_key || !col.primary_key) && !col.auto_increment && !col.nullable {
            column.not_null();
        }

        if let Some(ref default) = col.default {
            let default_owned = default.clone();
            column.default(Expr::cust(default_owned));
        }

        table.col(&mut column);
    }

    if composite_primary_key {
        let mut primary_key = Index::create();
        for column in &model.primary_keys {
            primary_key.col(Alias::new(column));
        }
        table.primary_key(&mut primary_key);
    }

    table.if_not_exists();

    match backend {
        Backend::Postgres => table.to_string(PostgresQueryBuilder),
        Backend::MySql => table.to_string(MysqlQueryBuilder),
        Backend::Sqlite => table.to_string(SqliteQueryBuilder),
    }
}

/// The native integer width an auto-increment column of this type needs.
///
/// `None` means the type cannot carry an auto-increment clause at all.
///
/// The width has to be the one [`ColumnType::to_sql`] would have rendered, or a
/// key column ends up wider than the plain column of the same Rust type - and
/// on PostgreSQL that difference is fatal, since `sea-orm` decodes a `u32` from
/// an `int4` and never from an `int8`. That is why [`ColumnType::Unsigned`]
/// asks for a regular integer rather than a big one.
#[derive(Clone, Copy)]
enum AutoIncrementWidth {
    Small,
    Regular,
    Big,
}

fn auto_increment_width(mapped: &ColumnType) -> Option<AutoIncrementWidth> {
    match mapped {
        ColumnType::SmallInteger | ColumnType::TinyUnsigned => Some(AutoIncrementWidth::Small),
        ColumnType::Integer | ColumnType::SmallUnsigned | ColumnType::Unsigned => {
            Some(AutoIncrementWidth::Regular)
        }
        ColumnType::BigInteger => Some(AutoIncrementWidth::Big),
        _ => None,
    }
}

/// Give `column` the SQL type the model's Rust type maps to.
///
/// No mapping decision is made here. The Rust type goes through
/// [`rust_type_to_column_type`] - the crate's single Rust-to-column table - and
/// the resulting logical type is rendered by [`ColumnType::to_sql`], the same
/// pair schema export and the migration builders use. Sync applies that
/// rendered SQL verbatim instead of re-deriving a type through `sea-query`, so
/// a `DB_SYNC`-built table and a migration-built table cannot disagree about
/// what a field becomes. Where the two vocabularies overlap the rendering is
/// deliberately kept in parity with `sea-query`'s - the drivers only bind and
/// decode what `sea-query`'s own DDL implies (`BINARY(16)` for a MySQL `Uuid`,
/// a REAL-affinity column for a SQLite decimal), so diverging breaks reads.
///
/// Auto-increment keys are the one exception: `sea-query` builds
/// `SERIAL`/`IDENTITY`/`AUTOINCREMENT` out of the column's *native* integer
/// type and panics on a custom one, so those keep the native builder.
///
/// Returns whether the column's type can carry an auto-increment clause; the
/// caller must not emit one otherwise.
fn apply_column_type(
    column: &mut OrmColumnDef,
    rust_type: &str,
    auto_increment: bool,
    backend: Backend,
) -> bool {
    let mapped = rust_type_to_column_type(rust_type).unwrap_or_else(|| {
        tide_warn!(
            "Unknown Rust type '{}' mapped to a TEXT column. Consider adding an explicit type mapping.",
            rust_type
        );
        ColumnType::Text
    });

    let width = auto_increment_width(&mapped);

    if auto_increment {
        match width {
            Some(AutoIncrementWidth::Small) => {
                column.small_integer();
            }
            Some(AutoIncrementWidth::Regular) => {
                column.integer();
            }
            Some(AutoIncrementWidth::Big) => {
                column.big_integer();
            }
            None => {
                tide_warn!(
                    "Column type '{}' cannot auto-increment; creating the column without it.",
                    rust_type
                );
                column.custom(Alias::new(mapped.to_sql(backend.as_database_type())));
            }
        }

        return width.is_some();
    }

    column.custom(Alias::new(mapped.to_sql(backend.as_database_type())));

    width.is_some()
}

/// Normalizes a Rust type string by removing whitespace
pub fn normalize_rust_type(rust_type: &str) -> String {
    rust_type.chars().filter(|ch| !ch.is_whitespace()).collect()
}

#[cfg(test)]
#[path = "../../tests/unit/sync_schema_tests.rs"]
mod tests;
