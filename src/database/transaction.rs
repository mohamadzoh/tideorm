use std::fmt::Write as _;
use std::future::Future;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::internal::{ConnectionTrait, TransactionTrait};
use crate::tide_warn;

use super::state::with_connection_override;
use super::{Database, TransactionOptions};
use crate::cache::{PendingInvalidations, Record};

/// Reported when a clone of the transaction handle outlived the closure.
const LEAKED_TRANSACTION_MESSAGE: &str = "transaction handle leaked outside the transaction scope";

/// How ending a leaked transaction went.
///
/// Carried into the error the caller sees so a leak never reports a clean
/// "handle leaked" while the transaction is, in fact, still holding locks.
enum LeakedRollback {
    /// The transaction, or the savepoint, was rolled back.
    RolledBack,
    /// The rollback failed.
    Failed(Error),
}

/// Roll back a transaction whose handle escaped its closure.
///
/// `Arc::try_unwrap` failed, so `commit`/`rollback`, which consume the
/// transaction, are unreachable; it is rolled back through the shared handle
/// instead, which ends the server-side transaction and its locks now, and the
/// stray handle's statements are refused from then on. A savepoint rolls back
/// to itself: the savepoints of one transaction take turns, so it is the
/// innermost one open.
///
/// Always reports the leak: it is a caller bug that otherwise only shows up as a
/// stalled connection much later.
async fn end_leaked_transaction(txn: &crate::internal::OrmTransaction) -> LeakedRollback {
    tide_warn!("{LEAKED_TRANSACTION_MESSAGE}. Rolling it back explicitly.");
    match txn.rollback_by_ref().await {
        Ok(()) => LeakedRollback::RolledBack,
        Err(err) => {
            let err = transaction_error(err);
            tide_warn!("Failed to roll back the leaked transaction: {err}");
            LeakedRollback::Failed(err)
        }
    }
}

/// Build the error a leaked transaction returns.
///
/// The leak is the headline — it is the caller bug — but neither of the two
/// things that happened alongside it may be dropped on the floor: a rollback the
/// engine rejected (the transaction is still open, which is the pool-exhaustion
/// case) and an error the closure had already returned (the reason the caller
/// was in the failure path to begin with). Both are folded into the message, and
/// whichever of them carries structured driver detail becomes the error's
/// source so SQLSTATE and the driver chain survive.
fn leaked_transaction_error(rollback: LeakedRollback, closure_error: Option<Error>) -> Error {
    let mut source = None;
    let outcome = match rollback {
        LeakedRollback::RolledBack => "it was rolled back explicitly".to_string(),
        LeakedRollback::Failed(err) => {
            let detail = format!("rolling it back failed: {err}");
            source = err.into_db_failure();
            detail
        }
    };

    let mut message = format!("{LEAKED_TRANSACTION_MESSAGE}, so {outcome}");

    if let Some(closure_error) = closure_error {
        let _ = write!(message, "; the closure had already failed: {closure_error}");
        source = source.or_else(|| closure_error.into_db_failure());
    }

    Error::Transaction { message, source }
}

/// Classify a transaction-control failure: a transaction error carrying the
/// backend message, unless the connection failed.
pub(crate) fn transaction_error(err: crate::internal::OrmError) -> Error {
    crate::internal::reclassify_error(err, |message, source| Error::Transaction {
        message,
        source,
    })
}

impl Database {
    /// Execute a closure within a database transaction
    ///
    /// After a deadlock, subsequent TideORM statements and nested transactions
    /// in this scope fail without reaching the server, even if the closure
    /// catches the original error. Retry the entire transaction instead.
    /// SQLite errors that roll back the entire transaction also block further
    /// statements and nested transactions. Ordinary statement-only constraint
    /// failures can still be caught without aborting the transaction.
    ///
    /// The transaction is always terminated before this returns. If a clone of
    /// the handle escaped the closure — for example a `Database` captured from
    /// `__current_db()` inside it and stored elsewhere — the transaction cannot
    /// be committed, and it is rolled back as far as the driver allows before
    /// the leak is reported. See `end_leaked_transaction` for what "as far as
    /// the driver allows" means per backend; the returned error says which of
    /// those outcomes happened, and carries the closure's own error when it had
    /// already failed.
    pub async fn transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: AsyncFnOnce(&Transaction) -> Result<T>,
    {
        self.transaction_with(TransactionOptions::default(), f)
            .await
    }

