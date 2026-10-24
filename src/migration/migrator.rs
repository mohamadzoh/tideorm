use std::collections::HashSet;
use std::sync::Arc;

use crate::database::{Database, require_db};
use crate::error::{Error, Result};
use crate::internal::sql_safety::is_safe_identifier_segment;
#[cfg(any(feature = "postgres", feature = "mysql"))]
use crate::internal::sqlx::{ConnectOptions, Connection};
#[cfg(any(feature = "postgres", feature = "mysql"))]
use crate::internal::translate_error;
use crate::tide_info;

use super::{
    DatabaseType, Ledger, Migration, MigrationInfo, MigrationResult, MigrationStatus, Schema,
};

/// Ledger table a migrator uses unless it is pointed at another one.
///
/// The CLI exposes the same setting as `[migration] table` in `tideorm.toml`. A
/// project that renames it there must call [`Migrator::migrations_table`] with
/// the same name, or the CLI and the runtime keep two independent ledgers and
/// every migration ends up applied twice.
const DEFAULT_MIGRATIONS_TABLE: &str = "_migrations";

/// Key used for the PostgreSQL advisory lock that serializes migrators.
#[cfg(feature = "postgres")]
const MIGRATION_LOCK_KEY: i64 = 0x0054_4944_454F_524D;

/// Name used for the MySQL/MariaDB named lock that serializes migrators.
#[cfg(feature = "mysql")]
const MIGRATION_LOCK_NAME: &str = "tideorm_migrations";

/// How long a migrator waits for the MySQL/MariaDB named lock, in seconds.
///
/// `GET_LOCK` requires a timeout, unlike the PostgreSQL advisory lock which
/// simply waits. It is generous on purpose: the wait covers however long the
/// migrator that holds the lock takes to finish its own migrations.
#[cfg(feature = "mysql")]
const MIGRATION_LOCK_TIMEOUT_SECONDS: i64 = 300;

#[derive(Clone, Copy)]
enum LockKind {
    Migration,
    Seed,
}

impl LockKind {
    #[cfg(feature = "sqlite")]
    fn suffix(self) -> &'static str {
        match self {
            Self::Migration => ".tideorm-migrations.lock",
            Self::Seed => ".tideorm-seeds.lock",
        }
    }

    #[cfg(feature = "postgres")]
    fn key(self) -> i64 {
        match self {
            Self::Migration => MIGRATION_LOCK_KEY,
            Self::Seed => 0x0054_4944_4553_4544,
        }
    }

    #[cfg(feature = "mysql")]
    fn name(self) -> &'static str {
        match self {
            Self::Migration => MIGRATION_LOCK_NAME,
            Self::Seed => "tideorm_seeds",
        }
    }
}

/// Whether the backend keeps DDL inside a transaction.
///
/// PostgreSQL and SQLite roll DDL back with the surrounding transaction, so a
/// migration and its ledger row can be committed together. MySQL and MariaDB
/// implicitly commit around every DDL statement, so wrapping a migration there
/// would only pretend to be atomic - they run unwrapped instead.
fn supports_transactional_ddl(db_type: DatabaseType) -> bool {
    matches!(db_type, DatabaseType::Postgres | DatabaseType::SQLite)
}

/// Cross-process guard for migration and seed operations, each with its own
/// lock namespace.
///
/// Without it, N replicas booting together each run every pending `up()` at the
/// same time. The lock is held on a connection of its own; the migrations
/// themselves keep running on the pool.
///
/// Lock sessions connect directly using the pool's connection options. They
/// do not occupy pool slots: even a one-connection pool remains available to
/// execute DDL while other migrators wait for the advisory lock.
/// SQLite uses an OS lock on a persistent sidecar file beside the database,
/// which must not be unlinked while another migrator could be waiting on it.
/// An in-memory database, which only its own process can reach, takes an
/// in-process lock instead: never a file in a shared directory, where another
/// user could create or replace it first.
enum LedgerLock {
    #[cfg(feature = "sqlite")]
    Sqlite(std::fs::File),
    #[cfg(feature = "sqlite")]
    SqliteMemory(super::memory_lock::MemoryLock),
    #[cfg(feature = "postgres")]
    Postgres(crate::internal::sqlx::PgConnection),
    #[cfg(feature = "mysql")]
    Session(crate::internal::sqlx::MySqlConnection),
}

