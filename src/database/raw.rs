use crate::DbValue;
use crate::error::Result;
use crate::internal::translate_error;

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
        use crate::internal::ConnectionTrait;

        let connection = self.own_handle()?;
        crate::profiling::__profile_future(connection.executor().execute_unprepared("SELECT 1"))
            .await
            .map_err(crate::internal::translate_connection_error)?;

        Ok(())
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
    pub async fn raw_with_params<T: crate::model::Model>(
        sql: &str,
        params: Vec<DbValue>,
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
        params: Vec<DbValue>,
    ) -> Result<Vec<T>> {
        self.invalidating(self.__raw_with_params::<T>(sql, params))
            .await
    }

    /// Run a statement TideORM rendered itself and decode it into models.
    ///
    /// The cache is deliberately left alone here: every caller is a builder or
    /// relation helper that knows exactly which tables it touched and performs
    /// the targeted invalidation itself. A blanket flush would throw away
    /// entries that are still valid — including, for reads, the entries the
    /// caller is about to consult again.
    #[doc(hidden)]
    pub async fn __raw_with_params<T: crate::model::Model>(
        &self,
        sql: &str,
        params: Vec<DbValue>,
    ) -> Result<Vec<T>> {
        let rows = self.fetch_rows(sql, params).await?;
        // The rows are remembered as read from the pool this handle runs on,
        // which a `query_with()` handle need not share with the scope.
        crate::model::__loading_through(&self.__get_connection()?, || Self::rows_to_models(rows))
    }

    /// Run a row-returning statement on this handle's connection, for rows
    /// decoded into Rust types.
    pub(crate) async fn fetch_rows(
        &self,
        sql: &str,
        params: Vec<DbValue>,
    ) -> Result<Vec<crate::internal::QueryResult>> {
        self.fetch_rows_as(sql, params, false).await
    }

    /// Run a row-returning statement on this handle's connection, for rows
    /// read as JSON, which takes values no Rust type holds.
    async fn fetch_json_rows(
        &self,
        sql: &str,
        params: Vec<DbValue>,
    ) -> Result<Vec<crate::internal::QueryResult>> {
        self.fetch_rows_as(sql, params, true).await
    }

    /// Run a row-returning statement on this handle's connection; `json`
    /// when the rows are read as JSON.
    async fn fetch_rows_as(
        &self,
        sql: &str,
        params: Vec<DbValue>,
        json: bool,
    ) -> Result<Vec<crate::internal::QueryResult>> {
        use crate::internal::{ConnectionTrait, build_statement_with_values};

        let connection = self.connection_running(sql)?;
        let executor = connection.executor();
        let executor = if json {
            executor.reading_json()
        } else {
            executor
        };
        let statement = build_statement_with_values(executor.get_database_backend(), sql, params);
        crate::profiling::__profile_future(executor.query_all_raw(statement))
            .await
            .map_err(translate_error)
    }

    /// This handle's connection, for running `sql` on: the connection's cache
    /// scope sees the statement first (an `ATTACH` widens it).
    fn connection_running(&self, sql: &str) -> Result<super::ConnectionRef> {
        let connection = self.__get_connection()?;
        connection.observe_cache_sql(sql);
        Ok(connection)
    }

    /// Decode raw rows through the generated entity model.
    fn rows_to_models<T: crate::model::Model>(
        results: Vec<crate::internal::QueryResult>,
    ) -> Result<Vec<T>> {
        use crate::internal::FromQueryResult;

        let mut models = Vec::with_capacity(results.len());
        for row in results {
            let model =
                <T::Entity as crate::internal::EntityTrait>::Model::from_query_result(&row, "")
                    .map_err(translate_error)?;
            models.push(T::try_from_entity_model(model)?);
        }

        Ok(models)
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
        use crate::internal::ConnectionTrait;

        let connection = self.connection_running(sql)?;
        let started = std::time::Instant::now();
        let executor = connection.executor();
        let result = connection
            .invalidating(crate::profiling::__profile_future(
                executor.execute_unprepared(sql),
            ))
            .await;
        // The engine reports no unprepared statement to the logging callback.
        crate::logging::log_statement(sql, started.elapsed(), result.is_err());

        Ok(result.map_err(translate_error)?.rows_affected())
    }

    /// Execute a raw SQL statement with parameters on the ambient connection
    ///
    /// This is an associated function, not a method — see [`Database::raw`].
    /// Use [`Database::exec_raw_with_params`] to run against a specific handle.
    ///
    /// Parameters are [`DbValue`](crate::DbValue)s — see
    /// [`Database::raw_with_params`].
    pub async fn execute_with_params(sql: &str, params: Vec<DbValue>) -> Result<u64> {
        crate::database::__current_db()?
            .exec_raw_with_params(sql, params)
            .await
    }

    /// Execute a raw SQL statement with parameters on this handle
    ///
    /// The instance form of [`Database::execute_with_params`].
    pub async fn exec_raw_with_params(&self, sql: &str, params: Vec<DbValue>) -> Result<u64> {
        self.invalidating(self.__execute_with_params(sql, params))
            .await
    }

    /// Run a statement TideORM rendered itself and report rows affected.
    ///
    /// Like `__raw_with_params`, this leaves the cache to the caller: builder
    /// mutations, relation helpers and the migration and seed ledgers all know
    /// their own table.
    #[doc(hidden)]
    pub async fn __execute_with_params(&self, sql: &str, params: Vec<DbValue>) -> Result<u64> {
        use crate::internal::{ConnectionTrait, build_statement_with_values};

        let connection = self.connection_running(sql)?;
        let executor = connection.executor();
        let statement = build_statement_with_values(executor.get_database_backend(), sql, params);
        let result = crate::profiling::__profile_future(executor.execute_raw(statement))
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
        params: Vec<DbValue>,
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
        params: Vec<DbValue>,
    ) -> Result<Vec<serde_json::Value>> {
        self.invalidating(self.__raw_json_with_params(sql, params))
            .await
    }

    /// Run a statement TideORM rendered itself and return the rows as JSON.
    ///
    /// Like `__raw_with_params`, cache invalidation belongs to the caller.
    #[doc(hidden)]
    pub async fn __raw_json_with_params(
        &self,
        sql: &str,
        params: Vec<DbValue>,
    ) -> Result<Vec<serde_json::Value>> {
        self.__raw_json_typed(sql, params, &|_| None).await
    }

    /// Run a statement a model query rendered and return the rows as JSON,
    /// decoding each column `model_type` knows as that model column.
    pub(crate) async fn __raw_json_typed(
        &self,
        sql: &str,
        params: Vec<DbValue>,
        model_type: ColumnTypeLookup<'_>,
    ) -> Result<Vec<serde_json::Value>> {
        let rows = self.fetch_json_rows(sql, params).await?;
        let schema = self.sqlite_schema(&rows, sql, model_type).await?;
        Self::query_rows_to_json(&rows, model_type, &schema)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/database_raw_tests.rs"]
mod raw_sql_tests;