    /// Execute a closure within a database transaction begun with `options`,
    /// as [`Database::transaction`] does.
    ///
    /// PostgreSQL and MySQL/MariaDB take the isolation level and access mode,
    /// SQLite the `BEGIN` mode (`IMMEDIATE` takes the write lock up front);
    /// SQLite refuses `AccessMode::ReadOnly`, since it cannot keep a
    /// transaction from writing.
    ///
    /// ```ignore
    /// let report = db.transaction_with(
    ///     TransactionOptions {
    ///         isolation_level: Some(IsolationLevel::Serializable),
    ///         access_mode: Some(AccessMode::ReadOnly),
    ///         ..TransactionOptions::default()
    ///     },
    ///     async |_| Report::build().await,
    /// ).await?;
    /// ```
    ///
    /// Inside a transaction on this database's pool it would open a
    /// savepoint, which begins inside that transaction and so takes no
    /// options: any is an error there.
    pub async fn transaction_with<F, T>(&self, options: TransactionOptions, f: F) -> Result<T>
    where
        F: AsyncFnOnce(&Transaction) -> Result<T>,
    {
        let connection = self.__get_connection()?;
        let scope = TransactionScope::begin(&connection, options)
            .await
            .map_err(transaction_error)?;
        let tx = Transaction {
            connection: scope.connection(),
        };
        let outcome = with_connection_override(scope.connection(), f(&tx)).await;
        drop(tx);
        scope.end(outcome, transaction_error).await
    }

    /// Run `future` inside a database transaction, committing when it returns
    /// `Ok`.
    ///
    /// The same transaction as [`Database::transaction`], for a future rather
    /// than a closure handed the transaction: every TideORM call inside joins
    /// the transaction anyway, so nothing needs the handle. The future may
    /// borrow local variables.
    ///
    /// ```ignore
    /// let order = db.atomically(async {
    ///     let order = Order::create(order).await?;
    ///     Item::update_all().decrement("stock", 1).where_eq("id", item_id).execute().await?;
    ///     Ok(order)
    /// }).await?;
    /// ```
    ///
    /// Inside a transaction on this database's pool, it opens a savepoint.
    /// Use `require_db()?.atomically(..)` for the global database.
    pub async fn atomically<F, T>(&self, future: F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        let connection = self.__get_connection()?;
        let scope = TransactionScope::begin(&connection, TransactionOptions::default())
            .await
            .map_err(transaction_error)?;
        let outcome = with_connection_override(scope.connection(), future).await;
        scope.end(outcome, transaction_error).await
    }

    /// Run `future` with this database as the ambient connection, without
    /// opening a transaction.
    ///
    /// Every TideORM call inside it that names no handle — `Model::create`,
    /// `save`, `update`, `delete`, `find`, `insert_all`, `Model::query()`,
    /// `Database::raw` and the rest — runs on this database instead of the
    /// global one, so a replica or a tenant database needs no handle threaded
    /// through every call. Inside a transaction opened on this database's
    /// pool, they join that transaction, as a statement on this handle does.
    ///
    /// ```ignore
    /// let user = tenant.scope(async {
    ///     let user = User::create(user).await?;
    ///     Profile::create(Profile { user_id: user.id, ..Default::default() }).await?;
    ///     Ok(user)
    /// }).await?;
    /// ```
    pub async fn scope<F, T>(&self, future: F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        let connection = self.__get_connection()?;
        with_connection_override(connection, future).await
    }
}

/// A transaction, or a savepoint in the one a connection belongs to, from
/// its begin to its end: what `Database::transaction` and a batch insert
/// share.
///
/// The engine refuses the statements and the commit of a transaction a
/// failure aborted, on every level of it, and makes savepoints opened side by
/// side take turns.
pub(crate) struct TransactionScope {
    txn: Arc<crate::internal::OrmTransaction>,
    /// The pool, or session, it was begun on.
    pool: Arc<crate::internal::InternalConnection>,
    /// The record its statements mark, and that commits replay.
    record: Record,
}

