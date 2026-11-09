//! Database seeding system
//!
//! This module runs repeatable database seeders and tracks which ones already
//! executed in the `_seeds` table.
//!
//! If a seed does not run when expected, check its stored name, dependency list,
//! and whether it was already recorded as executed.

use std::collections::HashMap;
use std::sync::Arc;

use crate::database::{Database, require_db};
use crate::error::{Error, Result};
use crate::migration::{Ledger, finished, with_seed_lock};
use crate::tide_info;

mod results;

pub use async_trait::async_trait;
pub use results::{SeedFailure, SeedInfo, SeedResult, SeedStatus};

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
/// Complete operations take a database lock before reading the ledger, so
/// concurrent runners cannot execute the same pending seed. Seed callbacks
/// must not invoke another mutating seed operation: they already hold its lock.
/// Inside a caller's transaction, the lock stays held until that transaction
/// ends, including across savepoints and sequential seed operations.
pub struct Seeder {
    seeds: Vec<Arc<dyn Seed>>,
    db: Option<Database>,
}

impl Seeder {
    /// Create a new seeder
    #[must_use]
    pub fn new() -> Self {
        Self {
            seeds: Vec::new(),
            db: None,
        }
    }

    /// Add a seed
    #[allow(clippy::should_implement_trait)]
    #[must_use]
    pub fn add<S: Seed + 'static>(mut self, seed: S) -> Self {
        self.seeds.push(Arc::new(seed));
        self
    }

    /// Seed `db` instead of the global connection: one database of several,
    /// a tenant's, or a test's own. Its seeds, ledger and lock all run on
    /// `db`, and so do the model calls a seed makes.
    #[must_use]
    pub fn database(mut self, db: &Database) -> Self {
        self.db = Some(db.clone());
        self
    }

    /// The database this seeder runs on.
    fn db(&self) -> Result<Database> {
        match &self.db {
            Some(db) => Ok(db.clone()),
            None => require_db(),
        }
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
    /// An enclosing SQLite transaction with an outdated read snapshot fails before
    /// any seed callback runs; restart that transaction to see the committed ledger.
    ///
    /// A failing seed stops the run; the seeds run before it stay run, and the
    /// [`SeedFailure`] lists them.
    pub async fn run(&self) -> std::result::Result<SeedResult, SeedFailure> {
        let database = self.db()?;
        let mut result = SeedResult::default();
        let outcome = with_seed_lock(&database, self.run_locked(&database, &mut result)).await;
        finished(outcome, result)
    }

    /// Run the pending seeds, recording each in `result` once it has run.
    async fn run_locked(&self, database: &Database, result: &mut SeedResult) -> Result<()> {
        let ledger = open_ledger(database).await?;

        let executed = ledger.keys(database).await?;

        for seed in self.sort_seeds_by_priority_and_deps()? {
            let name = seed.name();

            if executed.iter().any(|executed| executed == name) {
                result.skipped.push(SeedInfo {
                    name: name.to_string(),
                });
                continue;
            }

            for dependency in seed.depends_on() {
                if !result.executed.iter().any(|seed| seed.name == dependency) {
                    ensure_dependency_ran(&executed, name, dependency)?;
                }
            }

            log_seed("Seed running", name);
            apply(Arc::clone(&seed), database, true).await?;
            log_seed("Seed completed", name);

            result.executed.push(SeedInfo {
                name: name.to_string(),
            });
        }

        Ok(())
    }

    /// Run a specific seed by name (even if already executed)
    ///
    /// This will force-run the seed regardless of whether it's been executed
    /// before, once every seed it depends on has run, as [`run`](Self::run)
    /// requires.
    pub async fn run_seed(&self, seed_name: &str) -> Result<SeedResult> {
        let seed = self.find(seed_name)?;
        let database = self.db()?;
        with_seed_lock(&database, async {
            let executed = open_ledger(&database).await?.keys(&database).await?;
            for dependency in seed.depends_on() {
                ensure_dependency_ran(&executed, seed_name, dependency)?;
            }

            log_seed("Seed running", seed_name);
            // A seed that is re-run keeps its original ledger entry.
            let recorded = executed.iter().any(|executed| executed == seed_name);
            apply(seed, &database, !recorded).await?;
            log_seed("Seed completed", seed_name);

            let mut result = SeedResult::default();
            result.executed.push(SeedInfo {
                name: seed_name.to_string(),
            });
            Ok(result)
        })
        .await
    }

    /// Rollback the last executed seed
    ///
    /// A ledger whose last seed this seeder does not hold, one renamed or
    /// removed since it ran, is an error: it cannot be reverted, and skipping
    /// it would leave every seed before it recorded as well.
    pub async fn rollback(&self) -> std::result::Result<SeedResult, SeedFailure> {
        self.rollback_steps(1).await
    }

    /// Rollback a specific seed by name, once the ledger records it.
    pub async fn rollback_seed(&self, seed_name: &str) -> Result<SeedResult> {
        let seed = self.find(seed_name)?;
        let database = self.db()?;
        with_seed_lock(&database, async {
            let executed = open_ledger(&database).await?.keys(&database).await?;
            if !executed.iter().any(|executed| executed == seed_name) {
                return Err(Error::query(format!("Seed '{seed_name}' has not run")));
            }

            let mut result = SeedResult::default();
            result.rolled_back.push(revert(seed, &database).await?);
            Ok(result)
        })
        .await
    }

    /// Rollback multiple seeds
    ///
    /// A failing revert stops it; the seeds rolled back before it stay rolled
    /// back, and the [`SeedFailure`] lists them.
    pub async fn rollback_steps(
        &self,
        steps: usize,
    ) -> std::result::Result<SeedResult, SeedFailure> {
        if steps == 0 {
            return Ok(SeedResult::default());
        }
        let database = self.db()?;
        let mut result = SeedResult::default();
        let outcome = with_seed_lock(
            &database,
            self.rollback_steps_locked(&database, steps, &mut result),
        )
        .await;
        finished(outcome, result)
    }

    /// Revert the last `steps` seeds the ledger records, newest first, reading
    /// the ledger once: each revert removes the entry it pops, and is recorded
    /// in `result`.
    async fn rollback_steps_locked(
        &self,
        database: &Database,
        steps: usize,
        result: &mut SeedResult,
    ) -> Result<()> {
        let mut executed = open_ledger(database).await?.keys(database).await?;

        for _ in 0..steps {
            let Some(last_name) = executed.pop() else {
                break;
            };
            let seed = self.find(&last_name).map_err(|_| {
                Error::not_found(format!(
                    "Seed '{}' is the last one the _seeds table records, but this seeder does not register it; register it again to roll it back, or delete its _seeds row",
                    last_name
                ))
            })?;
            result.rolled_back.push(revert(seed, database).await?);
        }

        Ok(())
    }

    /// Reset all seeds (rollback all)
    ///
    /// The tracking table is created first, so a reset on a fresh database
    /// returns an empty result instead of failing on a missing `_seeds` table -
    /// which is what makes "reset then run" usable as a bootstrap.
    pub async fn reset(&self) -> std::result::Result<SeedResult, SeedFailure> {
        let database = self.db()?;
        let mut result = SeedResult::default();
        let outcome = with_seed_lock(&database, self.reset_locked(&database, &mut result)).await;
        finished(outcome, result)
    }

    async fn reset_locked(&self, database: &Database, result: &mut SeedResult) -> Result<()> {
        self.rollback_steps_locked(database, usize::MAX, result)
            .await
    }

    /// Refresh seeds (reset + run)
    pub async fn refresh(&self) -> std::result::Result<SeedResult, SeedFailure> {
        let database = self.db()?;
        let mut result = SeedResult::default();
        let outcome = with_seed_lock(&database, async {
            self.reset_locked(&database, &mut result).await?;
            self.run_locked(&database, &mut result).await
        })
        .await;
        finished(outcome, result)
    }

    /// Get seed status
    pub async fn status(&self) -> Result<Vec<SeedStatus>> {
        let database = self.db()?;
        let ledger = open_ledger(&database).await?;

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
        let edges: Vec<(usize, usize)> = seeds
            .iter()
            .enumerate()
            .flat_map(|(index, seed)| {
                seed.depends_on()
                    .into_iter()
                    .filter_map(|dependency| index_by_name.get(dependency).copied())
                    .map(move |dependency| (dependency, index))
                    .collect::<Vec<_>>()
            })
            .collect();

        let (sorted, mut cycle) =
            crate::internal::topological_order(seeds.len(), &edges, |index| {
                (seeds[index].priority(), index)
            });

        // A seed that never became ready is on, or behind, a dependency cycle.
        if !cycle.is_empty() {
            cycle.sort_by_key(|&index| seeds[index].priority());
            let cycle_names = cycle
                .into_iter()
                .map(|index| seeds[index].name())
                .collect::<Vec<_>>()
                .join(", ");

            return Err(Error::configuration(format!(
                "Circular seed dependency detected involving: {}",
                cycle_names
            )));
        }

        Ok(sorted
            .into_iter()
            .map(|index| Arc::clone(&seeds[index]))
            .collect())
    }
}

/// Refuse to run `seed` unless the ledger's `executed` keys hold `dependency`.
fn ensure_dependency_ran(executed: &[String], seed: &str, dependency: &str) -> Result<()> {
    if executed.iter().any(|executed| executed == dependency) {
        return Ok(());
    }
    Err(Error::configuration(format!(
        "Seed '{}' depends on '{}' which has not been executed",
        seed, dependency
    )))
}

/// The seed ledger on the locked database, created if it is missing.
async fn open_ledger(database: &Database) -> Result<Ledger<'static>> {
    let ledger = Ledger::seeds();
    ledger.ensure(database).await?;
    Ok(ledger)
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
        .transaction(async move |_| {
            seed.run(&db).await?;
            if record {
                Ledger::seeds().record(&db, seed.name(), &[]).await?;
            }
            Ok(())
        })
        .await
}

/// Roll `seed` back and remove its ledger entry, as one transaction.
async fn revert(seed: Arc<dyn Seed>, database: &Database) -> Result<SeedInfo> {
    log_seed("Seed rolling back", seed.name());
    let name = seed.name().to_string();
    let db = database.clone();
    database
        .transaction(async move |_| {
            seed.rollback(&db).await?;
            Ledger::seeds().remove(&db, seed.name()).await
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
