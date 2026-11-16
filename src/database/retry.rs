//! Running a transaction again after a failure a new attempt can get past.

use std::hash::{BuildHasher, RandomState};
use std::time::Duration;

use crate::error::{DbFailureKind, Error, Result};

use super::{ConnectionRef, Database, Transaction, TransactionOptions};

/// The wait before the second attempt; it doubles up to [`LONGEST_WAIT`].
const FIRST_WAIT: Duration = Duration::from_millis(10);
const LONGEST_WAIT: Duration = Duration::from_secs(1);

impl Database {
    /// Run `f` in a transaction begun with `options`, as
    /// [`Database::transaction_with`] does, and run it again in a new one
    /// when it fails with a deadlock, a serialization failure (`40001`, what
    /// CockroachDB and a `SERIALIZABLE` PostgreSQL transaction ask the client
    /// to retry on) or a lock it could not take, up to `attempts` runs in all.
    ///
    /// ```ignore
    /// let total = db.transaction_retrying(
    ///     TransactionOptions {
    ///         isolation_level: Some(IsolationLevel::Serializable),
    ///         ..TransactionOptions::default()
    ///     },
    ///     5,
    ///     async |_| {
    ///         let mut account = Account::find_or_fail(id).await?;
    ///         account.balance += amount;
    ///         Ok(account.update().await?.balance)
    ///     },
    /// ).await?;
    /// ```
    ///
    /// `f` may run more than once, so what it does outside the database,
    /// such as sending a message, belongs after this returns. Between
    /// attempts it waits 10 ms, doubling up to 1 s, each wait shortened by a
    /// random part so competing transactions spread out. Any other failure,
    /// and the last attempt's, is returned as it is; a lost connection is
    /// never retried, since the commit may have gone through.
    ///
    /// Inside a transaction on this database's pool it opens a savepoint and
    /// runs `f` once, leaving `options` to the enclosing transaction, which
    /// began with its own: a deadlock or a serialization failure aborts that
    /// whole transaction, so it is the one to retry.
    pub async fn transaction_retrying<F, T>(
        &self,
        options: TransactionOptions,
        attempts: u32,
        f: F,
    ) -> Result<T>
    where
        F: AsyncFn(&Transaction) -> Result<T>,
    {
        if matches!(self.__get_connection()?, ConnectionRef::Transaction(..)) {
            return self
                .transaction_with(TransactionOptions::default(), &f)
                .await;
        }
        let mut wait = FIRST_WAIT;
        let mut attempt = 1;
        loop {
            match self.transaction_with(options, &f).await {
                Err(error) if attempt < attempts && worth_retrying(&error) => {
                    crate::tide_debug!("Transaction attempt {attempt} failed, retrying: {error}");
                    crate::internal::driver::__rt::sleep(jittered(wait)).await;
                    wait = (wait * 2).min(LONGEST_WAIT);
                    attempt += 1;
                }
                outcome => return outcome,
            }
        }
    }
}

/// Whether a new attempt can get past `error`, which left nothing committed.
fn worth_retrying(error: &Error) -> bool {
    matches!(
        error.failure_kind(),
        DbFailureKind::Deadlock
            | DbFailureKind::SerializationFailure
            | DbFailureKind::LockNotAvailable
    )
}

/// `wait` shortened by a random part of up to half of it.
fn jittered(wait: Duration) -> Duration {
    let random = RandomState::new().hash_one(wait) % 1_000;
    wait.mul_f64(1.0 - random as f64 / 2_000.0)
}

#[cfg(test)]
#[path = "../../tests/unit/database_retry_tests.rs"]
mod tests;
