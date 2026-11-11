//! The cross-process locks migrators and seeders take turns on.

use crate::config::DatabaseType;
use crate::database::Database;
use crate::error::{Error, Result};
use crate::migration::Ledger;

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

/// Cross-process guard for migration and seed operations, each with its own
/// lock namespace.
///
/// Without it, N replicas booting together each run every pending `up()` at the
/// same time. The lock is held on a session of its own
/// ([`Database::pin_session`]); the migrations themselves keep running on the
/// pool.
///
/// A session opens its connection with the pool's settings but takes none of
/// the pool's: even a one-connection pool remains available to execute DDL
/// while other migrators wait for the lock. A session that drops closes its
/// connection, which releases the lock, so a cancelled run leaves none.
/// SQLite uses an OS lock on a persistent sidecar file beside the database,
/// which must not be unlinked while another migrator could be waiting on it.
/// An in-memory database, which only its own process can reach, takes an
/// in-process lock instead: never a file in a shared directory, where another
/// user could create or replace it first.
enum LedgerLock {
    #[cfg(feature = "sqlite")]
    Sqlite(std::fs::File),
    #[cfg(feature = "sqlite")]
    SqliteMemory(crate::migration::memory_lock::MemoryLock),
    /// PostgreSQL's advisory lock or MySQL's named one, on `session`.
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    Session {
        session: Database,
        /// The statement releasing the lock, and the lock it names.
        release: (&'static str, crate::DbValue),
    },
}

impl LedgerLock {
    /// The lock for `db`'s pool, which a transaction's handle names too.
    async fn acquire(db: &Database, kind: LockKind) -> Result<Self> {
        #[cfg(not(any(feature = "postgres", feature = "mysql", feature = "sqlite")))]
        let _ = kind;
        let db = &db.pool_handle()?;
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
        // Where SQLite opened the main database, whatever the URL's
        // spelling (`sqlite:///C:/..` names `/C:/..`, which Windows
        // cannot resolve), recorded when the pool opened: inside a
        // transaction, a one-connection pool has no other to hand out.
        let pool = db.current_inner()?;
        let Some(file) = pool.sqlite_file.clone() else {
            let sqlite = pool.conn.get_sqlite_connection_pool().ok_or_else(|| {
                crate::Error::connection("a SQLite migration lock needs a SQLite connection")
            })?;
            let mut name = sqlite
                .connect_options()
                .get_filename()
                .as_os_str()
                .to_os_string();
            name.push(kind.suffix());
            return Ok(Self::SqliteMemory(
                crate::migration::memory_lock::MemoryLock::acquire(name.into()).await,
            ));
        };
        let mut path = file.into_os_string();
        path.push(kind.suffix());
        Ok(Self::Sqlite(sqlite_file_lock(path.into()).await?))
    }

    /// An advisory lock, held on a session of its own.
    #[cfg(feature = "postgres")]
    async fn acquire_postgres(db: &Database, kind: LockKind) -> Result<Self> {
        let session = db.pin_session().await?;
        session
            .fetch_rows("SELECT pg_advisory_lock($1)", vec![kind.key().into()])
            .await?;
        Ok(Self::Session {
            session,
            release: ("SELECT pg_advisory_unlock($1)", kind.key().into()),
        })
    }

    /// A named lock, held on a session of its own.
    #[cfg(feature = "mysql")]
    async fn acquire_mysql(db: &Database, kind: LockKind) -> Result<Self> {
        let session = db.pin_session().await?;
        let database = session
            .fetch_rows("SELECT DATABASE() AS name", Vec::new())
            .await?
            .first()
            .and_then(|row| row.try_get::<Option<String>>("name").ok())
            .flatten();
        let name = mysql_lock_name(kind, database.as_deref());
        let rows = session
            .fetch_rows(
                "SELECT GET_LOCK(?, ?) AS acquired",
                vec![name.clone().into(), MIGRATION_LOCK_TIMEOUT_SECONDS.into()],
            )
            .await?;
        // `GET_LOCK` is 1 once acquired, 0 on timeout, NULL on error.
        let acquired = rows
            .first()
            .and_then(|row| row.try_get::<Option<i64>>("acquired").ok())
            .flatten();
        if acquired != Some(1) {
            return Err(Error::transaction(format!(
                "Timed out after {MIGRATION_LOCK_TIMEOUT_SECONDS}s waiting for the '{name}' lock; another ledger operation is still running",
            )));
        }
        Ok(Self::Session {
            session,
            release: ("SELECT RELEASE_LOCK(?)", name.into()),
        })
    }

    async fn release(self) -> Result<()> {
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
            #[cfg(any(feature = "postgres", feature = "mysql"))]
            Self::Session {
                session,
                release: (sql, lock),
            } => {
                let released = session.fetch_rows(sql, vec![lock]).await;
                let closed = session.close().await;
                released.map(drop).and(closed)
            }
        }
    }
}

/// The named lock `kind` takes in `database`. `GET_LOCK` names are
/// server-wide, so the database's name is in it: applications on one server
/// do not wait on each other's migrations. MySQL refuses a name past 64
/// characters, so a long one is cut, and two databases whose names share
/// their first 45 characters take turns.
#[cfg(feature = "mysql")]
fn mysql_lock_name(kind: LockKind, database: Option<&str>) -> String {
    format!("{}:{}", kind.name(), database.unwrap_or_default())
        .chars()
        .take(64)
        .collect()
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

/// Run `work` holding the migration lock, so concurrent migrators, the CLI's
/// included, take turns.
///
/// Boxed, as is [`with_seed_lock`]: a ledger command's work is one large
/// future, and held inline in the caller's own it overflows a debug build's stack.
pub(in crate::migration) fn with_migration_lock<'a, T: 'a>(
    db: &'a Database,
    work: impl std::future::Future<Output = Result<T>> + 'a,
) -> impl std::future::Future<Output = Result<T>> + 'a {
    Box::pin(with_ledger_lock(db, LockKind::Migration, work))
}

/// Seeds use their own lock namespace, so a migration may run seeds without
/// waiting on the migration lock it already holds. Each complete seed
/// operation holds this lock before reading or changing its ledger.
pub(crate) fn with_seed_lock<'a, T: 'a>(
    db: &'a Database,
    work: impl std::future::Future<Output = Result<T>> + 'a,
) -> impl std::future::Future<Output = Result<T>> + 'a {
    Box::pin(seed_locked(db, work))
}

async fn seed_locked<T>(
    db: &Database,
    work: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    // A seed's savepoint can finish before its ledger entry is visible to
    // other runners. Keep the guard in the enclosing transaction's record,
    // shared with all its savepoints, rather than releasing it with `work`.
    let record = db.__get_connection()?.record().cloned();
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
    let released = lock.release().await;

    let result = outcome?;
    released?;
    Ok(result)
}

#[cfg(all(test, feature = "mysql"))]
#[path = "../../../tests/unit/migration_ledger_lock_tests.rs"]
mod tests;
