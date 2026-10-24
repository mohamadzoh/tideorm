use std::future::Future;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::internal::Backend;
use crate::tide_warn;

use super::Database;
use super::state::with_connection_override;
use crate::cache::{PendingInvalidations, Record};

/// Reported when a clone of the transaction handle outlived the closure.
const LEAKED_TRANSACTION_MESSAGE: &str = "transaction handle leaked outside the transaction scope";

/// Decide whether a leaked transaction can be ended with an explicit rollback.
///
/// `Arc::try_unwrap` failed, so `commit`/`rollback` — both of which consume the
/// transaction — are unreachable and the engine will not run its drop-time
/// rollback until the stray clone is released. Sending `ROLLBACK` over the
/// shared handle ends the server-side transaction immediately, which is what
/// releases the locks and lets the pooled connection go back once the clone
/// drops. Two conditions have to hold before that is safe:
///
/// - The transaction must be top-level. A nested one is a SAVEPOINT whose name
///   is private to the driver, so a bare `ROLLBACK` would abort the *enclosing*
///   transaction instead of this one. A savepoint also shares the enclosing
///   transaction's connection, so there is no separate connection to free.
/// - The driver must resynchronize afterwards. Postgres and MySQL decrement
///   their transaction-depth counter unconditionally when the engine later runs
///   its queued drop-time rollback. A `BEGIN` follows the `ROLLBACK`, so the
///   stray handle writes into a transaction of its own, which that drop-time
///   rollback ends, instead of committing each statement on its own.
///   SQLite only decrements when its own rollback succeeds, so a rollback issued
///   behind its back would leave the pooled connection permanently off by one
///   and turn later `BEGIN`s into savepoints that never close. SQLite (and any
///   backend TideORM does not recognize) therefore keeps the drop-time rollback.
fn can_rollback_leaked_transaction(backend: Option<Backend>, nested: bool) -> bool {
    !nested && matches!(backend, Some(Backend::Postgres | Backend::MySql))
}

/// How far ending a leaked transaction actually got.
///
/// Carried into the error the caller sees so a leak never reports a clean
/// "handle leaked" while the transaction is, in fact, still holding locks.
enum LeakedRollback {
    /// `ROLLBACK` was issued and accepted, so the server-side transaction is
    /// over and only the pooled connection waits on the stray handle.
    RolledBack,
    /// No explicit rollback was possible on this backend; the engine rolls the
    /// transaction back when the stray handle finally drops.
    DeferredToDrop,
    /// `ROLLBACK` was issued and the engine rejected it.
    Failed(Error),
}

/// End a transaction whose handle escaped its closure, as far as that is
/// possible without owning it.
///
/// Always reports the leak: it is a caller bug that otherwise only shows up as a
/// stalled connection much later.
async fn end_leaked_transaction(
    txn: &crate::internal::OrmTransaction,
    nested: bool,
) -> LeakedRollback {
    use crate::internal::ConnectionTrait;

    let backend = Backend::from_orm_backend(txn.get_database_backend());

    if !can_rollback_leaked_transaction(backend, nested) {
        tide_warn!(
            "{LEAKED_TRANSACTION_MESSAGE}. It cannot be rolled back explicitly here, so it stays \
             open until the stray handle is dropped."
        );
        return LeakedRollback::DeferredToDrop;
    }

    tide_warn!("{LEAKED_TRANSACTION_MESSAGE}. Rolling it back explicitly.");

    match txn.execute_unprepared("ROLLBACK").await {
        Ok(_) => {
            // Without a transaction open, a statement through the stray
            // handle would commit by itself; a leaked transaction never does.
            if let Err(err) = txn.execute_unprepared("BEGIN").await {
                tide_warn!(
                    "Reopening the leaked transaction failed, so statements through the stray handle commit on their own: {}",
                    transaction_error(err)
                );
            }
            LeakedRollback::RolledBack
        }
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
        LeakedRollback::DeferredToDrop => "it rolls back when the stray handle drops".to_string(),
        LeakedRollback::Failed(err) => {
            let detail = format!("rolling it back failed: {err}");
            source = err.into_db_failure();
            detail
        }
    };

    let mut message = format!("{LEAKED_TRANSACTION_MESSAGE}, so {outcome}");

    if let Some(closure_error) = closure_error {
        message.push_str(&format!(
            "; the closure had already failed: {closure_error}"
        ));
        source = source.or_else(|| closure_error.into_db_failure());
    }

    Error::Transaction { message, source }
}

