use crate::DbValue;
use crate::error::Result;
use crate::internal::translate_error;
use crate::logging::EngineLog;

use super::Database;
use super::json_rows::ColumnTypeLookup;

// ── Raw SQL: ambient vs. instance ──────────────────────────────────────────
//
// The raw entry points come in two shapes, and the difference is deliberate:
//
// - `Database::raw`, `Database::execute` and `Database::raw_json` (with their
//   `_with_params` forms) are *associated functions*, not methods. They take no
//   receiver and always run against the ambient connection: the transaction
//   installed by an enclosing `Database::transaction` scope, otherwise the
//   global connection. That is why `replica.raw(..)` does not compile — there is
//   no receiver to honor.
// - `query_raw`, `exec_raw` and `query_raw_json` (with their `_with_params`
//   forms) are the instance equivalents, for code that holds a specific handle.
//   Like every other *executing* method on `Database`, an enclosing transaction
//   scope still wins over the handle stored in `self`, so a statement issued
//   inside `Database::transaction` joins that transaction rather than opening a
//   second, independent one on `self`'s pool. Metadata is the exception:
//   `ping` — like `backend` — answers for the handle it was called on, because a
//   question about a specific connection cannot be answered by another one.
//
// The associated functions are thin wrappers over the instance methods, so the
// two shapes cannot drift apart.
impl Database {
    /// Check whether this database connection is responsive.
    ///
    /// A health check is about the handle it was called on, so it reaches that
    /// handle's own connection even inside a transaction scope opened on
    /// another one — a replica that reported on the primary would answer the
    /// wrong question. Any failure is a connection failure by construction.
    pub async fn ping(&self) -> Result<()> {
        self.current_inner()?
            .conn
            .ping()
            .await
            .map_err(crate::internal::translate_connection_error)
    }

    /// How many statements the driver keeps prepared on one of this handle's
    /// connections: the one its pool hands out next, or a pinned session's own.
    ///
    /// Each connection prepares a parameterized statement once and reuses it
    /// while it lives, keeping up to
    /// [`statement_cache_capacity`](super::DatabaseBuilder::statement_cache_capacity)
    /// statements (100 by default) and dropping the least recently used. Like [`ping`](Self::ping), this answers for the handle it
    /// was called on, even inside a transaction scope.
    pub async fn cached_statements(&self) -> Result<usize> {
        self.current_inner()?
            .conn
            .cached_statements()
            .await
            .map_err(crate::internal::translate_connection_error)
    }

    /// Execute a raw SQL query on the ambient connection and return all results
    ///
    /// This is an associated function, not a method: it always runs against the
    /// enclosing transaction scope, or the global connection when there is
    /// none. Use [`Database::query_raw`] to run against a specific handle.
    ///
    /// Raw SQL is opaque to TideORM. Every public raw call flushes the query
    /// cache: even SELECT can write through functions or views. ORM-generated
    /// reads retain their normal targeted cache behavior.
    ///
    /// Rows are read as models `T`; [`Database::raw_as`] reads them into a
    /// struct deriving [`FromQueryResult`](crate::FromQueryResult), and
    /// [`Database::raw_json`] as JSON.
    pub async fn raw<T: crate::model::Model>(sql: &str) -> Result<Vec<T>> {
        crate::database::__current_db()?.query_raw::<T>(sql).await
    }

    /// Execute a raw SQL query on this handle and return all results
    ///
    /// The instance form of [`Database::raw`]. An enclosing
    /// [`Database::transaction`] scope still takes precedence over `self`, so
    /// the statement joins that transaction instead of opening a second one.
    pub async fn query_raw<T: crate::model::Model>(&self, sql: &str) -> Result<Vec<T>> {
        self.query_raw_with_params(sql, Vec::new()).await
    }

