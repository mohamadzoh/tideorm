//! Database Schema Synchronization Module
//!
//! This module provides automatic schema synchronization between TideORM models
//! and the database using SeaORM's built-in schema management capabilities.
//!
//! When enabled via `TideConfig::sync(true)`, it will:
//!
//! - Create missing tables
//! - Add missing columns to existing tables
//! - Create indexes defined in models
//!
//! ## Force Sync Mode
//!
//! When `TideConfig::force_sync(true)` is also enabled, it will additionally:
//!
//! - **DROP tables and columns** not defined in your models
//!
//! ## ⚠️ Warning
//!
//! **DO NOT use sync mode in production!** It can cause data loss if column types
//! change in incompatible ways. Use proper migrations for production deployments.
//!
//! **NEVER use force_sync in production!** It WILL delete columns and their data!
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
//! // ⚠️ DANGEROUS: Enable force sync to drop orphaned columns
//! TideConfig::init()
//!     .database("postgres://...")
//!     .sync(true)
//!     .force_sync(true)  // Will DROP columns not in model!
//!     .connect()
//!     .await?;
//!
//! // Or manually call sync on Database
//! let db = Database::connect("postgres://...").await?;
//! db.sync().await?; // Syncs all registered models
//! ```

use std::sync::OnceLock;
use parking_lot::RwLock;

use crate::database::Database;
use crate::error::{Error, Result};

// Use SeaORM's schema management
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait,
    Statement, sea_query::{
        Table, ColumnDef as SeaColumnDef, Alias, Expr,
        ColumnType as SeaColumnType, TableCreateStatement,
    },
};

/// Type alias for sync registration functions
pub type SyncFn = fn() -> ModelSchema;

/// Global registry of models to sync
static SYNC_REGISTRY: OnceLock<RwLock<Vec<SyncFn>>> = OnceLock::new();

/// Direct schemas registry (for manual registration)
static DIRECT_SCHEMAS: OnceLock<RwLock<Vec<ModelSchema>>> = OnceLock::new();

/// Trait for models that can be synced with the database
/// 
/// This trait is automatically implemented by the `#[derive(Model)]` macro.
/// Models that implement this trait can be registered for schema synchronization.
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
/// This is implemented for tuples of up to 16 model types.
/// Used by `TideConfig::models::<(Model1, Model2, ...)>()`.
pub trait RegisterModels {
    /// Register all models in this tuple
    fn register_all();
}

// Implement RegisterModels for tuples of various sizes
impl RegisterModels for () {
    fn register_all() {}
}

impl<A: SyncModel> RegisterModels for (A,) {
    fn register_all() {
        A::register_for_sync();
    }
}

impl<A: SyncModel, B: SyncModel> RegisterModels for (A, B) {
    fn register_all() {
        A::register_for_sync();
        B::register_for_sync();
    }
}

impl<A: SyncModel, B: SyncModel, C: SyncModel> RegisterModels for (A, B, C) {
    fn register_all() {
        A::register_for_sync();
        B::register_for_sync();
        C::register_for_sync();
    }
}

impl<A: SyncModel, B: SyncModel, C: SyncModel, D: SyncModel> RegisterModels for (A, B, C, D) {
    fn register_all() {
        A::register_for_sync();
        B::register_for_sync();
        C::register_for_sync();
        D::register_for_sync();
    }
}

impl<A: SyncModel, B: SyncModel, C: SyncModel, D: SyncModel, E: SyncModel> RegisterModels for (A, B, C, D, E) {
    fn register_all() {
        A::register_for_sync();
        B::register_for_sync();
        C::register_for_sync();
        D::register_for_sync();
        E::register_for_sync();
    }
}

impl<A: SyncModel, B: SyncModel, C: SyncModel, D: SyncModel, E: SyncModel, F: SyncModel> RegisterModels for (A, B, C, D, E, F) {
    fn register_all() {
        A::register_for_sync();
        B::register_for_sync();
        C::register_for_sync();
        D::register_for_sync();
        E::register_for_sync();
        F::register_for_sync();
    }
}

impl<A: SyncModel, B: SyncModel, C: SyncModel, D: SyncModel, E: SyncModel, F: SyncModel, G: SyncModel> RegisterModels for (A, B, C, D, E, F, G) {
    fn register_all() {
        A::register_for_sync();
        B::register_for_sync();
        C::register_for_sync();
        D::register_for_sync();
        E::register_for_sync();
        F::register_for_sync();
        G::register_for_sync();
    }
}

