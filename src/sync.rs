//! Database Schema Synchronization Module
//!
//! This module provides automatic schema synchronization between TideORM models
//! and the database using SeaORM built-in SchemaBuilder capabilities.
//!
//! ## Two Synchronization Approaches
//!
//! ### 1. TideORM Models (Primary)
//!
//! TideORM models use `ModelSchema` for schema definition. This is automatically
//! handled by the `#[derive(Model)]` macro:
//!
//! ```rust,ignore
//! #[derive(Model)]
//! #[tide(table = "users")]
//! pub struct User {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub email: String,
//! }
//!
//! // Register via TideConfig
//! TideConfig::init()
//!     .database("postgres://...")
//!     .sync(true)
//!     .models::<(User, Post, Comment)>()
//!     .connect()
//!     .await?;
//! ```
//!
//! ### 2. SeaORM Entities (Advanced)
//!
//! For SeaORM entities, you can use `SyncRegistry::register_entity::<E>()` to
//! leverage SeaORM  native SchemaBuilder with incremental sync:
//!
//! ```rust,ignore
//! use tideorm::sync::SyncRegistry;
//! use sea_orm::entity::prelude::*;
//!
//! // Your SeaORM entity
//! #[derive(Clone, Debug, DeriveEntityModel)]
//! #[sea_orm(table_name = "products")]
//! pub struct Model {
//!     #[sea_orm(primary_key)]
//!     pub id: i32,
//!     pub name: String,
//! }
//!
//! // Register the SeaORM entity
//! SyncRegistry::register_entity::<Entity>();
//! ```
//!
//! ## SeaORM Schema Sync Features
//!
//! When using SeaORM entities, the sync uses SeaORM  native capabilities:
//!
//! - **Incremental Schema Sync**: Creates missing tables, columns, indexes, and foreign keys
//! - **Schema Discovery**: Automatically discovers existing database schema
//! - **Type-safe Entity Registration**: Uses SeaORM's EntityTrait for schema generation
//! - **Enum Support**: Creates PostgreSQL enums when needed
//! - **Foreign Key Management**: Properly handles foreign key relationships
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
//! - For SeaORM entities: Uses `apply` mode (fresh creation, fails if tables exist)
//! - For TideORM models: Drops and recreates tables
//!
//! ## ⚠️ Warning
//!
//! **DO NOT use sync mode in production!** It can cause data loss if column types
//! change in incompatible ways. Use proper migrations for production deployments.
//!
//! **NEVER use force_sync in production!** It WILL delete tables and their data!
//!
//! ## Usage
//!
//! Enable sync during TideORM initialization:
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//!
//! #[derive(Model)]
//! #[tide(table = "users")]
//! pub struct User {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub email: String,
//!     pub name: Option<String>,
//! }
//!
//! // Enable sync via TideConfig (recommended)
//! TideConfig::init()
//!     .database("postgres://...")
//!     .sync(true)  // Enable auto-sync
//!     .connect()
//!     .await?;
//!
//! // Or manually call sync on Database
//! let db = Database::connect("postgres://...").await?;
//! db.sync().await?; // Syncs all registered models
//! ```

use parking_lot::RwLock;
use std::sync::OnceLock;

use crate::database::Database;
use crate::error::{Error, Result};
use crate::{tide_debug, tide_info, tide_warn};

// Use SeaORM  schema management
use sea_orm::{
    ConnectionTrait, DbBackend, EntityTrait, Statement,
    schema::{Schema, SchemaBuilder},
    sea_query::{
        Alias, ColumnDef as SeaColumnDef, ColumnType as SeaColumnType, Expr, MysqlQueryBuilder,
        PostgresQueryBuilder, SqliteQueryBuilder, Table,
    },
};

/// Type alias for entity registration functions that register with SchemaBuilder
pub type EntityRegistrationFn = Box<dyn Fn(SchemaBuilder) -> SchemaBuilder + Send + Sync>;

/// Global registry of entity registration functions
static ENTITY_REGISTRY: OnceLock<RwLock<Vec<EntityRegistrationFn>>> = OnceLock::new();

/// Direct schemas registry (for manual/legacy registration)
static DIRECT_SCHEMAS: OnceLock<RwLock<Vec<ModelSchema>>> = OnceLock::new();

fn get_entity_registry() -> &'static RwLock<Vec<EntityRegistrationFn>> {
    ENTITY_REGISTRY.get_or_init(|| RwLock::new(Vec::new()))
}