/// Refuse to commit a PostgreSQL transaction that a failed statement aborted.
///
/// PostgreSQL answers the `COMMIT` of such a transaction with a rollback and
/// no error, so a closure that caught the failure and returned `Ok` would
/// report a commit that never happened. Any statement fails in an aborted
/// transaction, so one is sent first; the other backends keep the transaction
/// going after a failed statement.
async fn ensure_transaction_can_commit(txn: &crate::internal::OrmTransaction) -> Result<()> {
    use crate::internal::ConnectionTrait;

    if Backend::from_orm_backend(txn.get_database_backend()) != Some(Backend::Postgres) {
        return Ok(());
    }
    match txn.execute_unprepared("SELECT 1").await {
        Ok(_) => Ok(()),
        Err(err) => Err(Error::Transaction {
            message: format!(
                "the transaction cannot commit: a statement inside it failed, which aborted it, so it was rolled back ({})",
                err
            ),
            source: transaction_error(err).into_db_failure(),
        }),
    }
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
        F: for<'c> FnOnce(
                &'c Transaction,
            )
                -> std::pin::Pin<Box<dyn Future<Output = Result<T>> + Send + 'c>>
            + Send,
        T: Send,
    {
        let connection = self.__get_connection()?;
        let scope = TransactionScope::begin(&connection)
            .await
            .map_err(transaction_error)?;
        let tx = Transaction {
            inner: scope.txn.clone(),
        };
        let outcome = with_connection_override(scope.connection(), f(&tx)).await;
        drop(tx);
        scope.end(outcome, transaction_error).await
    }
}

/// A transaction, or a savepoint in the one a connection belongs to, from
/// its begin to its end: what `Database::transaction` and a batch insert
/// share.
///
/// Its fields drop in order: the record, which holds the savepoints opened
/// inside this one, before this savepoint, and the slot last, so one opened
/// beside it begins only once it is gone.
pub(crate) struct TransactionScope {
    /// Its savepoint, left with the enclosing transaction if the scope is
    /// dropped before it ends.
    cancelled: CancelledSavepoint,
    txn: Arc<crate::internal::OrmTransaction>,
    // Drop the driver transaction before its record: cancellation must queue
    // rollback before releasing guards retained by that record.
    /// The record its statements mark, and that commits replay.
    record: Record,
    /// The pool it runs on, for what is remembered per database, and its
    /// record.
    source: TransactionSource,
    /// A savepoint inside a caller's transaction rather than a top-level
    /// transaction, which decides how a leaked handle can be terminated.
    nested: bool,
    /// The enclosing transaction's savepoint slot; see `Executor::begin`.
    slot: Option<futures_util::lock::OwnedMutexGuard<()>>,
}

impl TransactionScope {
    /// Begin a transaction on `connection`'s pool, or a savepoint in its
    /// transaction.
    pub(crate) async fn begin(
        connection: &ConnectionRef,
    ) -> std::result::Result<Self, crate::internal::OrmError> {
        let nested = matches!(connection, ConnectionRef::Transaction(..));
        let enclosing = connection.source();
        let begun = connection.executor().begin().await?;
        let txn = Arc::new(begun.transaction);
        let record: Record = Arc::new(parking_lot::Mutex::new(PendingInvalidations::new(
            enclosing.record.clone(),
        )));
        Ok(Self {
            source: TransactionSource {
                origin: enclosing.origin,
                cache_scope: enclosing.cache_scope,
                mariadb: enclosing.mariadb,
                record: Some(record.clone()),
            },
            record,
            cancelled: CancelledSavepoint {
                savepoint: nested.then(|| txn.clone()),
                enclosing: enclosing.record,
            },
            txn,
            nested,
            slot: begun.slot,
        })
    }

    /// The connection the scope's statements run on.
    pub(crate) fn connection(&self) -> ConnectionRef {
        ConnectionRef::Transaction(self.txn.clone(), self.source.clone())
    }

