//! Terminals that read rows, and the fetches behind them.

use super::*;

#[allow(missing_docs)]
impl<M: Model> QueryBuilder<M> {
    /// Run `statement`, which the engine does not log (`EngineLog::Off`),
    /// with query logging, attaching this query's error context to a failure.
    pub(super) async fn logged<T>(
        &self,
        sql: &str,
        statement: impl Future<Output = Result<T>>,
        row_count: impl FnOnce(&T) -> u64,
    ) -> Result<T> {
        let timer = Self::start_query_log(sql);
        let result = statement
            .await
            .map_err(|err| err.with_context(self.build_query_error_context(Some(sql))));
        Self::finish_query_log(timer, &result, row_count);
        result
    }

    /// Run a row-returning statement and hand the rows back as JSON objects,
    /// with `M`'s own columns decoded as `M` reads them.
    ///
    /// A column the projection reads from a joined table is decoded by its own
    /// declared type, even when one of `M`'s columns has the same name.
    pub(in crate::query) async fn fetch_json(
        &self,
        sql: &str,
        params: Vec<Value>,
    ) -> Result<Vec<serde_json::Value>> {
        self.fetch_json_with_types(sql, params, &[]).await
    }

    /// [`fetch_json`](Self::fetch_json), decoding each named output of
    /// `output_types` as its type: an aggregate of a joined model's column
    /// comes back as that column does.
    pub(in crate::query) async fn fetch_json_with_types(
        &self,
        sql: &str,
        params: Vec<Value>,
        output_types: &[(String, crate::orm::ColumnType)],
    ) -> Result<Vec<serde_json::Value>> {
        let db = self.current_db()?;
        let outputs = self.projection_outputs();
        // An output is typed by where it comes from, not by its name: a model
        // column under any alias by that column's type, including registered
        // joined models. Unknown sources and expressions use the driver's type,
        // even when a model field shares their output name.
        let model_type = |name: &str| {
            if let Some(subquery) = self
                .clauses
                .subquery_select_expressions
                .iter()
                .find(|s| s.alias == name)
            {
                return subquery.column_type.clone();
            }
            if let Some((_, column_type)) = output_types.iter().find(|(output, _)| output == name) {
                return Some(column_type.clone());
            }
            match outputs.iter().find(|(output, _)| output == name) {
                Some((_, Some((table, column)))) => self.column_type(&format!("{table}.{column}")),
                Some(_) => None,
                None => crate::internal::column_type_of::<M>(name),
            }
        };
        let rows = db.__raw_json_typed(sql, params, &model_type, EngineLog::Off);
        let mut rows = self.logged(sql, rows, |rows| rows.len() as u64).await?;
        self.decrypt_projected_rows(&mut rows, outputs)?;
        Ok(rows)
    }

