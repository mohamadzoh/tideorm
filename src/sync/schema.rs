use crate::config::DatabaseType;
use crate::database::Database;
use crate::error::{Error, ErrorContext, Result};
use crate::internal::sql_safety::quote_qualified;
use crate::internal::{ConnectionTrait, OrmConnection, build_statement, translate_error};
use crate::migration::ddl::{self, ColumnDefinition};
use crate::migration::{ColumnType, StringLen, can_auto_increment, column_type_sql};
use crate::model::IndexDefinition;
use crate::schema::{ColumnSchema, TableSchema, catalog};
use crate::{tide_debug, tide_info, tide_warn};

use super::SyncRegistry;

/// Column definition for TideORM schema synchronization
#[derive(Debug, Clone)]
pub struct ColumnDef {
    /// Column name
    pub name: String,
    /// Column type: the model derive passes `<T as ValueType>::column_type()`
    /// of the field's type `T`.
    pub col_type: ColumnType,
    /// Whether the column allows NULL values
    pub nullable: bool,
    /// Whether this is the primary key
    pub primary_key: bool,
    /// Whether this column auto-increments
    pub auto_increment: bool,
    /// Default value expression (if any)
    pub default: Option<String>,
}

crate::schema::column_declaration_methods!(ColumnDef, col_type: ColumnType);

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
    #[must_use]
    pub fn schema(mut self, schema: impl Into<String>) -> Self {
        self.schema_name = schema.into();
        self
    }

    /// Add a column definition
    #[must_use]
    pub fn column(mut self, col: ColumnDef) -> Self {
        self.columns.push(col);
        self
    }

    /// Set the model primary keys.
    #[must_use]
    pub fn primary_keys(mut self, columns: Vec<String>) -> Self {
        self.primary_keys = columns;
        self
    }

    /// Set the indexes sync creates on the table.
    #[must_use]
    pub fn indexes(mut self, indexes: Vec<IndexDefinition>) -> Self {
        self.indexes = indexes;
        self
    }

    /// The table sync would create for this model on `db_type`, for a
    /// [`SchemaGenerator`](crate::schema::SchemaGenerator) to render.
    pub(crate) fn table_schema(&self, db_type: DatabaseType) -> TableSchema {
        let backend = db_type;
        let primary_keys = primary_key_columns(self);
        let columns = self
            .columns
            .iter()
            .map(|column| {
                let column_type = keyed_column_type(self, column, backend);
                ColumnSchema {
                    name: column.name.clone(),
                    sql_type: column_type_sql(&column_type, db_type),
                    nullable: column.nullable,
                    default: column.default.clone(),
                    primary_key: primary_keys.len() == 1 && primary_keys.contains(&column.name),
                    auto_increment: column.auto_increment && can_auto_increment(&column_type),
                }
            })
            .collect();
        TableSchema {
            name: self.table_name.clone(),
            schema_name: qualifying_schema(self, backend).map(str::to_string),
            columns,
            indexes: self.indexes.clone(),
            primary_keys,
        }
    }
}

