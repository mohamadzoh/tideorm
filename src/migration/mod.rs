//! Database migration system
//!
//! This module defines TideORM's schema migration entry points.
//!
//! Use this path for additive, reviewable schema changes in deployed systems.
//! Reach for sync only in local or test environments where destructive or
//! additive auto-reconciliation is acceptable.

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