    /// Decode generated aggregate outputs using only their declared result types.
    /// Their aliases are independent of the model's fields and query projection.
    pub(in crate::query) async fn fetch_aggregate_json(
        &self,
        sql: &str,
        params: Vec<Value>,
        output_types: &[(String, crate::orm::ColumnType)],
        source_columns: &[(String, String)],
    ) -> Result<Vec<serde_json::Value>> {
        let db = self.current_db()?;
        let result_type = |name: &str| {
            output_types
                .iter()
                .find(|(output, _)| output == name)
                .map(|(_, ty)| ty.clone())
        };
        let rows = db.__raw_json_typed(sql, params, &result_type, EngineLog::Off);
        let mut rows = self.logged(sql, rows, |rows| rows.len() as u64).await?;
        let outputs = rows
            .first()
            .and_then(serde_json::Value::as_object)
            .map(|row| {
                row.keys()
                    .map(|alias| {
                        let source = source_columns
                            .iter()
                            .find(|(name, _)| name == alias)
                            .and_then(|(_, column)| self.aggregate_source(column));
                        (alias.clone(), source)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.decrypt_projected_rows(&mut rows, outputs)?;
        Ok(rows)
    }

    pub(super) fn decrypt_projected_rows(
        &self,
        rows: &mut [serde_json::Value],
        outputs: Vec<(String, Option<(String, String)>)>,
    ) -> Result<()> {
        let outputs: Vec<_> = outputs
            .into_iter()
            .map(|(name, source)| {
                let encrypted = self.projected_encrypted_column(&name, source.as_ref());
                (name, encrypted)
            })
            .collect();
        crate::model::decrypt_json_rows::<M>(rows, &outputs)
    }

    /// Run a mutation and drop every cached result it may have made stale.
    pub(super) async fn run_mutation(&self, sql: &str, params: Vec<Value>) -> Result<u64> {
        let db = self.current_db()?;
        Self::invalidating_model_state(
            &db,
            self.logged(
                sql,
                db.execute_logged(sql, params, EngineLog::Off),
                |rows| *rows,
            ),
            |rows| *rows,
        )
        .await
    }

    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).get().await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn get(self) -> Result<Vec<M>> {
        self.ensure_query_is_executable()?;
        self.ensure_projection_covers_model()?;

        // A read performed inside a transaction never touches the process-global
        // cache: the payload would outlive a rollback for the rest of its TTL, and
        // a hit would hide rows the transaction itself has already written.
        // A model whose own serde skips or converts a stored field is not
        // cached: the copy read back would lack those values.
        let cache = (self.clauses.cache_options.is_some()
            && M::__serde_round_trips()
            && !self.clauses.lock_for_update
            && !self.runs_in_transaction())
        .then(crate::cache::QueryCache::global);
        if let Some(cache) = cache {
            cache.warn_once_if_disabled();
        }
        // A disabled cache neither serves nor stores, so skip its key and fill point.
        let cache_fill = if let Some(cache) = cache.filter(|cache| cache.is_enabled()) {
            let key = self.generate_cache_key();
            if let Some(cached) = cache.get::<Vec<M>>(&key) {
                let connection = self.current_db()?.__get_connection()?;
                return Ok(cached
                    .into_iter()
                    .map(|model| {
                        let mut model = M::__rebuild_relations(model);
                        crate::model::__finish_load(&mut model, &connection);
                        model
                    })
                    .collect());
            }
            // Taken before the read: a write that lands while it runs may
            // replace the rows it returns.
            let database = self.current_db()?.__get_connection()?.cache_scope();
            Some((key, cache.fill_point_for_database(database)))
        } else {
            None
        };

        let (sql, params) = self.build_select_sql_with_params()?;
        let db = self.current_db()?;
        let results = self
            .logged(
                &sql,
                db.raw_models::<M>(&sql, params, EngineLog::Off),
                |rows| rows.len() as u64,
            )
            .await?;

        if let (Some((key, fill_point)), Some(options)) = (cache_fill, &self.clauses.cache_options)
        {
            let _ = crate::cache::QueryCache::global().fill_tagged(
                fill_point,
                &key,
                &results,
                Some(options.ttl),
                &self.cache_tables(),
            );
        }

        Ok(results)
    }

    /// Return the first matching row, or `None` when nothing matches.
    ///
    /// Without an order, which row comes first is up to the database; order
    /// the query when the choice matters.
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).first().await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn first(self) -> Result<Option<M>> {
        let results = self.limit(1).get().await?;
        Ok(results.into_iter().next())
    }

    /// [`first()`](Self::first), with a not-found error when nothing matches.
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).first_or_fail().await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn first_or_fail(self) -> Result<M> {
        self.first()
            .await?
            .ok_or_else(|| Error::not_found(format!("No {} found matching query", M::table_name())))
    }

    /// Refuse a `chunk()` its primary-key cursor cannot page: a zero batch
    /// size, or a query the cursor would change or not reach all of.
    pub(super) fn ensure_query_is_chunkable(&self, chunk_size: u64) -> Result<()> {
        if chunk_size == 0 {
            return Err(Error::query(
                "chunk() requires chunk_size to be greater than 0",
            ));
        }

        if !self.clauses.window_functions.is_empty() {
            return Err(Error::query(
                "chunk() does not support window functions: its cursor changes the window input; use get() or page()",
            ));
        }

        if !self.clauses.group_by.is_empty() || !self.clauses.having_conditions.is_empty() {
            return Err(Error::query(
                "chunk() does not support GROUP BY or HAVING: its primary-key cursor changes group membership; read grouped results with get() or page()",
            ));
        }

        if self.clauses.offset_value.unwrap_or(0) > 0 {
            return Err(Error::query(
                "chunk() does not support offset(); use page()/get() for fixed windows or chunk over primary-key order",
            ));
        }

        if !self.clauses.unions.is_empty() {
            return Err(Error::query(
                "chunk() pages by the model's primary key, which the other branches of a union() do not follow; read a union with get(), or page() it",
            ));
        }

        Ok(())
    }

    /// Count the rows the query matches.
    ///
    /// Ordering, `limit()` and `offset()` are ignored, so a paged query counts
    /// its total across every page. The aggregates honour them.
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).count().await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn count(self) -> Result<u64> {
        let query = self.without_distinct_ordering();
        query.ensure_query_is_executable()?;

        let (sql, params) = query.build_count_sql_with_params()?;
        let rows = query.fetch_aggregate_json(&sql, params, &[], &[]).await?;
        Self::decode_count_value(rows.first().and_then(|row| row.get("count")), "count")
    }

