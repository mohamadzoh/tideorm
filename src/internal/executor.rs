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
///
/// It also marks, in the record of the transaction it runs on, the statements
/// that fail there, and refuses statements once that transaction was rolled
/// back underneath TideORM.
#[doc(hidden)]
pub struct Executor<'c> {
    inner: DatabaseExecutor<'c>,
    record: Option<Record>,
    /// Whether the rows are read as JSON, which reads every value; otherwise
    /// they are decoded into Rust types, and a value no Rust type can hold is
    /// refused (see [`ensure_typed_readable`]).
    reads_json: bool,
}

use crate::cache::Record;

/// A transaction [`Executor::begin`] began.
pub(crate) struct Begun {
    pub(crate) transaction: DatabaseTransaction,
    /// For a savepoint, the enclosing transaction's slot, to hold until the
    /// savepoint ends.
    pub(crate) slot: Option<futures_util::lock::OwnedMutexGuard<()>>,
}

impl<'c> Executor<'c> {
    /// An executor for a pool, whose statements run outside every
    /// transaction.
    pub(crate) fn new(inner: DatabaseExecutor<'c>) -> Self {
        Self {
            inner,
            record: None,
            reads_json: false,
        }
    }

    /// An executor for a transaction `record` records.
    pub(crate) fn with_record(inner: DatabaseExecutor<'c>, record: Option<Record>) -> Self {
        Self {
            inner,
            record,
            reads_json: false,
        }
    }

    /// This executor, for rows read as JSON rather than decoded into Rust types.
    pub(crate) fn reading_json(mut self) -> Self {
        self.reads_json = true;
        self
    }

    pub(crate) fn is_transaction(&self) -> bool {
        !matches!(self.inner, DatabaseExecutor::Connection(_))
    }

    /// The record of the transaction this runs on; none for a pool, whose
    /// statements run outside every transaction.
    pub(crate) fn record(&self) -> Option<&Record> {
        if !self.is_transaction() {
            return None;
        }
        self.record.as_ref()
    }

    fn ensure_transaction_active(&self) -> Result<(), DbErr> {
        if let Some(abort) = self.record().and_then(|record| record.lock().aborted()) {
            return Err(DbErr::Custom(format!(
                "{}{}",
                super::TRANSACTION_ABORTED,
                abort.statement_error()
            )));
        }
        Ok(())
    }

    /// Mark a failed statement in the transaction's record: a failure can
    /// leave it unable to commit, which it asks before it does (see
    /// `Database::transaction`).
    fn note_failure<T>(&self, result: &Result<T, DbErr>) {
        if let Err(error) = result
            && let Some(record) = self.record()
        {
            let deadlocked = super::driver_failure(error)
                .is_some_and(|failure| failure.kind() == crate::error::DbFailureKind::Deadlock);
            record.lock().note_failed_statement(deadlocked);
        }
    }

    /// Wait for the transaction's connection and hold it while the returned
    /// guard lives (see `PendingInvalidations::gate`); a pool's statements
    /// each take a connection of their own.
    async fn hold(&self) -> Option<futures_util::lock::OwnedMutexGuard<()>> {
        let gate = self.record()?.lock().gate();
        Some(gate.lock_owned().await)
    }

    /// Run a statement's `result` through the checks every statement gets.
    async fn checked<T>(&self, result: Result<T, DbErr>) -> Result<T, DbErr> {
        self.note_failure(&result);
        self.check_sqlite_failure(result).await
    }

