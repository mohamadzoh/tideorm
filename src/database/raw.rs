use crate::DbValue;
use crate::error::{Error, Result};
use crate::internal::translate_error;

use super::{Connection, ConnectionRef, Database};

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
    /// wrong question.
    pub async fn ping(&self) -> Result<()> {
        use crate::internal::ConnectionTrait;

        match self.own_connection()? {
            ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(conn.connection().execute_unprepared("SELECT 1"))
                    .await
            }
            ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(tx.as_ref().execute_unprepared("SELECT 1")).await
            }
        }
        .map_err(|err| {
            // A failed health check is a connection failure by construction,
            // even when the driver reports it as an execution error. Override
            // the classification, but carry the structured driver detail across
            // so the SQLSTATE and the source chain still survive.
            let message = err.to_string();
            match translate_error(err) {
                connection @ Error::Connection { .. } => connection,
                other => Error::Connection {
                    message,
                    source: other.into_db_failure(),
                },
            }
        })?;

        Ok(())
    }

    /// Execute a raw SQL query on the ambient connection and return all results
    ///
    /// This is an associated function, not a method: it always runs against the
    /// enclosing transaction scope, or the global connection when there is
    /// none. Use [`Database::query_raw`] to run against a specific handle.
    ///
    /// Raw SQL is opaque to TideORM, so a statement that is not unambiguously
    /// read-only (for example `INSERT ... RETURNING`) flushes the entire query
    /// cache instead of leaving stale cached rows behind.
    pub async fn raw<T: crate::model::Model>(sql: &str) -> Result<Vec<T>> {
        crate::database::__current_db()?.query_raw::<T>(sql).await
    }

    /// Execute a raw SQL query on this handle and return all results
    ///
    /// The instance form of [`Database::raw`]. An enclosing
    /// [`Database::transaction`] scope still takes precedence over `self`, so
    /// the statement joins that transaction instead of opening a second one.
    pub async fn query_raw<T: crate::model::Model>(&self, sql: &str) -> Result<Vec<T>> {
        use crate::internal::{ConnectionTrait, build_statement};

        let results = match self.__get_connection()? {
            ConnectionRef::Database(conn) => {
                let stmt = build_statement(conn.connection().get_database_backend(), sql);
                crate::profiling::__profile_future(conn.connection().query_all_raw(stmt)).await
            }
            ConnectionRef::Transaction(tx) => {
                let stmt = build_statement(tx.as_ref().get_database_backend(), sql);
                crate::profiling::__profile_future(tx.as_ref().query_all_raw(stmt)).await
            }
        };
        Self::invalidate_cache_after_raw_sql(sql);

        Self::rows_to_models(results.map_err(translate_error)?)
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
        let models = self.__raw_with_params::<T>(sql, params).await;
        Self::invalidate_cache_after_raw_sql(sql);

        models
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
        use crate::internal::{ConnectionTrait, build_statement_with_values};

        let results = match self.__get_connection()? {
            ConnectionRef::Database(conn) => {
                let stmt = build_statement_with_values(
                    conn.connection().get_database_backend(),
                    sql,
                    params,
                );
                crate::profiling::__profile_future(conn.connection().query_all_raw(stmt)).await
            }
            ConnectionRef::Transaction(tx) => {
                let stmt =
                    build_statement_with_values(tx.as_ref().get_database_backend(), sql, params);
                crate::profiling::__profile_future(tx.as_ref().query_all_raw(stmt)).await
            }
        };

        Self::rows_to_models(results.map_err(translate_error)?)
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
    /// TideORM cannot know which tables a raw statement touches, so any
    /// statement that is not unambiguously read-only flushes the entire query
    /// cache rather than leaving stale rows behind.
    pub async fn execute(sql: &str) -> Result<u64> {
        crate::database::__current_db()?.exec_raw(sql).await
    }

    /// Execute a raw SQL statement on this handle and return rows affected
    ///
    /// The instance form of [`Database::execute`].
    pub async fn exec_raw(&self, sql: &str) -> Result<u64> {
        use crate::internal::ConnectionTrait;

        let result = match self.__get_connection()? {
            ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(conn.connection().execute_unprepared(sql)).await
            }
            ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(tx.as_ref().execute_unprepared(sql)).await
            }
        };
        Self::invalidate_cache_after_raw_sql(sql);
        let result = result.map_err(translate_error)?;

        Ok(result.rows_affected())
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
        let result = self.__execute_with_params(sql, params).await;
        Self::invalidate_cache_after_raw_sql(sql);

        result
    }

    /// Run a statement TideORM rendered itself and report rows affected.
    ///
    /// Like `__raw_with_params`, this leaves the cache to the caller: builder
    /// mutations, relation helpers and the migration and seed ledgers all know
    /// their own table.
    #[doc(hidden)]
    pub async fn __execute_with_params(&self, sql: &str, params: Vec<DbValue>) -> Result<u64> {
        use crate::internal::{ConnectionTrait, build_statement_with_values};

        let result = match self.__get_connection()? {
            ConnectionRef::Database(conn) => {
                let stmt = build_statement_with_values(
                    conn.connection().get_database_backend(),
                    sql,
                    params,
                );
                crate::profiling::__profile_future(conn.connection().execute_raw(stmt)).await
            }
            ConnectionRef::Transaction(tx) => {
                let stmt =
                    build_statement_with_values(tx.as_ref().get_database_backend(), sql, params);
                crate::profiling::__profile_future(tx.as_ref().execute_raw(stmt)).await
            }
        };
        let result = result.map_err(translate_error)?;

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
        use crate::internal::{ConnectionTrait, build_statement};

        let results = match self.__get_connection()? {
            ConnectionRef::Database(conn) => {
                let stmt = build_statement(conn.connection().get_database_backend(), sql);
                crate::profiling::__profile_future(conn.connection().query_all_raw(stmt)).await
            }
            ConnectionRef::Transaction(tx) => {
                let stmt = build_statement(tx.as_ref().get_database_backend(), sql);
                crate::profiling::__profile_future(tx.as_ref().query_all_raw(stmt)).await
            }
        };
        Self::invalidate_cache_after_raw_sql(sql);

        Self::query_rows_to_json(results.map_err(translate_error)?)
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

    /// Read a single column out of a statement TideORM rendered itself.
    ///
    /// Like the other internal entry points this never touches the cache; its
    /// callers know what they queried.
    #[doc(hidden)]
    pub async fn __query_scalar<T>(&self, sql: &str, column: &str) -> Result<Option<T>>
    where
        T: crate::internal::TryGetable,
    {
        use crate::internal::{ConnectionTrait, build_statement};

        let result = match self.__get_connection()? {
            ConnectionRef::Database(conn) => {
                let stmt =
                    build_statement(conn.connection().get_database_backend(), sql.to_string());
                crate::profiling::__profile_future(conn.connection().query_one_raw(stmt)).await
            }
            ConnectionRef::Transaction(tx) => {
                let stmt = build_statement(tx.as_ref().get_database_backend(), sql.to_string());
                crate::profiling::__profile_future(tx.as_ref().query_one_raw(stmt)).await
            }
        };
        let result = result.map_err(translate_error)?;

        match result {
            Some(row) => row.try_get("", column).map(Some).map_err(translate_error),
            None => Ok(None),
        }
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
        let rows = self.__raw_json_with_params(sql, params).await;
        Self::invalidate_cache_after_raw_sql(sql);

        rows
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
        use crate::internal::{ConnectionTrait, build_statement_with_values};

        let results = match self.__get_connection()? {
            ConnectionRef::Database(conn) => {
                let stmt = build_statement_with_values(
                    conn.connection().get_database_backend(),
                    sql,
                    params,
                );
                crate::profiling::__profile_future(conn.connection().query_all_raw(stmt)).await
            }
            ConnectionRef::Transaction(tx) => {
                let stmt =
                    build_statement_with_values(tx.as_ref().get_database_backend(), sql, params);
                crate::profiling::__profile_future(tx.as_ref().query_all_raw(stmt)).await
            }
        };

        Self::query_rows_to_json(results.map_err(translate_error)?)
    }

    /// Flush the query cache when a raw statement may have modified data.
    ///
    /// Only the hand-written raw entry points reach this. The tables such a
    /// statement touches cannot be recovered reliably, so the cache is cleared
    /// wholesale instead of guessing; everything TideORM renders itself runs
    /// through the `__`-prefixed entry points, whose callers invalidate the one
    /// table they wrote. This runs whether or not the statement succeeded: a
    /// failing multi-statement batch can still have written rows.
    fn invalidate_cache_after_raw_sql(sql: &str) {
        if Self::raw_sql_may_write(sql) {
            crate::cache::QueryCache::global().clear();
        }
    }

    /// Report whether a raw statement is anything other than a plain read.
    ///
    /// The verdict comes from the statement's own leading keyword, never from a
    /// keyword found somewhere in its text. `deleted_at` and `updated_at` carry
    /// `DELETE` and `UPDATE` as substrings and appear in the rendered `WHERE`
    /// clause of every soft-delete read, so a text scan classified those reads
    /// as writes and flushed the whole cache on each one.
    ///
    /// Deliberately conservative otherwise: a statement whose leading keyword is
    /// not unambiguously read-only counts as a write.
    fn raw_sql_may_write(sql: &str) -> bool {
        let statement = Self::strip_leading_sql_noise(sql);

        match Self::leading_keyword(statement).as_str() {
            "SELECT" | "SHOW" | "EXPLAIN" | "DESCRIBE" | "DESC" | "PRAGMA" | "VALUES" => false,
            "WITH" => Self::with_statement_may_write(statement),
            _ => true,
        }
    }

    /// Classify a `WITH` statement, the one shape whose leading keyword does
    /// not settle the question.
    ///
    /// A CTE list can contain data-modifying CTEs, and the statement that
    /// follows it can itself be a write, so both are parsed: every parenthesized
    /// group is classified as a statement of its own, and the first keyword
    /// reached at the top level decides the rest. Quoted text is skipped, so
    /// neither a `"deleted_at"` identifier nor a `'delete me'` literal can be
    /// mistaken for a statement keyword.
    fn with_statement_may_write(statement: &str) -> bool {
        let (_, mut rest) = Self::split_word(statement);

        loop {
            rest = Self::skip_sql_noise(rest);

            let Some(next) = rest.chars().next() else {
                // The CTE list never reached a statement, so this is not SQL
                // that can be reasoned about. Stay conservative.
                return true;
            };

            match next {
                '(' => {
                    let (group, after) = Self::split_parenthesized_group(rest);
                    if Self::cte_body_may_write(group) {
                        return true;
                    }
                    rest = after;
                }
                '\'' | '"' | '`' => rest = Self::skip_quoted(rest, next),
                _ if next.is_ascii_alphabetic() || next == '_' => {
                    let (word, after) = Self::split_word(rest);
                    match word.to_ascii_uppercase().as_str() {
                        "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "REPLACE" => return true,
                        "SELECT" | "VALUES" => return false,
                        _ => {}
                    }
                    rest = after;
                }
                _ => rest = &rest[next.len_utf8()..],
            }
        }
    }

    /// Report whether one parenthesized group inside a `WITH` clause writes.
    ///
    /// Such a group is either a CTE body or the optional column list in front of
    /// `AS`. Only a body opens with a statement keyword, so a group that does
    /// not is no statement at all and cannot write.
    fn cte_body_may_write(group: &str) -> bool {
        let body = Self::strip_leading_sql_noise(group);

        match Self::leading_keyword(body).as_str() {
            "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "REPLACE" => true,
            "WITH" => Self::with_statement_may_write(body),
            _ => false,
        }
    }

    /// Return the uppercased keyword a statement opens with.
    fn leading_keyword(statement: &str) -> String {
        statement
            .chars()
            .take_while(char::is_ascii_alphabetic)
            .collect::<String>()
            .to_ascii_uppercase()
    }

    /// Split the keyword or identifier at the start of `sql` from the rest.
    fn split_word(sql: &str) -> (&str, &str) {
        let end = sql
            .find(|character: char| {
                !(character.is_ascii_alphanumeric() || character == '_' || character == '$')
            })
            .unwrap_or(sql.len());

        sql.split_at(end)
    }

    /// Split the `(..)` group at the start of `sql` into its body and the rest.
    ///
    /// Nested groups, quoted text, and comments inside the group are skipped, so
    /// the split lands on the matching close parenthesis. An unterminated group
    /// yields everything that was left.
    fn split_parenthesized_group(sql: &str) -> (&str, &str) {
        let Some(body) = sql.strip_prefix('(') else {
            return ("", sql);
        };

        let mut rest = body;
        let mut depth = 1_usize;

        while let Some(next) = rest.chars().next() {
            match next {
                '(' => {
                    depth += 1;
                    rest = &rest[1..];
                }
                ')' => {
                    depth -= 1;
                    rest = &rest[1..];
                    if depth == 0 {
                        return (&body[..body.len() - rest.len() - 1], rest);
                    }
                }
                '\'' | '"' | '`' => rest = Self::skip_quoted(rest, next),
                '-' if rest.starts_with("--") => rest = Self::skip_sql_noise(rest),
                '/' if rest.starts_with("/*") => rest = Self::skip_sql_noise(rest),
                _ => rest = &rest[next.len_utf8()..],
            }
        }

        (body, "")
    }

    /// Skip the quoted string or identifier at the start of `sql`.
    ///
    /// A doubled delimiter is SQL's escape for the delimiter itself, so it
    /// continues the quoted run rather than ending it.
    fn skip_quoted(sql: &str, delimiter: char) -> &str {
        let mut rest = &sql[delimiter.len_utf8()..];

        loop {
            let Some(end) = rest.find(delimiter) else {
                return "";
            };

            rest = &rest[end + delimiter.len_utf8()..];
            if !rest.starts_with(delimiter) {
                return rest;
            }

            rest = &rest[delimiter.len_utf8()..];
        }
    }

    /// Skip leading whitespace, comments, and opening parentheses.
    fn strip_leading_sql_noise(sql: &str) -> &str {
        let mut rest = Self::skip_sql_noise(sql);

        while let Some(after) = rest.strip_prefix('(') {
            rest = Self::skip_sql_noise(after);
        }

        rest
    }

    /// Skip leading whitespace and comments, keeping parentheses.
    fn skip_sql_noise(sql: &str) -> &str {
        let mut rest = sql.trim_start();

        loop {
            if let Some(after) = rest.strip_prefix("--") {
                rest = after
                    .find('\n')
                    .map_or("", |end| &after[end + 1..])
                    .trim_start();
            } else if let Some(after) = rest.strip_prefix("/*") {
                rest = after
                    .find("*/")
                    .map_or("", |end| &after[end + 2..])
                    .trim_start();
            } else {
                return rest;
            }
        }
    }

    fn query_rows_to_json(
        results: Vec<crate::internal::QueryResult>,
    ) -> Result<Vec<serde_json::Value>> {
        let mut json_results = Vec::new();
        for row in results {
            json_results.push(Self::query_row_to_json(&row));
        }

        Ok(json_results)
    }

    fn query_row_to_json(row: &crate::internal::QueryResult) -> serde_json::Value {
        #[cfg(feature = "postgres")]
        if let Some(pg_row) = row.try_as_pg_row() {
            return Self::sqlx_row_to_json(
                row,
                pg_row,
                |result, index, type_name| match type_name {
                    "BOOL" => Self::typed_or_fallback::<bool>(result, index),
                    "INT2" => Self::typed_or_fallback::<i16>(result, index),
                    "INT4" => Self::typed_or_fallback::<i32>(result, index),
                    "INT8" => Self::typed_or_fallback::<i64>(result, index),
                    "FLOAT4" => Self::float_or_fallback::<f32>(result, index),
                    "FLOAT8" => Self::float_or_fallback::<f64>(result, index),
                    "NUMERIC" => Self::decimal_or_fallback(result, index),
                    "UUID" => Self::typed_or_fallback::<uuid::Uuid>(result, index),
                    "JSON" | "JSONB" => Self::typed_or_fallback::<serde_json::Value>(result, index),
                    "DATE" => Self::typed_or_fallback::<chrono::NaiveDate>(result, index),
                    "TIME" => Self::typed_or_fallback::<chrono::NaiveTime>(result, index),
                    "TIMESTAMP" => Self::typed_or_fallback::<chrono::NaiveDateTime>(result, index),
                    "TIMESTAMPTZ" => {
                        Self::typed_or_fallback::<chrono::DateTime<chrono::FixedOffset>>(
                            result, index,
                        )
                    }
                    _ => Self::fallback_try_get_json(result, index),
                },
            );
        }

        #[cfg(feature = "mysql")]
        if let Some(mysql_row) = row.try_as_mysql_row() {
            return Self::sqlx_row_to_json(
                row,
                mysql_row,
                |result, index, type_name| match type_name {
                    "BOOLEAN" | "BOOL" => Self::typed_or_fallback::<bool>(result, index),
                    "TINYINT" => Self::typed_or_fallback::<i8>(result, index),
                    "SMALLINT" => Self::typed_or_fallback::<i16>(result, index),
                    "INT" | "INTEGER" | "MEDIUMINT" => {
                        Self::typed_or_fallback::<i32>(result, index)
                    }
                    "BIGINT" => Self::typed_or_fallback::<i64>(result, index),
                    "FLOAT" => Self::float_or_fallback::<f32>(result, index),
                    "DOUBLE" => Self::float_or_fallback::<f64>(result, index),
                    "DECIMAL" | "NUMERIC" => Self::decimal_or_fallback(result, index),
                    "JSON" => Self::typed_or_fallback::<serde_json::Value>(result, index),
                    "DATE" => Self::typed_or_fallback::<chrono::NaiveDate>(result, index),
                    "TIME" => Self::typed_or_fallback::<chrono::NaiveTime>(result, index),
                    "DATETIME" | "TIMESTAMP" => {
                        Self::typed_or_fallback::<chrono::NaiveDateTime>(result, index)
                    }
                    _ => Self::fallback_try_get_json(result, index),
                },
            );
        }

        #[cfg(feature = "sqlite")]
        if let Some(sqlite_row) = row.try_as_sqlite_row() {
            return Self::sqlx_row_to_json(row, sqlite_row, |result, index, type_name| {
                match type_name {
                    "BOOLEAN" | "BOOL" => Self::typed_or_fallback::<bool>(result, index),
                    "INTEGER" | "INT" => Self::typed_or_fallback::<i64>(result, index),
                    "REAL" | "FLOAT" | "DOUBLE" => Self::float_or_fallback::<f64>(result, index),
                    "NUMERIC" | "DECIMAL" => Self::decimal_or_fallback(result, index),
                    "JSON" => Self::typed_or_fallback::<serde_json::Value>(result, index),
                    "DATE" => Self::typed_or_fallback::<chrono::NaiveDate>(result, index),
                    "TIME" => Self::typed_or_fallback::<chrono::NaiveTime>(result, index),
                    "DATETIME" | "TIMESTAMP" => {
                        Self::typed_or_fallback::<chrono::NaiveDateTime>(result, index)
                    }
                    "TEXT" => Self::typed_or_fallback::<String>(result, index),
                    "BLOB" => Self::typed_or_fallback::<Vec<u8>>(result, index),
                    _ => Self::sqlite_unknown_type_or_fallback(result, index),
                }
            });
        }

        let mut obj = serde_json::Map::new();
        for (index, col_name) in row.column_names().into_iter().enumerate() {
            obj.insert(col_name, Self::fallback_try_get_json(row, index));
        }

        serde_json::Value::Object(obj)
    }

    fn fallback_try_get_json(
        row: &crate::internal::QueryResult,
        index: usize,
    ) -> serde_json::Value {
        Self::try_get_json::<serde_json::Value>(row, index)
            .or_else(|| Self::try_get_json::<uuid::Uuid>(row, index))
            .or_else(|| Self::try_get_decimal_json(row, index))
            .or_else(|| Self::try_get_json::<chrono::DateTime<chrono::FixedOffset>>(row, index))
            .or_else(|| Self::try_get_json::<chrono::DateTime<chrono::Utc>>(row, index))
            .or_else(|| Self::try_get_json::<chrono::NaiveDateTime>(row, index))
            .or_else(|| Self::try_get_json::<chrono::NaiveDate>(row, index))
            .or_else(|| Self::try_get_json::<chrono::NaiveTime>(row, index))
            .or_else(|| Self::try_get_json::<i64>(row, index))
            .or_else(|| Self::try_get_json::<u64>(row, index))
            .or_else(|| Self::try_get_json::<f64>(row, index))
            .or_else(|| Self::try_get_json::<bool>(row, index))
            .or_else(|| Self::try_get_json::<String>(row, index))
            .unwrap_or(serde_json::Value::Null)
    }

    #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
    fn typed_or_fallback<T>(row: &crate::internal::QueryResult, index: usize) -> serde_json::Value
    where
        T: crate::internal::TryGetable + serde::Serialize,
    {
        Self::try_get_json::<T>(row, index)
            .unwrap_or_else(|| Self::fallback_try_get_json(row, index))
    }

    /// Decode a floating-point column, keeping non-finite values visible.
    #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
    fn float_or_fallback<T>(row: &crate::internal::QueryResult, index: usize) -> serde_json::Value
    where
        T: crate::internal::TryGetable + Into<f64>,
    {
        match row.try_get_by_index::<Option<T>>(index) {
            Ok(Some(value)) => Self::f64_to_json(value.into()),
            Ok(None) => serde_json::Value::Null,
            Err(_) => Self::fallback_try_get_json(row, index),
        }
    }

    /// Represent an `f64` as JSON.
    ///
    /// JSON has no `NaN` or `Infinity`, so those are rendered as strings — a
    /// `null` would be indistinguishable from a real SQL `NULL`.
    fn f64_to_json(value: f64) -> serde_json::Value {
        match serde_json::Number::from_f64(value) {
            Some(number) => serde_json::Value::Number(number),
            None => serde_json::Value::String(value.to_string()),
        }
    }

    #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
    fn decimal_or_fallback(row: &crate::internal::QueryResult, index: usize) -> serde_json::Value {
        Self::try_get_decimal_json(row, index)
            .unwrap_or_else(|| Self::fallback_try_get_json(row, index))
    }

    #[cfg(feature = "sqlite")]
    fn sqlite_unknown_type_or_fallback(
        row: &crate::internal::QueryResult,
        index: usize,
    ) -> serde_json::Value {
        let value = Self::fallback_try_get_json(row, index);

        if let serde_json::Value::String(text) = &value {
            if let Ok(integer) = text.parse::<i64>() {
                return serde_json::json!(integer);
            }

            if let Ok(unsigned) = text.parse::<u64>() {
                return serde_json::json!(unsigned);
            }
        }

        value
    }

    fn try_get_decimal_json(
        row: &crate::internal::QueryResult,
        index: usize,
    ) -> Option<serde_json::Value> {
        if let Some(value) = Self::try_get_json::<rust_decimal::Decimal>(row, index) {
            return Some(value);
        }

        if let Ok(Some(value)) = row.try_get_by_index::<Option<String>>(index) {
            return rust_decimal::Decimal::from_str_exact(&value)
                .ok()
                .and_then(|decimal| serde_json::to_value(decimal).ok())
                .or(Some(serde_json::Value::String(value)));
        }

        if let Ok(Some(value)) = row.try_get_by_index::<Option<i64>>(index) {
            return serde_json::to_value(rust_decimal::Decimal::from(value))
                .ok()
                .or(Some(serde_json::json!(value)));
        }

        if let Ok(Some(value)) = row.try_get_by_index::<Option<u64>>(index) {
            return serde_json::to_value(rust_decimal::Decimal::from(value))
                .ok()
                .or(Some(serde_json::json!(value)));
        }

        if let Ok(Some(value)) = row.try_get_by_index::<Option<f64>>(index) {
            let value_text = value.to_string();
            return Some(
                rust_decimal::Decimal::from_str_exact(&value_text)
                    .ok()
                    .and_then(|decimal| serde_json::to_value(decimal).ok())
                    .unwrap_or_else(|| Self::f64_to_json(value)),
            );
        }

        None
    }

    fn try_get_json<T>(
        row: &crate::internal::QueryResult,
        index: usize,
    ) -> Option<serde_json::Value>
    where
        T: crate::internal::TryGetable + serde::Serialize,
    {
        row.try_get_by_index::<Option<T>>(index)
            .ok()
            .and_then(Self::option_to_json)
    }

    /// Convert a decoded column into JSON.
    ///
    /// Returns `None` when the value decoded but could not be represented as
    /// JSON, so callers keep trying other decoders instead of reporting a
    /// value that never existed. Only a real SQL `NULL` yields
    /// `Some(Value::Null)`.
    fn option_to_json<T>(value: Option<T>) -> Option<serde_json::Value>
    where
        T: serde::Serialize,
    {
        match value {
            Some(value) => serde_json::to_value(value).ok(),
            None => Some(serde_json::Value::Null),
        }
    }

    #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
    fn sqlx_row_to_json<R, F>(
        result: &crate::internal::QueryResult,
        row: &R,
        decoder_for_type: F,
    ) -> serde_json::Value
    where
        R: crate::internal::sqlx::Row,
        F: Fn(&crate::internal::QueryResult, usize, &str) -> serde_json::Value,
    {
        use crate::internal::sqlx::{Column, TypeInfo};

        let mut obj = serde_json::Map::new();
        for (index, column) in row.columns().iter().enumerate() {
            let type_name = column.type_info().name().to_ascii_uppercase();
            obj.insert(
                column.name().to_string(),
                decoder_for_type(result, index, type_name.as_str()),
            );
        }

        serde_json::Value::Object(obj)
    }
}