    /// Execute a raw SQL query with parameters on the ambient connection
    ///
    /// This is an associated function, not a method — see [`Database::raw`].
    /// Use [`Database::query_raw_with_params`] to run against a specific handle.
    ///
    /// Parameters are [`DbValue`](crate::DbValue)s — `tideorm::DbValue`, also in
    /// the prelude. Most are built with `.into()` from the corresponding Rust
    /// type, so the name is only needed for annotations and explicit `NULL`
    /// bindings:
    ///
    /// ```ignore
    /// use tideorm::prelude::*;
    ///
    /// let params: Vec<DbValue> = vec![true.into(), "alice".into()];
    /// let sql = "SELECT * FROM users WHERE active = $1 AND name = $2";
    /// let users: Vec<User> = Database::raw_with_params(sql, params).await?;
    /// ```
    ///
    /// `params` is any iterator of values: a `Vec`, an array
    /// (`[true.into(), "alice".into()]`) or an iterator chain.
    pub async fn raw_with_params<T: crate::model::Model>(
        sql: &str,
        params: impl IntoIterator<Item = DbValue>,
    ) -> Result<Vec<T>> {
        crate::database::__current_db()?
            .query_raw_with_params::<T>(sql, params)
            .await
    }

    /// Execute a raw SQL query with parameters on this handle
    ///
    /// The instance form of [`Database::raw_with_params`].
    pub async fn query_raw_with_params<T: crate::model::Model>(
        &self,
        sql: &str,
        params: impl IntoIterator<Item = DbValue>,
    ) -> Result<Vec<T>> {
        crate::rules::ensure_raw_sql_allowed(
            "Database::raw()",
            Some((std::any::type_name::<T>(), T::__rules())),
        )?;
        let params = params.into_iter().collect();
        self.invalidating(self.raw_models::<T>(sql, params, EngineLog::On))
            .await
    }

    /// Run a statement TideORM rendered itself and decode it into models,
    /// logged by the engine as `log` says.
    ///
    /// The cache is deliberately left alone here: every caller is a builder or
    /// relation helper that knows exactly which tables it touched and performs
    /// the targeted invalidation itself. A blanket flush would throw away
    /// entries that are still valid — including, for reads, the entries the
    /// caller is about to consult again.
    pub(crate) async fn raw_models<T: crate::model::Model>(
        &self,
        sql: &str,
        params: Vec<DbValue>,
        log: EngineLog,
    ) -> Result<Vec<T>> {
        let connection = self.__get_connection()?;
        let rows = Self::query_rows(&connection, sql, params, log).await?;
        // Consumed, so each raw row is freed once it is decoded.
        let rows = rows
            .into_iter()
            .map(|row| T::from_query_result(&row).map_err(translate_error))
            .collect::<Result<Vec<_>>>()?;
        // The rows are remembered as read from the pool this handle runs on,
        // which a `query_with()` handle need not share with the scope.
        Ok(crate::internal::models_read_through(&connection, rows))
    }

    /// Run a row-returning statement on this handle's connection.
    pub(crate) async fn fetch_rows(
        &self,
        sql: &str,
        params: Vec<DbValue>,
    ) -> Result<Vec<crate::internal::QueryResult>> {
        self.fetch_rows_logged(sql, params, EngineLog::On).await
    }

    /// [`fetch_rows`](Self::fetch_rows), logged by the engine as `log` says.
    pub(crate) async fn fetch_rows_logged(
        &self,
        sql: &str,
        params: Vec<DbValue>,
        log: EngineLog,
    ) -> Result<Vec<crate::internal::QueryResult>> {
        Self::query_rows(&self.__get_connection()?, sql, params, log).await
    }

    /// Run a row-returning statement on `connection`.
    async fn query_rows(
        connection: &super::ConnectionRef,
        sql: &str,
        params: Vec<DbValue>,
        log: EngineLog,
    ) -> Result<Vec<crate::internal::QueryResult>> {
        use crate::internal::{ConnectionTrait, build_statement_with_values};

        let executor = connection.executor();
        let statement = log.apply(build_statement_with_values(
            executor.get_database_backend(),
            sql,
            params,
        ));
        executor
            .query_all_raw(statement)
            .await
            .map_err(translate_error)
    }

    /// Execute a raw SQL statement (INSERT, UPDATE, DELETE) on the ambient
    /// connection and return rows affected
    ///
    /// This is an associated function, not a method — see [`Database::raw`].
    /// Use [`Database::exec_raw`] to run against a specific handle.
    ///
    /// Every public raw statement flushes the query cache: even a SELECT may
    /// write through a function, view, operator or cast.
    pub async fn execute(sql: &str) -> Result<u64> {
        crate::database::__current_db()?.exec_raw(sql).await
    }