impl TransactionScope {
    /// Begin a transaction with `options` on `connection`'s pool, or a
    /// savepoint in its transaction, which refuses any.
    pub(crate) async fn begin(
        connection: &ConnectionRef,
        options: TransactionOptions,
    ) -> std::result::Result<Self, crate::internal::OrmError> {
        connection.observe_attach();
        // Boxed: unbroken, the begin's future (down to the driver's connect)
        // nests a caller's layout past the compiler's query depth limit.
        let txn = Box::pin(connection.executor().begin_with_options(options)).await?;
        Ok(Self {
            txn: Arc::new(txn),
            pool: connection.pool().clone(),
            record: Arc::new(parking_lot::Mutex::new(PendingInvalidations::new(
                connection.record().cloned(),
            ))),
        })
    }

    /// The connection the scope's statements run on.
    pub(crate) fn connection(&self) -> ConnectionRef {
        ConnectionRef::Transaction(self.txn.clone(), self.pool.clone(), self.record.clone())
    }

    /// The record the scope's statements mark.
    pub(crate) fn record(&self) -> &Record {
        &self.record
    }

    /// Commit the transaction when `outcome` is a success, else roll it back.
    /// A commit failure is reported through `commit_error`.
    pub(crate) async fn end<T>(
        self,
        outcome: Result<T>,
        commit_error: impl FnOnce(crate::internal::OrmError) -> Error,
    ) -> Result<T> {
        let Self { record, txn, .. } = self;
        match (Arc::try_unwrap(txn), outcome) {
            (Ok(txn), Ok(result)) => {
                // A commit that fails or is cancelled may still have
                // committed: what the transaction invalidated is dropped
                // again either way.
                let unknown = UnknownCommit(Some(record.clone()));
                txn.commit().await.map_err(commit_error)?;
                unknown.disarm();
                std::mem::take(&mut *record.lock()).replay();
                Ok(result)
            }
            (Ok(txn), Err(e)) => {
                roll_back_failed(txn).await;
                Err(e)
            }
            (Err(txn), outcome) => {
                // A caller bug the enclosing transaction does not commit
                // past.
                txn.abort("a nested transaction's handle leaked outside its scope");
                let rollback = end_leaked_transaction(txn.as_ref()).await;
                Err(leaked_transaction_error(rollback, outcome.err()))
            }
        }
    }
}

/// Roll back a transaction that failed. The failure is the one the caller
/// needs; a rollback failure on top of it is reported, not swallowed.
async fn roll_back_failed(txn: crate::internal::OrmTransaction) {
    if let Err(rollback) = txn.rollback().await {
        tide_warn!(
            "Rolling back the failed transaction also failed: {}",
            transaction_error(rollback)
        );
    }
}

/// Replays a transaction's cache invalidations when dropped armed: when its
/// commit fails or is cancelled, which may have committed it.
struct UnknownCommit(Option<Record>);

impl UnknownCommit {
    fn disarm(mut self) {
        self.0 = None;
    }
}

impl Drop for UnknownCommit {
    fn drop(&mut self) {
        if let Some(pending) = self.0.take() {
            pending.lock().replay_invalidations();
        }
    }
}

impl Database {
    /// Run `note` recording in the transaction this handle's statements run
    /// in; see [`ConnectionRef::recording`].
    pub(crate) fn recording<T>(&self, note: impl FnOnce() -> T) -> T {
        match self.__get_connection() {
            Ok(connection) => connection.recording(note),
            Err(_) => note(),
        }
    }

    /// Run the write `future` on this handle's connection, invalidating as
    /// [`ConnectionRef::invalidating`] does.
    pub(crate) async fn invalidating<F: Future>(&self, future: F) -> F::Output {
        match self.__get_connection() {
            Ok(connection) => connection.invalidating(future).await,
            // Nothing can run, so nothing was written.
            Err(_) => future.await,
        }
    }
}