#[cfg(test)]
mod raw_sql_tests {
    use super::Database;

    #[test]
    fn read_only_statements_do_not_flush_the_cache() {
        assert!(!Database::raw_sql_may_write("SELECT 1"));
        assert!(!Database::raw_sql_may_write(
            "  -- comment\n select * from users"
        ));
        assert!(!Database::raw_sql_may_write("/* hint */ EXPLAIN SELECT 1"));
        assert!(!Database::raw_sql_may_write("(SELECT 1)"));
        assert!(!Database::raw_sql_may_write(
            "WITH active AS (SELECT 1) SELECT * FROM active"
        ));
    }

    #[test]
    fn soft_delete_columns_are_not_write_keywords() {
        // `deleted_at` and `updated_at` contain `DELETE` and `UPDATE`, and the
        // soft-delete scope renders them into the `WHERE` clause of every read.
        assert!(!Database::raw_sql_may_write(
            "WITH scoped AS (SELECT id FROM users WHERE \"deleted_at\" IS NULL) \
             SELECT * FROM scoped"
        ));
        assert!(!Database::raw_sql_may_write(
            "WITH recent AS (SELECT id, updated_at FROM users) \
             SELECT * FROM recent ORDER BY updated_at DESC"
        ));
        // Nor can a literal that merely mentions one.
        assert!(!Database::raw_sql_may_write(
            "WITH notes AS (SELECT 'delete me' AS body) SELECT * FROM notes"
        ));
        // A column list in front of `AS` is not a statement.
        assert!(!Database::raw_sql_may_write(
            "WITH scoped (id) AS (SELECT id FROM users) SELECT * FROM scoped"
        ));
    }

