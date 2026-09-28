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
mod ledger;
mod migrator;
mod schema;
#[allow(missing_docs)]
mod table;
mod types;

pub use alter::{AlterColumnBuilder, AlterTableBuilder};
pub use api::{Migration, MigrationInfo, MigrationResult, MigrationStatus};
pub use async_trait::async_trait;
pub(crate) use ledger::{Ledger, write_report_section};
pub use migrator::{
    __ensure_migration_ledger, __ensure_seed_ledger, __mark_migration, __run_sql_migration,
    Migrator,
};
pub use schema::Schema;
pub use table::{ColumnBuilder, TableBuilder};
pub use types::{ColumnType, DefaultValue};

#[cfg(test)]
#[path = "../../tests/unit/migration_tests.rs"]
mod tests;
