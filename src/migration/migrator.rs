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

/// Whether the backend keeps DDL inside a transaction.
///
/// PostgreSQL and SQLite roll DDL back with the surrounding transaction, so a
/// migration and its ledger row can be committed together. MySQL and MariaDB
/// implicitly commit around every DDL statement, so wrapping a migration there
/// would only pretend to be atomic - they run unwrapped instead.
fn supports_transactional_ddl(db_type: DatabaseType) -> bool {
    matches!(db_type, DatabaseType::Postgres | DatabaseType::SQLite)
}

/// Cross-process guard that serializes concurrent migrators.
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
enum MigrationLock {
    #[cfg(feature = "sqlite")]
    Sqlite(std::fs::File),
    #[cfg(feature = "sqlite")]
    SqliteMemory(futures_util::lock::OwnedMutexGuard<()>),
    #[cfg(feature = "postgres")]
    Postgres(crate::internal::sqlx::PgConnection),
    #[cfg(feature = "mysql")]
    Session(crate::internal::sqlx::MySqlConnection),
}

impl MigrationLock {
    async fn acquire(db: &Database, db_type: DatabaseType) -> Result<Self> {
        #[cfg(not(any(feature = "postgres", feature = "mysql")))]
        let _ = db;
        match db_type {
            #[cfg(feature = "sqlite")]
            DatabaseType::SQLite => {
                let pool = db.__internal_connection()?;
                let sqlite = pool.get_sqlite_connection_pool();
                // Where SQLite opened the main database, whatever the URL's
                // spelling (`sqlite:///C:/..` names `/C:/..`, which Windows
                // cannot resolve); empty for an in-memory database.
                let file: String = crate::internal::sqlx::query_scalar(
                    "SELECT file FROM pragma_database_list WHERE name = 'main'",
                )
                .fetch_one(sqlite)
                .await
                .map_err(|error| crate::internal::translate_error(sqlx_error(error)))?;
                if file.is_empty() {
                    let name = sqlite.connect_options().get_filename().to_path_buf();
                    return Ok(Self::SqliteMemory(sqlite_memory_lock(name).await));
                }
                let file = std::fs::canonicalize(&file)
                    .unwrap_or_else(|_| std::path::PathBuf::from(&file));
                let mut path = file.into_os_string();
                path.push(".tideorm-migrations.lock");
                Ok(Self::Sqlite(sqlite_file_lock(path.into()).await?))
            }
            #[cfg(not(feature = "sqlite"))]
            DatabaseType::SQLite => Err(Error::backend_not_supported(
                "SQLite migrations require the sqlite feature",
                "SQLite",
            )),
            #[cfg(feature = "postgres")]
            DatabaseType::Postgres => {
                let pool = db.__internal_connection()?;
                if pool.get_database_backend() != crate::internal::OrmBackend::Postgres {
                    return Err(Error::transaction(
                        "the PostgreSQL migration lock needs a PostgreSQL connection",
                    ));
                }
                let mut connection = pool
                    .get_postgres_connection_pool()
                    .connect_options()
                    .connect()
                    .await
                    .map_err(|error| {
                        crate::internal::translate_connection_error(sqlx_error(error))
                    })?;
                crate::internal::sqlx::query("SELECT pg_advisory_lock($1)")
                    .bind(MIGRATION_LOCK_KEY)
                    .execute(&mut connection)
                    .await
                    .map_err(|error| translate_error(sqlx_error(error)))?;
                Ok(Self::Postgres(connection))
            }
            #[cfg(not(feature = "postgres"))]
            DatabaseType::Postgres => Err(Error::backend_not_supported(
                "the migration lock needs TideORM's `postgres` feature",
                "PostgreSQL",
            )),
            #[cfg(feature = "mysql")]
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                use crate::internal::sqlx::Row;

                let pool = db.__internal_connection()?;
                if pool.get_database_backend() != crate::internal::OrmBackend::MySql {
                    return Err(Error::transaction(
                        "the MySQL migration lock needs a MySQL connection",
                    ));
                }
                let mut connection = pool
                    .get_mysql_connection_pool()
                    .connect_options()
                    .connect()
                    .await
                    .map_err(|error| {
                        crate::internal::translate_connection_error(sqlx_error(error))
                    })?;

                let row = crate::internal::sqlx::query("SELECT GET_LOCK(?, ?)")
                    .bind(MIGRATION_LOCK_NAME)
                    .bind(MIGRATION_LOCK_TIMEOUT_SECONDS)
                    .fetch_one(&mut connection)
                    .await
                    .map_err(|error| translate_error(sqlx_error(error)))?;
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
                        "Timed out after {}s waiting for the '{}' migration lock; another migrator is still running",
                        MIGRATION_LOCK_TIMEOUT_SECONDS, MIGRATION_LOCK_NAME
                    )));
                }
                Ok(Self::Session(connection))
            }
            #[cfg(not(feature = "mysql"))]
            DatabaseType::MySQL | DatabaseType::MariaDB => Err(Error::backend_not_supported(
                "the migration lock needs TideORM's `mysql` feature",
                db_type.to_string(),
            )),
        }
    }

    async fn release(self) -> Result<()> {
        match self {
            #[cfg(feature = "sqlite")]
            Self::Sqlite(file) => file
                .unlock()
                .map_err(|e| Error::transaction(format!("migration unlock: {e}"))),
            #[cfg(feature = "sqlite")]
            Self::SqliteMemory(guard) => {
                drop(guard);
                Ok(())
            }
            #[cfg(feature = "postgres")]
            Self::Postgres(mut connection) => {
                crate::internal::sqlx::query("SELECT pg_advisory_unlock($1)")
                    .bind(MIGRATION_LOCK_KEY)
                    .execute(&mut connection)
                    .await
                    .map_err(|error| translate_error(sqlx_error(error)))?;
                connection
                    .close()
                    .await
                    .map_err(|error| translate_error(sqlx_error(error)))
            }
            #[cfg(feature = "mysql")]
            Self::Session(mut connection) => {
                crate::internal::sqlx::query("SELECT RELEASE_LOCK(?)")
                    .bind(MIGRATION_LOCK_NAME)
                    .execute(&mut connection)
                    .await
                    .map_err(|error| translate_error(sqlx_error(error)))?;
                connection
                    .close()
                    .await
                    .map_err(|error| translate_error(sqlx_error(error)))
            }
        }
    }
}

