use std::fmt;

use crate::error::{Error, Result};

use super::Schema;
use super::ledger::write_report_section;

/// Trait for defining database migrations
///
/// Implement this trait to create a migration. Each migration must have:
/// - A unique version string (typically a timestamp)
/// - A descriptive name
/// - An `up` method that applies the migration
/// - A `down` method that reverts the migration
#[super::async_trait]
pub trait Migration: Send + Sync {
    /// Unique version identifier for this migration
    ///
    /// Format: `YYYYMMDD_NNN` (e.g., "20260106_001")
    /// Migrations are run in lexicographical order by version.
    fn version(&self) -> &str;

    /// Human-readable name for this migration
    fn name(&self) -> &str;

    /// Apply the migration
    async fn up(&self, schema: &mut Schema) -> Result<()>;

    /// Revert the migration
    async fn down(&self, schema: &mut Schema) -> Result<()>;

    /// Whether the migration runs with SQLite's foreign key enforcement off,
    /// as rebuilding a table requires: its `DROP TABLE` deletes the rows that
    /// point at the table with `ON DELETE CASCADE`, or fails on them. SQLite
    /// changes the setting only outside a transaction, and a migration runs
    /// in one, where `PRAGMA foreign_keys = OFF` is refused; this runs it on
    /// a connection of its own with the setting off instead, and checks the
    /// foreign keys before committing, failing the migration if one points
    /// nowhere. Not inside a transaction of the caller's, nor on an
    /// in-memory database with a private cache. Other backends ignore it.
    fn foreign_keys_off(&self) -> bool {
        false
    }
}

/// Result of migration operations
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MigrationResult {
    /// Successfully applied migrations
    pub applied: Vec<MigrationInfo>,
    /// Skipped (already applied) migrations
    pub skipped: Vec<MigrationInfo>,
    /// Rolled back migrations
    pub rolled_back: Vec<MigrationInfo>,
}

impl MigrationResult {
    /// Check if any migrations were applied
    pub fn has_applied(&self) -> bool {
        !self.applied.is_empty()
    }
}

impl fmt::Display for MigrationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label =
            |migration: &MigrationInfo| format!("{} - {}", migration.version, migration.name);

        write_report_section(f, "Applied migrations", "✓", &self.applied, label)?;
        write_report_section(
            f,
            "Skipped migrations (already applied)",
            "-",
            &self.skipped,
            label,
        )?;
        write_report_section(f, "Rolled back migrations", "↩", &self.rolled_back, label)
    }
}

/// A batch command of a [`Migrator`](super::Migrator) or a
/// [`Seeder`](crate::seeding::Seeder) that stopped on an error, with what it
/// did before the error: each migration or seed commits on its own, so those
/// stay applied, run or rolled back.
///
/// It displays as its error, and `?` turns it into that [`Error`].
#[derive(Debug)]
pub struct BatchFailure<R> {
    /// What stopped the command.
    pub error: Error,
    /// What the command did before it stopped, boxed to keep the `Err` of
    /// every batch command small.
    pub result: Box<R>,
}

/// A [`Migrator`](super::Migrator) batch command that stopped on an error,
/// with the migrations it applied or rolled back before it.
pub type MigrationFailure = BatchFailure<MigrationResult>;

impl<R: Default> From<Error> for BatchFailure<R> {
    fn from(error: Error) -> Self {
        Self {
            error,
            result: Box::default(),
        }
    }
}

impl<R> From<BatchFailure<R>> for Error {
    fn from(failure: BatchFailure<R>) -> Self {
        failure.error
    }
}

impl<R> fmt::Display for BatchFailure<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.error, f)
    }
}

impl<R: fmt::Debug> std::error::Error for BatchFailure<R> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        std::error::Error::source(&self.error)
    }
}

/// What a batch command did, or the failure that stopped it after it did that.
pub(crate) fn finished<R>(
    outcome: Result<()>,
    result: R,
) -> std::result::Result<R, BatchFailure<R>> {
    match outcome {
        Ok(()) => Ok(result),
        Err(error) => Err(BatchFailure {
            error,
            result: Box::new(result),
        }),
    }
}

/// Information about a single migration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationInfo {
    /// Migration version
    pub version: String,
    /// Migration name
    pub name: String,
}

/// Status of a single migration, as [`Migrator::status`](super::Migrator::status)
/// reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MigrationStatus {
    /// Migration version
    pub version: String,
    /// Migration name
    pub name: String,
    /// Whether the migration has been applied
    pub applied: bool,
    /// When it was applied, as [`AppliedMigration::applied_at`] gives it;
    /// `None` while it is pending.
    pub applied_at: Option<chrono::NaiveDateTime>,
}

impl fmt::Display for MigrationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.applied { "✓" } else { "○" };
        write!(f, "[{}] {} - {}", status, self.version, self.name)
    }
}

/// A migration the ledger records as applied, as [`Migrator::applied`](super::Migrator::applied)
/// reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedMigration {
    /// Migration version
    pub version: String,
    /// Migration name, as recorded when it was applied
    pub name: String,
    /// When it was applied, as the database stamped it: in the session's
    /// time zone on PostgreSQL, MySQL and MariaDB, in UTC on SQLite.
    pub applied_at: chrono::NaiveDateTime,
}
