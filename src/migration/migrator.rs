use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::database::{Database, require_db};
use crate::error::{Error, Result};
use crate::internal::sql_safety::is_safe_identifier_segment;
use crate::tide_info;

use super::api::finished;
use super::{
    AppliedMigration, DatabaseType, Ledger, Migration, MigrationFailure, MigrationInfo,
    MigrationResult, MigrationStatus, Schema,
};

mod ledger_lock;

pub(super) use ledger_lock::with_migration_lock;
pub(crate) use ledger_lock::with_seed_lock;

/// Ledger table a migrator uses unless it is pointed at another one.
///
/// The CLI exposes the same setting as `[migration] table` in `tideorm.toml`. A
/// project that renames it there must call [`Migrator::migrations_table`] with
/// the same name, or the CLI and the runtime keep two independent ledgers and
/// every migration ends up applied twice.
const DEFAULT_MIGRATIONS_TABLE: &str = "_migrations";

/// Whether the backend keeps DDL inside a transaction.
///
/// PostgreSQL and SQLite roll DDL back with the surrounding transaction, so a
/// migration and its ledger row can be committed together. MySQL and MariaDB
/// implicitly commit around every DDL statement, so wrapping a migration there
/// would only pretend to be atomic - they run unwrapped instead.
fn supports_transactional_ddl(db_type: DatabaseType) -> bool {
    matches!(db_type, DatabaseType::Postgres | DatabaseType::SQLite)
}

/// Migration runner
///
/// Manages and executes database migrations, on the global connection unless
/// [`database`](Self::database) names another.
pub struct Migrator {
    /// Migrations are held behind `Arc` so a single migration can be moved into
    /// the transaction closure that applies it, which must own `'static` data.
    migrations: Vec<Arc<dyn Migration>>,
    /// Name of the ledger table that records which migrations have been applied.
    table: String,
    /// The database to migrate, when it is not the global one.
    db: Option<Database>,
}

impl Migrator {
    /// Create a new migrator
    #[must_use]
    pub fn new() -> Self {
        Self {
            migrations: Vec::new(),
            table: DEFAULT_MIGRATIONS_TABLE.to_string(),
            db: None,
        }
    }

