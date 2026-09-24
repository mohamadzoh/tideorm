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

    /// Begin a transaction, or a savepoint inside the one this runs on.
    pub(crate) async fn begin(&self) -> Result<DatabaseTransaction, DbErr> {
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
        self.0
            .execute_raw(bind_zoned_timestamps_as_datetime(statement))
            .await
    }

    async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
        self.0.execute_unprepared(sql).await
    }

    async fn query_one_raw(&self, statement: Statement) -> Result<Option<QueryResult>, DbErr> {
        self.0
            .query_one_raw(bind_zoned_timestamps_as_datetime(statement))
            .await
    }

    async fn query_all_raw(&self, statement: Statement) -> Result<Vec<QueryResult>, DbErr> {
        self.0
            .query_all_raw(bind_zoned_timestamps_as_datetime(statement))
            .await
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
