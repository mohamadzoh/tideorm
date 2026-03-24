use std::fmt;

use crate::error::Result;

use super::Schema;

/// Trait for defining database migrations
///
/// Implement this trait to create a migration. Each migration must have:
/// - A unique version string (typically a timestamp)
/// - A descriptive name
/// - An `up` method that applies the migration
/// - A `down` method that reverts the migration
///
/// # Example
///
/// ```rust,no_run
/// use tideorm::{ColumnType, Migration, Result, Schema};
/// use tideorm::migration::async_trait;
///
/// struct AddEmailVerifiedToUsers;
///
/// #[async_trait]
/// impl Migration for AddEmailVerifiedToUsers {
///     fn version(&self) -> &str { "20260106_002" }
///     fn name(&self) -> &str { "add_email_verified_to_users" }
///
///     async fn up(&self, schema: &mut Schema) -> Result<()> {
///         schema.alter_table("users", |t| {
///             t.add_column("email_verified", ColumnType::Boolean)
///                 .default(false)
///                 .not_null();
///         }).await
///     }
///
///     async fn down(&self, schema: &mut Schema) -> Result<()> {
///         schema.alter_table("users", |t| {
///             t.drop_column("email_verified");
///         }).await
///     }
/// }
/// ```
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
#[derive(Debug, Clone)]
pub struct MigrationResult {
    /// Successfully applied migrations
    pub applied: Vec<MigrationInfo>,
    /// Skipped (already applied) migrations
    pub skipped: Vec<MigrationInfo>,
    /// Rolled back migrations
    pub rolled_back: Vec<MigrationInfo>,
}

impl MigrationResult {
    pub(super) fn new() -> Self {
        Self {
            applied: Vec::new(),
            skipped: Vec::new(),
            rolled_back: Vec::new(),
        }
    }

    /// Check if any migrations were applied
    pub fn has_applied(&self) -> bool {
        !self.applied.is_empty()
    }

    /// Check if any migrations were rolled back
    pub fn has_rolled_back(&self) -> bool {
        !self.rolled_back.is_empty()
    }
}

impl fmt::Display for MigrationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.applied.is_empty() {
            writeln!(f, "Applied migrations:")?;
            for migration in &self.applied {
                writeln!(f, "  ✓ {} - {}", migration.version, migration.name)?;
            }
        }

        if !self.skipped.is_empty() {
            writeln!(f, "Skipped migrations (already applied):")?;
            for migration in &self.skipped {
                writeln!(f, "  - {} - {}", migration.version, migration.name)?;
            }
        }

        if !self.rolled_back.is_empty() {
            writeln!(f, "Rolled back migrations:")?;
            for migration in &self.rolled_back {
                writeln!(f, "  ↩ {} - {}", migration.version, migration.name)?;
            }
        }

        Ok(())
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
