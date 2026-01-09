//! Database Schema Synchronization Module
//!
//! This module provides automatic schema synchronization between TideORM models
//! and the database. When enabled via `TideConfig::sync(true)`, it will:
//!
//! - Create missing tables
//! - Add missing columns to existing tables
//! - Modify column types when possible (with warnings for potentially destructive changes)
//!
//! ## Force Sync Mode
//!
//! When `TideConfig::force_sync(true)` is also enabled, it will additionally:
//!
//! - **DROP columns** from database tables that are not defined in your models
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
//! #[derive(Model, Clone, Debug, Serialize, Deserialize)]
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

use std::collections::HashMap;
use std::sync::OnceLock;
use parking_lot::RwLock;

use crate::database::Database;
use crate::error::{Error, Result};

// Re-use internal SeaORM types (hidden from users)
use crate::internal::{
    ConnectionTrait, DatabaseConnection, Statement, EntityTrait,
    ColumnTrait,
};
use crate::sea_orm::DbBackend;

/// Get the database backend from a connection
fn get_backend(conn: &DatabaseConnection) -> DbBackend {
    conn.get_database_backend()
}

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
    /// PostgreSQL column type (e.g., "VARCHAR", "INTEGER", "TIMESTAMPTZ")
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
    
    /// Register an entity for sync by extracting its schema
    #[doc(hidden)]
    pub fn register_entity<E: EntityTrait>(table_name: &str) {
        use crate::sea_orm::Iterable as SeaOrmIterable;
        use crate::sea_orm::Iden;
        
        let mut schema = ModelSchema::new(table_name);
        
        // Iterate over all columns defined in the Entity
        for col in E::Column::iter() {
            let col_def = col.def();
            let col_name = <E::Column as Iden>::to_string(&col);
            let pg_type = sea_orm_to_postgres_type(col_def.get_column_type());
            
            let mut def = ColumnDef::new(&col_name, pg_type);
            
            // Check if nullable
            if !col_def.is_null() {
                def = def.not_null();
            }
            
            // Primary key detection (heuristic: "id" column)
            if col_name == "id" {
                def = def.primary_key();
            }
            
            schema = schema.column(def);
        }
        
        Self::register(schema);
    }
}

/// Information about an existing column in the database
#[derive(Debug, Clone)]
struct ExistingColumn {
    name: String,
    data_type: String,
    is_nullable: bool,
    #[allow(dead_code)]
    column_default: Option<String>,
}

