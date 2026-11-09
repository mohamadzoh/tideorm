//! Database migration system
//!
//! This module defines TideORM's schema migration entry points.
//!
//! Use this path for additive, reviewable schema changes in deployed systems.
//! Reach for sync only in local or test environments where destructive or
//! additive auto-reconciliation is acceptable.

use crate::config::DatabaseType;

mod alter;
mod api;
pub(crate) mod ddl;
mod foreign_key;
mod ledger;
#[cfg(feature = "sqlite")]
mod memory_lock;
mod migrator;
mod schema;
mod selective;
mod sqlite_foreign_keys;
mod table;
mod types;

pub use alter::{AlterColumnBuilder, AlterTableBuilder, ColumnChangeBuilder};
pub(crate) use api::finished;
pub use api::{
    AppliedMigration, BatchFailure, Migration, MigrationFailure, MigrationInfo, MigrationResult,
    MigrationStatus,
};
pub use async_trait::async_trait;
pub use foreign_key::{ForeignKeyAction, ForeignKeyBuilder};
pub(crate) use ledger::{Ledger, write_report_section};
pub use migrator::Migrator;
pub(crate) use migrator::with_seed_lock;
pub use schema::Schema;
pub use table::{ColumnBuilder, TableBuilder};
pub(crate) use types::can_auto_increment;
pub use types::{ColumnType, DefaultValue, StringLen, column_type_sql};

#[cfg(test)]
#[path = "../../tests/unit/migration_tests.rs"]
mod tests;
