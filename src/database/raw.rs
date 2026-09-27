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
        let rows = self.fetch_rows(sql, params).await?;
        // The rows are remembered as read from the pool this handle runs on,
        // which a `query_with()` handle need not share with the scope.
        let origin = super::origin_of(&self.__get_connection()?);
        crate::model::__loading_from(origin, || Self::rows_to_models(rows))
    }

    /// Run a row-returning statement on this handle's connection.
    async fn fetch_rows(
        &self,
        sql: &str,
        params: Vec<DbValue>,
    ) -> Result<Vec<crate::internal::QueryResult>> {
        use crate::internal::{ConnectionTrait, build_statement_with_values};

        let connection = self.__get_connection()?;
        let executor = connection.executor();
        let statement = build_statement_with_values(executor.get_database_backend(), sql, params);
        crate::profiling::__profile_future(executor.query_all_raw(statement))
            .await
            .map_err(translate_error)
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
    /// The instance form of [`Database::execute`]. The statement is sent
    /// unprepared, so it may contain several `;`-separated statements.
    pub async fn exec_raw(&self, sql: &str) -> Result<u64> {
        use crate::internal::ConnectionTrait;

        let connection = self.__get_connection()?;
        let started = std::time::Instant::now();
        let result =
            crate::profiling::__profile_future(connection.executor().execute_unprepared(sql)).await;
        // The engine reports no unprepared statement to the logging callback.
        crate::logging::log_statement(sql, started.elapsed(), result.is_err());
        Self::invalidate_cache_after_raw_sql(sql);

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

        let connection = self.__get_connection()?;
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

        let connection = self.__get_connection()?;
        let executor = connection.executor();
        let statement = build_statement(executor.get_database_backend(), sql);
        let row = crate::profiling::__profile_future(executor.query_one_raw(statement))
            .await
            .map_err(translate_error)?;

        match row {
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
        let rows = self.fetch_rows(sql, params).await?;
        Ok(Self::query_rows_to_json(&rows, model_type))
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
    ///
    /// MySQL reads a backslash inside quotes as an escape and PostgreSQL does
    /// not, so `'O\'Brien'` ends at a different quote on each. The SQL is read
    /// both ways, and counts as a write when either reading finds one.
    fn raw_sql_may_write(sql: &str) -> bool {
        Self::may_write(sql, false) || Self::may_write(sql, true)
    }

    /// [`raw_sql_may_write`](Self::raw_sql_may_write) for one reading of
    /// backslashes inside quotes.
    fn may_write(sql: &str, backslash_escapes: bool) -> bool {
        let statement = Self::strip_leading_sql_noise(sql);
        // A batch is read-only only if every statement is; its first keyword
        // speaks for the first one.
        if Self::holds_more_statements(statement, backslash_escapes) {
            return true;
        }

        match Self::leading_keyword(statement).as_str() {
            "SELECT" | "SHOW" | "DESCRIBE" | "DESC" | "PRAGMA" | "VALUES" => false,
            "EXPLAIN" => Self::explain_may_write(statement, backslash_escapes),
            "WITH" => Self::with_statement_may_write(statement, backslash_escapes),
            _ => true,
        }
    }

    /// Classify an `EXPLAIN`: a plain one only plans its statement, while
    /// `EXPLAIN ANALYZE` (PostgreSQL, MySQL) runs it, so a `DELETE` it
    /// explains is deleted.
    fn explain_may_write(statement: &str, backslash_escapes: bool) -> bool {
        let (_, mut rest) = Self::split_word(statement);
        let mut analyzes = false;
        loop {
            rest = Self::skip_sql_noise(rest);
            if rest.starts_with('(') {
                // PostgreSQL's option list: `EXPLAIN (ANALYZE, BUFFERS) ..`.
                let (options, after) = Self::split_parenthesized_group(rest, backslash_escapes);
                analyzes |= options
                    .split(|character: char| !character.is_ascii_alphabetic())
                    .any(|word| word.eq_ignore_ascii_case("ANALYZE"));
                rest = after;
                continue;
            }
            let (word, after) = Self::split_word(rest);
            match word.to_ascii_uppercase().as_str() {
                "ANALYZE" | "ANALYSE" => analyzes = true,
                "VERBOSE" | "EXTENDED" | "PARTITIONS" | "FORMAT" => {}
                _ => break,
            }
            rest = after;
        }
        analyzes && Self::may_write(rest, backslash_escapes)
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
    fn with_statement_may_write(statement: &str, backslash_escapes: bool) -> bool {
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
                    let (group, after) = Self::split_parenthesized_group(rest, backslash_escapes);
                    if Self::cte_body_may_write(group, backslash_escapes) {
                        return true;
                    }
                    rest = after;
                }
                '\'' | '"' | '`' => rest = Self::skip_quoted(rest, next, backslash_escapes),
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
    fn cte_body_may_write(group: &str, backslash_escapes: bool) -> bool {
        let body = Self::strip_leading_sql_noise(group);

        match Self::leading_keyword(body).as_str() {
            "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "REPLACE" => true,
            "WITH" => Self::with_statement_may_write(body, backslash_escapes),
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

    /// Whether a statement separator outside quoted text and comments is
    /// followed by another statement.
    fn holds_more_statements(sql: &str, backslash_escapes: bool) -> bool {
        let mut rest = sql;

        while let Some(next) = rest.chars().next() {
            match next {
                ';' => {
                    rest = Self::skip_sql_noise(&rest[1..]);
                    if !rest.is_empty() && !rest.starts_with(';') {
                        return true;
                    }
                }
                '\'' | '"' | '`' => rest = Self::skip_quoted(rest, next, backslash_escapes),
                '-' if rest.starts_with("--") => rest = Self::skip_sql_noise(rest),
                '/' if rest.starts_with("/*") => rest = Self::skip_sql_noise(rest),
                _ => rest = &rest[next.len_utf8()..],
            }
        }

        false
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
    fn split_parenthesized_group(sql: &str, backslash_escapes: bool) -> (&str, &str) {
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
                '\'' | '"' | '`' => rest = Self::skip_quoted(rest, next, backslash_escapes),
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
    /// continues the quoted run rather than ending it; with
    /// `backslash_escapes`, as MySQL reads a string, so does one after a
    /// backslash.
    fn skip_quoted(sql: &str, delimiter: char, backslash_escapes: bool) -> &str {
        let mut rest = &sql[delimiter.len_utf8()..];

        loop {
            let end = if backslash_escapes && delimiter != '`' {
                let mut escaped = false;
                rest.char_indices().find_map(|(at, character)| {
                    if escaped {
                        escaped = false;
                        None
                    } else if character == '\\' {
                        escaped = true;
                        None
                    } else {
                        (character == delimiter).then_some(at)
                    }
                })
            } else {
                rest.find(delimiter)
            };
            let Some(end) = end else {
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
}

#[cfg(test)]
#[path = "../../tests/unit/database_raw_tests.rs"]
mod raw_sql_tests;