fn get_direct_schemas() -> &'static RwLock<Vec<ModelSchema>> {
    DIRECT_SCHEMAS.get_or_init(|| RwLock::new(Vec::new()))
}

/// Trait for models that can be synced with the database
///
/// This trait is automatically implemented by the `#[derive(Model)]` macro.
/// Models that implement this trait can be registered for schema synchronization.
///
/// For TideORM models, this uses `ModelSchema` to define the table structure.
/// For SeaORM entities, you can use `SyncRegistry::register_entity::<E>()` directly.
pub trait SyncModel {
    /// Get the schema for this model
    fn sync_schema() -> ModelSchema;

    /// Register this model for synchronization
    fn register_for_sync() {
        SyncRegistry::register(Self::sync_schema());
    }
}

/// Trait for registering multiple models at once
///
/// This is implemented for tuples of up to 200 model types.
/// Used by `TideConfig::models::<(Model1, Model2, ...)>()`.
pub trait RegisterModels {
    /// Register all models in this tuple
    fn register_all();
}

// Implement RegisterModels for empty tuple
impl RegisterModels for () {
    fn register_all() {}
}

/// Generate `RegisterModels` for tuples of size 1..N
macro_rules! impl_register_models_tuples {
    // Base case: single type
    ($first:ident) => {
        impl<$first: SyncModel> RegisterModels for ($first,) {
            fn register_all() {
                $first::register_for_sync();
            }
        }
    };
    // Recursive case: first + rest
    ($first:ident, $($rest:ident),+) => {
        // Recurse for smaller tuples first
        impl_register_models_tuples!($($rest),+);

        impl<$first: SyncModel, $($rest: SyncModel),+> RegisterModels for ($first, $($rest),+) {
            fn register_all() {
                $first::register_for_sync();
                $($rest::register_for_sync();)+
            }
        }
    };
}

// Generate impls for tuples of 1 through 200 elements
impl_register_models_tuples!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, T17, T18, T19, T20, T21,
    T22, T23, T24, T25, T26, T27, T28, T29, T30, T31, T32, T33, T34, T35, T36, T37, T38, T39, T40,
    T41, T42, T43, T44, T45, T46, T47, T48, T49, T50, T51, T52, T53, T54, T55, T56, T57, T58, T59,
    T60, T61, T62, T63, T64, T65, T66, T67, T68, T69, T70, T71, T72, T73, T74, T75, T76, T77, T78,
    T79, T80, T81, T82, T83, T84, T85, T86, T87, T88, T89, T90, T91, T92, T93, T94, T95, T96, T97,
    T98, T99, T100, T101, T102, T103, T104, T105, T106, T107, T108, T109, T110, T111, T112, T113,
    T114, T115, T116, T117, T118, T119, T120, T121, T122, T123, T124, T125, T126, T127, T128, T129,
    T130, T131, T132, T133, T134, T135, T136, T137, T138, T139, T140, T141, T142, T143, T144, T145,
    T146, T147, T148, T149, T150, T151, T152, T153, T154, T155, T156, T157, T158, T159, T160, T161,
    T162, T163, T164, T165, T166, T167, T168, T169, T170, T171, T172, T173, T174, T175, T176, T177,
    T178, T179, T180, T181, T182, T183, T184, T185, T186, T187, T188, T189, T190, T191, T192, T193,
    T194, T195, T196, T197, T198, T199, T200
);

/// Registry for models to be synchronized using SeaORM  SchemaBuilder
pub struct SyncRegistry;