    #[test]
    fn writing_statements_flush_the_cache() {
        assert!(Database::raw_sql_may_write(
            "INSERT INTO users (id) VALUES (1)"
        ));
        assert!(Database::raw_sql_may_write("update users set active = 1"));
        assert!(Database::raw_sql_may_write("DELETE FROM users"));
        assert!(Database::raw_sql_may_write(
            "CREATE TABLE users (id INTEGER)"
        ));
        assert!(Database::raw_sql_may_write(
            "WITH removed AS (DELETE FROM users RETURNING id) SELECT * FROM removed"
        ));
        // Unparseable input stays conservative.
        assert!(Database::raw_sql_may_write(""));
    }

    #[test]
    fn data_modifying_ctes_still_flush_the_cache() {
        // The soft-delete filter inside the CTE is not what makes this a write:
        // the CTE body itself is.
        assert!(Database::raw_sql_may_write(
            "WITH removed AS (DELETE FROM users WHERE deleted_at IS NOT NULL RETURNING id) \
             SELECT * FROM removed"
        ));
        // The statement following the CTE list counts too.
        assert!(Database::raw_sql_may_write(
            "WITH stale AS (SELECT id FROM users) \
             UPDATE users SET updated_at = NULL WHERE id IN (SELECT id FROM stale)"
        ));
        // As does a nested one.
        assert!(Database::raw_sql_may_write(
            "WITH outer_rows AS (WITH inner_rows AS (INSERT INTO audit (id) VALUES (1) \
             RETURNING id) SELECT * FROM inner_rows) SELECT * FROM outer_rows"
        ));
        // A `WITH` that never reaches a statement stays conservative.
        assert!(Database::raw_sql_may_write("WITH scoped AS ("));
    }

