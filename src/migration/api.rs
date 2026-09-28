use std::fmt;

use crate::error::Result;

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
}

/// Result of migration operations
#[derive(Debug, Clone, Default)]
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

/// Information about a single migration
#[derive(Debug, Clone)]
pub struct MigrationInfo {
    /// Migration version
    pub version: String,
    /// Migration name
    pub name: String,
}

/// Status of a single migration
#[derive(Debug, Clone)]
pub struct MigrationStatus {
    /// Migration version
    pub version: String,
    /// Migration name
    pub name: String,
    /// Whether the migration has been applied
    pub applied: bool,
}

impl fmt::Display for MigrationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.applied { "✓" } else { "○" };
        write!(f, "[{}] {} - {}", status, self.version, self.name)
    }
}
