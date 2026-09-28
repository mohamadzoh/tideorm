//! Database Schema Synchronization Module
//!
//! This module applies model schema definitions to a live database.
//!
//! Use it in local development and tests when you want missing tables or
//! columns created automatically. It is not a replacement for migrations in
//! deployed environments.
//!
//! TideORM models describe themselves with a `ModelSchema`, which the macros
//! generate. Register them through `TideConfig::models::<(... )>()`,
//! `TideConfig::models_matching("src/models/*.model.rs")`, or the corresponding
//! `SyncRegistry` helpers, and enable synchronization with `sync(true)`.
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
//! - Reports columns the model no longer declares
//! - **Does NOT drop existing tables or columns**
//!
//! Sync creates the indexes a model declares with `#[index]` and
//! `#[unique_index]`, but no foreign keys or enum types; declare those in a
//! migration.
//!
//! A model's `#[tideorm(schema = "...")]` qualifies every statement sync issues
//! on PostgreSQL, so the schema has to exist before sync runs. MySQL and SQLite
//! ignore it: their "schema" is the connected database.
//!
//! ### Force Sync (`force_sync(true)`)
//!
//! - Drops and recreates the table of every registered model
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

use crate::database::Database;
use crate::error::Result;
use crate::{tide_info, tide_warn};

mod registry;
mod schema;

pub(crate) use registry::registered_column_type;
#[doc(hidden)]
pub use registry::{ColumnTypeOf, CompiledModelRegistration};
use registry::{MODEL_SCHEMAS, register_compiled_models_matching};
pub use registry::{RegisterModels, SyncModel};
pub(crate) use schema::decode_table_exists;
use schema::sync_model_schemas;
pub use schema::{ColumnDef, ModelSchema};

/// Registry of the model schemas schema sync applies.
pub struct SyncRegistry;

impl SyncRegistry {
    /// Get the number of registered TideORM model schemas
    pub fn schema_count() -> usize {
        MODEL_SCHEMAS.read().len()
    }

    /// Clear all registered models (for testing)
    pub fn clear() {
        MODEL_SCHEMAS.write().clear();
    }

    /// Register a TideORM model schema for synchronization, once per table:
    /// a table of the same name in another schema is another table.
    pub fn register_schema(schema: ModelSchema) {
        let mut schemas = MODEL_SCHEMAS.write();
        if !schemas.iter().any(|registered| {
            registered.table_name == schema.table_name
                && registered.schema_name == schema.schema_name
        }) {
            schemas.push(schema);
        }
    }

    /// Register all compiled TideORM models whose source file path matches a glob pattern.
    ///
    /// This matches against the source path captured from each `#[tideorm::model]` invocation,
    /// so modules still need to be part of the crate through normal Rust `mod` declarations.
    /// Supported wildcards are `*` for one path segment and `**` across directories.
    pub fn register_models_matching(pattern: &str) -> usize {
        register_compiled_models_matching(pattern)
    }

    /// Get all registered TideORM model schemas
    pub fn get_all_schemas() -> Vec<ModelSchema> {
        MODEL_SCHEMAS.read().clone()
    }
}

/// Synchronize all registered models with the database.
///
/// Creates the tables that are missing and adds the columns an existing table
/// lacks; see the [module documentation](self) for what it leaves alone.
///
/// # Warning
///
/// **DO NOT use in production!** This is for development only.
/// Use proper migrations for production deployments.
pub async fn sync_database(db: &Database) -> Result<()> {
    sync_database_with_options(db, false).await
}

/// Synchronize all registered models, optionally recreating their tables.
///
/// With `force_sync`, the table of every registered model is dropped and
/// created afresh instead of being reconciled.
///
/// # ⚠️ DANGER
///
/// **DO NOT use in production!** This is for development only.
/// `force_sync` deletes the tables and all of their data.
pub async fn sync_database_with_options(db: &Database, force_sync: bool) -> Result<()> {
    if force_sync {
        tide_warn!(
            "Database FORCE sync mode is ENABLED - registered tables are dropped and recreated!"
        );
    } else {
        tide_warn!("Database sync mode is ENABLED - DO NOT use in production!");
    }

    let schema_count = SyncRegistry::schema_count();
    if schema_count == 0 {
        tide_info!("No models registered for sync");
        return Ok(());
    }

    tide_info!("Syncing {} model(s)...", schema_count);
    sync_model_schemas(db, force_sync).await?;

    tide_info!("Database sync completed");
    Ok(())
}