    /// The record the scope's statements mark.
    pub(crate) fn record(&self) -> &Record {
        &self.record
    }

    /// Commit the transaction when `outcome` is a success, else roll it back,
    /// holding its connection, so nothing runs there meanwhile, and rolling
    /// back first the savepoints cancelled while it was held. A commit failure
    /// is reported through `commit_error`.
    pub(crate) async fn end<T>(
        self,
        outcome: Result<T>,
        commit_error: impl FnOnce(crate::internal::OrmError) -> Error,
    ) -> Result<T> {
        let Self {
            record,
            mut cancelled,
            txn,
            nested,
            slot,
            ..
        } = self;
        let gate = record.lock().gate();
        let held = gate.lock_owned().await;
        let ending = cancelled.ending();
        drop(record.lock().take_deferred());

        let mut leaked = false;
        let result = async {
            match (Arc::try_unwrap(txn), outcome) {
                (Ok(txn), Ok(result)) => {
                    let (statement_failed, aborted) = {
                        let record = record.lock();
                        (record.statement_failed(), record.aborted())
                    };
                    if let Some(abort) = aborted {
                        let _ = txn.rollback().await;
                        return Err(Error::transaction(abort.commit_error()));
                    }
                    if statement_failed {
                        ensure_transaction_can_commit(&txn).await?;
                    }
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
                    // The failure is the one the caller needs; a rollback
                    // failure on top of it is reported, not swallowed.
                    if let Err(rollback) = txn.rollback().await {
                        tide_warn!(
                            "Rolling back the failed transaction also failed: {}",
                            transaction_error(rollback)
                        );
                    }
                    Err(e)
                }
                (Err(txn), outcome) => {
                    leaked = true;
                    let rollback = end_leaked_transaction(txn.as_ref(), nested).await;
                    Err(leaked_transaction_error(rollback, outcome.err()))
                }
            }
        }
        .await;
        if leaked {
            ending.leaked();
        } else {
            ending.disarm();
        }
        drop(held);
        drop(slot);
        result
    }
}

/// A savepoint, until it ends: a nested transaction's, or a batch's. Dropped
/// with it while something else holds the connection, the driver could not
/// roll it back (and panics trying), so the enclosing transaction holds it
/// until the connection is free and refuses to commit, since what it would
/// run next would run inside the savepoint.
struct CancelledSavepoint {
    /// `None` for a transaction of its own, which no other future shares.
    savepoint: Option<Arc<crate::internal::OrmTransaction>>,
    enclosing: Option<Record>,
}

impl CancelledSavepoint {
    /// Let the savepoint end, once its connection is held: it is no longer
    /// left behind if cancelled, but a cancellation while it commits or rolls
    /// back leaves it in a state the driver no longer tracks, which the
    /// returned guard marks in the enclosing transaction until disarmed.
    fn ending(&mut self) -> SavepointEnd {
        SavepointEnd(self.savepoint.take().and(self.enclosing.take()))
    }
}