    #[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
    #[tokio::test]
    async fn only_unattributable_raw_writes_flush_the_cache() {
        use crate::cache::QueryCache;

        let cache = QueryCache::global();
        let was_enabled = cache.is_enabled();
        cache.enable();

        let db = Database::connect("sqlite::memory:")
            .await
            .expect("sqlite in-memory connection should succeed");
        db.exec_raw("CREATE TABLE raw_cache_probe (id INTEGER PRIMARY KEY, deleted_at TEXT)")
            .await
            .expect("the probe table should be created");

        let seed = || {
            cache.clear();
            cache
                .set_tagged(
                    "raw-cache-probe",
                    &[1_i64],
                    None,
                    &["unrelated_table".to_string()],
                )
                .expect("seeding the cache should succeed");
            assert!(
                cache.contains("raw-cache-probe"),
                "the seeded entry should be cached"
            );
        };

        // A builder read on a soft-delete model renders `deleted_at`, which used
        // to classify the read as a write and destroy the cache on every call.
        seed();
        db.query_raw_json(
            "WITH scoped AS (SELECT id FROM raw_cache_probe WHERE deleted_at IS NULL) \
             SELECT * FROM scoped",
        )
        .await
        .expect("the CTE read should succeed");
        assert!(
            cache.contains("raw-cache-probe"),
            "a read must never flush the query cache"
        );

        // Statements TideORM rendered itself leave invalidation to their caller,
        // which knows the one table it wrote.
        seed();
        db.__execute_with_params("INSERT INTO raw_cache_probe (id) VALUES (1)", Vec::new())
            .await
            .expect("the internal insert should succeed");
        assert!(
            cache.contains("raw-cache-probe"),
            "an internal write must leave unrelated entries to targeted invalidation"
        );

        // Hand-written raw SQL remains unattributable, so it still flushes.
        seed();
        db.exec_raw("INSERT INTO raw_cache_probe (id) VALUES (2)")
            .await
            .expect("the raw insert should succeed");
        assert!(
            !cache.contains("raw-cache-probe"),
            "an unattributable raw write must still flush the query cache"
        );

        if !was_enabled {
            cache.disable();
        }
    }

    #[test]
    fn non_finite_floats_stay_distinguishable_from_null() {
        assert_eq!(Database::f64_to_json(1.5), serde_json::json!(1.5));
        assert_eq!(
            Database::f64_to_json(f64::NAN),
            serde_json::Value::String("NaN".to_string())
        );
        assert_eq!(
            Database::f64_to_json(f64::INFINITY),
            serde_json::Value::String("inf".to_string())
        );
    }
}
