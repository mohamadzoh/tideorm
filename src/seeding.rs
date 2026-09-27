//! Database seeding system
//!
//! This module runs repeatable database seeders and tracks which ones already
//! executed in the `_seeds` table.
//!
//! If a seed does not run when expected, check its stored name, dependency list,
//! and whether it was already recorded as executed.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;

use crate::database::{Database, require_db};
use crate::error::{Error, Result};
use crate::migration::Ledger;
use crate::tide_info;

mod results;

pub use async_trait::async_trait;
pub use results::{SeedInfo, SeedResult, SeedStatus};

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

/// Seed runner
///
/// Manages and executes database seeds with tracking to prevent duplicates.
pub struct Seeder {
    seeds: Vec<Arc<dyn Seed>>,
}

impl Seeder {
    /// Create a new seeder
    pub fn new() -> Self {
        Self { seeds: Vec::new() }
    }

    /// Add a seed
    #[allow(clippy::should_implement_trait)]
    pub fn add<S: Seed + 'static>(mut self, seed: S) -> Self {
        self.seeds.push(Arc::new(seed));
        self
    }

    /// Add a boxed seed (used internally)
    #[doc(hidden)]
    pub fn add_boxed(mut self, seed: Box<dyn Seed>) -> Self {
        self.seeds.push(Arc::from(seed));
        self
    }

    /// The names of the seeds, in the order they were added.
    ///
    /// Needs no database, so a name can be checked before anything runs.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.seeds.iter().map(|seed| seed.name())
    }

    /// Run all pending seeds
    ///
    /// Seeds that have already been run (tracked in the `_seeds` table) will be skipped.
    /// Each seed runs in one transaction with its ledger entry, so a seed that fails part
    /// way leaves neither rows nor an entry behind and can be fixed and run again.
    pub async fn run(&self) -> Result<SeedResult> {
        let database = require_db()?;
        let ledger = Ledger::seeds();
        ledger.ensure(&database).await?;

        let executed = ledger.keys(&database).await?;
        let mut result = SeedResult::new();

        for seed in self.sort_seeds_by_priority_and_deps()? {
            let name = seed.name();

            if executed.iter().any(|executed| executed == name) {
                result.skipped.push(SeedInfo {
                    name: name.to_string(),
                });
                continue;
            }

            for dependency in seed.depends_on() {
                if !executed.iter().any(|executed| executed == dependency)
                    && !result.executed.iter().any(|seed| seed.name == dependency)
                {
                    return Err(Error::configuration(format!(
                        "Seed '{}' depends on '{}' which has not been executed",
                        name, dependency
                    )));
                }
            }

            log_seed("Seed running", name);
            apply(Arc::clone(&seed), &database, true).await?;
            log_seed("Seed completed", name);

            result.executed.push(SeedInfo {
                name: name.to_string(),
            });
        }

        Ok(result)
    }

    /// Run a specific seed by name (even if already executed)
    ///
    /// This will force-run the seed regardless of whether it's been executed before.
    pub async fn run_seed(&self, seed_name: &str) -> Result<SeedResult> {
        let database = require_db()?;
        let ledger = Ledger::seeds();
        ledger.ensure(&database).await?;

        let seed = self.find(seed_name)?;

        log_seed("Seed running", seed_name);
        // A seed that is re-run keeps its original ledger entry.
        let recorded = ledger
            .keys(&database)
            .await?
            .iter()
            .any(|executed| executed == seed_name);
        apply(seed, &database, !recorded).await?;
        log_seed("Seed completed", seed_name);

        let mut result = SeedResult::new();
        result.executed.push(SeedInfo {
            name: seed_name.to_string(),
        });
        Ok(result)
    }

    /// Rollback the last executed seed
    ///
    /// A ledger whose last seed this seeder does not hold, one renamed or
    /// removed since it ran, is an error: it cannot be reverted, and skipping
    /// it would leave every seed before it recorded as well.
    pub async fn rollback(&self) -> Result<SeedResult> {
        let database = require_db()?;
        let ledger = Ledger::seeds();
        ledger.ensure(&database).await?;

        let mut result = SeedResult::new();
        let Some(last_name) = ledger.keys(&database).await?.pop() else {
            return Ok(result);
        };

        let seed = self.find(&last_name).map_err(|_| {
            Error::not_found(format!(
                "Seed '{}' is the last one the _seeds table records, but this seeder does not register it; register it again to roll it back, or delete its _seeds row",
                last_name
            ))
        })?;
        result.rolled_back.push(revert(seed, &database).await?);

        Ok(result)
    }

    /// Rollback a specific seed by name
    pub async fn rollback_seed(&self, seed_name: &str) -> Result<SeedResult> {
        let database = require_db()?;
        let ledger = Ledger::seeds();
        ledger.ensure(&database).await?;

        let seed = self.find(seed_name)?;

        let mut result = SeedResult::new();
        result.rolled_back.push(revert(seed, &database).await?);
        Ok(result)
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
    ///
    /// The tracking table is created first, so a reset on a fresh database
    /// returns an empty result instead of failing on a missing `_seeds` table -
    /// which is what makes "reset then run" usable as a bootstrap.
    pub async fn reset(&self) -> Result<SeedResult> {
        let database = require_db()?;
        let ledger = Ledger::seeds();
        ledger.ensure(&database).await?;

        let executed = ledger.keys(&database).await?;
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
        let database = require_db()?;
        let ledger = Ledger::seeds();
        ledger.ensure(&database).await?;

        let executed = ledger.keys(&database).await?;

        Ok(self
            .sort_seeds_by_priority_and_deps()?
            .into_iter()
            .map(|seed| SeedStatus {
                name: seed.name().to_string(),
                executed: executed.iter().any(|executed| executed == seed.name()),
                priority: seed.priority(),
            })
            .collect())
    }

    fn find(&self, name: &str) -> Result<Arc<dyn Seed>> {
        self.seeds
            .iter()
            .find(|seed| seed.name() == name)
            .map(Arc::clone)
            .ok_or_else(|| Error::not_found(format!("Seed '{}' not found", name)))
    }

    /// Order the seeds so each one runs after the seeds it depends on.
    ///
    /// Among the seeds whose dependencies have all run, the lowest priority
    /// goes next and equal priorities keep registration order. A dependency on
    /// a seed this seeder does not hold orders nothing; `run` reports it when
    /// the ledger has not recorded it either.
    fn sort_seeds_by_priority_and_deps(&self) -> Result<Vec<Arc<dyn Seed>>> {
        let seeds = &self.seeds;
        let index_by_name: HashMap<&str, usize> = seeds
            .iter()
            .enumerate()
            .map(|(index, seed)| (seed.name(), index))
            .collect();

        let mut unmet = vec![0usize; seeds.len()];
        let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); seeds.len()];
        for (index, seed) in seeds.iter().enumerate() {
            for dependency in seed.depends_on() {
                if let Some(&dependency_index) = index_by_name.get(dependency) {
                    dependents[dependency_index].push(index);
                    unmet[index] += 1;
                }
            }
        }

        // Kahn's algorithm over a min-heap of (priority, registration index).
        let mut ready: BinaryHeap<Reverse<(u32, usize)>> = (0..seeds.len())
            .filter(|&index| unmet[index] == 0)
            .map(|index| Reverse((seeds[index].priority(), index)))
            .collect();
        let mut sorted = Vec::with_capacity(seeds.len());

        while let Some(Reverse((_, index))) = ready.pop() {
            sorted.push(Arc::clone(&seeds[index]));

            for &dependent in &dependents[index] {
                unmet[dependent] -= 1;
                if unmet[dependent] == 0 {
                    ready.push(Reverse((seeds[dependent].priority(), dependent)));
                }
            }
        }

        // A seed that never became ready is on, or behind, a dependency cycle.
        if sorted.len() < seeds.len() {
            let mut remaining: Vec<usize> =
                (0..seeds.len()).filter(|&index| unmet[index] > 0).collect();
            remaining.sort_by_key(|&index| seeds[index].priority());

            let cycle_names = remaining
                .into_iter()
                .map(|index| seeds[index].name())
                .collect::<Vec<_>>()
                .join(", ");

            return Err(Error::configuration(format!(
                "Circular seed dependency detected involving: {}",
                cycle_names
            )));
        }

        Ok(sorted)
    }
}