    /// Add a migration
    #[allow(clippy::should_implement_trait)]
    #[must_use]
    pub fn add<M: Migration + 'static>(mut self, migration: M) -> Self {
        self.migrations.push(Arc::new(migration));
        self
    }

    /// Migrate `db` instead of the global connection: one database of
    /// several, a tenant's, or a test's own. Its migrations, ledger and lock
    /// all run on `db`.
    #[must_use]
    pub fn database(mut self, db: &Database) -> Self {
        self.db = Some(db.clone());
        self
    }

    /// Add a boxed migration (used internally by TideConfig)
    #[doc(hidden)]
    #[must_use]
    pub fn add_boxed(mut self, migration: Box<dyn Migration>) -> Self {
        self.migrations.push(Arc::from(migration));
        self
    }

    /// Record applied migrations in `name` instead of the default `_migrations`.
    ///
    /// The CLI's migration runner passes its `[migration] table` setting from
    /// `tideorm.toml` here. An application migrating at startup must name the
    /// same table: if the two disagree each keeps its own ledger, so
    /// migrations the CLI already applied look pending to the application and
    /// are applied a second time.
    ///
    /// The name is interpolated into DDL, so it may only contain ASCII letters,
    /// numbers, and underscores. An invalid name is reported the first time the
    /// ledger is touched, not here - a builder method has nowhere to return the
    /// error, and quietly falling back to the default would hand the caller the
    /// very second ledger this setting exists to avoid.
    #[must_use]
    pub fn migrations_table(mut self, name: impl Into<String>) -> Self {
        self.table = name.into();
        self
    }

    /// Run all pending migrations
    ///
    /// Concurrent migrators are serialized with a backend lock, so replicas
    /// booting together apply each migration once instead of racing. On
    /// backends with transactional DDL the migration and its ledger row are
    /// committed together, so a statement failing halfway cannot leave an
    /// applied change with no ledger row.
    ///
    /// A failing migration stops the run; the ones applied before it stay
    /// applied, and the [`MigrationFailure`] lists them.
    pub async fn run(&self) -> std::result::Result<MigrationResult, MigrationFailure> {
        self.run_up_to(None).await
    }

    /// Run the first `steps` pending migrations, in version order, as
    /// [`run`](Self::run) runs them all.
    pub async fn run_steps(
        &self,
        steps: usize,
    ) -> std::result::Result<MigrationResult, MigrationFailure> {
        self.run_up_to(Some(steps)).await
    }

    async fn run_up_to(
        &self,
        limit: Option<usize>,
    ) -> std::result::Result<MigrationResult, MigrationFailure> {
        let db = self.db()?;
        let mut result = MigrationResult::default();
        let outcome = with_migration_lock(&db, self.run_locked(&db, limit, &mut result)).await;
        finished(outcome, result)
    }

    /// Apply the pending migrations, the first `limit` of them if given,
    /// recording each in `result` once it is applied.
    async fn run_locked(
        &self,
        db: &Database,
        limit: Option<usize>,
        result: &mut MigrationResult,
    ) -> Result<()> {
        let ledger = self.ledger()?;
        ledger.ensure(db).await?;

        let applied = self.applied_versions(&ledger, db).await?;

        for migration in self.sorted() {
            let version = migration.version();
            let info = info_of(migration.as_ref());

            if applied.iter().any(|applied| applied == version) {
                result.skipped.push(info);
                continue;
            }
            if limit == Some(result.applied.len()) {
                break;
            }

            tide_info!("Running migration: {} - {}", version, migration.name());
            run_migration(db, Arc::clone(migration), ledger.table().to_string(), true).await?;
            tide_info!("Completed migration: {} - {}", version, migration.name());

            result.applied.push(info);
        }

        Ok(())
    }

    /// Rollback the last migration
    ///
    /// Takes the same lock as [`Migrator::run`], and reverts plus un-records the
    /// migration in one transaction where the backend allows it.
    pub async fn rollback(&self) -> std::result::Result<MigrationResult, MigrationFailure> {
        self.rollback_steps(1).await
    }

    /// Rollback multiple migrations while holding one migration lock. A
    /// failure stops it, and the [`MigrationFailure`] lists the migrations
    /// rolled back before it.
    pub async fn rollback_steps(
        &self,
        steps: usize,
    ) -> std::result::Result<MigrationResult, MigrationFailure> {
        if steps == 0 {
            return Ok(MigrationResult::default());
        }
        let db = self.db()?;
        let mut result = MigrationResult::default();
        let outcome =
            with_migration_lock(&db, self.rollback_steps_locked(&db, steps, &mut result)).await;
        finished(outcome, result)
    }

    async fn rollback_steps_locked(
        &self,
        db: &Database,
        steps: usize,
        result: &mut MigrationResult,
    ) -> Result<()> {
        let ledger = self.ledger()?;
        ledger.ensure(db).await?;
        let applied = ledger.keys(db).await?;
        self.revert_newest(db, &ledger, applied, steps, result)
            .await
    }

    /// Revert the newest `steps` of the `applied` keys the ledger read, each
    /// revert removing the entry it pops, so the ledger is read only once,
    /// and recording each in `result` once it is reverted.
    ///
    /// The ledger is ordered by insertion id, so the last entry is the
    /// migration applied most recently - not the one with the highest
    /// version. After a long-lived branch merges those are routinely
    /// different migrations. Every entry is read, registered or not: one
    /// this migrator cannot revert must stop the rollback, not be skipped
    /// over to revert an older migration instead.
    async fn revert_newest(
        &self,
        db: &Database,
        ledger: &Ledger<'_>,
        mut applied: Vec<String>,
        steps: usize,
        result: &mut MigrationResult,
    ) -> Result<()> {
        for _ in 0..steps {
            let Some(last_version) = applied.pop() else {
                break;
            };
            let Some(migration) = self
                .migrations
                .iter()
                .find(|migration| migration.version() == last_version)
            else {
                return Err(Error::query(format!(
                    "Cannot roll back migration {last_version}: it is recorded but not registered"
                )));
            };

            tide_info!(
                "Rolling back migration: {} - {}",
                last_version,
                migration.name()
            );
            run_migration(db, Arc::clone(migration), ledger.table().to_string(), false).await?;
            result.rolled_back.push(info_of(migration.as_ref()));
        }

        Ok(())
    }

    /// Reset all migrations (rollback all)
    ///
    /// Only migrations still registered on this migrator can be reverted, since
    /// a ledger row for a version that no longer exists in code has no `down()`
    /// left to run. Reset fails before reverting anything if such rows exist.
    pub async fn reset(&self) -> std::result::Result<MigrationResult, MigrationFailure> {
        let db = self.db()?;
        let mut result = MigrationResult::default();
        let outcome = with_migration_lock(&db, self.reset_locked(&db, &mut result)).await;
        finished(outcome, result)
    }

    async fn reset_locked(&self, db: &Database, result: &mut MigrationResult) -> Result<()> {
        let ledger = self.ledger()?;
        ledger.ensure(db).await?;

        let recorded = ledger.keys(db).await?;
        let registered = self.registered_versions();
        let unknown: Vec<&str> = recorded
            .iter()
            .filter(|version| !registered.contains(version.as_str()))
            .map(String::as_str)
            .collect();

        if !unknown.is_empty() {
            return Err(Error::query(format!(
                "Cannot reset migrations: recorded versions are not registered: {}",
                unknown.join(", ")
            )));
        }

        let steps = recorded.len();
        self.revert_newest(db, &ledger, recorded, steps, result)
            .await
    }

    /// Refresh migrations (reset + run)
    pub async fn refresh(&self) -> std::result::Result<MigrationResult, MigrationFailure> {
        let db = self.db()?;
        let mut result = MigrationResult::default();
        let outcome = with_migration_lock(&db, async {
            self.reset_locked(&db, &mut result).await?;
            self.run_locked(&db, None, &mut result).await
        })
        .await;
        finished(outcome, result)
    }

    /// Get migration status
    ///
    /// One entry per registered migration; [`unregistered`](Self::unregistered)
    /// lists what the ledger records beyond them.
    pub async fn status(&self) -> Result<Vec<MigrationStatus>> {
        let db = self.db()?;
        let ledger = self.ledger()?;
        ledger.ensure(&db).await?;

        let applied: HashMap<String, chrono::NaiveDateTime> = ledger
            .applied_migrations(&db)
            .await?
            .into_iter()
            .map(|migration| (migration.version, migration.applied_at))
            .collect();

        Ok(self
            .sorted()
            .into_iter()
            .map(|migration| {
                let applied_at = applied.get(migration.version()).copied();
                MigrationStatus {
                    version: migration.version().to_string(),
                    name: migration.name().to_string(),
                    applied: applied_at.is_some(),
                    applied_at,
                }
            })
            .collect())
    }

    /// Every migration the ledger records, registered here or not, in the
    /// order they were applied, with when each was.
    pub async fn applied(&self) -> Result<Vec<AppliedMigration>> {
        let db = self.db()?;
        let ledger = self.ledger()?;
        ledger.ensure(&db).await?;
        ledger.applied_migrations(&db).await
    }

    /// The migrations the ledger records that none registered here matches,
    /// in the order they were applied: [`status`](Self::status) leaves them
    /// out, and [`reset`](Self::reset) refuses to run while any exist.
    pub async fn unregistered(&self) -> Result<Vec<MigrationInfo>> {
        let registered = self.registered_versions();
        Ok(self
            .applied()
            .await?
            .into_iter()
            .filter(|migration| !registered.contains(migration.version.as_str()))
            .map(|migration| MigrationInfo {
                version: migration.version,
                name: migration.name,
            })
            .collect())
    }

    /// The database this migrator runs on.
    pub(super) fn db(&self) -> Result<Database> {
        match &self.db {
            Some(db) => Ok(db.clone()),
            None => require_db(),
        }
    }

    /// The ledger this migrator records into, its table name checked before it
    /// reaches any SQL string.
    ///
    /// Validation lives here rather than in [`Migrator::migrations_table`]
    /// because every path that touches the ledger already returns `Result`,
    /// while the builder method has nowhere to report a bad name.
    pub(super) fn ledger(&self) -> Result<Ledger<'_>> {
        checked_ledger(&self.table)
    }

    /// The registered migrations, in the order they were added.
    pub(super) fn registered(&self) -> &[Arc<dyn Migration>] {
        &self.migrations
    }

    /// The registered migrations in version order.
    fn sorted(&self) -> Vec<&Arc<dyn Migration>> {
        let mut migrations: Vec<_> = self.migrations.iter().collect();
        migrations.sort_by_key(|migration| migration.version());
        migrations
    }

    /// Versions of the migrations registered on this migrator.
    fn registered_versions(&self) -> HashSet<&str> {
        self.migrations
            .iter()
            .map(|migration| migration.version())
            .collect()
    }

    /// Recorded versions this migrator still knows how to revert, in the order
    /// they were applied.
    async fn applied_versions(&self, ledger: &Ledger<'_>, db: &Database) -> Result<Vec<String>> {
        let registered = self.registered_versions();

        Ok(ledger
            .keys(db)
            .await?
            .into_iter()
            .filter(|version| registered.contains(version.as_str()))
            .collect())
    }
}