impl SyncRegistry {
    /// Register an entity type for synchronization using SeaORM
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
    /// Uses SeaORM  native SchemaBuilder.register() for each entity.
    pub fn build_schema_builder(backend: DbBackend) -> SchemaBuilder {
        let registry = get_entity_registry();
        let fns = registry.read();

        let schema = Schema::new(backend);
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

    /// Get the number of legacy schemas
    pub fn legacy_count() -> usize {
        let direct = get_direct_schemas();
        let schemas = direct.read();
        schemas.len()
    }

    /// Clear all registered models (for testing)
    pub fn clear() {
        let registry = get_entity_registry();
        let mut fns = registry.write();
        fns.clear();

        let direct = get_direct_schemas();
        let mut schemas = direct.write();
        schemas.clear();
    }

    // ========================================================================
    // Legacy API support (for backward compatibility)
    // ========================================================================

    /// Register a model schema for synchronization (legacy API)
    ///
    /// This is kept for backward compatibility with existing code.
    /// New code should use `register_entity::<E>()` instead.
    pub fn register(schema: ModelSchema) {
        let direct = get_direct_schemas();
        let mut schemas = direct.write();

        if !schemas.iter().any(|s| s.table_name == schema.table_name) {
            schemas.push(schema);
        }
    }

    /// Get all registered legacy model schemas
    pub fn get_all() -> Vec<ModelSchema> {
        let direct = get_direct_schemas();
        let schemas = direct.read();
        schemas.clone()
    }
}

// ============================================================================
// Legacy ModelSchema support (for backward compatibility)
// ============================================================================

/// Column definition for schema comparison (legacy)
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

/// Model schema definition for synchronization (legacy)
#[derive(Debug, Clone)]
pub struct ModelSchema {
    /// Table name in the database
    pub table_name: String,
    /// Schema name (default: "public")
    pub schema_name: String,
    /// Column definitions
    pub columns: Vec<ColumnDef>,
}

impl ModelSchema {
    /// Create a new model schema
    pub fn new(table_name: impl Into<String>) -> Self {
        Self {
            table_name: table_name.into(),
            schema_name: "public".to_string(),
            columns: Vec::new(),
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
}

// ============================================================================
// Main sync functions using SeaORM  SchemaBuilder
// ============================================================================

/// Synchronize all registered models with the database using SeaORM
///
/// This uses SeaORM's built-in `SchemaBuilder.sync()` to:
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
/// This uses SeaORM  schema management with additional options.
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
        tide_warn!("Database FORCE sync mode is ENABLED - using SeaORM apply mode!");
    } else {
        tide_warn!("Database sync mode is ENABLED - DO NOT use in production!");
    }

    let conn = db.__internal_connection();
    let backend = conn.get_database_backend();

    let entity_count = SyncRegistry::entity_count();
    let legacy_count = SyncRegistry::legacy_count();
    let total_count = entity_count + legacy_count;

    if total_count == 0 {
        tide_info!("No models registered for sync");
        return Ok(());
    }

    tide_info!(
        "Syncing {} model(s) using SeaORM SchemaBuilder...",
        total_count
    );
    tide_debug!("  - {} entity-based models", entity_count);
    tide_debug!("  - {} legacy schema models", legacy_count);