    /// Execute a raw SQL statement on this handle and return rows affected
    ///
    /// The instance form of [`Database::execute`]. The statement is sent
    /// unprepared, so it may contain several `;`-separated statements.
    pub async fn exec_raw(&self, sql: &str) -> Result<u64> {
        crate::rules::ensure_raw_sql_allowed("Database::execute()", None)?;
        self.exec_unprepared(sql).await
    }

    /// [`exec_raw`](Self::exec_raw) for TideORM's own statements, which the
    /// project rules on raw SQL do not hold: a migration's DDL.
    pub(crate) async fn exec_unprepared(&self, sql: &str) -> Result<u64> {
        use crate::internal::ConnectionTrait;

        let connection = self.__get_connection()?;
        let executor = connection.executor();
        let result = connection
            .invalidating(executor.execute_unprepared(sql))
            .await;

        Ok(result.map_err(translate_error)?.rows_affected())
    }

    /// Execute a raw SQL statement with parameters on the ambient connection
    ///
    /// This is an associated function, not a method — see [`Database::raw`].
    /// Use [`Database::exec_raw_with_params`] to run against a specific handle.
    ///
    /// Parameters are [`DbValue`](crate::DbValue)s — see
    /// [`Database::raw_with_params`].
    pub async fn execute_with_params(
        sql: &str,
        params: impl IntoIterator<Item = DbValue>,
    ) -> Result<u64> {
        crate::database::__current_db()?
            .exec_raw_with_params(sql, params)
            .await
    }

    /// Execute a raw SQL statement with parameters on this handle
    ///
    /// The instance form of [`Database::execute_with_params`].
    pub async fn exec_raw_with_params(
        &self,
        sql: &str,
        params: impl IntoIterator<Item = DbValue>,
    ) -> Result<u64> {
        crate::rules::ensure_raw_sql_allowed("Database::execute_with_params()", None)?;
        let params = params.into_iter().collect();
        self.invalidating(self.__execute_with_params(sql, params))
            .await
    }

    /// Run a statement TideORM rendered itself and report rows affected.
    ///
    /// Like `raw_models`, this leaves the cache to the caller: builder
    /// mutations, relation helpers and the migration and seed ledgers all know
    /// their own table.
    #[doc(hidden)]
    pub async fn __execute_with_params(&self, sql: &str, params: Vec<DbValue>) -> Result<u64> {
        self.execute_logged(sql, params, EngineLog::On).await
    }

    /// [`__execute_with_params`](Self::__execute_with_params), logged by the
    /// engine as `log` says.
    pub(crate) async fn execute_logged(
        &self,
        sql: &str,
        params: Vec<DbValue>,
        log: EngineLog,
    ) -> Result<u64> {
        use crate::internal::{ConnectionTrait, build_statement_with_values};

        let connection = self.__get_connection()?;
        let executor = connection.executor();
        let statement = log.apply(build_statement_with_values(
            executor.get_database_backend(),
            sql,
            params,
        ));
        let result = executor
            .execute_raw(statement)
            .await
            .map_err(translate_error)?;

        Ok(result.rows_affected())
    }

    /// Execute a raw SQL query on the ambient connection and return results as
    /// JSON
    ///
    /// This is an associated function, not a method — see [`Database::raw`].
    /// Use [`Database::query_raw_json`] to run against a specific handle.
    pub async fn raw_json(sql: &str) -> Result<Vec<serde_json::Value>> {
        crate::database::__current_db()?.query_raw_json(sql).await
    }

    /// Execute a raw SQL query on this handle and return results as JSON
    ///
    /// The instance form of [`Database::raw_json`].
    pub async fn query_raw_json(&self, sql: &str) -> Result<Vec<serde_json::Value>> {
        self.query_raw_json_with_params(sql, Vec::new()).await
    }

    /// Execute a raw SQL query with parameters on the ambient connection and
    /// return results as JSON
    ///
    /// This is an associated function, not a method — see [`Database::raw`].
    /// Use [`Database::query_raw_json_with_params`] to run against a specific
    /// handle.
    ///
    /// Parameters are [`DbValue`](crate::DbValue)s — see
    /// [`Database::raw_with_params`].
    pub async fn raw_json_with_params(
        sql: &str,
        params: impl IntoIterator<Item = DbValue>,
    ) -> Result<Vec<serde_json::Value>> {
        crate::database::__current_db()?
            .query_raw_json_with_params(sql, params)
            .await
    }

