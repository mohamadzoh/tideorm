//! Database Schema Synchronization Module
//!
//! This module applies model or entity schema definitions to a live database.
//!
//! Use it in local development and tests when you want missing tables or
//! columns created automatically. It is not a replacement for migrations in
//! deployed environments.
//!
//! ## Two Synchronization Approaches
//!
//! ### 1. TideORM Models
//!
//! TideORM models use `ModelSchema`, which the macros generate for you.
//!
//! Register TideORM models through `TideConfig::models::<(... )>()` and enable
//! synchronization with `sync(true)`.
//!
//! ### 2. ORM Entities
//!
//! ORM entities can be registered through
//! `SyncRegistry::register_entity::<E>()`.
//!
//! Register entity types with `SyncRegistry::register_entity::<Entity>()`.
//!
//! When sync fails, inspect the reported SQL/backend error first. Common causes
//! are unsupported type changes, existing tables with incompatible columns, or a
//! production database being pointed at a development-only sync path.
//!
//! ## Sync Modes
//!
//! ### Normal Sync (`sync(true)`)
//!
//! - Creates missing tables
//! - Adds missing columns to existing tables
//! - Creates indexes defined in models
//! - Creates foreign keys
//! - Creates enums (PostgreSQL)
//! - **Does NOT drop existing tables or columns**
//!
//! ### Force Sync (`force_sync(true)`)
//!
//! - For registered entities: uses `apply` mode and fails if tables already exist
//! - For TideORM models: drops and recreates tables
//!
//! ## ⚠️ Warning
//!
//! **Do not use sync mode in production.** It can still fail or damage data when
//! a schema change is not additive. Use explicit migrations for deployed systems.
//!
//! **Do not use force_sync in production.** It deletes tables and their data.
//!
//! Enable synchronization with `TideConfig::init().sync(true)` or call
//! `Database::sync()` directly after connecting.

use parking_lot::RwLock;
use std::sync::OnceLock;

use crate::database::Database;
use crate::error::{Error, Result};
use crate::internal::sql_safety::quote_ident_for_backend;
use crate::internal::{
    Alias, Backend, ConnectionTrait, EntityTrait, Expr, Index, MysqlQueryBuilder, OrmColumnDef,
    OrmColumnType, OrmConnection, PostgresQueryBuilder, Schema, SchemaBuilder, SqliteQueryBuilder,
    Table, build_statement, build_statement_with_values,
};
use crate::{tide_debug, tide_info, tide_warn};

mod registry;

pub use registry::{RegisterModels, SyncModel};
use registry::{get_entity_registry, get_model_schemas};

/// Type alias for entity registration functions that register with SchemaBuilder
pub type EntityRegistrationFn = Box<dyn Fn(SchemaBuilder) -> SchemaBuilder + Send + Sync>;

/// Registry for models to be synchronized using the ORM schema builder.
pub struct SyncRegistry;

impl SyncRegistry {
    /// Register an entity type for synchronization using the ORM schema builder.
    ///
    /// This stores a registration function that will call SchemaBuilder.register()
    /// when sync is performed.
    pub fn register_entity<E: EntityTrait + Default + 'static>() {
        let registry = get_entity_registry();
        let mut fns = registry.write();

        // Create a registration function for this entity type
        let register_fn: EntityRegistrationFn =
            Box::new(|builder: SchemaBuilder| builder.register(E::default()));

        fns.push(register_fn);
    }

    /// Build a SchemaBuilder with all registered entities
    ///
    /// Uses the current ORM engine's native SchemaBuilder.register() for each entity.
    pub fn build_schema_builder(backend: Backend) -> SchemaBuilder {
        let registry = get_entity_registry();
        let fns = registry.read();

        let schema = Schema::new(backend.into());
        let mut builder = schema.builder();

        for register_fn in fns.iter() {
            builder = register_fn(builder);
        }

        builder
    }

    /// Get the number of registered entities
    pub fn entity_count() -> usize {
        let registry = get_entity_registry();
        let fns = registry.read();
        fns.len()
    }

    /// Get the number of registered TideORM model schemas
    pub fn schema_count() -> usize {
        let direct = get_model_schemas();
        let schemas = direct.read();
        schemas.len()
    }

    /// Clear all registered models (for testing)
    pub fn clear() {
        let registry = get_entity_registry();
        let mut fns = registry.write();
        fns.clear();

        let direct = get_model_schemas();
        let mut schemas = direct.write();
        schemas.clear();
    }

    /// Register a TideORM model schema for synchronization
    pub fn register_schema(schema: ModelSchema) {
        let direct = get_model_schemas();
        let mut schemas = direct.write();

        if !schemas.iter().any(|s| s.table_name == schema.table_name) {
            schemas.push(schema);
        }
    }

    /// Get all registered TideORM model schemas
    pub fn get_all_schemas() -> Vec<ModelSchema> {
        let direct = get_model_schemas();
        let schemas = direct.read();
        schemas.clone()
    }
}

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