impl Drop for CancelledSavepoint {
    fn drop(&mut self) {
        if let (Some(savepoint), Some(enclosing)) = (self.savepoint.take(), &self.enclosing) {
            PendingInvalidations::defer_or_drop(enclosing, vec![Box::new(savepoint)]);
        }
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

/// Marks the enclosing transaction aborted when dropped armed: when a
/// savepoint fails or is cancelled while it starts, commits or rolls back,
/// which leaves it in a state the driver no longer tracks.
pub(crate) struct SavepointEnd(Option<Record>);

impl SavepointEnd {
    /// Guard a savepoint of the transaction `enclosing` records.
    pub(crate) fn new(enclosing: Option<Record>) -> Self {
        Self(enclosing)
    }

    pub(crate) fn disarm(mut self) {
        self.0 = None;
    }

    /// The savepoint's handle leaked, which leaves it open: the enclosing
    /// transaction cannot go on.
    fn leaked(mut self) {
        if let Some(enclosing) = self.0.take() {
            enclosing.lock().abort(crate::cache::Abort::Leaked);
        }
    }
}

impl Drop for SavepointEnd {
    fn drop(&mut self) {
        if let Some(enclosing) = self.0.take() {
            enclosing.lock().abort(crate::cache::Abort::Savepoint);
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
    pub(super) inner: Arc<crate::internal::OrmTransaction>,
}

impl Transaction {
    /// Get a reference to the underlying connection.
    pub fn connection(&self) -> &crate::internal::OrmTransaction {
        self.inner.as_ref()
    }
}

/// The connection a statement runs on: the pool, or the transaction an
/// enclosing [`Database::transaction`] scope installed.
#[doc(hidden)]
#[derive(Clone)]
pub enum ConnectionRef {
    Database(Arc<crate::internal::InternalConnection>),
    Transaction(Arc<crate::internal::OrmTransaction>, TransactionSource),
}

/// What a transaction's handle cannot tell: the identity of the pool it was
/// begun on, whether that pool reaches MariaDB, which the driver reports as
/// MySQL, and the record its statements mark.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct TransactionSource {
    pub(crate) origin: Option<u64>,
    pub(crate) cache_scope: super::cache_scope::CacheScope,
    pub(crate) mariadb: bool,
    pub(crate) record: Option<Record>,
}

impl ConnectionRef {
    /// Borrow this connection as one executor, so a statement is written once
    /// whichever of the two it runs on.
    pub fn executor(&self) -> crate::internal::Executor<'_> {
        match self {
            Self::Database(conn) => crate::internal::Executor::new(conn.connection().into()),
            Self::Transaction(tx, source) => {
                crate::internal::Executor::with_record(tx.as_ref().into(), source.record.clone())
            }
        }
    }

    /// Run `note` with what it records, cache invalidations and rollback
    /// steps, going to the transaction this connection belongs to, or to none
    /// for a pool, whose statements are final even inside another database's
    /// transaction. The innermost scope's transaction may be another
    /// database's.
    pub(crate) fn recording<T>(&self, note: impl FnOnce() -> T) -> T {
        let record = match self {
            Self::Database(_) => None,
            Self::Transaction(_, source) => source.record.clone(),
        };
        let _scope = crate::cache::replace_pending_invalidations(record);
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
                self.0.recording(|| {
                    let cache = crate::cache::QueryCache::global();
                    match self.0.cache_scope() {
                        Some(database) => cache.invalidate_database(database),
                        None => cache.clear(),
                    }
                });
            }
        }

        let _clear = Clear(self);
        future.await
    }

    /// Whether `self` and `other` are the same pool or the same transaction,
    /// so statements on either run on one connection.
    pub(crate) fn is_same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Database(a), Self::Database(b)) => Arc::ptr_eq(a, b),
            (Self::Transaction(a, _), Self::Transaction(b, _)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    pub(crate) fn backend(&self) -> Backend {
        use crate::internal::ConnectionTrait;

        Backend::from(self.executor().get_database_backend())
    }

    /// The identity of the pool this connection's statements run on, read
    /// without building the whole [`source`](Self::source).
    pub(crate) fn origin(&self) -> Option<u64> {
        match self {
            Self::Database(pool) => Some(super::state::connection_identity(pool)),
            Self::Transaction(_, source) => source.origin,
        }
    }

    /// The pool this connection's statements run on: the pool itself, or the
    /// one the transaction was begun on.
    pub(crate) fn source(&self) -> TransactionSource {
        match self {
            Self::Database(pool) => TransactionSource {
                origin: self.origin(),
                cache_scope: pool.cache_scope.clone(),
                mariadb: pool.is_mariadb(),
                record: None,
            },
            Self::Transaction(_, source) => source.clone(),
        }
    }

    /// The backend the statements run on, MariaDB told apart from MySQL by
    /// what the pool found when it opened.
    pub(crate) fn database_type(&self) -> crate::config::DatabaseType {
        let mariadb = match self {
            Self::Database(pool) => pool.is_mariadb(),
            Self::Transaction(_, source) => source.mariadb,
        };
        match self.backend() {
            Backend::MySql if mariadb => crate::config::DatabaseType::MariaDB,
            backend => backend.as_database_type(),
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/database_transaction_tests.rs"]
mod leaked_transaction_tests;