    /// The driver does not expose SQLite's autocommit flag through SeaORM.
    /// BEGIN fails without changing an active transaction; if it succeeds,
    /// SQLite already rolled back. Keep that replacement transaction empty
    /// and blocked, so the driver's eventual rollback can balance its depth.
    async fn check_sqlite_failure<T>(&self, result: Result<T, DbErr>) -> Result<T, DbErr> {
        if result.is_err()
            && self.get_database_backend() == DbBackend::Sqlite
            && let Some(record) = self.record()
        {
            record.lock().abort(crate::cache::Abort::SqliteRollback);
            match self.inner.execute_unprepared("BEGIN").await {
                Err(error)
                    if error
                        .to_string()
                        .contains("cannot start a transaction within a transaction") =>
                {
                    record.lock().clear_sqlite_rollback();
                }
                Ok(_) => {
                    // Recreate only the empty SQLx savepoint stack. This is
                    // SQLx's ANSI savepoint naming convention, also used by
                    // SeaORM. No application statements may run on this stack.
                    let depth = record.lock().depth();
                    for depth in 1..depth {
                        if self
                            .inner
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
    ///
    /// A savepoint that fails, or whose future is dropped, while it starts
    /// leaves the driver rolling back at the enclosing transaction's depth,
    /// which ends that transaction: everything after it would autocommit. The
    /// enclosing transaction is then marked aborted, so its statements are
    /// refused and its commit fails instead.
    ///
    /// A savepoint cancelled while it waits for the connection has not
    /// started, which leaves the enclosing transaction as it was.
    ///
    /// The driver names a savepoint by how many are open on the connection,
    /// so one ending while a later one is open would end that one in its
    /// place. A savepoint therefore waits until another open in the same
    /// transaction ends, holding the transaction's slot meanwhile, which it
    /// takes before the connection so neither waits for the other.
    pub(crate) async fn begin(&self) -> Result<Begun, DbErr> {
        if !self.is_transaction() {
            return Ok(Begun {
                transaction: self.inner.begin().await?,
                slot: None,
            });
        }
        let slot = match self.record() {
            Some(record) => {
                let slot = record.lock().savepoint_slot();
                Some(slot.lock_owned().await)
            }
            None => None,
        };
        let _held = self.hold().await;
        self.ensure_transaction_active()?;
        let guard = crate::database::SavepointEnd::new(self.record().cloned());
        let savepoint = self.inner.begin().await?;
        guard.disarm();
        Ok(Begun {
            transaction: savepoint,
            slot,
        })
    }
}

/// Refuse rows a decode into Rust types would misread: a PostgreSQL date or
/// timestamp holding `infinity`, on which sqlx panics, or `TIME '24:00:00'`,
/// which it reads as midnight. JSON reads take such values as their text.
#[cfg(feature = "postgres")]
fn ensure_typed_readable(rows: &[QueryResult]) -> Result<(), DbErr> {
    use crate::database::json_rows::{binary_bytes, is_moment_type, unreadable_moment};
    use crate::internal::sqlx::{Column, Row, TypeInfo};

    let Some(first) = rows.first().and_then(QueryResult::try_as_pg_row) else {
        return Ok(());
    };
    let moments: Vec<(usize, &str, &str)> = first
        .columns()
        .iter()
        .enumerate()
        .filter_map(|(index, column)| {
            let type_name = column.type_info().name();
            is_moment_type(type_name).then(|| (index, column.name(), type_name))
        })
        .collect();
    if moments.is_empty() {
        return Ok(());
    }
    for row in rows.iter().filter_map(QueryResult::try_as_pg_row) {
        for &(index, name, type_name) in &moments {
            if binary_bytes(row, index).is_some_and(|bytes| unreadable_moment(type_name, bytes)) {
                return Err(DbErr::Type(format!(
                    "column '{name}' holds a {type_name} no Rust date or time type can hold \
                     (`infinity` or `24:00:00`); read it with get_json() or raw JSON"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(not(feature = "postgres"))]
fn ensure_typed_readable(_rows: &[QueryResult]) -> Result<(), DbErr> {
    Ok(())
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
        self.inner.get_database_backend()
    }

    async fn execute_raw(&self, statement: Statement) -> Result<ExecResult, DbErr> {
        let _held = self.hold().await;
        self.ensure_transaction_active()?;
        let result = self
            .inner
            .execute_raw(bind_zoned_timestamps_as_datetime(statement))
            .await;
        self.checked(result).await
    }

    async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
        let _held = self.hold().await;
        self.ensure_transaction_active()?;
        let result = self.inner.execute_unprepared(sql).await;
        self.checked(result).await
    }

    async fn query_one_raw(&self, statement: Statement) -> Result<Option<QueryResult>, DbErr> {
        let _held = self.hold().await;
        self.ensure_transaction_active()?;
        let result = self
            .inner
            .query_one_raw(bind_zoned_timestamps_as_datetime(statement))
            .await;
        let row = self.checked(result).await?;
        if !self.reads_json {
            ensure_typed_readable(row.as_slice())?;
        }
        Ok(row)
    }

    async fn query_all_raw(&self, statement: Statement) -> Result<Vec<QueryResult>, DbErr> {
        let _held = self.hold().await;
        self.ensure_transaction_active()?;
        let result = self
            .inner
            .query_all_raw(bind_zoned_timestamps_as_datetime(statement))
            .await;
        let rows = self.checked(result).await?;
        if !self.reads_json {
            ensure_typed_readable(&rows)?;
        }
        Ok(rows)
    }

    fn support_returning(&self) -> bool {
        self.inner.support_returning()
    }

    fn is_mock_connection(&self) -> bool {
        self.inner.is_mock_connection()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/internal_executor_tests.rs"]
mod tests;
