//! Database Schema Synchronization Module
//!
//! This module applies model schema definitions to a live database.
//!
//! Use it in local development and tests when you want missing tables or
//! columns created automatically. It is not a replacement for migrations in
//! deployed environments.
//!
//! TideORM models describe themselves with a `ModelSchema`, which the macros
//! generate. Discover all linked models with `TideConfig::auto_models()`, or
//! select a subset with `TideConfig::models::<User>()` or
//! `TideConfig::models::<(User, Post)>()`, or by source path with
//! `TideConfig::models_matching("src/models/*")`. Enable synchronization
//! with `sync(true)`.
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
//! - Adds missing columns to existing tables. A `NOT NULL` column without a
//!   default can be added only to an empty table (and not on SQLite, which
//!   adds it as nullable); with rows in the table, sync fails and names the
//!   column, since its rows would hold `NULL`
//! - Reports columns the model no longer declares
//! - **Does NOT drop existing tables or columns**
//!
//! Sync creates the indexes a model declares with `#[index]` and
//! `#[unique_index]`, but no foreign keys or enum types; declare those in a
//! migration.
//! An index name owned by another table is an error; sync cannot enforce the
//! declared index by silently reusing that name.
//!
//! A model's `#[tideorm(schema = "...")]` qualifies every statement sync issues
//! on PostgreSQL, so the schema has to exist before sync runs. MySQL and SQLite
//! ignore it: their "schema" is the connected database.
//!
//! ### Force Sync (`force_sync(true)`)
//!
//! - Drops the table of every registered model, a table before the registered
//!   tables its foreign keys reference, and then creates them all
//!
//! ## ⚠️ Warning
//!
//! **Do not use sync mode in production.** It can still fail or damage data when
//! a schema change is not additive. Use explicit migrations for deployed systems.
//!
//! **Do not use force_sync in production.** It deletes tables and their data.
//! A process whose `TIDEORM_ENV` is `production` refuses it unless
//! `TIDEORM_ALLOW_FORCE_SYNC` is `1` or `true`.
//!
//! Enable synchronization with `TideConfig::init().sync(true)` or call
//! `Database::sync()` directly after connecting.

use crate::database::Database;
use crate::error::Result;
use crate::{tide_info, tide_warn};

mod recreate;
mod registry;
mod schema;

#[doc(hidden)]
pub use registry::{ColumnTypeOf, CompiledModelRegistration};
use registry::{MODEL_SCHEMAS, register_compiled_models_matching};
pub use registry::{RegisterModels, SyncModel};
pub(crate) use registry::{
    registered_column_names, registered_column_type, registered_encrypted_column,
};
use schema::sync_model_schemas;
pub use schema::{ColumnDef, ModelSchema};

/// Registry of the model schemas schema sync applies.
pub struct SyncRegistry;

/// Append each of `added` whose name none of `known` has.
fn push_missing_by_name<T>(known: &mut Vec<T>, added: Vec<T>, name: impl Fn(&T) -> &String) {
    for item in added {
        if !known.iter().any(|existing| name(existing) == name(&item)) {
            known.push(item);
        }
    }
}

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
    ///
    /// Another model's schema of a table already registered adds the columns
    /// and indexes the table lacks, so two models of one table each get the
    /// columns they read; a column both declare keeps the first definition.
    pub fn register_schema(schema: ModelSchema) {
        let mut schemas = MODEL_SCHEMAS.write();
        let registered = schemas.iter_mut().find(|registered| {
            registered.table_name == schema.table_name
                && registered.schema_name == schema.schema_name
        });
        let Some(registered) = registered else {
            schemas.push(schema);
            return;
        };
        push_missing_by_name(&mut registered.columns, schema.columns, |column| {
            &column.name
        });
        push_missing_by_name(&mut registered.indexes, schema.indexes, |index| &index.name);
    }

    /// Register every compiled model, independent of its source file layout.
    ///
    /// `#[tideorm::model]` submits metadata to `inventory` automatically. Only
    /// modules linked into the program participate. Explicit registration and
    /// path selection remain available for applications that sync a subset.
    pub fn register_all_models() -> usize {
        let mut count = 0;
        for model in inventory::iter::<CompiledModelRegistration> {
            Self::register_schema((model.sync_schema)());
            count += 1;
        }
        count
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

/// How [`sync_database_with`] brings a registered model's table in line with
/// the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncMode {
    /// Create the tables that are missing and add the columns an existing
    /// table lacks; see the [module documentation](self) for what it leaves
    /// alone.
    #[default]
    Additive,
    /// Drop the table of every registered model and create it afresh,
    /// deleting its rows.
    Recreate,
}

/// Synchronize all registered models with the database, additively
/// ([`SyncMode::Additive`]).
///
/// # Warning
///
/// **DO NOT use in production!** This is for development only.
/// Use proper migrations for production deployments.
pub async fn sync_database(db: &Database) -> Result<()> {
    sync_database_with(db, SyncMode::Additive).await
}

/// Synchronize all registered models with the database in `mode`.
///
/// # ⚠️ DANGER
///
/// **DO NOT use in production!** This is for development only.
/// [`SyncMode::Recreate`] deletes the tables and all of their data; it is an
/// [`Error::Configuration`](crate::Error::Configuration) where `TIDEORM_ENV` is
/// `production`, unless `TIDEORM_ALLOW_FORCE_SYNC` is `1` or `true`.
pub async fn sync_database_with(db: &Database, mode: SyncMode) -> Result<()> {
    let force_sync = mode == SyncMode::Recreate;
    if force_sync {
        recreate::refuse_in_production(
            std::env::var("TIDEORM_ENV").ok().as_deref(),
            crate::logging::env_flag_enabled("TIDEORM_ALLOW_FORCE_SYNC"),
        )?;
        tide_warn!(
            "Database FORCE sync mode is ENABLED - registered tables are dropped and recreated!"
        );
    } else {
        tide_warn!("Database sync mode is ENABLED - DO NOT use in production!");
    }

    let schema_count = SyncRegistry::schema_count();
    if schema_count == 0 {
        let mut available: Vec<_> = inventory::iter::<CompiledModelRegistration>
            .into_iter()
            .map(|model| (model.table_name)())
            .collect();
        available.sort_unstable();
        available.dedup();
        return Err(crate::Error::query(format!(
            "No models registered for schema sync; call auto_models() or models::<YourModel>(). Available compiled model tables: {}",
            if available.is_empty() {
                "(none; include your model modules with mod)".to_owned()
            } else {
                available.join(", ")
            }
        )));
    }

    tide_info!("Syncing {} model(s)...", schema_count);
    sync_model_schemas(db, force_sync).await?;

    tide_info!("Database sync completed");
    Ok(())
}