/// The migration lock of the in-memory database `name` (SQLx's name for it,
/// which a named shared-cache database keeps across pools): one in-process
/// mutex per database.
#[cfg(feature = "sqlite")]
async fn sqlite_memory_lock(name: std::path::PathBuf) -> futures_util::lock::OwnedMutexGuard<()> {
    type Locks = std::collections::HashMap<std::path::PathBuf, Arc<futures_util::lock::Mutex<()>>>;
    static LOCKS: std::sync::OnceLock<parking_lot::Mutex<Locks>> = std::sync::OnceLock::new();
    let lock = Arc::clone(
        LOCKS
            .get_or_init(Default::default)
            .lock()
            .entry(name)
            .or_default(),
    );
    lock.lock_owned().await
}

/// Blocking file locks live on a worker thread, never the async executor.
/// Dropping a cancelled wait drops the eventual file guard too.
#[cfg(feature = "sqlite")]
async fn sqlite_file_lock(path: std::path::PathBuf) -> Result<std::fs::File> {
    type Waiting = (Option<Result<std::fs::File>>, Option<std::task::Waker>);
    let state = Arc::new(std::sync::Mutex::new(Waiting::default()));
    let worker = Arc::clone(&state);
    std::thread::Builder::new()
        .name("tideorm-migration-lock".into())
        .spawn(move || {
            let result = open_locked(&path);
            let mut state = worker.lock().unwrap();
            state.0 = Some(result);
            let waker = state.1.take();
            drop(state);
            if let Some(waker) = waker {
                waker.wake();
            }
        })
        .map_err(|e| Error::transaction(format!("start migration lock worker: {e}")))?;
    std::future::poll_fn(|cx| {
        let mut state = state.lock().unwrap();
        if let Some(result) = state.0.take() {
            std::task::Poll::Ready(result)
        } else {
            state.1 = Some(cx.waker().clone());
            std::task::Poll::Pending
        }
    })
    .await
}