impl LedgerLock {
    async fn acquire(db: &Database, kind: LockKind) -> Result<Self> {
        #[cfg(not(any(feature = "postgres", feature = "mysql", feature = "sqlite")))]
        let _ = kind;
        match db.backend() {
            #[cfg(feature = "sqlite")]
            DatabaseType::SQLite => Self::acquire_sqlite(db, kind).await,
            #[cfg(not(feature = "sqlite"))]
            DatabaseType::SQLite => Err(Error::backend_not_supported(
                "SQLite ledger locks require the sqlite feature",
                "SQLite",
            )),
            #[cfg(feature = "postgres")]
            DatabaseType::Postgres => Self::acquire_postgres(db, kind).await,
            #[cfg(not(feature = "postgres"))]
            DatabaseType::Postgres => Err(Error::backend_not_supported(
                "the ledger lock needs TideORM's `postgres` feature",
                "PostgreSQL",
            )),
            #[cfg(feature = "mysql")]
            DatabaseType::MySQL | DatabaseType::MariaDB => Self::acquire_mysql(db, kind).await,
            #[cfg(not(feature = "mysql"))]
            DatabaseType::MySQL | DatabaseType::MariaDB => Err(Error::backend_not_supported(
                "the ledger lock needs TideORM's `mysql` feature",
                db.backend().to_string(),
            )),
        }
    }

    /// An OS lock on a sidecar file beside the database, or an in-process
    /// lock for an in-memory one.
    #[cfg(feature = "sqlite")]
    async fn acquire_sqlite(db: &Database, kind: LockKind) -> Result<Self> {
        use crate::internal::ConnectionTrait;

        // Where SQLite opened the main database, whatever the URL's
        // spelling (`sqlite:///C:/..` names `/C:/..`, which Windows
        // cannot resolve); empty for an in-memory database. Read on
        // the connection statements run on: inside a transaction, a
        // one-connection pool has no other to hand out.
        let connection = db.__get_connection()?;
        let executor = connection.executor();
        let probe = crate::internal::build_statement_with_values(
            executor.get_database_backend(),
            "SELECT file FROM pragma_database_list WHERE name = 'main'",
            Vec::new(),
        );
        let file: String = match executor
            .query_one_raw(probe)
            .await
            .map_err(crate::internal::translate_error)?
        {
            Some(row) => row
                .try_get("", "file")
                .map_err(crate::internal::translate_error)?,
            None => String::new(),
        };
        if file.is_empty() {
            let pool = db.__internal_connection()?;
            let sqlite = pool.get_sqlite_connection_pool();
            let mut name = sqlite
                .connect_options()
                .get_filename()
                .as_os_str()
                .to_os_string();
            name.push(kind.suffix());
            return Ok(Self::SqliteMemory(
                super::memory_lock::MemoryLock::acquire(name.into()).await,
            ));
        }
        let file = std::fs::canonicalize(&file).unwrap_or_else(|_| std::path::PathBuf::from(&file));
        let mut path = file.into_os_string();
        path.push(kind.suffix());
        Ok(Self::Sqlite(sqlite_file_lock(path.into()).await?))
    }