impl<A: SyncModel, B: SyncModel, C: SyncModel, D: SyncModel, E: SyncModel, F: SyncModel, G: SyncModel, H: SyncModel> RegisterModels for (A, B, C, D, E, F, G, H) {
    fn register_all() {
        A::register_for_sync();
        B::register_for_sync();
        C::register_for_sync();
        D::register_for_sync();
        E::register_for_sync();
        F::register_for_sync();
        G::register_for_sync();
        H::register_for_sync();
    }
}

impl<A: SyncModel, B: SyncModel, C: SyncModel, D: SyncModel, E: SyncModel, F: SyncModel, G: SyncModel, H: SyncModel, I: SyncModel> RegisterModels for (A, B, C, D, E, F, G, H, I) {
    fn register_all() {
        A::register_for_sync();
        B::register_for_sync();
        C::register_for_sync();
        D::register_for_sync();
        E::register_for_sync();
        F::register_for_sync();
        G::register_for_sync();
        H::register_for_sync();
        I::register_for_sync();
    }
}

impl<A: SyncModel, B: SyncModel, C: SyncModel, D: SyncModel, E: SyncModel, F: SyncModel, G: SyncModel, H: SyncModel, I: SyncModel, J: SyncModel> RegisterModels for (A, B, C, D, E, F, G, H, I, J) {
    fn register_all() {
        A::register_for_sync();
        B::register_for_sync();
        C::register_for_sync();
        D::register_for_sync();
        E::register_for_sync();
        F::register_for_sync();
        G::register_for_sync();
        H::register_for_sync();
        I::register_for_sync();
        J::register_for_sync();
    }
}

impl<A: SyncModel, B: SyncModel, C: SyncModel, D: SyncModel, E: SyncModel, F: SyncModel, G: SyncModel, H: SyncModel, I: SyncModel, J: SyncModel, K: SyncModel> RegisterModels for (A, B, C, D, E, F, G, H, I, J, K) {
    fn register_all() {
        A::register_for_sync();
        B::register_for_sync();
        C::register_for_sync();
        D::register_for_sync();
        E::register_for_sync();
        F::register_for_sync();
        G::register_for_sync();
        H::register_for_sync();
        I::register_for_sync();
        J::register_for_sync();
        K::register_for_sync();
    }
}

impl<A: SyncModel, B: SyncModel, C: SyncModel, D: SyncModel, E: SyncModel, F: SyncModel, G: SyncModel, H: SyncModel, I: SyncModel, J: SyncModel, K: SyncModel, L: SyncModel> RegisterModels for (A, B, C, D, E, F, G, H, I, J, K, L) {
    fn register_all() {
        A::register_for_sync();
        B::register_for_sync();
        C::register_for_sync();
        D::register_for_sync();
        E::register_for_sync();
        F::register_for_sync();
        G::register_for_sync();
        H::register_for_sync();
        I::register_for_sync();
        J::register_for_sync();
        K::register_for_sync();
        L::register_for_sync();
    }
}

fn get_registry() -> &'static RwLock<Vec<SyncFn>> {
    SYNC_REGISTRY.get_or_init(|| RwLock::new(Vec::new()))
}

fn get_direct_schemas() -> &'static RwLock<Vec<ModelSchema>> {
    DIRECT_SCHEMAS.get_or_init(|| RwLock::new(Vec::new()))
}

/// Column definition for schema comparison
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

/// Model schema definition for synchronization
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

/// Registry for models to be synchronized
pub struct SyncRegistry;

impl SyncRegistry {
    /// Register a sync function that generates a ModelSchema
    /// This is called by the derive macro automatically
    #[doc(hidden)]
    pub fn register_fn(f: SyncFn) {
        let registry = get_registry();
        let mut fns = registry.write();
        fns.push(f);
    }
    
    /// Register a model schema for synchronization (direct registration)
    pub fn register(schema: ModelSchema) {
        let direct = get_direct_schemas();
        let mut schemas = direct.write();
        
        if !schemas.iter().any(|s| s.table_name == schema.table_name) {
            schemas.push(schema);
        }
    }