    /// Execute a raw SQL query with parameters on this handle and return
    /// results as JSON
    ///
    /// The instance form of [`Database::raw_json_with_params`].
    pub async fn query_raw_json_with_params(
        &self,
        sql: &str,
        params: impl IntoIterator<Item = DbValue>,
    ) -> Result<Vec<serde_json::Value>> {
        crate::rules::ensure_raw_sql_allowed("Database::raw_json()", None)?;
        let params = params.into_iter().collect();
        self.invalidating(self.__raw_json_with_params(sql, params))
            .await
    }

    /// Execute a raw SQL query on the ambient connection and read each row as
    /// `T`
    ///
    /// For rows that are not a model's: a projection, a report, a join. `T`
    /// derives [`FromQueryResult`](crate::FromQueryResult), which reads each
    /// field from the column of its name as that field's type, the way a
    /// model reads its columns; an `Option` field takes SQL `NULL`. Use
    /// [`Database::query_raw_as`] to run against a specific handle.
    ///
    /// ```ignore
    /// #[derive(FromQueryResult)]
    /// struct Totals { country: String, users: i64 }
    ///
    /// let totals: Vec<Totals> =
    ///     Database::raw_as("SELECT country, COUNT(*) AS users FROM users GROUP BY country").await?;
    /// ```
    pub async fn raw_as<T: crate::orm::FromQueryResult>(sql: &str) -> Result<Vec<T>> {
        crate::database::__current_db()?
            .query_raw_as::<T>(sql)
            .await
    }

    /// Execute a raw SQL query on this handle and read each row as `T`
    ///
    /// The instance form of [`Database::raw_as`].
    pub async fn query_raw_as<T: crate::orm::FromQueryResult>(&self, sql: &str) -> Result<Vec<T>> {
        self.query_raw_as_with_params::<T>(sql, Vec::new()).await
    }

    /// Execute a raw SQL query with parameters on the ambient connection and
    /// read each row as `T`
    ///
    /// Parameters are [`DbValue`](crate::DbValue)s, as for
    /// [`Database::raw_with_params`]. Use
    /// [`Database::query_raw_as_with_params`] to run against a specific handle.
    pub async fn raw_as_with_params<T: crate::orm::FromQueryResult>(
        sql: &str,
        params: impl IntoIterator<Item = DbValue>,
    ) -> Result<Vec<T>> {
        crate::database::__current_db()?
            .query_raw_as_with_params::<T>(sql, params)
            .await
    }

    /// Execute a raw SQL query with parameters on this handle and read each
    /// row as `T`
    ///
    /// The instance form of [`Database::raw_as_with_params`].
    pub async fn query_raw_as_with_params<T: crate::orm::FromQueryResult>(
        &self,
        sql: &str,
        params: impl IntoIterator<Item = DbValue>,
    ) -> Result<Vec<T>> {
        crate::rules::ensure_raw_sql_allowed("Database::raw_as()", None)?;
        let params = params.into_iter().collect();
        self.invalidating(self.fetch_rows(sql, params))
            .await?
            .iter()
            .map(|row| {
                T::from_query_result(row).map_err(|error| {
                    crate::Error::conversion(format!(
                        "raw_as() could not read a row as {}: {}",
                        std::any::type_name::<T>(),
                        translate_error(error)
                    ))
                })
            })
            .collect()
    }

    /// Run a statement TideORM rendered itself and return the rows as JSON.
    ///
    /// Like `raw_models`, cache invalidation belongs to the caller.
    #[doc(hidden)]
    pub async fn __raw_json_with_params(
        &self,
        sql: &str,
        params: Vec<DbValue>,
    ) -> Result<Vec<serde_json::Value>> {
        self.__raw_json_typed(sql, params, &|_| None, EngineLog::On)
            .await
    }

    /// Run a statement a model query rendered and return the rows as JSON,
    /// decoding each column `model_type` knows as that model column, logged
    /// by the engine as `log` says.
    pub(crate) async fn __raw_json_typed(
        &self,
        sql: &str,
        params: Vec<DbValue>,
        model_type: ColumnTypeLookup<'_>,
        log: EngineLog,
    ) -> Result<Vec<serde_json::Value>> {
        let rows = self.fetch_rows_logged(sql, params, log).await?;
        let schema = self.sqlite_schema(&rows, sql, model_type).await?;
        Self::query_rows_to_json(&rows, model_type, &schema)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/database_raw_tests.rs"]
mod raw_sql_tests;