/// What a run reports about `migration`.
pub(super) fn info_of(migration: &dyn Migration) -> MigrationInfo {
    MigrationInfo {
        version: migration.version().to_string(),
        name: migration.name().to_string(),
    }
}

/// Apply one migration and record it in the ledger, or with `apply` false
/// revert it and remove its ledger row, in one [`in_ddl_scope`].
pub(super) async fn run_migration(
    db: &Database,
    migration: Arc<dyn Migration>,
    table: String,
    apply: bool,
) -> Result<()> {
    let db_type = db.backend();
    let foreign_keys_off = migration.foreign_keys_off();
    let handle = db.clone();
    in_ddl_scope(db, foreign_keys_off, async move {
        // `Schema` runs its statements on the scope's connection: the
        // transaction opened on `handle`, or `handle` itself where DDL is not
        // transactional.
        handle
            .scope(async {
                let mut schema = Schema::new(db_type).with_foreign_keys_off(foreign_keys_off);
                let ledger = Ledger::migrations(&table);
                if apply {
                    migration.up(&mut schema).await?;
                    ledger
                        .record(&handle, migration.version(), &[migration.name()])
                        .await
                } else {
                    migration.down(&mut schema).await?;
                    ledger.remove(&handle, migration.version()).await
                }
            })
            .await
    })
    .await
}