impl Default for Seeder {
    fn default() -> Self {
        Self::new()
    }
}

/// Run `seed`, recording it in the ledger when `record` is set, as one
/// transaction: a seed that fails part way leaves neither its rows nor a ledger
/// entry behind, so running it again does not duplicate what it had written.
async fn apply(seed: Arc<dyn Seed>, database: &Database, record: bool) -> Result<()> {
    let db = database.clone();
    database
        .transaction(move |_| {
            Box::pin(async move {
                seed.run(&db).await?;
                if record {
                    Ledger::seeds().record(&db, seed.name(), &[]).await?;
                }
                Ok(())
            })
        })
        .await
}

/// Roll `seed` back and remove its ledger entry, as one transaction.
async fn revert(seed: Arc<dyn Seed>, database: &Database) -> Result<SeedInfo> {
    log_seed("Seed rolling back", seed.name());
    let name = seed.name().to_string();
    let db = database.clone();
    database
        .transaction(move |_| {
            Box::pin(async move {
                seed.rollback(&db).await?;
                Ledger::seeds().remove(&db, seed.name()).await
            })
        })
        .await?;

    Ok(SeedInfo { name })
}

/// Log seed progress when `TIDE_LOG_SEEDS` or `TIDE_LOG_QUERIES` asks for it.
fn log_seed(action: &str, name: &str) {
    if crate::logging::env_flag_enabled("TIDE_LOG_SEEDS") || crate::logging::query_logging_enabled()
    {
        tide_info!("{}: {}", action, name);
    }
}

#[cfg(test)]
#[path = "../tests/unit/seeding_tests.rs"]
mod tests;