// ============================================================================
// Main sync functions using the ORM schema builder
// ============================================================================

/// Synchronize all registered models with the database.
///
/// This uses the ORM engine's built-in schema sync to:
/// 1. Create missing tables
/// 2. Add missing columns to existing tables
/// 3. Create indexes and foreign keys
/// 4. Create enums (PostgreSQL)
///
/// # Arguments
///
/// * `db` - Database connection
///
/// # Returns
///
/// Returns `Ok(())` on success, or an Error on failure
///
/// # Warning
///
/// **DO NOT use in production!** This is for development only.
/// Use proper migrations for production deployments.
pub async fn sync_database(db: &Database) -> Result<()> {
    sync_database_with_options(db, false).await
}

/// Synchronize all registered models with force_sync option
///
/// This uses ORM schema management with additional options.
///
/// # Arguments
///
/// * `db` - Database connection
/// * `force_sync` - If true, uses `apply` instead of `sync` (fresh creation, fails if exists)
///
/// # Returns
///
/// Returns `Ok(())` on success, or an Error on failure
///
/// # ⚠️ DANGER
///
/// **DO NOT use in production!** This is for development only.
/// When `force_sync` is enabled, apply mode is used which expects tables to not exist.
pub async fn sync_database_with_options(db: &Database, force_sync: bool) -> Result<()> {
    if force_sync {
        tide_warn!("Database FORCE sync mode is ENABLED - using schema apply mode!");
    } else {
        tide_warn!("Database sync mode is ENABLED - DO NOT use in production!");
    }

    let conn = db.__internal_connection()?;
    let backend = conn.get_database_backend();

    let entity_count = SyncRegistry::entity_count();
    let schema_count = SyncRegistry::schema_count();
    let total_count = entity_count + schema_count;

    if total_count == 0 {
        tide_info!("No models registered for sync");
        return Ok(());
    }

    tide_info!(
        "Syncing {} model(s) using the ORM schema builder...",
        total_count
    );
    tide_debug!("  - {} entity-based models", entity_count);
    tide_debug!("  - {} TideORM schema models", schema_count);

    // Build SchemaBuilder with all registered entities
    if entity_count > 0 {
        let schema_builder = SyncRegistry::build_schema_builder(Backend::from(backend));

        #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
        // Use the ORM engine's sync/apply based on force_sync.
        if force_sync {
            tide_debug!("  Using SchemaBuilder.apply() - fresh schema creation");
            schema_builder
                .apply(&conn)
                .await
                .map_err(|e| Error::query(format!("Schema apply failed: {}", e)))?;
        } else {
            tide_debug!("  Using SchemaBuilder.sync() - incremental sync");
            schema_builder
                .sync(&conn)
                .await
                .map_err(|e| Error::query(format!("Schema sync failed: {}", e)))?;
        }

        #[cfg(not(any(feature = "postgres", feature = "mysql", feature = "sqlite")))]
        {
            let _ = schema_builder;
            return Err(Error::configuration(
                "database sync requires at least one backend feature: postgres, mysql, or sqlite",
            ));
        }
    }

    // Handle TideORM model schemas if any
    if schema_count > 0 {
        tide_debug!("  Processing {} TideORM schema(s)...", schema_count);
        sync_model_schemas(db, force_sync).await?;
    }

    tide_info!("Database sync completed");
    Ok(())
}

/// Sync TideORM ModelSchema definitions.
async fn sync_model_schemas(db: &Database, force_sync: bool) -> Result<()> {
    let models = SyncRegistry::get_all_schemas();
    let conn = db.__internal_connection()?;
    let backend = Backend::from(conn.get_database_backend());

    for model in models {
        let table_exists =
            check_table_exists(&conn, &model.schema_name, &model.table_name, backend).await?;

        if force_sync && table_exists {
            // Drop existing table for force sync
            let quoted_table = quote_ident_for_backend(backend, &model.table_name);
            let drop_sql = match backend {
                Backend::Postgres => format!(
                    "DROP TABLE IF EXISTS {}.{} CASCADE",
                    quote_ident_for_backend(backend, &model.schema_name),
                    quoted_table
                ),
                _ => format!("DROP TABLE IF EXISTS {}", quoted_table),
            };

            let drop_stmt = build_statement(backend, drop_sql);
            conn.execute_raw(drop_stmt)
                .await
                .map_err(|e| Error::query(e.to_string()))?;

            tide_warn!("Dropped TideORM table: {}", model.table_name);
        }

        if !table_exists || force_sync {
            // Create new table
            create_table_from_model_schema(&conn, &model, backend).await?;
            tide_info!("Created TideORM table: {}", model.table_name);
        } else {
            tide_debug!("TideORM table exists: {}", model.table_name);
        }
    }

    Ok(())
}

