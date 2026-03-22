//! Database migration system
//!
//! This module provides a schema migration system for TideORM.
//!
//! ## Features
//!
//! - Create, alter, and drop tables
//! - Add, modify, and remove columns
//! - Create and drop indexes
//! - Track applied migrations in the database
//! - Rollback support
//!
//! ## Example
//!
//! ```rust,ignore
//! use tideorm::migration::*;
//! use tideorm::prelude::*;
//!
//! // Define a migration
//! struct CreateUsersTable;
//!
//! #[async_trait]
//! impl Migration for CreateUsersTable {
//!     fn version(&self) -> &str { "20260106_001" }
//!     fn name(&self) -> &str { "create_users_table" }
//!
//!     async fn up(&self, schema: &mut Schema) -> Result<()> {
//!         schema.create_table("users", |t| {
//!             t.id();
//!             t.string("email").unique();
//!             t.string("name");
//!             t.boolean("active").default(true);
//!             t.timestamps();
//!         }).await
//!     }
//!
//!     async fn down(&self, schema: &mut Schema) -> Result<()> {
//!         schema.drop_table("users").await
//!     }
//! }
//!
//! // Run migrations
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     TideConfig::init()
//!         .database("postgres://localhost/myapp")
//!         .connect()
//!         .await?;
//!
//!     Migrator::new()
//!         .add(CreateUsersTable)
//!         .run()
//!         .await?;
//!
//!     Ok(())
//! }
//! ```

use crate::config::DatabaseType;
use crate::database::Database;
use crate::{tide_debug, tide_info};

mod alter;
mod api;
mod migrator;
mod schema;
#[allow(missing_docs)]
mod table;
mod types;

pub use alter::{AlterColumnBuilder, AlterTableBuilder};
pub use api::{Migration, MigrationInfo, MigrationResult, MigrationStatus};
pub use async_trait::async_trait;
pub use migrator::Migrator;
pub use schema::Schema;
pub use table::{ColumnBuilder, CompositePrimaryKey, TableBuilder, UniqueConstraint};
pub use types::{ColumnType, DefaultValue};

pub(super) fn detect_database_type(db: &Database) -> DatabaseType {
    db.backend()
}

pub(super) fn quote_identifier_for_db(name: &str, db_type: DatabaseType) -> String {
    match db_type {
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            format!("`{}`", name.replace('`', "``"))
        }
        _ => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

pub(super) fn quote_migration_identifier(name: &str, db_type: DatabaseType) -> String {
    quote_identifier_for_db(name, db_type)
}

pub(super) fn log_migration_sql(sql: &str) {
    if std::env::var("TIDE_LOG_QUERIES").is_ok() {
        tide_debug!("Migration SQL: {}", sql);
    }
}

pub(super) fn log_migration_start(version: &str, name: &str) {
    tide_info!("Running migration: {} - {}", version, name);
}

pub(super) fn log_migration_complete(version: &str, name: &str) {
    tide_info!("Completed migration: {} - {}", version, name);
}

pub(super) fn log_migration_rollback(version: &str, name: &str) {
    tide_info!("Rolling back migration: {} - {}", version, name);
}

#[cfg(test)]
#[path = "../testing/migration_tests.rs"]
mod tests;
