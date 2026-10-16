//! The executor every statement runs on.

use async_trait::async_trait;

use crate::orm::{
    ConnectionTrait, DatabaseExecutor, DatabaseTransaction, DbBackend, DbErr, ExecResult,
    QueryResult, Statement, TransactionTrait, Value,
};

/// The engine's executor, except that on MySQL and MariaDB a `DateTime<Utc>` or
/// `DateTime<Local>` is bound as the `DATETIME` TideORM stores it in.
///
/// The driver sends both as `TIMESTAMP` parameters holding the UTC time, and
/// MariaDB turns a `TIMESTAMP` outside that type's range (1970 to 2038, or to
/// 2106 from 11.5) into NULL before it reaches the column, so a birth date or a
/// far expiry could be neither written nor matched. Bound as `DATETIME`, the
/// same UTC time is stored as given, as MySQL already stored it.
#[doc(hidden)]
pub struct Executor<'c>(DatabaseExecutor<'c>);

impl<'c> Executor<'c> {
    pub(crate) fn new(inner: DatabaseExecutor<'c>) -> Self {
        Self(inner)
    }

    pub(crate) fn is_transaction(&self) -> bool {
        !matches!(self.0, DatabaseExecutor::Connection(_))
    }

    fn ensure_transaction_active(&self) -> Result<(), DbErr> {
        if self.is_transaction()
            && (crate::cache::transaction_deadlocked()
                || crate::cache::transaction_sqlite_aborted())
        {
            return Err(DbErr::Custom(
                "transaction was aborted; further statements are blocked".into(),
            ));
        }
        Ok(())
    }

    /// The driver does not expose SQLite's autocommit flag through SeaORM.
    /// BEGIN fails without changing an active transaction; if it succeeds,
    /// SQLite already rolled back. Keep that replacement transaction empty
    /// and blocked, so the driver's eventual rollback can balance its depth.
    async fn check_sqlite_failure<T>(&self, result: Result<T, DbErr>) -> Result<T, DbErr> {
        if result.is_err()
            && self.is_transaction()
            && self.get_database_backend() == DbBackend::Sqlite
        {
            crate::cache::note_sqlite_aborted(true);
            match self.0.execute_unprepared("BEGIN").await {
                Err(error)
                    if error
                        .to_string()
                        .contains("cannot start a transaction within a transaction") =>
                {
                    crate::cache::note_sqlite_aborted(false);
                }
                Ok(_) => {
                    // Recreate only the empty SQLx savepoint stack. This is
                    // SQLx's ANSI savepoint naming convention, also used by
                    // SeaORM. No application statements may run on this stack.
                    for depth in 1..crate::cache::transaction_depth() {
                        if self
                            .0
                            .execute_unprepared(&format!("SAVEPOINT _sqlx_savepoint_{depth}"))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                Err(_) => {} // Unknown connection state must remain blocked.
            }
        }
        result
    }

    /// Begin a transaction, or a savepoint inside the one this runs on.
    pub(crate) async fn begin(&self) -> Result<DatabaseTransaction, DbErr> {
        self.ensure_transaction_active()?;
        self.0.begin().await
    }
}

/// `statement`, with each zoned timestamp bound as its UTC `DATETIME` when the
/// statement goes to MySQL or MariaDB.
fn bind_zoned_timestamps_as_datetime(mut statement: Statement) -> Statement {
    if statement.db_backend == DbBackend::MySql
        && let Some(values) = statement.values.as_mut()
    {
        for value in &mut values.0 {
            let utc = match value {
                Value::ChronoDateTimeUtc(at) => at.map(|at| at.naive_utc()),
                Value::ChronoDateTimeLocal(at) => at.map(|at| at.naive_utc()),
                _ => continue,
            };
            *value = Value::ChronoDateTime(utc);
        }
    }
    statement
}

#[async_trait]
impl ConnectionTrait for Executor<'_> {
    fn get_database_backend(&self) -> DbBackend {
        self.0.get_database_backend()
    }

    async fn execute_raw(&self, statement: Statement) -> Result<ExecResult, DbErr> {
        self.ensure_transaction_active()?;
        let result = self
            .0
            .execute_raw(bind_zoned_timestamps_as_datetime(statement))
            .await;
        self.check_sqlite_failure(result).await
    }

    async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
        self.ensure_transaction_active()?;
        let result = self.0.execute_unprepared(sql).await;
        self.check_sqlite_failure(result).await
    }

    async fn query_one_raw(&self, statement: Statement) -> Result<Option<QueryResult>, DbErr> {
        self.ensure_transaction_active()?;
        let result = self
            .0
            .query_one_raw(bind_zoned_timestamps_as_datetime(statement))
            .await;
        self.check_sqlite_failure(result).await
    }

    async fn query_all_raw(&self, statement: Statement) -> Result<Vec<QueryResult>, DbErr> {
        self.ensure_transaction_active()?;
        let result = self
            .0
            .query_all_raw(bind_zoned_timestamps_as_datetime(statement))
            .await;
        self.check_sqlite_failure(result).await
    }

    fn support_returning(&self) -> bool {
        self.0.support_returning()
    }

    fn is_mock_connection(&self) -> bool {
        self.0.is_mock_connection()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/internal_executor_tests.rs"]
mod tests;
