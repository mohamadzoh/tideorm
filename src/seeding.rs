//! Database seeding system
//!
//! This module provides a database seeding system for TideORM.
//! Seeds are tracked in the database to prevent duplicate runs.
//!
//! ## Features
//!
//! - Define reusable seed classes
//! - Track executed seeds in the database (`_seeds` table)
//! - Prevent duplicate seed runs
//! - Support for seed dependencies
//! - Rollback/unseed support
//!
//! ## Example
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//! use tideorm::seeding::{Seed, async_trait};
//!
//! // Define a seed
//! #[derive(Default)]
//! struct UserSeeder;
//!
//! #[async_trait]
//! impl Seed for UserSeeder {
//!     fn name(&self) -> &str { "user_seeder" }
//!
//!     async fn run(&self, db: &Database) -> Result<()> {
//!         // Insert seed data
//!         db.execute_raw(r#"
//!             INSERT INTO users (email, name, active)
//!             VALUES
//!                 ('admin@example.com', 'Admin User', true),
//!                 ('user@example.com', 'Regular User', true)
//!         "#).await?;
//!         Ok(())
//!     }
//!
//!     async fn rollback(&self, db: &Database) -> Result<()> {
//!         // Remove seed data
//!         db.execute_raw(r#"
//!             DELETE FROM users WHERE email IN ('admin@example.com', 'user@example.com')
//!         "#).await?;
//!         Ok(())
//!     }
//! }
//!
//! // Run seeds via TideConfig
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     TideConfig::init()
//!         .database("postgres://localhost/myapp")
//!         .seeds::<(UserSeeder, CategorySeeder, ProductSeeder)>()
//!         .run_seeds(true)
//!         .connect()
//!         .await?;
//!
//!     Ok(())
//! }
//! ```

use std::fmt;

use crate::config::DatabaseType;
use crate::database::{Database, require_db};
use crate::error::{Error, Result};
use crate::internal::ConnectionTrait;
use crate::tide_info;

// Re-export async_trait for users
pub use async_trait::async_trait;

// ============================================================================
// SEED TRAIT
// ============================================================================

/// Trait for defining database seeds
///
/// Implement this trait to create a seed. Each seed must have:
/// - A unique name string
/// - A `run` method that inserts the seed data
/// - An optional `rollback` method that removes the seed data
///
/// # Example
///
/// ```rust,ignore
/// struct AdminUserSeeder;
///
/// #[async_trait]
/// impl Seed for AdminUserSeeder {
///     fn name(&self) -> &str { "admin_user_seeder" }
///
///     async fn run(&self, db: &Database) -> Result<()> {
///         db.execute_raw(r#"
///             INSERT INTO users (email, name, role)
///             VALUES ('admin@example.com', 'Admin', 'admin')
///             ON CONFLICT (email) DO NOTHING
///         "#).await?;
///         Ok(())
///     }
///
///     async fn rollback(&self, db: &Database) -> Result<()> {
///         db.execute_raw(r#"
///             DELETE FROM users WHERE email = 'admin@example.com'
///         "#).await?;
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait Seed: Send + Sync {
    /// Unique name identifier for this seed
    ///
    /// This name is stored in the database to track which seeds have been run.
    /// Use a descriptive, snake_case name like "user_seeder" or "initial_categories".
    fn name(&self) -> &str;

    /// Run the seed - insert data into the database
    async fn run(&self, db: &Database) -> Result<()>;

    /// Rollback the seed - remove the seeded data (optional)
    ///
    /// By default, this does nothing. Override to provide cleanup logic.
    async fn rollback(&self, _db: &Database) -> Result<()> {
        Ok(())
    }

    /// Order priority for this seed (lower runs first)
    ///
    /// Seeds with the same priority run in the order they were added.
    /// Default is 100.
    fn priority(&self) -> u32 {
        100
    }

    /// Dependencies that must run before this seed
    ///
    /// Return a list of seed names that should be executed before this seed.
    /// Default is empty (no dependencies).
    fn depends_on(&self) -> Vec<&str> {
        Vec::new()
    }
}