    /// An advisory lock, held on a session of its own.
    #[cfg(feature = "postgres")]
    async fn acquire_postgres(db: &Database, kind: LockKind) -> Result<Self> {
        let pool = db.__internal_connection()?;
        if pool.get_database_backend() != crate::internal::OrmBackend::Postgres {
            return Err(Error::transaction(
                "the PostgreSQL ledger lock needs a PostgreSQL connection",
            ));
        }
        let mut connection = pool
            .get_postgres_connection_pool()
            .connect_options()
            .connect()
            .await
            .map_err(|error| crate::internal::translate_connection_error(sqlx_error(error)))?;
        crate::internal::sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(kind.key())
            .execute(&mut connection)
            .await
            .map_err(sqlx_failure)?;
        Ok(Self::Postgres(connection))
    }

    /// A named lock, held on a session of its own.
    #[cfg(feature = "mysql")]
    async fn acquire_mysql(db: &Database, kind: LockKind) -> Result<Self> {
        use crate::internal::sqlx::Row;

        let pool = db.__internal_connection()?;
        if pool.get_database_backend() != crate::internal::OrmBackend::MySql {
            return Err(Error::transaction(
                "the MySQL ledger lock needs a MySQL connection",
            ));
        }
        let mut connection = pool
            .get_mysql_connection_pool()
            .connect_options()
            .connect()
            .await
            .map_err(|error| crate::internal::translate_connection_error(sqlx_error(error)))?;

        let row = crate::internal::sqlx::query("SELECT GET_LOCK(?, ?)")
            .bind(kind.name())
            .bind(MIGRATION_LOCK_TIMEOUT_SECONDS)
            .fetch_one(&mut connection)
            .await
            .map_err(sqlx_failure)?;
        // `GET_LOCK` is 1 once acquired, 0 on timeout, NULL on error.
        let acquired = row
            .try_get::<Option<i64>, _>(0)
            .or_else(|_| {
                row.try_get::<Option<i32>, _>(0)
                    .map(|flag| flag.map(i64::from))
            })
            .ok()
            .flatten();
        if acquired != Some(1) {
            return Err(Error::transaction(format!(
                "Timed out after {}s waiting for the '{}' lock; another ledger operation is still running",
                MIGRATION_LOCK_TIMEOUT_SECONDS,
                kind.name()
            )));
        }
        Ok(Self::Session(connection))
    }

    async fn release(self, kind: LockKind) -> Result<()> {
        #[cfg(not(any(feature = "postgres", feature = "mysql")))]
        let _ = kind;
        match self {
            #[cfg(feature = "sqlite")]
            Self::Sqlite(file) => file
                .unlock()
                .map_err(|e| Error::transaction(format!("ledger unlock: {e}"))),
            #[cfg(feature = "sqlite")]
            Self::SqliteMemory(guard) => {
                drop(guard);
                Ok(())
            }
            #[cfg(feature = "postgres")]
            Self::Postgres(mut connection) => {
                crate::internal::sqlx::query("SELECT pg_advisory_unlock($1)")
                    .bind(kind.key())
                    .execute(&mut connection)
                    .await
                    .map_err(sqlx_failure)?;
                connection.close().await.map_err(sqlx_failure)
            }
            #[cfg(feature = "mysql")]
            Self::Session(mut connection) => {
                crate::internal::sqlx::query("SELECT RELEASE_LOCK(?)")
                    .bind(kind.name())
                    .execute(&mut connection)
                    .await
                    .map_err(sqlx_failure)?;
                connection.close().await.map_err(sqlx_failure)
            }
        }
    }
}

/// Poll a nonblocking OS lock with a runtime-independent timer. Cancelling
/// this future drops the file immediately; no worker remains blocked on it.
#[cfg(feature = "sqlite")]
async fn sqlite_file_lock(path: std::path::PathBuf) -> Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| Error::transaction(format!("open ledger lock: {e}")))?;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => {
                async_io::Timer::after(std::time::Duration::from_millis(25)).await;
            }
            Err(error) => {
                return Err(Error::transaction(format!("acquire ledger lock: {error}")));
            }
        }
    }
}

/// A driver error from a connection of the migrator's own, as SeaORM reports one.
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
pub(super) fn sqlx_error(error: crate::internal::sqlx::Error) -> crate::internal::OrmError {
    crate::internal::OrmError::Query(crate::internal::RuntimeErr::SqlxError(error.into()))
}

