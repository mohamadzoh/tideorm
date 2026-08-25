use std::future::Future;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::internal::Backend;
use crate::tide_warn;

use super::state::with_connection_override;
use super::{Database, DatabaseHandle};

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
///   its queued drop-time rollback, so the extra rollback is a harmless no-op.
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
        Ok(_) => LeakedRollback::RolledBack,
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

/// Classify a transaction-control failure.
///
/// The engine error is translated first so that connection-level failures (a
/// closed connection, an exhausted pool) keep their own variant instead of
/// being flattened into `Error::Transaction`; everything else stays a
/// transaction error, carrying the backend message. Reclassifying moves the
/// error onto a different variant, so the structured driver failure is carried
/// over explicitly — otherwise a serialization failure would arrive with no
/// SQLSTATE and no source chain.
fn transaction_error(err: crate::internal::OrmError) -> Error {
    let message = err.to_string();
    match crate::internal::translate_error(err) {
        connection @ Error::Connection { .. } => connection,
        other => Error::Transaction {
            message,
            source: other.into_db_failure(),
        },
    }
}

impl Database {
    /// Execute a closure within a database transaction
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
        use crate::internal::TransactionTrait;

        // Whether this is a SAVEPOINT inside a caller's transaction rather than
        // a top-level one, which decides how a leaked handle can be terminated.
        let (txn, nested) = match self.__get_connection()? {
            ConnectionRef::Database(conn) => (
                conn.connection().begin().await.map_err(transaction_error)?,
                false,
            ),
            ConnectionRef::Transaction(tx) => {
                (tx.as_ref().begin().await.map_err(transaction_error)?, true)
            }
        };

        let outcome = {
            let txn = Arc::new(txn);
            let tx = Transaction { inner: txn.clone() };
            let override_handle = DatabaseHandle::Transaction(txn.clone());
            let outcome = with_connection_override(override_handle, f(&tx)).await;

            (txn, outcome)
        };

        let (txn, outcome) = outcome;

        match (Arc::try_unwrap(txn), outcome) {
            (Ok(txn), Ok(result)) => {
                txn.commit().await.map_err(transaction_error)?;
                Ok(result)
            }
            (Ok(txn), Err(e)) => {
                let _ = txn.rollback().await;
                Err(e)
            }
            (Err(txn), outcome) => {
                let rollback = end_leaked_transaction(txn.as_ref(), nested).await;
                Err(leaked_transaction_error(rollback, outcome.err()))
            }
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

    /// Get the raw internal transaction (for internal use only)
    #[doc(hidden)]
    pub fn __internal_transaction(&self) -> &crate::internal::OrmTransaction {
        self.inner.as_ref()
    }
}

/// Trait for types that can be used as a database connection
pub trait Connection: Send + Sync {
    /// Get the internal connection for query execution
    #[doc(hidden)]
    fn __get_connection(&self) -> Result<ConnectionRef>;
}

/// Internal connection reference (hidden from users)
#[doc(hidden)]
pub enum ConnectionRef {
    Database(Arc<crate::internal::InternalConnection>),
    Transaction(Arc<crate::internal::OrmTransaction>),
}

impl Connection for Database {
    fn __get_connection(&self) -> Result<ConnectionRef> {
        Ok(match self.current_handle()? {
            DatabaseHandle::Connection(inner) => ConnectionRef::Database(inner),
            DatabaseHandle::Transaction(tx) => ConnectionRef::Transaction(tx),
            #[cfg(test)]
            DatabaseHandle::TestScope => {
                unreachable!("test scope marker does not carry a connection reference")
            }
        })
    }
}

impl Connection for Transaction {
    fn __get_connection(&self) -> Result<ConnectionRef> {
        Ok(ConnectionRef::Transaction(self.inner.clone()))
    }
}

#[cfg(test)]
mod leaked_transaction_tests {
    use super::{Backend, can_rollback_leaked_transaction};
    use super::{Error, LEAKED_TRANSACTION_MESSAGE, LeakedRollback, leaked_transaction_error};

    #[test]
    fn top_level_leaks_are_rolled_back_where_the_driver_resynchronizes() {
        assert!(can_rollback_leaked_transaction(
            Some(Backend::Postgres),
            false
        ));
        assert!(can_rollback_leaked_transaction(Some(Backend::MySql), false));
    }

    #[test]
    fn sqlite_and_unknown_backends_keep_the_drop_time_rollback() {
        assert!(!can_rollback_leaked_transaction(
            Some(Backend::Sqlite),
            false
        ));
        assert!(!can_rollback_leaked_transaction(None, false));
    }

    #[test]
    fn a_leaked_savepoint_never_issues_a_bare_rollback() {
        for backend in [
            Some(Backend::Postgres),
            Some(Backend::MySql),
            Some(Backend::Sqlite),
            None,
        ] {
            assert!(
                !can_rollback_leaked_transaction(backend, true),
                "a bare ROLLBACK would abort the enclosing transaction: {backend:?}"
            );
        }
    }

    #[test]
    fn the_rollback_outcome_is_part_of_the_reported_leak() {
        let rolled_back = leaked_transaction_error(LeakedRollback::RolledBack, None).to_string();
        assert!(
            rolled_back.contains(LEAKED_TRANSACTION_MESSAGE),
            "{rolled_back}"
        );
        assert!(
            rolled_back.contains("rolled back explicitly"),
            "{rolled_back}"
        );

        let deferred = leaked_transaction_error(LeakedRollback::DeferredToDrop, None).to_string();
        assert!(deferred.contains(LEAKED_TRANSACTION_MESSAGE), "{deferred}");
        assert!(
            deferred.contains("when the stray handle drops"),
            "{deferred}"
        );
    }

    #[test]
    fn a_rejected_rollback_is_never_swallowed() {
        let rollback = LeakedRollback::Failed(Error::connection("connection closed"));
        let message = leaked_transaction_error(rollback, None).to_string();

        assert!(message.contains(LEAKED_TRANSACTION_MESSAGE), "{message}");
        assert!(message.contains("rolling it back failed"), "{message}");
        assert!(message.contains("connection closed"), "{message}");
    }

    #[test]
    fn the_closure_error_survives_the_leak_report() {
        let closure_error = Some(Error::validation("email", "is required"));
        let message = leaked_transaction_error(LeakedRollback::RolledBack, closure_error);
        let message = message.to_string();

        assert!(message.contains(LEAKED_TRANSACTION_MESSAGE), "{message}");
        assert!(message.contains("email"), "{message}");
    }
}