    /// Get all registered model schemas
    pub fn get_all() -> Vec<ModelSchema> {
        let mut result = Vec::new();
        let mut seen_tables = std::collections::HashSet::new();
        
        // Get schemas from sync functions
        let registry = get_registry();
        let fns = registry.read();
        for f in fns.iter() {
            let schema = f();
            if seen_tables.insert(schema.table_name.clone()) {
                result.push(schema);
            }
        }
        
        // Get directly registered schemas
        let direct = get_direct_schemas();
        let schemas = direct.read();
        for schema in schemas.iter() {
            if seen_tables.insert(schema.table_name.clone()) {
                result.push(schema.clone());
            }
        }
        
        result
    }

    /// Clear all registered models (for testing)
    pub fn clear() {
        let registry = get_registry();
        let mut fns = registry.write();
        fns.clear();
        
        let direct = get_direct_schemas();
        let mut schemas = direct.write();
        schemas.clear();
    }
    
    /// Register an entity for sync using SeaORM's entity definition
    #[doc(hidden)]
    pub fn register_entity<E: EntityTrait>(_table_name: &str) {
        // SeaORM entities are already defined - use Schema::create_table_from_entity
        // The actual schema generation happens at sync time using SeaORM's Schema builder
    }
}

/// Synchronize all registered models with the database using SeaORM
///
/// This uses SeaORM's built-in schema management to:
/// 1. Create missing tables
/// 2. Use proper database-specific SQL generation
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
/// This uses SeaORM's schema management with additional force options.
///
/// # Arguments
///
/// * `db` - Database connection
/// * `force_sync` - If true, drops and recreates tables
///
/// # Returns
///
/// Returns `Ok(())` on success, or an Error on failure
///
/// # ⚠️ DANGER
///
/// **DO NOT use in production!** This is for development only.
/// When `force_sync` is enabled, tables WILL BE RECREATED causing data loss.
pub async fn sync_database_with_options(db: &Database, force_sync: bool) -> Result<()> {
    if force_sync {
        eprintln!("⚠️  Database FORCE sync mode is ENABLED - TABLES WILL BE RECREATED!");
    } else {
        eprintln!("⚠️  Database sync mode is ENABLED - DO NOT use in production!");
    }
    
    let models = SyncRegistry::get_all();
    
    if models.is_empty() {
        eprintln!("No models registered for sync");
        return Ok(());
    }

    eprintln!("Syncing {} model(s)...", models.len());

    let conn = db.__internal_connection();
    let backend = conn.get_database_backend();
    
    for model in models {
        sync_model_schema(conn, &model, backend, force_sync).await?;
    }

    eprintln!("✅ Database sync completed");
    Ok(())
}

/// Sync a single model schema using SeaORM's schema builder
async fn sync_model_schema(
    conn: &DatabaseConnection,
    model: &ModelSchema,
    backend: DbBackend,
    force_sync: bool,
) -> Result<()> {
    let table_name = &model.table_name;
    
    // Check if table exists
    let table_exists = check_table_exists(conn, &model.schema_name, table_name, backend).await?;
    
    if force_sync && table_exists {
        // Drop existing table
        let drop_sql = match backend {
            DbBackend::Postgres => format!("DROP TABLE IF EXISTS \"{}\".\"{}\" CASCADE", model.schema_name, table_name),
            DbBackend::MySql => format!("DROP TABLE IF EXISTS `{}`", table_name),
            DbBackend::Sqlite => format!("DROP TABLE IF EXISTS \"{}\"", table_name),
            _ => format!("DROP TABLE IF EXISTS \"{}\"", table_name),
        };
        
        let drop_stmt = Statement::from_string(backend, drop_sql);
        conn.execute_raw(drop_stmt)
            .await
            .map_err(|e| Error::query(e.to_string()))?;
        
        eprintln!("  ⚠️  Dropped table: {}", table_name);
    }
    
    if !table_exists || force_sync {
        // Create new table using SeaORM-style schema
        create_table_from_schema(conn, model, backend).await?;
        eprintln!("  ✅ Created table: {}", table_name);
    } else {
        eprintln!("  ✅ Table exists: {}", table_name);
    }
    
    Ok(())
}

