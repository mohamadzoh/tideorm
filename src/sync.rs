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
//! Register TideORM models through `TideConfig::models::<(... )>()`,
//! `TideConfig::models_matching("src/models/*.model.rs")`, or the corresponding
//! `SyncRegistry` helpers, and enable synchronization with `sync(true)`.
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
use crate::internal::{Backend, EntityTrait, Schema, SchemaBuilder};
use crate::{tide_debug, tide_info, tide_warn};

mod registry;
mod schema;

#[doc(hidden)]
pub use registry::CompiledModelRegistration;
pub use registry::{RegisterModels, SyncModel};
use registry::{
    register_compiled_models_matching, with_entity_registry, with_entity_registry_mut,
    with_model_schemas, with_model_schemas_mut,
};
use schema::sync_model_schemas;
pub use schema::{ColumnDef, ModelSchema, normalize_rust_type};

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
        with_entity_registry_mut(|fns| {
            let register_fn: EntityRegistrationFn =
                Box::new(|builder: SchemaBuilder| builder.register(E::default()));
            fns.push(register_fn);
        });
    }

    /// Build a SchemaBuilder with all registered entities
    ///
    /// Uses the current ORM engine's native SchemaBuilder.register() for each entity.
    pub fn build_schema_builder(backend: Backend) -> SchemaBuilder {
        with_entity_registry(|fns| {
            let schema = Schema::new(backend.into());
            let mut builder = schema.builder();
            for register_fn in fns.iter() {
                builder = register_fn(builder);
            }
            builder
        })
    }

    /// Get the number of registered entities
    pub fn entity_count() -> usize {
        with_entity_registry(|fns| fns.len())
    }

    /// Get the number of registered TideORM model schemas
    pub fn schema_count() -> usize {
        with_model_schemas(|schemas| schemas.len())
    }

    /// Clear all registered models (for testing)
    pub fn clear() {
        with_entity_registry_mut(|fns| fns.clear());
        with_model_schemas_mut(|schemas| schemas.clear());
    }

    /// Register a TideORM model schema for synchronization
    pub fn register_schema(schema: ModelSchema) {
        with_model_schemas_mut(|schemas| {
            if !schemas.iter().any(|s| s.table_name == schema.table_name) {
                schemas.push(schema);
            }
        });
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
        with_model_schemas(|schemas| schemas.clone())
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
