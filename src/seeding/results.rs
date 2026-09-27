use std::fmt;

use crate::migration::write_report_section;

/// Result of seed operations
#[derive(Debug, Clone)]
pub struct SeedResult {
    /// Successfully executed seeds
    pub executed: Vec<SeedInfo>,
    /// Skipped (already executed) seeds
    pub skipped: Vec<SeedInfo>,
    /// Rolled back seeds
    pub rolled_back: Vec<SeedInfo>,
}

impl SeedResult {
    pub(super) fn new() -> Self {
        Self {
            executed: Vec::new(),
            skipped: Vec::new(),
            rolled_back: Vec::new(),
        }
    }

    /// Check if any seeds were executed
    pub fn has_executed(&self) -> bool {
        !self.executed.is_empty()
    }
}

impl fmt::Display for SeedResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = |seed: &SeedInfo| seed.name.clone();

        write_report_section(f, "Executed seeds", "✓", &self.executed, label)?;
        write_report_section(
            f,
            "Skipped seeds (already executed)",
            "-",
            &self.skipped,
            label,
        )?;
        write_report_section(f, "Rolled back seeds", "↩", &self.rolled_back, label)
    }
}

/// Information about a single seed
#[derive(Debug, Clone)]
pub struct SeedInfo {
    /// Seed name
    pub name: String,
}

/// Status of a single seed
#[derive(Debug, Clone)]
pub struct SeedStatus {
    /// Seed name
    pub name: String,
    /// Whether the seed has been executed
    pub executed: bool,
    /// Seed priority
    pub priority: u32,
}

impl fmt::Display for SeedStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.executed { "✓" } else { "○" };
        write!(
            f,
            "[{}] {} (priority: {})",
            status, self.name, self.priority
        )
    }
}