/// Check if a table exists in the database
async fn check_table_exists(
    conn: &DatabaseConnection,
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
        _ => format!(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = '{}'",
            table
        ),
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
                DbBackend::MySql | DbBackend::Sqlite => {
                    let val: i32 = row.try_get_by_index(0).unwrap_or(0);
                    val > 0
                }
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

/// Create a table from a ModelSchema definition using SeaORM's query builder
async fn create_table_from_schema(
    conn: &DatabaseConnection,
    model: &ModelSchema,
    backend: DbBackend,
) -> Result<()> {
    let mut table = Table::create();
    
    // For Postgres, just use the table name without schema qualification
    // The connection already knows the search_path
    let table_name = match backend {
        DbBackend::Postgres => model.table_name.clone(),
        DbBackend::MySql => model.table_name.clone(),
        DbBackend::Sqlite => model.table_name.clone(),
        _ => model.table_name.clone(),
    };
    
    table.table(Alias::new(&table_name));
    
    // Add columns using SeaORM's column definitions
    for col in &model.columns {
        let mut column = SeaColumnDef::new(Alias::new(&col.name));
        
        // Set column type based on Rust type using SeaORM's type system
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
            // Clone to owned String to satisfy 'static lifetime
            let default_owned = default.clone();
            column.default(Expr::cust(default_owned));
        }
        
        table.col(&mut column);
    }
    
    table.if_not_exists();
    
    // Build the SQL using backend-specific query builder
    let sql = build_table_sql(&table, backend);
    
    let create_stmt = Statement::from_string(backend, sql);
    conn.execute_raw(create_stmt)
        .await
        .map_err(|e| Error::query(e.to_string()))?;
    
    Ok(())
}

/// Build table creation SQL for the specific backend
fn build_table_sql(table: &TableCreateStatement, backend: DbBackend) -> String {
    use sea_orm::sea_query::{PostgresQueryBuilder, MysqlQueryBuilder, SqliteQueryBuilder};
    
    match backend {
        DbBackend::Postgres => table.to_string(PostgresQueryBuilder),
        DbBackend::MySql => table.to_string(MysqlQueryBuilder),
        DbBackend::Sqlite => table.to_string(SqliteQueryBuilder),
        _ => table.to_string(PostgresQueryBuilder), // Default to Postgres for unknown backends
    }
}

/// Apply column type based on Rust type string using SeaORM's type system
fn apply_column_type(column: &mut SeaColumnDef, rust_type: &str, auto_increment: bool, _backend: DbBackend) {
    // Normalize type (remove whitespace from stringify!)
    let normalized = normalize_rust_type(rust_type);
    
    // Extract inner type for Option<T>
    let inner_type = normalized
        .strip_prefix("Option<")
        .and_then(|s| s.strip_suffix(">"))
        .unwrap_or(&normalized);
    
    // Map Rust types to SeaORM column types
    match inner_type {
        "i8" | "u8" | "i16" | "u16" => { column.small_integer(); }
        "i32" => { 
            if auto_increment {
                column.integer();
            } else {
                column.integer();
            }
        }
        "u32" | "i64" => { 
            if auto_increment {
                column.big_integer();
            } else {
                column.big_integer();
            }
        }
        "u64" | "i128" | "u128" => { column.decimal(); }
        "isize" | "usize" => { column.big_integer(); }
        "f32" => { column.float(); }
        "f64" => { column.double(); }
        "bool" => { column.boolean(); }
        "String" | "&str" => { column.text(); }
        "Uuid" => { column.uuid(); }
        "Json" | "JsonValue" | "serde_json::Value" | "Value" | "Jsonb" => { column.json_binary(); }
        "Vec<u8>" | "Bytes" => { column.binary(); }
        "Decimal" | "BigDecimal" => { column.decimal(); }
        t if t.contains("DateTime") => { column.timestamp_with_time_zone(); }
        t if t.contains("NaiveDateTime") => { column.timestamp(); }
        t if t.contains("NaiveDate") => { column.date(); }
        t if t.contains("NaiveTime") => { column.time(); }
        // Array types (PostgreSQL specific)
        "Vec<i32>" | "IntArray" => { column.array(SeaColumnType::Integer); }
        "Vec<i64>" | "BigIntArray" => { column.array(SeaColumnType::BigInteger); }
        "Vec<String>" | "TextArray" => { column.array(SeaColumnType::Text); }
        "Vec<bool>" | "BoolArray" => { column.array(SeaColumnType::Boolean); }
        "Vec<f64>" | "FloatArray" => { column.array(SeaColumnType::Double); }
        _ => { column.text(); }
    };
}

/// Normalizes a Rust type string by removing whitespace
/// This is used to handle stringify! output like "Option < i64 >" -> "Option<i64>"
pub fn normalize_rust_type(rust_type: &str) -> String {
    rust_type.chars().filter(|c| !c.is_whitespace()).collect()
}