/// Run `work` in one transaction where the backend's DDL is transactional,
/// so a statement failing partway cannot leave schema changes behind with no
/// ledger row; elsewhere unwrapped, because the backend would implicitly
/// commit the DDL anyway. `Schema` and the ledger resolve their connection
/// from the ambient scope, which is what puts them inside the transaction.
///
/// With `foreign_keys_off` on SQLite, the transaction runs on a connection
/// of its own with foreign key enforcement off, which SQLite only changes
/// outside a transaction, and commits once no foreign key points nowhere.
async fn in_ddl_scope(
    db: &Database,
    foreign_keys_off: bool,
    work: impl std::future::Future<Output = Result<()>> + Send + 'static,
) -> Result<()> {
    if !supports_transactional_ddl(db.backend()) {
        return work.await;
    }
    #[cfg(feature = "sqlite")]
    if foreign_keys_off && db.backend() == DatabaseType::SQLite {
        return super::sqlite_foreign_keys::run_with_foreign_keys_off(db, work).await;
    }
    #[cfg(not(feature = "sqlite"))]
    let _ = foreign_keys_off;

    db.atomically(work).await
}

/// The migrations ledger `table` names, checked before it reaches any SQL.
fn checked_ledger(table: &str) -> Result<Ledger<'_>> {
    if !is_safe_identifier_segment(table) {
        return Err(Error::configuration(format!(
            "invalid migrations table name '{}': expected ASCII letters, numbers, and underscores",
            table
        )));
    }

    Ok(Ledger::migrations(table))
}

impl Default for Migrator {
    fn default() -> Self {
        Self::new()
    }
}
