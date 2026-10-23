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
#[cfg(feature = "sqlite")]
mod memory_lock;
mod migrator;
mod schema;
mod sqlite_foreign_keys;
#[allow(missing_docs)]
mod table;
mod types;

pub use alter::{AlterColumnBuilder, AlterTableBuilder};
pub use api::{Migration, MigrationInfo, MigrationResult, MigrationStatus};
pub use async_trait::async_trait;
pub(crate) use ledger::{Ledger, write_report_section};
pub(crate) use migrator::with_seed_lock;
pub use migrator::{
    __ensure_migration_ledger, __ensure_seed_ledger, __mark_migration, __run_sql_migration,
    Migrator,
};
pub use schema::Schema;
/// Whether SQL turns SQLite's foreign keys off, in any spelling SQLite reads,
/// for the CLI's migration runner.
#[doc(hidden)]
pub use sqlite_foreign_keys::disables_foreign_keys as __disables_foreign_keys;
pub use table::{ColumnBuilder, TableBuilder};
pub use types::{ColumnType, DefaultValue};

/// The `DROP INDEX` statement [`Schema::drop_index`] runs, for the CLI's
/// migration generator, which writes it into a migration file and names the
/// table's schema apart.
#[doc(hidden)]
pub fn __drop_index_sql(
    db_type: DatabaseType,
    schema: Option<&str>,
    table: &str,
    name: &str,
    if_exists: bool,
) -> String {
    ddl::drop_index(db_type, schema, table, name, if_exists)
}

#[cfg(test)]
#[path = "../../tests/unit/migration_tests.rs"]
mod tests;