pub(super) async fn sync_model_schemas(db: &Database, force_sync: bool) -> Result<()> {
    let models = SyncRegistry::get_all_schemas();
    let conn = db.__internal_connection()?;
    let backend = conn.get_database_backend();

    let mut existing = Vec::with_capacity(models.len());
    for model in &models {
        let schema = qualifying_schema(model, backend);
        existing.push(catalog::table_exists(&conn, backend, schema, &model.table_name).await?);
    }
    // Every drop runs before any create, so a table is dropped before the
    // tables it references, whatever order the models registered in.
    if force_sync {
        super::recreate::drop_tables(&conn, backend, &models, &existing).await?;
    }

    for (model, table_exists) in models.iter().zip(existing) {
        let created = !table_exists || force_sync;
        if created {
            execute_ddl(
                &conn,
                backend,
                model,
                build_create_table_sql(model, backend),
            )
            .await?;
            tide_info!("Created TideORM table: {}", model.table_name);
        } else {
            tide_debug!("TideORM table exists: {}", model.table_name);
            reconcile_existing_table(&conn, model, backend).await?;
        }
        sync_indexes(&conn, model, backend, created).await?;
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
    backend: DatabaseType,
    new_table: bool,
) -> Result<()> {
    for index in &model.indexes {
        check_index_owner(conn, model, backend, &index.name).await?;
        // MySQL has no `CREATE INDEX IF NOT EXISTS`, so ask the catalog first.
        if matches!(backend, DatabaseType::MySQL | DatabaseType::MariaDB)
            && catalog::mysql_index_exists(
                conn,
                mysql_database(&model.schema_name),
                &model.table_name,
                &index.name,
            )
            .await?
        {
            continue;
        }

        let sql = ddl::create_index(
            backend,
            &index.name,
            &table_reference(model, backend),
            &index.columns,
            index.unique,
            !new_table,
        );
        match execute_ddl(conn, backend, model, sql).await {
            Ok(()) => tide_debug!(
                "Ensured index '{}' on TideORM table '{}'",
                index.name,
                model.table_name
            ),
            Err(error) if !new_table => tide_warn!(
                "Could not create index '{}' on TideORM table '{}': {}. \
                 Resolve the conflicting rows or create it with a migration.",
                index.name,
                model.table_name,
                error
            ),
            Err(error) => return Err(error),
        }
    }

    Ok(())
}

/// Index names are schema-wide on SQLite and PostgreSQL. IF NOT EXISTS
/// otherwise hides a collision with another table, including on repeated syncs.
async fn check_index_owner(
    conn: &OrmConnection,
    model: &ModelSchema,
    backend: DatabaseType,
    index: &str,
) -> Result<()> {
    let owner =
        catalog::index_table(conn, backend, qualifying_schema(model, backend), index).await?;
    if let Some(owner) = owner {
        let matches = if backend == DatabaseType::SQLite {
            owner.eq_ignore_ascii_case(&model.table_name)
        } else {
            owner == model.table_name
        };
        if !matches {
            return Err(Error::configuration(format!(
                "Index '{index}' belongs to table '{owner}', not '{}'; choose a distinct index name",
                model.table_name
            )));
        }
    }
    Ok(())
}

/// Run one DDL statement against `model`'s table, keeping the statement and
/// the table on the error.
pub(super) async fn execute_ddl(
    conn: &OrmConnection,
    backend: DatabaseType,
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
pub(super) fn qualifying_schema(model: &ModelSchema, backend: DatabaseType) -> Option<&str> {
    match backend {
        DatabaseType::Postgres if !model.schema_name.is_empty() => Some(&model.schema_name),
        DatabaseType::MySQL | DatabaseType::MariaDB => mysql_database(&model.schema_name),
        _ => None,
    }
}

/// The model's table as every statement sync issues names it.
pub(super) fn table_reference(model: &ModelSchema, backend: DatabaseType) -> String {
    quote_qualified(
        backend,
        qualifying_schema(model, backend),
        &model.table_name,
    )
}

/// Bring an existing table in line with its model definition, additively.
///
/// Columns the model declares but the table lacks are added with
/// `ALTER TABLE ... ADD COLUMN`. Columns the table carries but the model no
/// longer declares are only reported - sync never drops a column, because that
/// destroys data. A column declared `NOT NULL` without a default is added only
/// to an empty table, since existing rows have nothing to fill it with.
async fn reconcile_existing_table(
    conn: &OrmConnection,
    model: &ModelSchema,
    backend: DatabaseType,
) -> Result<()> {
    let existing = catalog::column_names(
        conn,
        backend,
        qualifying_schema(model, backend),
        &model.table_name,
    )
    .await?;

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

async fn add_missing_column(
    conn: &OrmConnection,
    model: &ModelSchema,
    col: &ColumnDef,
    backend: DatabaseType,
) -> Result<()> {
    let mut column = ColumnDefinition::new(&col.name, keyed_column_type(model, col, backend));
    column.default = col.default.clone();

    if backend == DatabaseType::SQLite
        && col
            .default
            .as_deref()
            .is_some_and(|default| !ddl::sqlite_can_add_with_default(default))
    {
        tide_warn!(
            "SQLite cannot add column '{}' to table '{}' with its default {}; \
             adding it without one. Use a migration that rebuilds the table to \
             keep the default.",
            col.name,
            model.table_name,
            col.default.as_deref().unwrap_or_default()
        );
        column.default = None;
    }
    if !col.nullable && !col.auto_increment {
        if column.default.is_some() {
            column.nullable = false;
        } else if table_has_rows(conn, model, backend).await? {
            // Added as nullable, its rows would hold NULL, which the model's
            // field cannot read.
            return Err(Error::configuration(format!(
                "Sync cannot add the NOT NULL column '{}' to table '{}', which has rows \
                 and no value for it. Give the field a default (#[tideorm(default = \"..\")]), \
                 make it an Option, or add it with a migration that fills it in.",
                col.name, model.table_name
            ))
            .with_context(ErrorContext::new().table(model.table_name.as_str())));
        } else if backend == DatabaseType::SQLite {
            tide_warn!(
                "SQLite adds a NOT NULL column only with a default; adding '{}' to the \
                 empty table '{}' as nullable. Use a migration to keep the constraint.",
                col.name,
                model.table_name
            );
        } else {
            column.nullable = false;
        }
    }

    let sql = ddl::add_column(backend, &table_reference(model, backend), &column);
    execute_ddl(conn, backend, model, sql).await
}

/// Whether `model`'s table holds a row.
async fn table_has_rows(
    conn: &OrmConnection,
    model: &ModelSchema,
    backend: DatabaseType,
) -> Result<bool> {
    let sql = format!("SELECT 1 FROM {} LIMIT 1", table_reference(model, backend));
    let row = conn
        .query_one_raw(build_statement(backend, sql.as_str()))
        .await
        .map_err(|error| {
            translate_error(error).with_context(
                ErrorContext::new()
                    .table(model.table_name.as_str())
                    .query(sql.as_str()),
            )
        })?;
    Ok(row.is_some())
}

/// Render the `CREATE TABLE` a model asks for.
///
/// This is the migration builders' DDL, so a synced table and a migrated one
/// agree column for column. Kept separate from execution so the rendered DDL -
/// in particular the schema qualification - can be asserted without a live
/// database.
fn build_create_table_sql(model: &ModelSchema, backend: DatabaseType) -> String {
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
        backend,
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
    let auto_increment = col.auto_increment && can_auto_increment(&column_type);
    if col.auto_increment && !auto_increment {
        tide_warn!(
            "Column type '{:?}' cannot auto-increment; creating the column without it.",
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
fn keyed_column_type(model: &ModelSchema, col: &ColumnDef, backend: DatabaseType) -> ColumnType {
    let column_type = col.col_type.clone();
    let keyed = primary_key_columns(model).contains(&col.name)
        || model
            .indexes
            .iter()
            .any(|index| index.columns.contains(&col.name));
    if matches!(backend, DatabaseType::MySQL | DatabaseType::MariaDB)
        && keyed
        && matches!(column_type, ColumnType::Text)
    {
        ColumnType::String(StringLen::None)
    } else {
        column_type
    }
}

#[cfg(test)]
#[path = "../../tests/unit/sync_schema_tests.rs"]
mod tests;