/// Open (creating) the lock file at `path` and wait for its exclusive lock.
#[cfg(feature = "sqlite")]
fn open_locked(path: &std::path::Path) -> Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| Error::transaction(format!("open migration lock: {e}")))?;
    file.lock()
        .map_err(|e| Error::transaction(format!("acquire migration lock: {e}")))?;
    Ok(file)
}

/// A driver error from the lock's own connection, as SeaORM reports one.
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
fn sqlx_error(error: crate::internal::sqlx::Error) -> crate::internal::OrmError {
    crate::internal::OrmError::Query(crate::internal::RuntimeErr::SqlxError(error.into()))
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
            apply_migration(db, Arc::clone(migration), ledger.table().to_string()).await?;
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

        revert_migration(
            db,
            Arc::clone(migration),
            last_version,
            ledger.table().to_string(),
        )
        .await?;

        result.rolled_back.push(info_of(migration.as_ref()));

        Ok(result)
    }

    /// Rollback multiple migrations
    pub async fn rollback_steps(&self, steps: usize) -> Result<MigrationResult> {
        let mut result = MigrationResult::default();

        for _ in 0..steps {
            let step_result = self.rollback().await?;
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
        let ledger = self.ledger()?;
        ledger.ensure(&db).await?;

        let recorded = ledger.keys(&db).await?;
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

        self.rollback_steps(recorded.len()).await
    }

    /// Refresh migrations (reset + run)
    pub async fn refresh(&self) -> Result<MigrationResult> {
        let reset_result = self.reset().await?;
        let run_result = self.run().await?;

        Ok(MigrationResult {
            applied: run_result.applied,
            skipped: run_result.skipped,
            rolled_back: reset_result.rolled_back,
        })
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

/// Apply one migration and record it in the ledger, in one
/// [`in_ddl_scope`].
async fn apply_migration(
    db: &Database,
    migration: Arc<dyn Migration>,
    table: String,
) -> Result<()> {
    let db_type = db.backend();
    in_ddl_scope(db, async move {
        let mut schema = Schema::new(db_type);
        migration.up(&mut schema).await?;
        Ledger::migrations(&table)
            .record(&require_db()?, migration.version(), &[migration.name()])
            .await
    })
    .await
}

/// Revert one migration and remove its ledger row, in one [`in_ddl_scope`].
async fn revert_migration(
    db: &Database,
    migration: Arc<dyn Migration>,
    version: &str,
    table: String,
) -> Result<()> {
    let db_type = db.backend();
    let version = version.to_string();
    in_ddl_scope(db, async move {
        let mut schema = Schema::new(db_type);
        migration.down(&mut schema).await?;
        Ledger::migrations(&table)
            .remove(&require_db()?, &version)
            .await
    })
    .await
}

/// Run `work` holding the migration lock, so concurrent migrators, the CLI's
/// included, take turns.
async fn with_migration_lock<T>(
    db: &Database,
    work: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    let lock = MigrationLock::acquire(db, db.backend()).await?;
    let outcome = work.await;
    let released = lock.release().await;

    let result = outcome?;
    released?;
    Ok(result)
}

/// Run `work` in one transaction where the backend's DDL is transactional,
/// so a statement failing partway cannot leave schema changes behind with no
/// ledger row; elsewhere unwrapped, because the backend would implicitly
/// commit the DDL anyway. `Schema` and the ledger resolve their connection
/// from the ambient scope, which is what puts them inside the transaction.
async fn in_ddl_scope(
    db: &Database,
    work: impl std::future::Future<Output = Result<()>> + Send + 'static,
) -> Result<()> {
    if supports_transactional_ddl(db.backend()) {
        return db.transaction(move |_| Box::pin(work)).await;
    }

    work.await
}

/// Apply one migration given as SQL statements on `db` and record it in the
/// ledger `table`, or with `apply` false revert it and remove it, as
/// [`Migrator`] does: holding the migration lock, in one transaction where
/// the backend's DDL is transactional. The CLI runs its migrations through it.
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
        in_ddl_scope(db, async move {
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
