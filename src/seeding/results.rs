use std::fmt;

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

    /// Check if any seeds were rolled back
    pub fn has_rolled_back(&self) -> bool {
        !self.rolled_back.is_empty()
    }

    /// Total number of seeds processed
    pub fn total(&self) -> usize {
        self.executed.len() + self.skipped.len() + self.rolled_back.len()
    }
}

impl fmt::Display for SeedResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.executed.is_empty() {
            writeln!(f, "Executed seeds:")?;
            for seed in &self.executed {
                writeln!(f, "  ✓ {}", seed.name)?;
            }
        }

        if !self.skipped.is_empty() {
            writeln!(f, "Skipped seeds (already executed):")?;
            for seed in &self.skipped {
                writeln!(f, "  - {}", seed.name)?;
            }
        }

        if !self.rolled_back.is_empty() {
            writeln!(f, "Rolled back seeds:")?;
            for seed in &self.rolled_back {
                writeln!(f, "  ↩ {}", seed.name)?;
            }
        }

        Ok(())
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
