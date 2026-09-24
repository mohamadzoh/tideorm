use crate::database::Database;
use crate::error::{Error, ErrorContext, Result};
use crate::internal::sql_safety::quote_ident_for_backend;
use crate::internal::{
    Backend, ConnectionTrait, OrmConnection, QueryResult, build_statement,
    build_statement_with_values, translate_error,
};
use crate::migration::ColumnType;
use crate::migration::ddl::{self, ColumnDefinition};
use crate::model::IndexDefinition;
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
    /// The model's `#[index]` and `#[unique_index]` declarations.
    pub indexes: Vec<IndexDefinition>,
}

/// The schema a [`ModelSchema`] names until [`ModelSchema::schema`] sets one.
const DEFAULT_SCHEMA: &str = "public";

/// The database a MySQL catalog probe or statement has to name for `schema`:
/// the declared one, or `None` for the connected database. MySQL's schema *is*
/// a database, and the PostgreSQL-flavoured default names none.
fn mysql_database(schema: &str) -> Option<&str> {
    (!schema.is_empty() && schema != DEFAULT_SCHEMA).then_some(schema)
}

impl ModelSchema {
    /// Create a new model schema
    pub fn new(table_name: impl Into<String>) -> Self {
        Self {
            table_name: table_name.into(),
            schema_name: DEFAULT_SCHEMA.to_string(),
            columns: Vec::new(),
            primary_keys: Vec::new(),
            indexes: Vec::new(),
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

    /// Set the model primary keys.
    pub fn primary_keys(mut self, columns: Vec<String>) -> Self {
        self.primary_keys = columns;
        self
    }

    /// Set the indexes sync creates on the table.
    pub fn indexes(mut self, indexes: Vec<IndexDefinition>) -> Self {
        self.indexes = indexes;
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
            let table = table_reference(&model, backend);
            let drop_sql = if qualifying_schema(&model, backend).is_some() {
                format!("DROP TABLE IF EXISTS {} CASCADE", table)
            } else {
                format!("DROP TABLE IF EXISTS {}", table)
            };

            execute_ddl(&conn, backend, &model, drop_sql).await?;
            tide_warn!("Dropped TideORM table: {}", model.table_name);
        }

        let created = !table_exists || force_sync;
        if created {
            execute_ddl(
                &conn,
                backend,
                &model,
                build_create_table_sql(&model, backend),
            )
            .await?;
            tide_info!("Created TideORM table: {}", model.table_name);
        } else {
            tide_debug!("TideORM table exists: {}", model.table_name);
            reconcile_existing_table(&conn, &model, backend).await?;
        }
        sync_indexes(&conn, &model, backend, created).await?;
    }

    Ok(())
}

/// Create the model's declared indexes the table lacks.
///
/// On a table sync has just created, a failure is an error: the model's own
/// declaration cannot be applied. On an existing table a unique index can be
/// blocked by rows that already break it, and sync never touches data, so that
/// is reported and the rest of the sync carries on.
async fn sync_indexes(
    conn: &OrmConnection,
    model: &ModelSchema,
    backend: Backend,
    new_table: bool,
) -> Result<()> {
    for index in &model.indexes {
        // MySQL has no `CREATE INDEX IF NOT EXISTS`, so ask the catalog first.
        if backend == Backend::MySql && mysql_index_exists(conn, model, &index.name).await? {
            continue;
        }

        let sql = ddl::create_index(
            backend.as_database_type(),
            &index.name,
            &table_reference(model, backend),
            &index.columns,
            index.unique,
            true,
        );
        match execute_ddl(conn, backend, model, sql).await {
            Ok(()) => tide_debug!(
                "Ensured index '{}' on TideORM table '{}'",
                index.name,
                model.table_name
            ),
            Err(error) if !new_table => tide_warn!(
                "Could not create index '{}' on TideORM table '{}': {}.                  Resolve the conflicting rows or create it with a migration.",
                index.name,
                model.table_name,
                error
            ),
            Err(error) => return Err(error),
        }
    }

    Ok(())
}

async fn mysql_index_exists(
    conn: &OrmConnection,
    model: &ModelSchema,
    index: &str,
) -> Result<bool> {
    let statement = build_statement_with_values(
        Backend::MySql,
        "SELECT COUNT(*) > 0 FROM information_schema.statistics          WHERE table_schema = COALESCE(?, DATABASE()) AND table_name = ? AND index_name = ?",
        vec![
            mysql_database(&model.schema_name).into(),
            model.table_name.as_str().into(),
            index.into(),
        ],
    );
    let row = conn.query_one_raw(statement).await.map_err(|error| {
        translate_error(error).with_context(ErrorContext::new().table(model.table_name.as_str()))
    })?;

    match row {
        Some(row) => decode_table_exists(&row, &model.table_name),
        None => Ok(false),
    }
}

/// Run one DDL statement against `model`'s table, keeping the statement and
/// the table on the error.
async fn execute_ddl(
    conn: &OrmConnection,
    backend: Backend,
    model: &ModelSchema,
    sql: String,
) -> Result<()> {
    conn.execute_raw(build_statement(backend, sql.as_str()))
        .await
        .map_err(|error| {
            translate_error(error).with_context(
                ErrorContext::new()
                    .table(model.table_name.as_str())
                    .query(sql),
            )
        })?;

    Ok(())
}

/// The schema a model's DDL has to be qualified with, if any.
///
/// PostgreSQL qualifies with the model's schema, `public` by default. On MySQL
/// the schema is a database, named only when the model declared one; SQLite
/// has none. The catalog probes resolve the same way, so the table is created
/// where they look for it - and where the model's queries go.
///
/// Every statement that names the table has to go through this, or `CREATE
/// TABLE` lands somewhere the existence probe and the force `DROP` never look.
fn qualifying_schema(model: &ModelSchema, backend: Backend) -> Option<&str> {
    match backend {
        Backend::Postgres if !model.schema_name.is_empty() => Some(&model.schema_name),
        Backend::MySql => mysql_database(&model.schema_name),
        _ => None,
    }
}

/// The model's table as every statement sync issues names it.
fn table_reference(model: &ModelSchema, backend: Backend) -> String {
    let table = quote_ident_for_backend(backend, &model.table_name);

    match qualifying_schema(model, backend) {
        Some(schema) => format!("{}.{}", quote_ident_for_backend(backend, schema), table),
        None => table,
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
            "SELECT column_name::text FROM information_schema.columns WHERE table_schema = $1 AND table_name = $2",
            vec![schema.into(), table.into()],
        ),
        Backend::MySql => build_statement_with_values(
            Backend::MySql,
            "SELECT column_name FROM information_schema.columns WHERE table_schema = COALESCE(?, DATABASE()) AND table_name = ?",
            vec![mysql_database(schema).into(), table.into()],
        ),
        Backend::Sqlite => build_statement_with_values(
            Backend::Sqlite,
            "SELECT name FROM pragma_table_info(?)",
            vec![table.into()],
        ),
    };