    /// Decode the `column` of a rendered COUNT projection.
    ///
    /// A COUNT projection always returns exactly one row, so a missing or
    /// non-numeric value is a decode failure and not an empty result. Reporting
    /// zero for it would turn a broken read into a plausible-looking answer.
    pub(in crate::query) fn decode_count_value(
        value: Option<&serde_json::Value>,
        column: &str,
    ) -> Result<u64> {
        let Some(value) = value else {
            return Err(Error::query(format!(
                "Database returned no '{}' column for the query count",
                column
            )));
        };

        if let Some(count) = value.as_u64() {
            Ok(count)
        } else if let Some(count) = value.as_i64() {
            crate::internal::count_to_u64(count, column)
        } else {
            Err(Error::query(format!(
                "Unable to decode the '{}' count as an integer (got {})",
                column, value
            )))
        }
    }

    /// Whether any row matches the query; its ordering, limit and offset are
    /// ignored.
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).exists().await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn exists(self) -> Result<bool> {
        let query = self.without_distinct_ordering();
        query.ensure_query_is_executable()?;

        let (sql, params) = query.build_exists_sql_with_params()?;
        let rows = query.fetch_aggregate_json(&sql, params, &[], &[]).await?;

        // `build_exists_sql_with_params` renders one of two shapes. The
        // `SELECT EXISTS(..) AS "exists_result"` shape always returns a single
        // row whose answer lives in that column, so an undecodable value there
        // is an error — falling back to "a row came back" would report `true`
        // unconditionally. The union/CTE shape projects `SELECT 1 .. LIMIT 1`
        // instead, carries no `exists_result` column, and *is* answered by the
        // row count.
        match rows.first().and_then(|row| row.get("exists_result")) {
            Some(value) => Self::decode_exists_flag(value),
            None => Ok(!rows.is_empty()),
        }
    }

    /// Decode the `exists_result` column of an `EXISTS(..)` projection.
    ///
    /// Backends spell the answer differently — PostgreSQL returns a boolean
    /// while MySQL, MariaDB, and SQLite return an integer — so every numeric or
    /// boolean spelling is accepted and anything else is reported rather than
    /// guessed at.
    pub(super) fn decode_exists_flag(value: &serde_json::Value) -> Result<bool> {
        if let Some(exists) = value.as_bool() {
            return Ok(exists);
        }
        if let Some(exists) = value.as_u64() {
            return Ok(exists != 0);
        }
        if let Some(exists) = value.as_i64() {
            return Ok(exists != 0);
        }

        Err(Error::query(format!(
            "Unable to decode the database EXISTS result as a boolean or integer (got {})",
            value
        )))
    }
}