/// A database transaction handle
pub struct Transaction {
    connection: ConnectionRef,
}

impl Transaction {
    /// Get a reference to the underlying connection.
    ///
    /// TideORM does not see the writes of statements run on it directly, so
    /// once it is taken the transaction's commit drops this database's cached
    /// reads.
    #[doc(hidden)]
    pub fn connection(&self) -> &crate::internal::OrmTransaction {
        let ConnectionRef::Transaction(transaction, ..) = &self.connection else {
            unreachable!("a transaction handle holds its transaction");
        };
        self.connection.drop_cached_reads();
        transaction
    }
}

/// The connection a statement runs on: the pool, or the transaction an
/// enclosing [`Database::transaction`] scope installed, with the pool it was
/// begun on and the record its statements mark.
#[doc(hidden)]
#[derive(Clone)]
pub enum ConnectionRef {
    Database(Arc<crate::internal::InternalConnection>),
    Transaction(
        Arc<crate::internal::OrmTransaction>,
        Arc<crate::internal::InternalConnection>,
        Record,
    ),
}

impl ConnectionRef {
    /// Borrow this connection as one executor, so a statement is written once
    /// whichever of the two it runs on.
    pub fn executor(&self) -> crate::orm::DatabaseExecutor<'_> {
        match self {
            Self::Database(conn) => conn.connection().into(),
            Self::Transaction(tx, ..) => tx.as_ref().into(),
        }
    }

    /// The pool this connection's statements run on: the pool itself, or the
    /// one the transaction was begun on.
    pub(crate) fn pool(&self) -> &Arc<crate::internal::InternalConnection> {
        match self {
            Self::Database(pool) | Self::Transaction(_, pool, _) => pool,
        }
    }

    /// The record of the transaction this connection belongs to; none for a
    /// pool.
    pub(crate) fn record(&self) -> Option<&Record> {
        match self {
            Self::Database(_) => None,
            Self::Transaction(.., record) => Some(record),
        }
    }

    /// Run `note` with what it records, cache invalidations and rollback
    /// steps, going to the transaction this connection belongs to, or to none
    /// for a pool, whose statements are final even inside another database's
    /// transaction. The innermost scope's transaction may be another
    /// database's.
    pub(crate) fn recording<T>(&self, note: impl FnOnce() -> T) -> T {
        let _scope = crate::cache::replace_pending_invalidations(self.record().cloned());
        note()
    }

    /// Run the write `future`, then drop this database's cached reads, recorded in the
    /// transaction this connection belongs to: also when the future fails or
    /// is dropped part way, since the database may have applied the write by
    /// then.
    pub(crate) async fn invalidating<F: Future>(&self, future: F) -> F::Output {
        struct Clear<'a>(&'a ConnectionRef);

        impl Drop for Clear<'_> {
            fn drop(&mut self) {
                self.0.drop_cached_reads();
            }
        }

        let _clear = Clear(self);
        future.await
    }

    /// Drop this database's cached reads, recorded in the transaction this
    /// connection belongs to so its commit drops them again.
    fn drop_cached_reads(&self) {
        self.recording(|| {
            let cache = crate::cache::QueryCache::global();
            match self.cache_scope() {
                Some(database) => cache.invalidate_database(database),
                None => cache.clear(),
            }
        });
    }

    /// Whether `self` and `other` are the same pool or the same transaction,
    /// so statements on either run on one connection.
    pub(crate) fn is_same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Database(a), Self::Database(b)) => Arc::ptr_eq(a, b),
            (Self::Transaction(a, ..), Self::Transaction(b, ..)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// The backend the statements run on, MariaDB told apart from MySQL by
    /// what the server said when the connection opened.
    pub(crate) fn backend(&self) -> crate::config::DatabaseType {
        self.executor().get_database_backend()
    }

    /// The identity of the pool this connection's statements run on; a
    /// session lent its pool's has that one.
    pub(crate) fn identity(&self) -> u64 {
        match self {
            Self::Database(pool) => pool.connection().identity(),
            Self::Transaction(transaction, ..) => transaction.identity(),
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/database_transaction_tests.rs"]
mod leaked_transaction_tests;