/// Check if a table exists in the database
async fn check_table_exists(
    conn: &OrmConnection,
    schema: &str,
    table: &str,
    backend: Backend,
) -> Result<bool> {
    let stmt = match backend {
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
        .query_one_raw(stmt)
        .await
        .map_err(|e| Error::query(e.to_string()))?;

    match result {
        Some(row) => {
            let exists: bool = match backend {
                Backend::Postgres => row.try_get_by_index(0).unwrap_or(false),
                _ => {
                    let val: i32 = row.try_get_by_index(0).unwrap_or(0);
                    val > 0
                }
            };
            Ok(exists)
        }
        None => Ok(false),
    }
}

/// Create a table from a TideORM ModelSchema definition
async fn create_table_from_model_schema(
    conn: &OrmConnection,
    model: &ModelSchema,
    backend: Backend,
) -> Result<()> {
    let mut table = Table::create();
    table.table(Alias::new(&model.table_name));
    let composite_primary_key = model.primary_keys.len() > 1;

    // Add columns using the ORM query builder's column definitions
    for col in &model.columns {
        let mut column = OrmColumnDef::new(Alias::new(&col.name));

        // Set column type based on Rust type
        apply_column_type(&mut column, &col.col_type, col.auto_increment, backend);

        if col.primary_key && !composite_primary_key {
            column.primary_key();
        }

        if col.auto_increment {
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

    // Build the SQL using backend-specific query builder
    let sql = match backend {
        Backend::Postgres => table.to_string(PostgresQueryBuilder),
        Backend::MySql => table.to_string(MysqlQueryBuilder),
        Backend::Sqlite => table.to_string(SqliteQueryBuilder),
    };

    let create_stmt = build_statement(backend, sql);
    conn.execute_raw(create_stmt)
        .await
        .map_err(|e| Error::query(e.to_string()))?;

    Ok(())
}

/// Apply column type based on Rust type string
fn apply_column_type(
    column: &mut OrmColumnDef,
    rust_type: &str,
    _auto_increment: bool,
    _backend: Backend,
) {
    // Normalize type (remove whitespace from stringify!)
    let normalized = normalize_rust_type(rust_type);

    // Extract inner type for Option<T>
    let inner_type = normalized
        .strip_prefix("Option<")
        .and_then(|s| s.strip_suffix(">"))
        .unwrap_or(&normalized);
    let inner_type = canonical_schema_type(inner_type);

    // Map Rust types to ORM column types
    match inner_type.as_str() {
        "i8" | "u8" | "i16" | "u16" => {
            column.small_integer();
        }
        "i32" => {
            column.integer();
        }
        "u32" | "i64" => {
            column.big_integer();
        }
        "u64" | "i128" | "u128" => {
            column.decimal();
        }
        "isize" | "usize" => {
            column.big_integer();
        }
        "f32" => {
            column.float();
        }
        "f64" => {
            column.double();
        }
        "bool" => {
            column.boolean();
        }
        "String" | "&str" => {
            column.text();
        }
        "Uuid" => {
            column.uuid();
        }
        "Json" | "JsonValue" | "serde_json::Value" | "Value" | "Jsonb" => {
            column.json_binary();
        }
        "Vec<u8>" | "Bytes" => {
            column.binary();
        }
        "Decimal" | "BigDecimal" => {
            column.decimal();
        }
        t if t.contains("DateTime") => {
            column.timestamp_with_time_zone();
        }
        t if t.contains("NaiveDateTime") => {
            column.timestamp();
        }
        t if t.contains("NaiveDate") => {
            column.date();
        }
        t if t.contains("NaiveTime") => {
            column.time();
        }
        // Array types (PostgreSQL specific)
        "Vec<i32>" | "IntArray" => {
            column.array(OrmColumnType::Integer);
        }
        "Vec<i64>" | "BigIntArray" => {
            column.array(OrmColumnType::BigInteger);
        }
        "Vec<String>" | "TextArray" => {
            column.array(OrmColumnType::Text);
        }
        "Vec<bool>" | "BoolArray" => {
            column.array(OrmColumnType::Boolean);
        }
        "Vec<f64>" | "FloatArray" => {
            column.array(OrmColumnType::Double);
        }
        unknown_type => {
            tide_warn!(
                "Unknown Rust type '{}' mapped to TEXT column. Consider adding explicit type mapping.",
                unknown_type
            );
            column.text();
        }
    };
}

fn canonical_schema_type(rust_type: &str) -> String {
    let normalized = rust_type.trim();

    for alias in [
        "Json",
        "JsonValue",
        "JsonArray",
        "Jsonb",
        "IntArray",
        "BigIntArray",
        "TextArray",
        "BoolArray",
        "FloatArray",
        "Decimal",
        "Uuid",
        "NaiveDate",
        "NaiveTime",
        "NaiveDateTime",
        "Text",
    ] {
        if normalized == alias || normalized.ends_with(&format!("::{}", alias)) {
            return alias.to_string();
        }
    }

    normalized.to_string()
}

/// Normalizes a Rust type string by removing whitespace
pub fn normalize_rust_type(rust_type: &str) -> String {
    rust_type.chars().filter(|c| !c.is_whitespace()).collect()
}