// ============================================================================
// SEEDER
// ============================================================================

/// Seed runner
///
/// Manages and executes database seeds with tracking to prevent duplicates.
///
/// # Example
///
/// ```rust,ignore
/// Seeder::new()
///     .add(UserSeeder)
///     .add(CategorySeeder)
///     .add(ProductSeeder)
///     .run()
///     .await?;
/// ```
pub struct Seeder {
    seeds: Vec<Box<dyn Seed>>,
}

impl Seeder {
    /// Create a new seeder
    pub fn new() -> Self {
        Self { seeds: Vec::new() }
    }

    /// Add a seed
    #[allow(clippy::should_implement_trait)]
    pub fn add<S: Seed + 'static>(mut self, seed: S) -> Self {
        self.seeds.push(Box::new(seed));
        self
    }

    /// Add a boxed seed (used internally)
    #[doc(hidden)]
    pub fn add_boxed(mut self, seed: Box<dyn Seed>) -> Self {
        self.seeds.push(seed);
        self
    }

    /// Run all pending seeds
    ///
    /// Seeds that have already been run (tracked in the `_seeds` table) will be skipped.
    pub async fn run(&self) -> Result<SeedResult> {
        self.ensure_seeds_table().await?;

        let executed = self.get_executed_seeds().await?;
        let mut result = SeedResult::new();

        let database = require_db()?;

        // Sort seeds by priority, then by dependency order
        let sorted_seeds = self.sort_seeds_by_priority_and_deps();

        for seed in sorted_seeds {
            let name = seed.name();

            if executed.contains(&name.to_string()) {
                result.skipped.push(SeedInfo {
                    name: name.to_string(),
                });
                continue;
            }

            // Check dependencies
            for dep in seed.depends_on() {
                if !executed.contains(&dep.to_string())
                    && !result.executed.iter().any(|s| s.name == dep)
                {
                    return Err(Error::configuration(format!(
                        "Seed '{}' depends on '{}' which has not been executed",
                        name, dep
                    )));
                }
            }

            log_seed_start(name);

            seed.run(database).await?;

            // Record seed as executed
            self.record_seed(name).await?;

            result.executed.push(SeedInfo {
                name: name.to_string(),
            });

            log_seed_complete(name);
        }

        Ok(result)
    }

    /// Run a specific seed by name (even if already executed)
    ///
    /// This will force-run the seed regardless of whether it's been executed before.
    pub async fn run_seed(&self, seed_name: &str) -> Result<SeedResult> {
        self.ensure_seeds_table().await?;

        let database = require_db()?;
        let mut result = SeedResult::new();

        for seed in &self.seeds {
            if seed.name() == seed_name {
                log_seed_start(seed_name);

                seed.run(database).await?;

                // Record seed (or update timestamp if already exists)
                let executed = self.get_executed_seeds().await?;
                if !executed.contains(&seed_name.to_string()) {
                    self.record_seed(seed_name).await?;
                }

                result.executed.push(SeedInfo {
                    name: seed_name.to_string(),
                });

                log_seed_complete(seed_name);
                return Ok(result);
            }
        }

        Err(Error::not_found(format!("Seed '{}' not found", seed_name)))
    }

    /// Rollback the last executed seed
    pub async fn rollback(&self) -> Result<SeedResult> {
        self.ensure_seeds_table().await?;

        let executed = self.get_executed_seeds().await?;
        let mut result = SeedResult::new();

        if executed.is_empty() {
            return Ok(result);
        }

        // Get the last executed seed name
        let last_name = match executed.last() {
            Some(n) => n,
            None => return Ok(result),
        };

        let database = require_db()?;

        // Find the seed
        for seed in &self.seeds {
            if seed.name() == last_name {
                log_seed_rollback(last_name);

                seed.rollback(database).await?;

                // Remove seed record
                self.remove_seed_record(last_name).await?;

                result.rolled_back.push(SeedInfo {
                    name: seed.name().to_string(),
                });

                break;
            }
        }

        Ok(result)
    }

    /// Rollback a specific seed by name
    pub async fn rollback_seed(&self, seed_name: &str) -> Result<SeedResult> {
        self.ensure_seeds_table().await?;

        let database = require_db()?;
        let mut result = SeedResult::new();

        for seed in &self.seeds {
            if seed.name() == seed_name {
                log_seed_rollback(seed_name);

                seed.rollback(database).await?;

                // Remove seed record
                self.remove_seed_record(seed_name).await?;

                result.rolled_back.push(SeedInfo {
                    name: seed_name.to_string(),
                });

                return Ok(result);
            }
        }

        Err(Error::not_found(format!("Seed '{}' not found", seed_name)))
    }

    /// Rollback multiple seeds
    pub async fn rollback_steps(&self, steps: usize) -> Result<SeedResult> {
        let mut result = SeedResult::new();

        for _ in 0..steps {
            let step_result = self.rollback().await?;
            if step_result.rolled_back.is_empty() {
                break;
            }
            result.rolled_back.extend(step_result.rolled_back);
        }

        Ok(result)
    }

    /// Reset all seeds (rollback all)
    pub async fn reset(&self) -> Result<SeedResult> {
        let executed = self.get_executed_seeds().await?;
        self.rollback_steps(executed.len()).await
    }

    /// Refresh seeds (reset + run)
    pub async fn refresh(&self) -> Result<SeedResult> {
        let reset_result = self.reset().await?;
        let run_result = self.run().await?;

        Ok(SeedResult {
            executed: run_result.executed,
            skipped: run_result.skipped,
            rolled_back: reset_result.rolled_back,
        })
    }

    /// Get seed status
    pub async fn status(&self) -> Result<Vec<SeedStatus>> {
        self.ensure_seeds_table().await?;

        let executed = self.get_executed_seeds().await?;
        let mut status = Vec::new();

        let sorted_seeds = self.sort_seeds_by_priority_and_deps();

        for seed in sorted_seeds {
            let is_executed = executed.contains(&seed.name().to_string());
            status.push(SeedStatus {
                name: seed.name().to_string(),
                executed: is_executed,
                priority: seed.priority(),
            });
        }

        Ok(status)
    }

    // =========================================================================
    // HELPER METHODS
    // =========================================================================

    /// Sort seeds by priority, respecting dependencies via topological sort
    fn sort_seeds_by_priority_and_deps(&self) -> Vec<&dyn Seed> {
        use std::collections::{HashMap, HashSet, VecDeque};

        let seeds: Vec<_> = self.seeds.iter().collect();

        // Build a name -> index map
        let name_to_idx: HashMap<String, usize> = seeds
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name().to_string(), i))
            .collect();

        let n = seeds.len();
        let mut in_degree = vec![0usize; n];
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];

        // Build dependency graph: if seed B depends on seed A, A -> B
        for (i, seed) in seeds.iter().enumerate() {
            for dep in seed.depends_on() {
                if let Some(&dep_idx) = name_to_idx.get(dep) {
                    adj[dep_idx].push(i);
                    in_degree[i] += 1;
                }
            }
        }

        // Kahn's algorithm for topological sort
        // Use a BinaryHeap to break ties by priority (lower priority first)
        let mut queue: VecDeque<usize> = VecDeque::new();

        // Collect all roots (no dependencies), sorted by priority
        let mut roots: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
        roots.sort_by_key(|&i| seeds[i].priority());
        for r in roots {
            queue.push_back(r);
        }

        let mut sorted_indices: Vec<usize> = Vec::with_capacity(n);
        let mut visited = HashSet::new();

        while let Some(idx) = queue.pop_front() {
            if !visited.insert(idx) {
                continue;
            }
            sorted_indices.push(idx);

            // Collect neighbors, reduce in-degree, add ready ones sorted by priority
            let mut next: Vec<usize> = Vec::new();
            for &neighbor in &adj[idx] {
                in_degree[neighbor] -= 1;
                if in_degree[neighbor] == 0 {
                    next.push(neighbor);
                }
            }
            next.sort_by_key(|&i| seeds[i].priority());
            for n in next {
                queue.push_back(n);
            }
        }

        // Any remaining seeds (circular deps) are appended at the end sorted by priority
        if sorted_indices.len() < n {
            let mut remaining: Vec<usize> = (0..n).filter(|i| !visited.contains(i)).collect();
            remaining.sort_by_key(|&i| seeds[i].priority());
            sorted_indices.extend(remaining);
        }

        sorted_indices
            .into_iter()
            .map(|i| seeds[i].as_ref())
            .collect()
    }

    // =========================================================================
    // SEEDS TABLE MANAGEMENT
    // =========================================================================

    /// Ensure the seeds table exists
    async fn ensure_seeds_table(&self) -> Result<()> {
        let database = require_db()?;
        let db_type = detect_database_type(database);

        let sql = match db_type {
            DatabaseType::Postgres => {
                r#"
                CREATE TABLE IF NOT EXISTS "_seeds" (
                    "id" SERIAL PRIMARY KEY,
                    "name" VARCHAR(255) NOT NULL UNIQUE,
                    "executed_at" TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
                "#
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                r#"
                CREATE TABLE IF NOT EXISTS `_seeds` (
                    `id` INT AUTO_INCREMENT PRIMARY KEY,
                    `name` VARCHAR(255) NOT NULL UNIQUE,
                    `executed_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
                "#
            }
            DatabaseType::SQLite => {
                r#"
                CREATE TABLE IF NOT EXISTS "_seeds" (
                    "id" INTEGER PRIMARY KEY AUTOINCREMENT,
                    "name" TEXT NOT NULL UNIQUE,
                    "executed_at" TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
                "#
            }
        };

        database
            .__internal_connection()
            .execute_unprepared(sql)
            .await
            .map_err(|e| Error::query(e.to_string()))?;

        Ok(())
    }

    /// Get list of executed seed names
    async fn get_executed_seeds(&self) -> Result<Vec<String>> {
        let database = require_db()?;

        use crate::internal::Statement;

        let backend = database.__internal_connection().get_database_backend();
        let q = |id: &str| quote_identifier(id, backend);
        let sql = format!(
            "SELECT {} FROM {} ORDER BY {} ASC",
            q("name"),
            q("_seeds"),
            q("executed_at")
        );
        let stmt = Statement::from_string(backend, sql);

        let results = database
            .__internal_connection()
            .query_all_raw(stmt)
            .await
            .map_err(|e| Error::query(e.to_string()))?;

        let mut names = Vec::new();
        for row in results {
            let name: String = row
                .try_get("", "name")
                .map_err(|e| Error::query(e.to_string()))?;
            names.push(name);
        }

        Ok(names)
    }

    /// Record a seed as executed
    async fn record_seed(&self, name: &str) -> Result<()> {
        let database = require_db()?;
        let backend = database.__internal_connection().get_database_backend();
        let q = |id: &str| quote_identifier(id, backend);

        let sql = format!(
            "INSERT INTO {} ({}) VALUES ('{}')",
            q("_seeds"),
            q("name"),
            name.replace('\'', "''")
        );

        database
            .__internal_connection()
            .execute_unprepared(&sql)
            .await
            .map_err(|e| Error::query(e.to_string()))?;

        Ok(())
    }

    /// Remove a seed record
    async fn remove_seed_record(&self, name: &str) -> Result<()> {
        let database = require_db()?;
        let backend = database.__internal_connection().get_database_backend();
        let q = |id: &str| quote_identifier(id, backend);

        let sql = format!(
            "DELETE FROM {} WHERE {} = '{}'",
            q("_seeds"),
            q("name"),
            name.replace('\'', "''")
        );

        database
            .__internal_connection()
            .execute_unprepared(&sql)
            .await
            .map_err(|e| Error::query(e.to_string()))?;

        Ok(())
    }
}

impl Default for Seeder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// RESULT TYPES
// ============================================================================

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
    fn new() -> Self {
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
            for s in &self.executed {
                writeln!(f, "  ✓ {}", s.name)?;
            }
        }

        if !self.skipped.is_empty() {
            writeln!(f, "Skipped seeds (already executed):")?;
            for s in &self.skipped {
                writeln!(f, "  - {}", s.name)?;
            }
        }

        if !self.rolled_back.is_empty() {
            writeln!(f, "Rolled back seeds:")?;
            for s in &self.rolled_back {
                writeln!(f, "  ↩ {}", s.name)?;
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

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Detect database type from connection
fn detect_database_type(database: &Database) -> DatabaseType {
    database.backend()
}

/// Quote an identifier (table/column name) for the current database backend
fn quote_identifier(name: &str, backend: crate::internal::DbBackend) -> String {
    use crate::internal::DbBackend;
    match backend {
        DbBackend::MySql => format!("`{}`", name),
        _ => format!(r#""{}""#, name), // Postgres & SQLite use double quotes
    }
}

/// Log seed start
fn log_seed_start(name: &str) {
    if std::env::var("TIDE_LOG_QUERIES").is_ok() || std::env::var("TIDE_LOG_SEEDS").is_ok() {
        tide_info!("Seed running: {}", name);
    }
}

/// Log seed complete
fn log_seed_complete(name: &str) {
    if std::env::var("TIDE_LOG_QUERIES").is_ok() || std::env::var("TIDE_LOG_SEEDS").is_ok() {
        tide_info!("Seed completed: {}", name);
    }
}

/// Log seed rollback
fn log_seed_rollback(name: &str) {
    if std::env::var("TIDE_LOG_QUERIES").is_ok() || std::env::var("TIDE_LOG_SEEDS").is_ok() {
        tide_info!("Seed rolling back: {}", name);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seed_result_new() {
        let result = SeedResult::new();
        assert!(result.executed.is_empty());
        assert!(result.skipped.is_empty());
        assert!(result.rolled_back.is_empty());
        assert!(!result.has_executed());
        assert!(!result.has_rolled_back());
    }

    #[test]
    fn test_seed_result_has_executed() {
        let mut result = SeedResult::new();
        result.executed.push(SeedInfo {
            name: "test_seed".to_string(),
        });
        assert!(result.has_executed());
        assert!(!result.has_rolled_back());
    }

    #[test]
    fn test_seed_result_total() {
        let mut result = SeedResult::new();
        result.executed.push(SeedInfo {
            name: "seed1".to_string(),
        });
        result.skipped.push(SeedInfo {
            name: "seed2".to_string(),
        });
        result.rolled_back.push(SeedInfo {
            name: "seed3".to_string(),
        });
        assert_eq!(result.total(), 3);
    }

    #[test]
    fn test_seed_result_display() {
        let mut result = SeedResult::new();
        result.executed.push(SeedInfo {
            name: "user_seeder".to_string(),
        });
        result.skipped.push(SeedInfo {
            name: "category_seeder".to_string(),
        });

        let display = format!("{}", result);
        assert!(display.contains("user_seeder"));
        assert!(display.contains("category_seeder"));
        assert!(display.contains("Executed seeds"));
        assert!(display.contains("Skipped seeds"));
    }

    #[test]
    fn test_seed_status_display() {
        let status = SeedStatus {
            name: "user_seeder".to_string(),
            executed: true,
            priority: 100,
        };
        let display = format!("{}", status);
        assert!(display.contains("[✓]"));
        assert!(display.contains("user_seeder"));
        assert!(display.contains("priority: 100"));
    }

    #[test]
    fn test_seed_status_not_executed() {
        let status = SeedStatus {
            name: "product_seeder".to_string(),
            executed: false,
            priority: 50,
        };
        let display = format!("{}", status);
        assert!(display.contains("[○]"));
        assert!(display.contains("product_seeder"));
    }

    #[test]
    fn test_seeder_default() {
        let seeder = Seeder::default();
        assert!(seeder.seeds.is_empty());
    }

    #[test]
    fn test_seed_info() {
        let info = SeedInfo {
            name: "test_seed".to_string(),
        };
        assert_eq!(info.name, "test_seed");
    }
}