/// Synchronize all registered models with the database
///
/// This will:
/// 1. Create missing tables
/// 2. Add missing columns to existing tables
/// 3. Attempt to modify column types (with warnings)
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
/// This will:
/// 1. Create missing tables
/// 2. Add missing columns to existing tables
/// 3. Attempt to modify column types (with warnings)
/// 4. **If force_sync is true**: DROP columns not defined in models
///
/// # Arguments
///
/// * `db` - Database connection
/// * `force_sync` - If true, removes columns from DB that are not in model
///
/// # Returns
///
/// Returns `Ok(())` on success, or an Error on failure
///
/// # ⚠️ DANGER
///
/// **DO NOT use in production!** This is for development only.
/// When `force_sync` is enabled, columns not in your model WILL BE DELETED.
pub async fn sync_database_with_options(db: &Database, force_sync: bool) -> Result<()> {
    if force_sync {
        eprintln!("⚠️  Database FORCE sync mode is ENABLED - COLUMNS NOT IN MODEL WILL BE DROPPED!");
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
    for model in models {
        sync_model_with_options(conn, &model, force_sync).await?;
    }

    eprintln!("✅ Database sync completed");
    Ok(())
}

/// Synchronize a single model with the database
#[allow(dead_code)]
async fn sync_model(conn: &DatabaseConnection, model: &ModelSchema) -> Result<()> {
    sync_model_with_options(conn, model, false).await
}

/// Synchronize a single model with the database (with force_sync option)
async fn sync_model_with_options(conn: &DatabaseConnection, model: &ModelSchema, force_sync: bool) -> Result<()> {
    let backend = get_backend(conn);
    let full_table_name = match backend {
        DbBackend::Postgres => format!("{}.{}", model.schema_name, model.table_name),
        _ => model.table_name.clone(),
    };
    
    // For PostgreSQL, ensure schema exists
    if backend == DbBackend::Postgres && model.schema_name != "public" {
        ensure_schema_exists(conn, &model.schema_name).await?;
    }
    
    // Check if table exists
    let table_exists = check_table_exists(conn, &model.schema_name, &model.table_name).await?;

    if !table_exists {
        // Create the table
        create_table(conn, model).await?;
        eprintln!("  ✅ Created table: {}", full_table_name);
    } else {
        // Get existing columns
        let existing_columns = get_existing_columns(conn, &model.schema_name, &model.table_name).await?;
        let existing_map: HashMap<String, ExistingColumn> = existing_columns
            .iter()
            .map(|c| (c.name.to_lowercase(), c.clone()))
            .collect();
        
        // Build set of model column names (lowercase for comparison)
        let model_columns: std::collections::HashSet<String> = model.columns
            .iter()
            .map(|c| c.name.to_lowercase())
            .collect();

        // Sync each column from the model
        for col_def in &model.columns {
            let col_name_lower = col_def.name.to_lowercase();
            if let Some(existing) = existing_map.get(&col_name_lower) {
                // Column exists - check if it needs modification
                if needs_modification(col_def, existing, backend) {
                    modify_column(conn, model, col_def, existing).await?;
                }
            } else {
                // Column doesn't exist - add it
                add_column(conn, model, col_def).await?;
                eprintln!("  ✅ Added column: {}.{}", full_table_name, col_def.name);
            }
        }
        
        // Force sync: drop columns that exist in DB but not in model
        if force_sync {
            for existing_col in &existing_columns {
                let col_name_lower = existing_col.name.to_lowercase();
                if !model_columns.contains(&col_name_lower) {
                    drop_column(conn, model, &existing_col.name).await?;
                    eprintln!("  ⚠️  DROPPED column: {}.{}", full_table_name, existing_col.name);
                }
            }
        }
        
        eprintln!("  ✅ Synced table: {}", full_table_name);
    }

    Ok(())
}

/// Ensure a schema exists (PostgreSQL only)
async fn ensure_schema_exists(conn: &DatabaseConnection, schema: &str) -> Result<()> {
    let sql = format!("CREATE SCHEMA IF NOT EXISTS \"{}\"", schema);
    conn.execute(Statement::from_string(DbBackend::Postgres, sql))
        .await
        .map_err(|e| Error::query(e.to_string()))?;
    Ok(())
}

/// Check if a table exists in the database
async fn check_table_exists(
    conn: &DatabaseConnection,
    schema: &str,
    table: &str,
) -> Result<bool> {
    let backend = get_backend(conn);
    
    let (sql, values): (String, Vec<crate::sea_orm::Value>) = match backend {
        DbBackend::Postgres => {
            let sql = r#"
                SELECT EXISTS (
                    SELECT FROM information_schema.tables 
                    WHERE table_schema = $1 AND table_name = $2
                )
            "#.to_string();
            (sql, vec![schema.into(), table.into()])
        }
        DbBackend::MySql => {
            let sql = r#"
                SELECT COUNT(*) > 0 as `exists`
                FROM information_schema.tables 
                WHERE table_schema = DATABASE() AND table_name = ?
            "#.to_string();
            (sql, vec![table.into()])
        }
        DbBackend::Sqlite => {
            let sql = r#"
                SELECT COUNT(*) > 0 as `exists`
                FROM sqlite_master 
                WHERE type = 'table' AND name = ?
            "#.to_string();
            (sql, vec![table.into()])
        }
    };

    let result = conn
        .query_one(Statement::from_sql_and_values(backend, &sql, values))
        .await
        .map_err(|e| Error::query(e.to_string()))?;

    match result {
        Some(row) => {
            // Handle different return types based on backend
            let exists: bool = match backend {
                DbBackend::Postgres => row.try_get_by_index(0)
                    .map_err(|e| Error::query(e.to_string()))?,
                DbBackend::MySql | DbBackend::Sqlite => {
                    // MySQL/SQLite might return as integer
                    let val: i32 = row.try_get_by_index(0)
                        .or_else(|_| row.try_get::<i64>("", "exists").map(|v| v as i32))
                        .or_else(|_| row.try_get::<i32>("", "exists"))
                        .unwrap_or(0);
                    val > 0
                }
            };
            Ok(exists)
        }
        None => Ok(false),
    }
}

/// Get existing columns for a table
async fn get_existing_columns(
    conn: &DatabaseConnection,
    schema: &str,
    table: &str,
) -> Result<Vec<ExistingColumn>> {
    let backend = get_backend(conn);
    
    let (sql, values): (String, Vec<crate::sea_orm::Value>) = match backend {
        DbBackend::Postgres => {
            let sql = r#"
                SELECT 
                    column_name,
                    data_type,
                    is_nullable,
                    column_default
                FROM information_schema.columns
                WHERE table_schema = $1 AND table_name = $2
                ORDER BY ordinal_position
            "#.to_string();
            (sql, vec![schema.into(), table.into()])
        }
        DbBackend::MySql => {
            let sql = r#"
                SELECT 
                    COLUMN_NAME as column_name,
                    DATA_TYPE as data_type,
                    IS_NULLABLE as is_nullable,
                    COLUMN_DEFAULT as column_default
                FROM information_schema.columns
                WHERE table_schema = DATABASE() AND table_name = ?
                ORDER BY ordinal_position
            "#.to_string();
            (sql, vec![table.into()])
        }
        DbBackend::Sqlite => {
            // SQLite uses pragma, but we can also use a simpler approach
            let sql = format!("PRAGMA table_info(\"{}\")", table);
            // SQLite doesn't use parameterized queries for PRAGMA
            let results = conn
                .query_all(Statement::from_string(backend, sql))
                .await
                .map_err(|e| Error::query(e.to_string()))?;

            let mut columns = Vec::new();
            for row in results {
                // PRAGMA table_info returns: cid, name, type, notnull, dflt_value, pk
                let name: String = row.try_get_by_index(1)
                    .map_err(|e| Error::query(e.to_string()))?;
                let data_type: String = row.try_get_by_index(2)
                    .map_err(|e| Error::query(e.to_string()))?;
                let notnull: i32 = row.try_get_by_index(3)
                    .map_err(|e| Error::query(e.to_string()))?;
                let column_default: Option<String> = row.try_get_by_index(4).ok();

                columns.push(ExistingColumn {
                    name,
                    data_type,
                    is_nullable: notnull == 0,
                    column_default,
                });
            }
            return Ok(columns);
        }
    };

    let results = conn
        .query_all(Statement::from_sql_and_values(backend, &sql, values))
        .await
        .map_err(|e| Error::query(e.to_string()))?;

    let mut columns = Vec::new();
    for row in results {
        let name: String = row.try_get_by_index(0)
            .or_else(|_| row.try_get("", "column_name"))
            .map_err(|e| Error::query(e.to_string()))?;
        let data_type: String = row.try_get_by_index(1)
            .or_else(|_| row.try_get("", "data_type"))
            .map_err(|e| Error::query(e.to_string()))?;
        let is_nullable_str: String = row.try_get_by_index(2)
            .or_else(|_| row.try_get("", "is_nullable"))
            .map_err(|e| Error::query(e.to_string()))?;
        let column_default: Option<String> = row.try_get_by_index(3)
            .or_else(|_| row.try_get("", "column_default"))
            .ok();

        columns.push(ExistingColumn {
            name,
            data_type,
            is_nullable: is_nullable_str.to_uppercase() == "YES",
            column_default,
        });
    }

    Ok(columns)
}

/// Create a new table
async fn create_table(conn: &DatabaseConnection, model: &ModelSchema) -> Result<()> {
    let backend = get_backend(conn);
    let mut column_defs = Vec::new();
    let mut primary_keys = Vec::new();

    for col in &model.columns {
        let col_type = get_column_type_for_backend(col, backend);
        
        let quote_char = get_quote_char(backend);
        let mut def = format!("{}{}{} {}", quote_char, col.name, quote_char, col_type);
        
        // Handle NOT NULL constraint
        if !col.nullable && !col.auto_increment {
            def.push_str(" NOT NULL");
        }
        
        // Handle default values
        if let Some(default) = &col.default {
            let adjusted_default = adjust_default_for_backend(default, backend);
            def.push_str(&format!(" DEFAULT {}", adjusted_default));
        }
        
        if col.primary_key {
            primary_keys.push(format!("{}{}{}", quote_char, col.name, quote_char));
        }
        
        column_defs.push(def);
    }

    // Add primary key constraint if any
    if !primary_keys.is_empty() {
        column_defs.push(format!("PRIMARY KEY ({})", primary_keys.join(", ")));
    }

    let sql = match backend {
        DbBackend::Postgres => format!(
            "CREATE TABLE \"{}\".\"{}\" (\n  {}\n)",
            model.schema_name,
            model.table_name,
            column_defs.join(",\n  ")
        ),
        DbBackend::MySql => format!(
            "CREATE TABLE `{}` (\n  {}\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
            model.table_name,
            column_defs.join(",\n  ")
        ),
        DbBackend::Sqlite => format!(
            "CREATE TABLE \"{}\" (\n  {}\n)",
            model.table_name,
            column_defs.join(",\n  ")
        ),
    };

    conn.execute(Statement::from_string(backend, sql))
        .await
        .map_err(|e| Error::query(e.to_string()))?;

    Ok(())
}

/// Get the appropriate quote character for identifiers
fn get_quote_char(backend: DbBackend) -> char {
    match backend {
        DbBackend::Postgres | DbBackend::Sqlite => '"',
        DbBackend::MySql => '`',
    }
}

/// Get column type adjusted for the specific database backend
fn get_column_type_for_backend(col: &ColumnDef, backend: DbBackend) -> String {
    let base_type = &col.col_type;
    
    // Check if this is a Rust type (contains lowercase letters like i64, String, Option<...>)
    // SQL types are typically all uppercase
    let is_rust_type = base_type.chars().any(|c| c.is_ascii_lowercase());
    
    if is_rust_type {
        // Convert Rust type to database-specific SQL type
        let sql_type = rust_type_to_db_type(base_type, backend);
        
        // Handle auto-increment primary keys
        if col.auto_increment && col.primary_key {
            return match backend {
                DbBackend::Postgres => {
                    if sql_type.contains("BIGINT") || sql_type.contains("INT8") {
                        "BIGSERIAL".to_string()
                    } else {
                        "SERIAL".to_string()
                    }
                }
                DbBackend::MySql => {
                    let int_type = if sql_type.contains("BIGINT") { "BIGINT" } else { "INT" };
                    format!("{} AUTO_INCREMENT", int_type)
                }
                DbBackend::Sqlite => "INTEGER".to_string(),
            };
        }
        
        return sql_type;
    }
    
    // Legacy: Handle SQL types (for backward compatibility)
    let base_type_upper = base_type.to_uppercase();
    
    if col.auto_increment && col.primary_key {
        return match backend {
            DbBackend::Postgres => {
                if base_type_upper.contains("BIGINT") || base_type_upper.contains("INT8") {
                    "BIGSERIAL".to_string()
                } else {
                    "SERIAL".to_string()
                }
            }
            DbBackend::MySql => {
                let int_type = if base_type_upper.contains("BIGINT") { "BIGINT" } else { "INT" };
                format!("{} AUTO_INCREMENT", int_type)
            }
            DbBackend::Sqlite => "INTEGER".to_string(), // SQLite auto-increment uses INTEGER PRIMARY KEY
        };
    }
    
    // Convert types between databases
    match backend {
        DbBackend::Postgres => convert_to_postgres_type(&base_type_upper),
        DbBackend::MySql => convert_to_mysql_type(&base_type_upper),
        DbBackend::Sqlite => convert_to_sqlite_type(&base_type_upper),
    }
}

/// Convert generic type to PostgreSQL type
fn convert_to_postgres_type(col_type: &str) -> String {
    match col_type {
        "TINYINT" | "TINYINT UNSIGNED" => "SMALLINT".to_string(),
        "DATETIME" => "TIMESTAMP".to_string(),
        "DOUBLE" => "DOUBLE PRECISION".to_string(),
        "BLOB" | "LONGBLOB" | "MEDIUMBLOB" | "TINYBLOB" => "BYTEA".to_string(),
        "LONGTEXT" | "MEDIUMTEXT" | "TINYTEXT" => "TEXT".to_string(),
        "ENUM" => "TEXT".to_string(), // PostgreSQL doesn't have ENUM in the same way
        t => t.to_string(),
    }
}

/// Convert generic type to MySQL type
fn convert_to_mysql_type(col_type: &str) -> String {
    match col_type {
        "SERIAL" => "INT AUTO_INCREMENT".to_string(),
        "BIGSERIAL" => "BIGINT AUTO_INCREMENT".to_string(),
        "BYTEA" => "LONGBLOB".to_string(),
        "TIMESTAMPTZ" | "TIMESTAMP WITH TIME ZONE" => "DATETIME".to_string(),
        "DOUBLE PRECISION" => "DOUBLE".to_string(),
        "JSONB" => "JSON".to_string(),
        "UUID" => "CHAR(36)".to_string(),
        t => t.to_string(),
    }
}

/// Convert generic type to SQLite type  
fn convert_to_sqlite_type(col_type: &str) -> String {
    // SQLite has very flexible type affinity, but we map to common types
    let t = col_type.to_uppercase();
    
    if t.contains("INT") {
        "INTEGER".to_string()
    } else if t.contains("CHAR") || t.contains("TEXT") || t.contains("CLOB") || t == "UUID" {
        "TEXT".to_string()
    } else if t.contains("BLOB") || t == "BYTEA" {
        "BLOB".to_string()
    } else if t.contains("REAL") || t.contains("FLOA") || t.contains("DOUB") {
        "REAL".to_string()
    } else if t.contains("BOOL") {
        "INTEGER".to_string() // SQLite uses 0/1 for booleans
    } else if t.contains("DATE") || t.contains("TIME") || t.contains("TIMESTAMP") {
        "TEXT".to_string() // SQLite stores datetime as text or integer
    } else if t.contains("JSON") {
        "TEXT".to_string() // SQLite can use JSON functions on TEXT
    } else if t.contains("NUMERIC") || t.contains("DECIMAL") {
        "NUMERIC".to_string()
    } else {
        t
    }
}

/// Adjust default value for the specific backend
fn adjust_default_for_backend(default: &str, backend: DbBackend) -> String {
    match backend {
        DbBackend::Postgres => default.to_string(),
        DbBackend::MySql => {
            // MySQL doesn't support some PostgreSQL functions
            match default.to_uppercase().as_str() {
                "NOW()" => "CURRENT_TIMESTAMP".to_string(),
                "GEN_RANDOM_UUID()" => "(UUID())".to_string(),
                _ => default.to_string(),
            }
        }
        DbBackend::Sqlite => {
            // SQLite has limited default functions
            match default.to_uppercase().as_str() {
                "NOW()" | "CURRENT_TIMESTAMP" => "CURRENT_TIMESTAMP".to_string(),
                "CURRENT_DATE" => "CURRENT_DATE".to_string(),
                "CURRENT_TIME" => "CURRENT_TIME".to_string(),
                "GEN_RANDOM_UUID()" | "UUID()" => {
                    // SQLite doesn't have built-in UUID generation
                    "(lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))),2) || '-' || substr('89ab',abs(random()) % 4 + 1, 1) || substr(lower(hex(randomblob(2))),2) || '-' || lower(hex(randomblob(6))))".to_string()
                }
                _ => default.to_string(),
            }
        }
    }
}

/// Check if a column needs modification
fn needs_modification(defined: &ColumnDef, existing: &ExistingColumn, backend: DbBackend) -> bool {
    // Normalize types for comparison based on backend
    let defined_type = normalize_type_for_backend(&defined.col_type, backend);
    let existing_type = normalize_type_for_backend(&existing.data_type, backend);
    
    if defined_type != existing_type {
        return true;
    }
    
    if defined.nullable != existing.is_nullable {
        return true;
    }
    
    false
}

/// Normalize type names for comparison based on database backend
fn normalize_type_for_backend(type_name: &str, backend: DbBackend) -> String {
    let t = type_name.to_uppercase().trim().to_string();
    
    match backend {
        DbBackend::Postgres => normalize_postgres_type(&t),
        DbBackend::MySql => normalize_mysql_type(&t),
        DbBackend::Sqlite => normalize_sqlite_type(&t),
    }
}

/// Normalize PostgreSQL type names for comparison
fn normalize_postgres_type(type_name: &str) -> String {
    let t = type_name.to_uppercase();
    
    // Map common aliases
    match t.as_str() {
        "INT" | "INT4" | "SERIAL" => "INTEGER".to_string(),
        "INT8" | "BIGSERIAL" => "BIGINT".to_string(),
        "INT2" | "SMALLSERIAL" => "SMALLINT".to_string(),
        "BOOL" => "BOOLEAN".to_string(),
        "FLOAT4" => "REAL".to_string(),
        "FLOAT8" | "FLOAT" => "DOUBLE PRECISION".to_string(),
        "TIMESTAMPTZ" => "TIMESTAMP WITH TIME ZONE".to_string(),
        "TIMETZ" => "TIME WITH TIME ZONE".to_string(),
        s if s.starts_with("VARCHAR") || s.starts_with("CHARACTER VARYING") => "CHARACTER VARYING".to_string(),
        s if s.starts_with("CHAR(") => "CHARACTER".to_string(),
        _ => t,
    }
}

/// Normalize MySQL type names for comparison
fn normalize_mysql_type(type_name: &str) -> String {
    let t = type_name.to_uppercase();
    
    match t.as_str() {
        "INT" | "INTEGER" => "INT".to_string(),
        "BOOL" | "BOOLEAN" => "TINYINT".to_string(),
        "DOUBLE PRECISION" => "DOUBLE".to_string(),
        s if s.starts_with("VARCHAR") => "VARCHAR".to_string(),
        s if s.starts_with("CHAR(") => "CHAR".to_string(),
        _ => t,
    }
}

/// Normalize SQLite type names for comparison
fn normalize_sqlite_type(type_name: &str) -> String {
    let t = type_name.to_uppercase();
    
    // SQLite uses type affinity - normalize to the 5 storage classes
    if t.contains("INT") {
        "INTEGER".to_string()
    } else if t.contains("CHAR") || t.contains("CLOB") || t.contains("TEXT") {
        "TEXT".to_string()
    } else if t.contains("BLOB") || t.is_empty() {
        "BLOB".to_string()
    } else if t.contains("REAL") || t.contains("FLOA") || t.contains("DOUB") {
        "REAL".to_string()
    } else {
        "NUMERIC".to_string()
    }
}

/// Add a new column to an existing table
async fn add_column(
    conn: &DatabaseConnection,
    model: &ModelSchema,
    col: &ColumnDef,
) -> Result<()> {
    let backend = get_backend(conn);
    let quote_char = get_quote_char(backend);
    let col_type = get_column_type_for_backend(col, backend);
    
    // SQLite has limited ALTER TABLE support
    if backend == DbBackend::Sqlite {
        return add_column_sqlite(conn, model, col).await;
    }
    
    let table_ref = match backend {
        DbBackend::Postgres => format!("\"{}\".\"{}\"", model.schema_name, model.table_name),
        DbBackend::MySql => format!("`{}`", model.table_name),
        DbBackend::Sqlite => format!("\"{}\"", model.table_name),
    };
    
    let mut sql = format!(
        "ALTER TABLE {} ADD COLUMN {}{}{} {}",
        table_ref, quote_char, col.name, quote_char, col_type
    );
    
    if !col.nullable {
        // For NOT NULL columns, we need a default or the table must be empty
        if let Some(default) = &col.default {
            let adjusted_default = adjust_default_for_backend(default, backend);
            sql.push_str(&format!(" NOT NULL DEFAULT {}", adjusted_default));
        } else {
            // Add with a sensible default based on type, then remove default
            let temp_default = get_temp_default_for_backend(&col.col_type, backend);
            sql.push_str(&format!(" NOT NULL DEFAULT {}", temp_default));
            
            conn.execute(Statement::from_string(backend, sql))
                .await
                .map_err(|e| Error::query(e.to_string()))?;
            
            // Remove the temporary default (PostgreSQL and MySQL support this)
            if backend != DbBackend::Sqlite {
                let drop_default_sql = match backend {
                    DbBackend::Postgres => format!(
                        "ALTER TABLE {} ALTER COLUMN {}{}{} DROP DEFAULT",
                        table_ref, quote_char, col.name, quote_char
                    ),
                    DbBackend::MySql => format!(
                        "ALTER TABLE {} ALTER COLUMN {}{}{} DROP DEFAULT",
                        table_ref, quote_char, col.name, quote_char
                    ),
                    _ => return Ok(()),
                };
                conn.execute(Statement::from_string(backend, drop_default_sql))
                    .await
                    .map_err(|e| Error::query(e.to_string()))?;
            }
            
            return Ok(());
        }
    }
    
    if let Some(default) = &col.default {
        let adjusted_default = adjust_default_for_backend(default, backend);
        sql.push_str(&format!(" DEFAULT {}", adjusted_default));
    }

    conn.execute(Statement::from_string(backend, sql))
        .await
        .map_err(|e| Error::query(e.to_string()))?;

    Ok(())
}

/// Add column for SQLite (limited ALTER TABLE support)
async fn add_column_sqlite(
    conn: &DatabaseConnection,
    model: &ModelSchema,
    col: &ColumnDef,
) -> Result<()> {
    let col_type = get_column_type_for_backend(col, DbBackend::Sqlite);
    
    let mut sql = format!(
        "ALTER TABLE \"{}\" ADD COLUMN \"{}\" {}",
        model.table_name, col.name, col_type
    );
    
    // SQLite requires default values for NOT NULL columns in ALTER TABLE
    if !col.nullable {
        let default = col.default.as_ref()
            .map(|d| adjust_default_for_backend(d, DbBackend::Sqlite))
            .unwrap_or_else(|| get_temp_default_for_backend(&col.col_type, DbBackend::Sqlite));
        sql.push_str(&format!(" NOT NULL DEFAULT {}", default));
    } else if let Some(default) = &col.default {
        let adjusted_default = adjust_default_for_backend(default, DbBackend::Sqlite);
        sql.push_str(&format!(" DEFAULT {}", adjusted_default));
    }
    
    conn.execute(Statement::from_string(DbBackend::Sqlite, sql))
        .await
        .map_err(|e| Error::query(e.to_string()))?;
    
    Ok(())
}

/// Drop a column from an existing table (force sync)
/// 
/// ⚠️ WARNING: This will permanently delete the column and all its data!
async fn drop_column(
    conn: &DatabaseConnection,
    model: &ModelSchema,
    column_name: &str,
) -> Result<()> {
    let backend = get_backend(conn);
    
    // SQLite has limited ALTER TABLE support - dropping columns requires recreating the table
    if backend == DbBackend::Sqlite {
        return drop_column_sqlite(conn, model, column_name).await;
    }
    
    let quote_char = get_quote_char(backend);
    
    let table_ref = match backend {
        DbBackend::Postgres => format!("\"{}\".\"{}\"", model.schema_name, model.table_name),
        DbBackend::MySql => format!("`{}`", model.table_name),
        DbBackend::Sqlite => format!("\"{}\"", model.table_name),
    };
    
    let sql = format!(
        "ALTER TABLE {} DROP COLUMN {}{}{}",
        table_ref, quote_char, column_name, quote_char
    );
    
    conn.execute(Statement::from_string(backend, sql))
        .await
        .map_err(|e| Error::query(e.to_string()))?;
    
    Ok(())
}

/// Drop column for SQLite (requires table recreation)
/// 
/// SQLite before 3.35.0 doesn't support DROP COLUMN, so we need to recreate the table.
/// This is a destructive operation!
async fn drop_column_sqlite(
    conn: &DatabaseConnection,
    model: &ModelSchema,
    column_name: &str,
) -> Result<()> {
    // SQLite 3.35.0+ supports DROP COLUMN directly
    // Try it first, fall back to table recreation if it fails
    let drop_sql = format!(
        "ALTER TABLE \"{}\" DROP COLUMN \"{}\"",
        model.table_name, column_name
    );
    
    match conn.execute(Statement::from_string(DbBackend::Sqlite, drop_sql)).await {
        Ok(_) => return Ok(()),
        Err(_) => {
            // Fall back to table recreation for older SQLite versions
            eprintln!("    Note: SQLite doesn't support DROP COLUMN directly, recreating table...");
        }
    }
    
    // Get all existing columns except the one to drop
    let existing_columns = get_existing_columns(conn, "main", &model.table_name).await?;
    let remaining_columns: Vec<_> = existing_columns
        .iter()
        .filter(|c| c.name.to_lowercase() != column_name.to_lowercase())
        .collect();
    
    if remaining_columns.is_empty() {
        return Err(Error::query("Cannot drop all columns from a table"));
    }
    
    // Build column list for SELECT and new table
    let column_names: Vec<String> = remaining_columns
        .iter()
        .map(|c| format!("\"{}\"", c.name))
        .collect();
    let column_list = column_names.join(", ");
    
    // 1. Create a backup table
    let backup_table = format!("__{}_backup", model.table_name);
    let create_backup_sql = format!(
        "CREATE TABLE \"{}\" AS SELECT {} FROM \"{}\"",
        backup_table, column_list, model.table_name
    );
    conn.execute(Statement::from_string(DbBackend::Sqlite, create_backup_sql))
        .await
        .map_err(|e| Error::query(e.to_string()))?;
    
    // 2. Drop the original table
    let drop_original_sql = format!("DROP TABLE \"{}\"", model.table_name);
    conn.execute(Statement::from_string(DbBackend::Sqlite, drop_original_sql))
        .await
        .map_err(|e| Error::query(e.to_string()))?;
    
    // 3. Rename backup to original
    let rename_sql = format!(
        "ALTER TABLE \"{}\" RENAME TO \"{}\"",
        backup_table, model.table_name
    );
    conn.execute(Statement::from_string(DbBackend::Sqlite, rename_sql))
        .await
        .map_err(|e| Error::query(e.to_string()))?;
    
    Ok(())
}

/// Get a temporary default value for a type (backend-aware)
fn get_temp_default_for_backend(col_type: &str, backend: DbBackend) -> String {
    let t = col_type.to_uppercase();
    
    if t.contains("INT") || t.contains("SERIAL") {
        "0".to_string()
    } else if t.contains("BOOL") {
        match backend {
            DbBackend::Postgres => "false".to_string(),
            DbBackend::MySql | DbBackend::Sqlite => "0".to_string(),
        }
    } else if t.contains("FLOAT") || t.contains("DOUBLE") || t.contains("REAL") || t.contains("NUMERIC") || t.contains("DECIMAL") {
        "0.0".to_string()
    } else if t.contains("TIMESTAMP") || t.contains("DATETIME") {
        match backend {
            DbBackend::Postgres => "NOW()".to_string(),
            DbBackend::MySql | DbBackend::Sqlite => "CURRENT_TIMESTAMP".to_string(),
        }
    } else if t.contains("DATE") {
        "CURRENT_DATE".to_string()
    } else if t.contains("TIME") && !t.contains("TIMESTAMP") {
        "CURRENT_TIME".to_string()
    } else if t.contains("UUID") {
        match backend {
            DbBackend::Postgres => "gen_random_uuid()".to_string(),
            DbBackend::MySql => "(UUID())".to_string(),
            DbBackend::Sqlite => {
                // SQLite UUID fallback
                "(lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' || substr(lower(hex(randomblob(2))),2) || '-' || substr('89ab',abs(random()) % 4 + 1, 1) || substr(lower(hex(randomblob(2))),2) || '-' || lower(hex(randomblob(6))))".to_string()
            }
        }
    } else if t.contains("JSON") {
        match backend {
            DbBackend::Postgres | DbBackend::MySql => "'{}'".to_string(),
            DbBackend::Sqlite => "'{}'".to_string(),
        }
    } else if t.contains("ARRAY") {
        match backend {
            DbBackend::Postgres => "'{}'".to_string(),
            _ => "'[]'".to_string(),
        }
    } else {
        "''".to_string()  // Empty string for text types
    }
}

/// Modify an existing column
async fn modify_column(
    conn: &DatabaseConnection,
    model: &ModelSchema,
    defined: &ColumnDef,
    existing: &ExistingColumn,
) -> Result<()> {
    let backend = get_backend(conn);
    let quote_char = get_quote_char(backend);
    
    let full_col = match backend {
        DbBackend::Postgres => format!("{}.{}.{}", model.schema_name, model.table_name, defined.name),
        _ => format!("{}.{}", model.table_name, defined.name),
    };
    
    let defined_type = normalize_type_for_backend(&defined.col_type, backend);
    let existing_type = normalize_type_for_backend(&existing.data_type, backend);
    let target_col_type = get_column_type_for_backend(defined, backend);
    
    // SQLite doesn't support column modification directly - requires table recreation
    if backend == DbBackend::Sqlite {
        eprintln!(
            "  ⚠️  SQLite doesn't support column modification. Column {} requires manual migration.",
            full_col
        );
        return Ok(());
    }
    
    let table_ref = match backend {
        DbBackend::Postgres => format!("\"{}\".\"{}\"", model.schema_name, model.table_name),
        DbBackend::MySql => format!("`{}`", model.table_name),
        DbBackend::Sqlite => format!("\"{}\"", model.table_name),
    };
    
    // Check if type change is needed
    if defined_type != existing_type {
        eprintln!(
            "  ⚠️  Modifying column type: {} ({} -> {})",
            full_col, existing.data_type, target_col_type
        );
        
        let sql = match backend {
            DbBackend::Postgres => {
                // Try to alter the type with USING clause for safe conversion
                format!(
                    "ALTER TABLE {} ALTER COLUMN {}{}{} TYPE {} USING {}{}{}::{}",
                    table_ref, quote_char, defined.name, quote_char,
                    target_col_type, quote_char, defined.name, quote_char, target_col_type
                )
            }
            DbBackend::MySql => {
                // MySQL uses MODIFY COLUMN
                let nullable = if defined.nullable { "NULL" } else { "NOT NULL" };
                let default = defined.default.as_ref()
                    .map(|d| format!(" DEFAULT {}", adjust_default_for_backend(d, backend)))
                    .unwrap_or_default();
                format!(
                    "ALTER TABLE {} MODIFY COLUMN {}{}{} {} {}{}",
                    table_ref, quote_char, defined.name, quote_char,
                    target_col_type, nullable, default
                )
            }
            DbBackend::Sqlite => {
                // SQLite doesn't support ALTER COLUMN
                return Ok(());
            }
        };
        
        match conn.execute(Statement::from_string(backend, sql)).await {
            Ok(_) => {
                eprintln!("  ✅ Modified column type: {}", full_col);
            }
            Err(e) => {
                eprintln!(
                    "  ❌ Failed to modify column type {}: {}. Manual migration required.",
                    full_col, e
                );
                // Don't fail the whole sync, just log the error
            }
        }
    }
    
    // Check if nullable change is needed
    if defined.nullable != existing.is_nullable {
        modify_column_nullable(conn, &table_ref, defined, existing, backend, &full_col).await?;
    }
    
    Ok(())
}

/// Modify column nullable constraint
async fn modify_column_nullable(
    conn: &DatabaseConnection,
    table_ref: &str,
    defined: &ColumnDef,
    _existing: &ExistingColumn,
    backend: DbBackend,
    full_col: &str,
) -> Result<()> {
    let quote_char = get_quote_char(backend);
    
    if defined.nullable {
        // Make nullable (safe operation)
        let sql = match backend {
            DbBackend::Postgres => format!(
                "ALTER TABLE {} ALTER COLUMN {}{}{} DROP NOT NULL",
                table_ref, quote_char, defined.name, quote_char
            ),
            DbBackend::MySql => {
                let col_type = get_column_type_for_backend(defined, backend);
                let default = defined.default.as_ref()
                    .map(|d| format!(" DEFAULT {}", adjust_default_for_backend(d, backend)))
                    .unwrap_or_default();
                format!(
                    "ALTER TABLE {} MODIFY COLUMN {}{}{} {} NULL{}",
                    table_ref, quote_char, defined.name, quote_char, col_type, default
                )
            }
            DbBackend::Sqlite => return Ok(()), // SQLite doesn't support this
        };
        
        conn.execute(Statement::from_string(backend, sql))
            .await
            .map_err(|e| Error::query(e.to_string()))?;
        eprintln!("  ✅ Made column nullable: {}", full_col);
    } else {
        // Make NOT NULL (might fail if there are NULL values)
        eprintln!(
            "  ⚠️  Making column NOT NULL: {} (will fail if NULL values exist)",
            full_col
        );
        
        let sql = match backend {
            DbBackend::Postgres => format!(
                "ALTER TABLE {} ALTER COLUMN {}{}{} SET NOT NULL",
                table_ref, quote_char, defined.name, quote_char
            ),
            DbBackend::MySql => {
                let col_type = get_column_type_for_backend(defined, backend);
                let default = defined.default.as_ref()
                    .map(|d| format!(" DEFAULT {}", adjust_default_for_backend(d, backend)))
                    .unwrap_or_default();
                format!(
                    "ALTER TABLE {} MODIFY COLUMN {}{}{} {} NOT NULL{}",
                    table_ref, quote_char, defined.name, quote_char, col_type, default
                )
            }
            DbBackend::Sqlite => return Ok(()), // SQLite doesn't support this
        };
        
        match conn.execute(Statement::from_string(backend, sql)).await {
            Ok(_) => {
                eprintln!("  ✅ Made column NOT NULL: {}", full_col);
            }
            Err(e) => {
                eprintln!(
                    "  ❌ Failed to make column NOT NULL {}: {}. Update NULL values first.",
                    full_col, e
                );
            }
        }
    }
    
    Ok(())
}

/// Maps SeaORM column types to PostgreSQL types
pub fn sea_orm_to_postgres_type(col_type: &crate::sea_orm::ColumnType) -> String {
    use crate::sea_orm::ColumnType;
    use crate::sea_orm::sea_query::StringLen;
    
    match col_type {
        ColumnType::Char(len) => match len {
            Some(l) => format!("CHAR({})", l),
            None => "CHAR(1)".to_string(),
        },
        ColumnType::String(len) => match len {
            StringLen::N(l) => format!("VARCHAR({})", l),
            StringLen::None | StringLen::Max => "TEXT".to_string(),
        },
        ColumnType::Text => "TEXT".to_string(),
        ColumnType::TinyInteger => "SMALLINT".to_string(),
        ColumnType::SmallInteger => "SMALLINT".to_string(),
        ColumnType::Integer => "INTEGER".to_string(),
        ColumnType::BigInteger => "BIGINT".to_string(),
        ColumnType::TinyUnsigned => "SMALLINT".to_string(),
        ColumnType::SmallUnsigned => "INTEGER".to_string(),
        ColumnType::Unsigned => "BIGINT".to_string(),
        ColumnType::BigUnsigned => "BIGINT".to_string(),
        ColumnType::Float => "REAL".to_string(),
        ColumnType::Double => "DOUBLE PRECISION".to_string(),
        ColumnType::Decimal(precision) => match precision {
            Some((p, s)) => format!("DECIMAL({}, {})", p, s),
            None => "DECIMAL".to_string(),
        },
        ColumnType::DateTime => "TIMESTAMP".to_string(),
        ColumnType::Timestamp => "TIMESTAMP".to_string(),
        ColumnType::TimestampWithTimeZone => "TIMESTAMPTZ".to_string(),
        ColumnType::Time => "TIME".to_string(),
        ColumnType::Date => "DATE".to_string(),
        ColumnType::Year => "INTEGER".to_string(),
        ColumnType::Binary(_) | ColumnType::VarBinary(_) => "BYTEA".to_string(),
        ColumnType::Bit(_) => "BIT".to_string(),
        ColumnType::VarBit(_) => "VARBIT".to_string(),
        ColumnType::Boolean => "BOOLEAN".to_string(),
        ColumnType::Money(precision) => match precision {
            Some((p, s)) => format!("DECIMAL({}, {})", p, s),
            None => "MONEY".to_string(),
        },
        ColumnType::Json => "JSON".to_string(),
        ColumnType::JsonBinary => "JSONB".to_string(),
        ColumnType::Uuid => "UUID".to_string(),
        ColumnType::Array(inner) => format!("{}[]", sea_orm_to_postgres_type(inner)),
        ColumnType::Cidr => "CIDR".to_string(),
        ColumnType::Inet => "INET".to_string(),
        ColumnType::MacAddr => "MACADDR".to_string(),
        ColumnType::LTree => "LTREE".to_string(),
        _ => "TEXT".to_string(), // Fallback
    }
}

/// Normalizes a Rust type string by removing whitespace
/// This is used to handle stringify! output like "Option < i64 >" -> "Option<i64>"
pub fn normalize_rust_type(rust_type: &str) -> String {
    rust_type.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Maps a Rust type name to the appropriate SQL type for the given database backend
pub fn rust_type_to_db_type(rust_type: &str, backend: crate::internal::DbBackend) -> String {
    use crate::internal::DbBackend;
    
    // Normalize the type by removing all whitespace (handles "Option < i64 >" from stringify!)
    let normalized: String = rust_type.chars().filter(|c| !c.is_whitespace()).collect();
    
    // Handle Option types - extract inner type
    let inner_type = normalized
        .strip_prefix("Option<")
        .and_then(|s| s.strip_suffix(">"))
        .unwrap_or(&normalized);
    
    match backend {
        DbBackend::Postgres => rust_type_to_postgres_internal(inner_type),
        DbBackend::MySql => rust_type_to_mysql_internal(inner_type),
        DbBackend::Sqlite => rust_type_to_sqlite_internal(inner_type),
    }
}

/// Internal: Maps Rust type to PostgreSQL type
fn rust_type_to_postgres_internal(rust_type: &str) -> String {
    // Handle common DateTime patterns
    if rust_type.contains("DateTime") {
        return "TIMESTAMPTZ".to_string();
    }
    if rust_type.contains("NaiveDateTime") {
        return "TIMESTAMP".to_string();
    }
    
    match rust_type {
        // Integer types
        "i8" | "u8" => "SMALLINT".to_string(),
        "i16" | "u16" => "SMALLINT".to_string(), 
        "i32" => "INTEGER".to_string(),
        "u32" => "BIGINT".to_string(),
        "i64" => "BIGINT".to_string(),
        "u64" | "i128" | "u128" => "NUMERIC".to_string(),
        "isize" | "usize" => "BIGINT".to_string(),
        
        // Float types
        "f32" => "REAL".to_string(),
        "f64" => "DOUBLE PRECISION".to_string(),
        
        // Boolean
        "bool" => "BOOLEAN".to_string(),
        
        // String types
        "String" | "&str" => "TEXT".to_string(),
        
        // UUID
        "Uuid" => "UUID".to_string(),
        
        // JSON
        "Json" | "JsonValue" | "serde_json::Value" | "Value" => "JSONB".to_string(),
        
        // Binary
        "Vec<u8>" | "Bytes" => "BYTEA".to_string(),
        
        // Decimal
        "Decimal" | "BigDecimal" => "DECIMAL".to_string(),
        
        // Array types
        "Vec<i32>" | "IntArray" => "INTEGER[]".to_string(),
        "Vec<i64>" | "BigIntArray" => "BIGINT[]".to_string(),
        "Vec<String>" | "TextArray" => "TEXT[]".to_string(),
        "Vec<bool>" | "BoolArray" => "BOOLEAN[]".to_string(),
        "Vec<f64>" | "FloatArray" => "DOUBLE PRECISION[]".to_string(),
        
        // Default
        _ => "TEXT".to_string(),
    }
}

/// Internal: Maps Rust type to MySQL type
fn rust_type_to_mysql_internal(rust_type: &str) -> String {
    // Handle common DateTime patterns
    if rust_type.contains("DateTime") || rust_type.contains("NaiveDateTime") {
        return "DATETIME".to_string();
    }
    
    match rust_type {
        // Integer types
        "i8" | "u8" => "TINYINT".to_string(),
        "i16" => "SMALLINT".to_string(),
        "u16" => "SMALLINT UNSIGNED".to_string(),
        "i32" => "INT".to_string(),
        "u32" => "INT UNSIGNED".to_string(),
        "i64" => "BIGINT".to_string(),
        "u64" => "BIGINT UNSIGNED".to_string(),
        "i128" | "u128" => "DECIMAL(65,0)".to_string(),
        "isize" | "usize" => "BIGINT".to_string(),
        
        // Float types
        "f32" => "FLOAT".to_string(),
        "f64" => "DOUBLE".to_string(),
        
        // Boolean
        "bool" => "TINYINT(1)".to_string(),
        
        // String types
        "String" | "&str" => "TEXT".to_string(),
        
        // UUID
        "Uuid" => "CHAR(36)".to_string(),
        
        // JSON
        "Json" | "JsonValue" | "serde_json::Value" | "Value" => "JSON".to_string(),
        
        // Binary
        "Vec<u8>" | "Bytes" => "LONGBLOB".to_string(),
        
        // Decimal
        "Decimal" | "BigDecimal" => "DECIMAL(65,30)".to_string(),
        
        // Default
        _ => "TEXT".to_string(),
    }
}

/// Internal: Maps Rust type to SQLite type
fn rust_type_to_sqlite_internal(rust_type: &str) -> String {
    // Handle common DateTime patterns - SQLite stores as TEXT
    if rust_type.contains("DateTime") || rust_type.contains("NaiveDateTime") ||
       rust_type.contains("NaiveDate") || rust_type.contains("NaiveTime") {
        return "TEXT".to_string();
    }
    
    match rust_type {
        // Integer types - SQLite uses INTEGER for all integer types
        "i8" | "u8" | "i16" | "u16" | "i32" | "u32" | "i64" | "u64" |
        "i128" | "u128" | "isize" | "usize" => "INTEGER".to_string(),
        
        // Float types
        "f32" | "f64" => "REAL".to_string(),
        
        // Boolean - SQLite uses INTEGER (0/1)
        "bool" => "INTEGER".to_string(),
        
        // String types
        "String" | "&str" => "TEXT".to_string(),
        
        // UUID - stored as TEXT in SQLite
        "Uuid" => "TEXT".to_string(),
        
        // JSON - stored as TEXT in SQLite
        "Json" | "JsonValue" | "serde_json::Value" | "Value" => "TEXT".to_string(),
        
        // Binary
        "Vec<u8>" | "Bytes" => "BLOB".to_string(),
        
        // Decimal - stored as TEXT or NUMERIC in SQLite
        "Decimal" | "BigDecimal" => "NUMERIC".to_string(),
        
        // Default
        _ => "TEXT".to_string(),
    }
}

/// Maps a Rust type name to PostgreSQL type (for backward compatibility)
/// Deprecated: Use rust_type_to_db_type instead for database-specific mapping
pub fn rust_type_to_postgres(rust_type: &str) -> &'static str {
    // Normalize the type by removing all whitespace (handles "Option < i64 >" from stringify!)
    let normalized: String = rust_type.chars().filter(|c| !c.is_whitespace()).collect();
    
    // Handle Option types
    let inner_type = normalized
        .strip_prefix("Option<")
        .and_then(|s| s.strip_suffix(">"))
        .unwrap_or(&normalized);
    
    // Handle common DateTime patterns
    if inner_type.contains("DateTime") {
        return "TIMESTAMPTZ";
    }
    if inner_type.contains("NaiveDateTime") {
        return "TIMESTAMP";
    }
    
    match inner_type {
        // Integer types
        "i8" | "u8" => "SMALLINT",
        "i16" | "u16" => "SMALLINT", 
        "i32" => "INTEGER",
        "u32" => "BIGINT",
        "i64" => "BIGINT",
        "u64" | "i128" | "u128" => "NUMERIC",
        "isize" | "usize" => "BIGINT",
        
        // Float types
        "f32" => "REAL",
        "f64" => "DOUBLE PRECISION",
        
        // Boolean
        "bool" => "BOOLEAN",
        
        // String types
        "String" | "&str" => "TEXT",
        
        // UUID
        "Uuid" => "UUID",
        
        // JSON
        "Json" | "JsonValue" | "serde_json::Value" | "Value" => "JSONB",
        
        // Binary
        "Vec<u8>" | "Bytes" => "BYTEA",
        
        // Decimal
        "Decimal" | "BigDecimal" => "DECIMAL",
        
        // Default
        _ => "TEXT",
    }
}
