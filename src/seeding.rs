//! Database seeding system
//!
//! This module runs repeatable database seeders and tracks which ones already
//! executed in the `_seeds` table.
//!
//! If a seed does not run when expected, check its stored name, dependency list,
//! and whether it was already recorded as executed.

use crate::config::DatabaseType;
use crate::database::{Database, require_db};
use crate::error::{Error, Result};
use crate::internal::ConnectionTrait;
use crate::internal::Value;
use crate::internal::sql_safety::quote_ident_for_backend;
use crate::tide_info;

mod results;
mod store;

// Re-export async_trait for users
pub use async_trait::async_trait;
pub use results::{SeedInfo, SeedResult, SeedStatus};

// ============================================================================
// SEED TRAIT
// ============================================================================

/// Trait for defining database seeds
///
/// Implement this trait to create a seed. Each seed must have:
/// - A unique name string
/// - A `run` method that inserts the seed data
/// - An optional `rollback` method that removes the seed data
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
        let sorted_seeds = self.sort_seeds_by_priority_and_deps()?;

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

            seed.run(&database).await?;

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

                seed.run(&database).await?;

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

        let last_name = match executed.last() {
            Some(n) => n,
            None => return Ok(result),
        };

        let database = require_db()?;

        // Find the seed
        for seed in &self.seeds {
            if seed.name() == last_name {
                log_seed_rollback(last_name);

                seed.rollback(&database).await?;

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

                seed.rollback(&database).await?;

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

        let sorted_seeds = self.sort_seeds_by_priority_and_deps()?;

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
    fn sort_seeds_by_priority_and_deps(&self) -> Result<Vec<&dyn Seed>> {
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

        // Unvisited seeds at this point participate in at least one dependency cycle.
        if sorted_indices.len() < n {
            let mut remaining: Vec<usize> = (0..n).filter(|i| !visited.contains(i)).collect();
            remaining.sort_by_key(|&i| seeds[i].priority());

            let cycle_names = remaining
                .into_iter()
                .map(|i| seeds[i].name().to_string())
                .collect::<Vec<_>>()
                .join(", ");

            return Err(Error::configuration(format!(
                "Circular seed dependency detected involving: {}",
                cycle_names
            )));
        }

        Ok(sorted_indices
            .into_iter()
            .map(|i| seeds[i].as_ref())
            .collect())
    }
}

impl Default for Seeder {
    fn default() -> Self {
        Self::new()
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
#[path = "../tests/unit/seeding_tests.rs"]
mod tests;