    // Build SchemaBuilder with all registered entities
    if entity_count > 0 {
        let schema_builder = SyncRegistry::build_schema_builder(backend);

        #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
        // Use SeaORM  sync or apply based on force_sync option
        if force_sync {
            tide_debug!("  Using SeaORM SchemaBuilder.apply() - fresh schema creation");
            schema_builder
                .apply(conn)
                .await
                .map_err(|e| Error::query(format!("Schema apply failed: {}", e)))?;
        } else {
            tide_debug!("  Using SeaORM SchemaBuilder.sync() - incremental sync");
            schema_builder
                .sync(conn)
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

    // Handle legacy schemas if any
    if legacy_count > 0 {
        tide_debug!("  Processing {} legacy schema(s)...", legacy_count);
        sync_legacy_schemas(db, force_sync).await?;
    }

    tide_info!("Database sync completed using SeaORM");
    Ok(())
}

/// Sync legacy ModelSchema definitions (backward compatibility)
async fn sync_legacy_schemas(db: &Database, force_sync: bool) -> Result<()> {
    let models = SyncRegistry::get_all();
    let conn = db.__internal_connection();
    let backend = conn.get_database_backend();

    for model in models {
        let table_exists =
            check_table_exists(conn, &model.schema_name, &model.table_name, backend).await?;

        if force_sync && table_exists {
            // Drop existing table for force sync
            let drop_sql = match backend {
                DbBackend::Postgres => format!(
                    "DROP TABLE IF EXISTS \"{}\".\"{}\" CASCADE",
                    model.schema_name, model.table_name
                ),
                DbBackend::MySql => format!("DROP TABLE IF EXISTS `{}`", model.table_name),
                DbBackend::Sqlite => format!("DROP TABLE IF EXISTS \"{}\"", model.table_name),
                _ => format!("DROP TABLE IF EXISTS \"{}\"", model.table_name),
            };

            let drop_stmt = Statement::from_string(backend, drop_sql);
            conn.execute_raw(drop_stmt)
                .await
                .map_err(|e| Error::query(e.to_string()))?;

            tide_warn!("Dropped legacy table: {}", model.table_name);
        }

        if !table_exists || force_sync {
            // Create new table
            create_table_from_legacy_schema(conn, &model, backend).await?;
            tide_info!("Created legacy table: {}", model.table_name);
        } else {
            tide_debug!("Legacy table exists: {}", model.table_name);
        }
    }

    Ok(())
}

/// Check if a table exists in the database
async fn check_table_exists(
    conn: &sea_orm::DatabaseConnection,
    schema: &str,
    table: &str,
    backend: DbBackend,
) -> Result<bool> {
    let sql = match backend {
        DbBackend::Postgres => format!(
            "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_schema = '{}' AND table_name = '{}')",
            schema, table
        ),
        DbBackend::MySql => format!(
            "SELECT COUNT(*) > 0 FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = '{}'",
            table
        ),
        DbBackend::Sqlite => format!(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = '{}'",
            table
        ),
        other => {
            tide_warn!(
                "Unknown backend {:?} in check_table_exists, falling back to SQLite SQL",
                other
            );
            format!(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = '{}'",
                table
            )
        }
    };

    let stmt = Statement::from_string(backend, sql);
    let result = conn
        .query_one_raw(stmt)
        .await
        .map_err(|e| Error::query(e.to_string()))?;

    match result {
        Some(row) => {
            let exists: bool = match backend {
                DbBackend::Postgres => row.try_get_by_index(0).unwrap_or(false),
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

/// Create a table from a legacy ModelSchema definition
async fn create_table_from_legacy_schema(
    conn: &sea_orm::DatabaseConnection,
    model: &ModelSchema,
    backend: DbBackend,
) -> Result<()> {
    let mut table = Table::create();
    table.table(Alias::new(&model.table_name));

    // Add columns using SeaORM's column definitions
    for col in &model.columns {
        let mut column = SeaColumnDef::new(Alias::new(&col.name));

        // Set column type based on Rust type
        apply_column_type(&mut column, &col.col_type, col.auto_increment, backend);

        if col.primary_key {
            column.primary_key();
        }

        if col.auto_increment {
            column.auto_increment();
        }

        if !col.nullable && !col.primary_key && !col.auto_increment {
            column.not_null();
        }

        if let Some(ref default) = col.default {
            let default_owned = default.clone();
            column.default(Expr::cust(default_owned));
        }

        table.col(&mut column);
    }

    table.if_not_exists();

    // Build the SQL using backend-specific query builder
    let sql = match backend {
        DbBackend::Postgres => table.to_string(PostgresQueryBuilder),
        DbBackend::MySql => table.to_string(MysqlQueryBuilder),
        DbBackend::Sqlite => table.to_string(SqliteQueryBuilder),
        _ => table.to_string(PostgresQueryBuilder),
    };

    let create_stmt = Statement::from_string(backend, sql);
    conn.execute_raw(create_stmt)
        .await
        .map_err(|e| Error::query(e.to_string()))?;

    Ok(())
}

/// Apply column type based on Rust type string
fn apply_column_type(
    column: &mut sea_orm::sea_query::ColumnDef,
    rust_type: &str,
    _auto_increment: bool,
    _backend: DbBackend,
) {
    // Normalize type (remove whitespace from stringify!)
    let normalized = normalize_rust_type(rust_type);

    // Extract inner type for Option<T>
    let inner_type = normalized
        .strip_prefix("Option<")
        .and_then(|s| s.strip_suffix(">"))
        .unwrap_or(&normalized);

    // Map Rust types to SeaORM column types
    match inner_type {
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
            column.array(SeaColumnType::Integer);
        }
        "Vec<i64>" | "BigIntArray" => {
            column.array(SeaColumnType::BigInteger);
        }
        "Vec<String>" | "TextArray" => {
            column.array(SeaColumnType::Text);
        }
        "Vec<bool>" | "BoolArray" => {
            column.array(SeaColumnType::Boolean);
        }
        "Vec<f64>" | "FloatArray" => {
            column.array(SeaColumnType::Double);
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

/// Normalizes a Rust type string by removing whitespace
pub fn normalize_rust_type(rust_type: &str) -> String {
    rust_type.chars().filter(|c| !c.is_whitespace()).collect()
}