    let table_context = || ErrorContext::new().table(table);
    let rows = conn
        .query_all_raw(statement)
        .await
        .map_err(|error| translate_error(error).with_context(table_context()))?;

    rows.iter()
        .map(|row| {
            row.try_get_by_index(0)
                .map_err(|error| translate_error(error).with_context(table_context()))
        })
        .collect()
}

async fn add_missing_column(
    conn: &OrmConnection,
    model: &ModelSchema,
    col: &ColumnDef,
    backend: Backend,
) -> Result<()> {
    let mut column = ColumnDefinition::new(&col.name, keyed_column_type(model, col, backend));
    column.default = col.default.clone();

    if !col.nullable && !col.auto_increment {
        if col.default.is_some() {
            column.nullable = false;
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

    let sql = ddl::add_column(
        backend.as_database_type(),
        &table_reference(model, backend),
        &column,
    );
    execute_ddl(conn, backend, model, sql).await
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
            "SELECT COUNT(*) > 0 FROM information_schema.tables WHERE table_schema = COALESCE(?, DATABASE()) AND table_name = ?",
            vec![mysql_database(schema).into(), table.into()],
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
        .map_err(|error| translate_error(error).with_context(ErrorContext::new().table(table)))?;

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

/// Render the `CREATE TABLE` a model asks for.
///
/// This is the migration builders' DDL, so a synced table and a migrated one
/// agree column for column. Kept separate from execution so the rendered DDL -
/// in particular the schema qualification - can be asserted without a live
/// database.
fn build_create_table_sql(model: &ModelSchema, backend: Backend) -> String {
    let primary_key = primary_key_columns(model);
    let single_key = primary_key.len() == 1;
    let columns: Vec<ColumnDefinition> = model
        .columns
        .iter()
        .map(|column| {
            column_definition(
                column,
                keyed_column_type(model, column, backend),
                single_key && primary_key.contains(&column.name),
            )
        })
        .collect();

    ddl::create_table(
        backend.as_database_type(),
        &table_reference(model, backend),
        true,
        &columns,
        &primary_key,
        &[],
    )
}

/// The model's primary key: its declared key columns, or else the columns
/// flagged as keys.
fn primary_key_columns(model: &ModelSchema) -> Vec<String> {
    if !model.primary_keys.is_empty() {
        return model.primary_keys.clone();
    }

    model
        .columns
        .iter()
        .filter(|column| column.primary_key)
        .map(|column| column.name.clone())
        .collect()
}

/// The DDL column a model column becomes.
///
/// `single_key` says whether the column is the table's one-column primary
/// key. A composite key's columns keep an explicit `NOT NULL`, which SQLite
/// does not imply for them.
fn column_definition(
    col: &ColumnDef,
    column_type: ColumnType,
    single_key: bool,
) -> ColumnDefinition {
    let auto_increment = col.auto_increment && column_type.can_auto_increment();
    if col.auto_increment && !auto_increment {
        tide_warn!(
            "Column type '{}' cannot auto-increment; creating the column without it.",
            col.col_type
        );
    }

    let mut column = ColumnDefinition::new(&col.name, column_type);
    column.nullable = col.nullable;
    column.default = col.default.clone();
    column.primary_key = single_key;
    column.auto_increment = auto_increment;
    column
}

/// The type sync creates `col` with.
///
/// MySQL cannot make a `TEXT` column a key, or index it, without a prefix
/// length, so on MySQL a string column in the primary key or in an index is a
/// `VARCHAR(255)`.
fn keyed_column_type(model: &ModelSchema, col: &ColumnDef, backend: Backend) -> ColumnType {
    let column_type = column_type(col);
    let keyed = primary_key_columns(model).contains(&col.name)
        || model
            .indexes
            .iter()
            .any(|index| index.columns.contains(&col.name));
    if backend == Backend::MySql && keyed && matches!(column_type, ColumnType::Text) {
        ColumnType::String
    } else {
        column_type
    }
}

/// The logical type a model column's Rust type maps to, through the crate's
/// single Rust-to-column table; `TEXT` for a type the table does not know.
fn column_type(col: &ColumnDef) -> ColumnType {
    rust_type_to_column_type(&col.col_type).unwrap_or_else(|| {
        tide_warn!(
            "Unknown Rust type '{}' mapped to a TEXT column. Consider adding an explicit type mapping.",
            col.col_type
        );
        ColumnType::Text
    })
}

#[cfg(test)]
#[path = "../../tests/unit/sync_schema_tests.rs"]
mod tests;