/// A statement failure on a lock session of the migrator's own.
#[cfg(any(feature = "postgres", feature = "mysql"))]
fn sqlx_failure(error: crate::internal::sqlx::Error) -> Error {
    translate_error(sqlx_error(error))
}

/// Migration runner
///
/// Manages and executes database migrations.
pub struct Migrator {
    /// Migrations are held behind `Arc` so a single migration can be moved into
    /// the transaction closure that applies it, which must own `'static` data.
    migrations: Vec<Arc<dyn Migration>>,
    /// Name of the ledger table that records which migrations have been applied.
    table: String,
}

impl Migrator {
    /// Create a new migrator
    pub fn new() -> Self {
        Self {
            migrations: Vec::new(),
            table: DEFAULT_MIGRATIONS_TABLE.to_string(),
        }
    }

    /// Add a migration
    #[allow(clippy::should_implement_trait)]
    pub fn add<M: Migration + 'static>(mut self, migration: M) -> Self {
        self.migrations.push(Arc::new(migration));
        self
    }

    /// Add a boxed migration (used internally by TideConfig)
    #[doc(hidden)]
    pub fn add_boxed(mut self, migration: Box<dyn Migration>) -> Self {
        self.migrations.push(Arc::from(migration));
        self
    }

    /// Record applied migrations in `name` instead of the default `_migrations`.
    ///
    /// This must match the CLI's `[migration] table` setting in `tideorm.toml`.
    /// If the two disagree the CLI and the application each keep their own
    /// ledger, so migrations the CLI already applied look pending to the
    /// application and are applied a second time.
    ///
    /// The name is interpolated into DDL, so it may only contain ASCII letters,
    /// numbers, and underscores. An invalid name is reported the first time the
    /// ledger is touched, not here - a builder method has nowhere to return the
    /// error, and quietly falling back to the default would hand the caller the
    /// very second ledger this setting exists to avoid.
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
    pub async fn run(&self) -> Result<MigrationResult> {
        let db = require_db()?;
        with_migration_lock(&db, self.run_locked(&db)).await
    }

    async fn run_locked(&self, db: &Database) -> Result<MigrationResult> {
        let ledger = self.ledger()?;
        ledger.ensure(db).await?;

        let applied = self.applied_versions(&ledger, db).await?;
        let mut result = MigrationResult::default();

        for migration in self.sorted() {
            let version = migration.version();
            let info = info_of(migration.as_ref());

            if applied.iter().any(|applied| applied == version) {
                result.skipped.push(info);
                continue;
            }

            tide_info!("Running migration: {} - {}", version, migration.name());
            run_migration(db, Arc::clone(migration), ledger.table().to_string(), true).await?;
            tide_info!("Completed migration: {} - {}", version, migration.name());

            result.applied.push(info);
        }

        Ok(result)
    }

    /// Rollback the last migration
    ///
    /// Takes the same lock as [`Migrator::run`], and reverts plus un-records the
    /// migration in one transaction where the backend allows it.
    pub async fn rollback(&self) -> Result<MigrationResult> {
        let db = require_db()?;
        with_migration_lock(&db, self.rollback_locked(&db)).await
    }

    async fn rollback_locked(&self, db: &Database) -> Result<MigrationResult> {
        let ledger = self.ledger()?;
        ledger.ensure(db).await?;

        // The ledger is ordered by insertion id, so the last entry is the
        // migration applied most recently - not the one with the highest
        // version. After a long-lived branch merges those are routinely
        // different migrations. Every entry is read, registered or not: one
        // this migrator cannot revert must stop the rollback, not be skipped
        // over to revert an older migration instead.
        let applied = ledger.keys(db).await?;
        let mut result = MigrationResult::default();

        let Some(last_version) = applied.last() else {
            return Ok(result);
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

        Ok(result)
    }

    /// Rollback multiple migrations while holding one migration lock.
    pub async fn rollback_steps(&self, steps: usize) -> Result<MigrationResult> {
        if steps == 0 {
            return Ok(MigrationResult::default());
        }
        let db = require_db()?;
        with_migration_lock(&db, self.rollback_steps_locked(&db, steps)).await
    }

    async fn rollback_steps_locked(&self, db: &Database, steps: usize) -> Result<MigrationResult> {
        let mut result = MigrationResult::default();

        for _ in 0..steps {
            let step_result = self.rollback_locked(db).await?;
            if step_result.rolled_back.is_empty() {
                break;
            }
            result.rolled_back.extend(step_result.rolled_back);
        }

        Ok(result)
    }

    /// Reset all migrations (rollback all)
    ///
    /// Only migrations still registered on this migrator can be reverted, since
    /// a ledger row for a version that no longer exists in code has no `down()`
    /// left to run. Reset fails before reverting anything if such rows exist.
    pub async fn reset(&self) -> Result<MigrationResult> {
        let db = require_db()?;
        with_migration_lock(&db, self.reset_locked(&db)).await
    }

    async fn reset_locked(&self, db: &Database) -> Result<MigrationResult> {
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

        self.rollback_steps_locked(db, recorded.len()).await
    }

    /// Refresh migrations (reset + run)
    pub async fn refresh(&self) -> Result<MigrationResult> {
        let db = require_db()?;
        with_migration_lock(&db, async {
            let reset_result = self.reset_locked(&db).await?;
            let run_result = self.run_locked(&db).await?;

            Ok(MigrationResult {
                applied: run_result.applied,
                skipped: run_result.skipped,
                rolled_back: reset_result.rolled_back,
            })
        })
        .await
    }

    /// Get migration status
    pub async fn status(&self) -> Result<Vec<MigrationStatus>> {
        let db = require_db()?;
        let ledger = self.ledger()?;
        ledger.ensure(&db).await?;

        let applied = self.applied_versions(&ledger, &db).await?;

        Ok(self
            .sorted()
            .into_iter()
            .map(|migration| MigrationStatus {
                version: migration.version().to_string(),
                name: migration.name().to_string(),
                applied: applied.iter().any(|applied| applied == migration.version()),
            })
            .collect())
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
fn info_of(migration: &dyn Migration) -> MigrationInfo {
    MigrationInfo {
        version: migration.version().to_string(),
        name: migration.name().to_string(),
    }
}

/// Apply one migration and record it in the ledger, or with `apply` false
/// revert it and remove its ledger row, in one [`in_ddl_scope`].
async fn run_migration(
    db: &Database,
    migration: Arc<dyn Migration>,
    table: String,
    apply: bool,
) -> Result<()> {
    let db_type = db.backend();
    let foreign_keys_off = migration.foreign_keys_off();
    in_ddl_scope(db, foreign_keys_off, async move {
        let mut schema = Schema::new(db_type).with_foreign_keys_off(foreign_keys_off);
        let ledger = Ledger::migrations(&table);
        if apply {
            migration.up(&mut schema).await?;
            ledger
                .record(&require_db()?, migration.version(), &[migration.name()])
                .await
        } else {
            migration.down(&mut schema).await?;
            ledger.remove(&require_db()?, migration.version()).await
        }
    })
    .await
}

/// Run `work` holding the migration lock, so concurrent migrators, the CLI's
/// included, take turns.
async fn with_migration_lock<T>(
    db: &Database,
    work: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    with_ledger_lock(db, LockKind::Migration, work).await
}

/// Seeds use their own lock namespace, so a migration may run seeds without
/// waiting on the migration lock it already holds. Each complete seed
/// operation holds this lock before reading or changing its ledger.
pub(crate) async fn with_seed_lock<T>(
    db: &Database,
    work: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    // A seed's savepoint can finish before its ledger entry is visible to
    // other runners. Keep the guard in the enclosing transaction's record,
    // shared with all its savepoints, rather than releasing it with `work`.
    let record = db.__get_connection()?.source().record;
    if let Some(record) = record {
        let retained = record.lock().seed_lock();
        let mut held = retained.lock().await;
        if held.is_none() {
            *held = Some(Box::new(LedgerLock::acquire(db, LockKind::Seed).await?));
        }
        if db.execution_backend() == DatabaseType::SQLite {
            use crate::internal::ConnectionTrait;
            Ledger::seeds().ensure(db).await?;
            // A read snapshot may predate a seed committed before this lock was
            // acquired. Reserve the writer before reading the ledger or calling
            // user code; SQLite rejects stale snapshots here without side effects.
            db.__get_connection()?
                .executor()
                .execute_unprepared("UPDATE _seeds SET name = name WHERE 0")
                .await
                .map_err(crate::internal::translate_error)?;
        }
        return work.await;
    }
    with_ledger_lock(db, LockKind::Seed, work).await
}

async fn with_ledger_lock<T>(
    db: &Database,
    kind: LockKind,
    work: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    let lock = LedgerLock::acquire(db, kind).await?;
    let outcome = work.await;
    let released = lock.release(kind).await;

    let result = outcome?;
    released?;
    Ok(result)
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

    db.transaction(move |_| Box::pin(work)).await
}

/// Apply one migration given as SQL statements on `db` and record it in the
/// ledger `table`, or with `apply` false revert it and remove it, as
/// [`Migrator`] does: holding the migration lock, in one transaction where
/// the backend's DDL is transactional. The CLI runs its migrations through it.
///
/// On SQLite, `PRAGMA foreign_keys` does nothing inside that transaction: a
/// caller rebuilding a table runs the statements on a one-connection `db`
/// that already has foreign keys off, as the CLI does.
///
/// Returns `false`, having run nothing, when the ledger read under the lock
/// shows another run already did it.
#[doc(hidden)]
pub async fn __run_sql_migration(
    db: &Database,
    table: &str,
    version: &str,
    name: &str,
    statements: Vec<String>,
    apply: bool,
) -> Result<bool> {
    let ledger = checked_ledger(table)?;
    with_migration_lock(db, async {
        ledger.ensure(db).await?;
        let recorded = ledger.keys(db).await?.iter().any(|key| key == version);
        if recorded == apply {
            return Ok(false);
        }

        let (handle, table, version, name) = (
            db.clone(),
            table.to_string(),
            version.to_string(),
            name.to_string(),
        );
        in_ddl_scope(db, false, async move {
            for statement in &statements {
                handle.exec_raw(statement).await?;
            }
            let ledger = Ledger::migrations(&table);
            if apply {
                ledger.record(&handle, &version, &[&name]).await
            } else {
                ledger.remove(&handle, &version).await
            }
        })
        .await?;
        Ok(true)
    })
    .await
}

/// Create the migrations ledger `table` on `db` if it is missing, in the shape
/// [`Migrator`] creates it.
#[doc(hidden)]
pub async fn __ensure_migration_ledger(db: &Database, table: &str) -> Result<()> {
    checked_ledger(table)?.ensure(db).await
}

/// Create the seed ledger on `db` if it is missing.
#[doc(hidden)]
pub async fn __ensure_seed_ledger(db: &Database) -> Result<()> {
    Ledger::seeds().ensure(db).await
}

/// Record a migration as applied in the ledger `table`, or with `applied`
/// false remove it, without running it: the CLI's `migrate mark`.
#[doc(hidden)]
pub async fn __mark_migration(
    db: &Database,
    table: &str,
    version: &str,
    name: &str,
    applied: bool,
) -> Result<()> {
    let ledger = checked_ledger(table)?;
    ledger.ensure(db).await?;
    if applied {
        ledger.record(db, version, &[name]).await
    } else {
        ledger.remove(db, version).await
    }
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
