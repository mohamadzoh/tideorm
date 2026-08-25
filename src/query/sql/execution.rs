use super::*;

use crate::internal::InternalConnection;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};

mod hash_helpers;
mod mutation_safety;

use hash_helpers::{
    hash_bound_values, hash_having_clause, hash_or_group, hash_where_condition,
    hash_window_function,
};

#[allow(missing_docs)]
impl<M: Model> QueryBuilder<M> {
    fn chunk_primary_key_column(&self) -> Result<&'static str> {
        match M::primary_key_names() {
            [primary_key] => Ok(*primary_key),
            _ => Err(Error::invalid_query(format!(
                "chunk() only supports models with a single-column primary key; model '{}' uses {} key columns",
                M::table_name(),
                M::primary_key_names().len()
            ))),
        }
    }

    fn is_chunk_primary_key_order(column: &str, primary_key: &str) -> bool {
        column == primary_key || column == format!("{}.{}", M::table_name(), primary_key)
    }

    fn chunk_order(&self, primary_key: &str) -> Result<crate::query::Order> {
        match self.order_by.as_slice() {
            [] => Ok(crate::query::Order::Asc),
            [(column, direction)] if Self::is_chunk_primary_key_order(column, primary_key) => {
                Ok(*direction)
            }
            _ => Err(Error::invalid_query(format!(
                "chunk() only supports explicit ordering by the single primary key '{}' for model '{}'",
                primary_key,
                M::table_name()
            ))),
        }
    }

    #[must_use]
    pub fn cache(mut self, ttl: std::time::Duration) -> Self {
        self.cache_options = Some(crate::cache::CacheOptions::new(ttl));
        self
    }

    #[must_use]
    pub fn cache_with_key(mut self, key: &str, ttl: std::time::Duration) -> Self {
        self.cache_key = Some(key.to_string());
        self.cache_options = Some(crate::cache::CacheOptions::new(ttl));
        self
    }

    #[must_use]
    pub fn cache_with_options(mut self, options: crate::cache::CacheOptions) -> Self {
        self.cache_options = Some(options);
        self
    }

    #[must_use]
    pub fn no_cache(mut self) -> Self {
        self.cache_options = None;
        self.cache_key = None;
        self
    }

    /// Validate the builder and reject conditions that cannot be rendered.
    ///
    /// `ensure_query_is_valid` only checks fragment syntax. A condition whose
    /// operator and value do not pair up renders to nothing at all, so it is
    /// rejected here instead of being silently dropped from the WHERE clause.
    fn ensure_query_is_executable(&self) -> Result<()> {
        self.ensure_query_is_valid()?;
        self.ensure_conditions_are_representable()
    }

    /// Mix the identity of an explicitly attached connection into the cache key.
    ///
    /// Without this, `query_with(&tenant_a).cache()` and `query_with(&tenant_b)`
    /// hash identically and one tenant's rows get served to the other. What is
    /// mixed in is the pooled connection's [`connection_identity`], never its
    /// address: an allocator reuses an address as soon as the connection at it is
    /// dropped, so tenant A's closed connection and tenant B's freshly opened one
    /// can hash the same and B would be served A's rows for the rest of the TTL.
    fn hash_database_identity<H: std::hash::Hasher>(&self, hasher: &mut H) {
        use std::hash::Hash;

        let Some(database) = &self.database else {
            return;
        };

        match database.current_inner() {
            Ok(connection) => connection_identity(&connection).hash(hasher),
            // A transaction handle carries no pooled identity of its own; keep it
            // out of the ambient-connection keyspace rather than sharing it.
            Err(_) => "tideorm::unresolved-connection".hash(hasher),
        }
    }

    /// True when this query will execute inside a transaction scope.
    ///
    /// The handle the query will actually use is inspected, so an explicitly
    /// attached transaction (`query_with(&tx)`) is recognised as well as the
    /// ambient override that `Database::transaction` installs.
    fn runs_in_transaction(&self) -> bool {
        let Ok(database) = self.current_db() else {
            return false;
        };

        matches!(
            crate::database::Connection::__get_connection(&database),
            Ok(crate::database::ConnectionRef::Transaction(_))
        )
    }

    /// Every table this query reads, used to tag its cache entry.
    ///
    /// A cached payload has to be dropped when *any* of its source tables is
    /// written, not just the model's own table, so the model's table and every
    /// joined table are reported from the builder's own structure. Union, CTE,
    /// and subquery operands keep no structured record of what they read — they
    /// survive only as rendered SQL text — so their tables are recovered by
    /// `collect_tables_from_sql` until those clauses carry their own sources.
    fn cache_tables(&self) -> Vec<String> {
        // The first tag is the declared table name verbatim: every write
        // invalidates with `M::table_name()`, so that tag has to match it
        // character for character. Everything after it is a name read off a join
        // or a rendered operand and is normalized to a bare identifier.
        let mut tables = vec![M::table_name().to_string()];
        push_table_tag(&mut tables, M::table_name());

        for join in &self.joins {
            push_table_tag(&mut tables, &join.table);
        }

        for union in &self.unions {
            collect_tables_from_sql(&union.query_sql, &mut tables);
        }

        for cte in &self.ctes {
            collect_tables_from_sql(&cte.query_sql, &mut tables);
        }

        for (query_sql, _) in &self.subquery_select_expressions {
            collect_tables_from_sql(query_sql, &mut tables);
        }

        for condition in &self.conditions {
            collect_condition_tables(condition, &mut tables);
        }

        for group in &self.or_groups {
            collect_or_group_tables(group, &mut tables);
        }

        tables
    }

    fn generate_cache_key(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        M::table_name().hash(&mut hasher);
        self.hash_database_identity(&mut hasher);

        if let Some(key) = &self.cache_key {
            // A caller-supplied key is only unique within its own model and
            // connection, so namespace it exactly like the structural key instead
            // of using it as a bare global key.
            "tideorm::explicit-cache-key".hash(&mut hasher);
            key.hash(&mut hasher);
            let hash = hasher.finish();
            return crate::cache::QueryCache::global().generate_key(M::table_name(), hash);
        }

        for condition in &self.conditions {
            hash_where_condition(condition, &mut hasher);
        }

        for group in &self.or_groups {
            hash_or_group(group, &mut hasher);
        }

        for (column, direction) in &self.order_by {
            column.hash(&mut hasher);
            direction.as_str().hash(&mut hasher);
        }

        self.limit_value.hash(&mut hasher);
        self.offset_value.hash(&mut hasher);
        self.include_trashed.hash(&mut hasher);
        self.only_trashed.hash(&mut hasher);
        self.select_columns.hash(&mut hasher);

        for raw_select in &self.raw_select_expressions {
            raw_select.hash(&mut hasher);
        }
        for (query_sql, alias) in &self.subquery_select_expressions {
            query_sql.hash(&mut hasher);
            alias.hash(&mut hasher);
        }

        for join in &self.joins {
            join.join_type.as_sql().hash(&mut hasher);
            join.table.hash(&mut hasher);
            join.alias.hash(&mut hasher);
            join.left_column.hash(&mut hasher);
            join.right_column.hash(&mut hasher);
        }

        for column in &self.group_by {
            column.hash(&mut hasher);
        }

        for (index, having) in self.having_conditions.iter().enumerate() {
            let bindings = self
                .having_bindings
                .get(index)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            hash_having_clause(having, bindings, &mut hasher);
        }

        // `union()` and `with_query()` render their operand as *parameterized*
        // SQL, so two operands differing only in a bound value are byte-identical
        // strings: the values have to be hashed alongside the text or the two
        // queries share one cache entry.
        for union in &self.unions {
            union.query_sql.hash(&mut hasher);
            union.union_type.as_sql().hash(&mut hasher);
            hash_bound_values(&union.params, &mut hasher);
        }

        for cte in &self.ctes {
            cte.name.hash(&mut hasher);
            cte.query_sql.hash(&mut hasher);
            cte.recursive.hash(&mut hasher);
            cte.columns.hash(&mut hasher);
            hash_bound_values(&cte.params, &mut hasher);
        }

        for window_function in &self.window_functions {
            hash_window_function(window_function, &mut hasher);
        }

        let hash = hasher.finish();
        crate::cache::QueryCache::global().generate_key(M::table_name(), hash)
    }

    pub async fn get(self) -> Result<Vec<M>> {
        self.ensure_query_is_executable()?;

        // A read performed inside a transaction never touches the process-global
        // cache: the payload would outlive a rollback for the rest of its TTL, and
        // a hit would hide rows the transaction itself has already written.
        let cache_key = if self.cache_options.is_some() && !self.runs_in_transaction() {
            let key = self.generate_cache_key();
            if let Some(cached) = crate::cache::QueryCache::global().get::<Vec<M>>(&key) {
                #[cfg(feature = "dirty-tracking")]
                let _ = crate::model::__remember_dirty_snapshots(&cached);
                return Ok(cached);
            }
            Some(key)
        } else {
            None
        };

        let (sql, params) = self.build_select_sql_with_params();
        let db = self.current_db()?;
        let timer = self.start_query_log(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        let results = db
            .__raw_with_params::<M>(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context));
        Self::finish_query_log(timer, &results, |rows| rows.len() as u64);
        let results = results?;

        if let (Some(key), Some(options)) = (cache_key, &self.cache_options) {
            let _ = crate::cache::QueryCache::global().set_tagged(
                &key,
                &results,
                Some(options.ttl),
                &self.cache_tables(),
            );
        }

        Ok(results)
    }

    pub async fn first(self) -> Result<Option<M>> {
        self.ensure_query_is_executable()?;
        let results = self.limit(1).get().await?;
        Ok(results.into_iter().next())
    }

    pub async fn first_or_fail(self) -> Result<M> {
        self.first()
            .await?
            .ok_or_else(|| Error::not_found(format!("No {} found matching query", M::table_name())))
    }

    /// Process matching models in batches without loading the full result set into memory.
    ///
    /// The traversal uses the model's single-column primary key as a cursor, so callbacks may
    /// safely update or delete already-processed rows without causing later batches to skip.
    /// Existing filters, caching, and any pre-applied `limit()` remain in effect. When you need
    /// descending traversal, order explicitly by the primary key before calling `chunk()`.
    pub async fn chunk<F, Fut>(self, chunk_size: u64, mut callback: F) -> Result<()>
    where
        F: FnMut(Vec<M>) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        self.ensure_query_is_executable()?;

        if chunk_size == 0 {
            return Err(Error::invalid_query(
                "chunk() requires chunk_size to be greater than 0",
            ));
        }

        if self.offset_value.unwrap_or(0) > 0 {
            return Err(Error::invalid_query(
                "chunk() does not support offset(); use page()/get() for fixed windows or chunk over primary-key order",
            ));
        }

        let primary_key = self.chunk_primary_key_column()?;
        let order = self.chunk_order(primary_key)?;
        let mut remaining = self.limit_value;
        let mut base_query = self;
        let explicit_cache_key = base_query.cache_key.clone();
        base_query.limit_value = None;
        base_query.offset_value = None;
        if base_query.order_by.is_empty() {
            base_query = base_query.order_by(format!("{}.{}", M::table_name(), primary_key), order);
        }
        let cursor_column = format!("{}.{}", M::table_name(), primary_key);
        let mut last_seen_primary_key: Option<serde_json::Value> = None;

        loop {
            let batch_limit =
                remaining.map_or(chunk_size, |limit| std::cmp::min(limit, chunk_size));
            if batch_limit == 0 {
                break;
            }

            let mut batch_query = base_query.clone().limit(batch_limit);
            if let Some(cursor) = &last_seen_primary_key {
                batch_query = match order {
                    crate::query::Order::Asc => {
                        batch_query.where_gt(&cursor_column, cursor.clone())
                    }
                    crate::query::Order::Desc => {
                        batch_query.where_lt(&cursor_column, cursor.clone())
                    }
                };
            }
            if let Some(cache_key) = &explicit_cache_key {
                let cursor_marker = match &last_seen_primary_key {
                    Some(cursor) => serde_json::to_string(cursor).map_err(Error::from)?,
                    None => "null".to_string(),
                };
                batch_query.cache_key = Some(format!(
                    "{}::chunk(cursor={},limit={})",
                    cache_key, cursor_marker, batch_limit
                ));
            }

            let batch = batch_query.get().await?;
            if batch.is_empty() {
                break;
            }

            let batch_len = batch.len() as u64;
            let last_primary_key = batch
                .last()
                .map(Model::primary_key)
                .ok_or_else(|| Error::internal("chunk() fetched an empty batch unexpectedly"))?;
            let next_cursor = serde_json::to_value(last_primary_key).map_err(Error::from)?;
            callback(batch).await?;
            last_seen_primary_key = Some(next_cursor);

            if let Some(limit) = &mut remaining {
                *limit = limit.saturating_sub(batch_len);
                if *limit == 0 {
                    break;
                }
            }

            if batch_len < batch_limit {
                break;
            }
        }

        Ok(())
    }

    pub async fn count(self) -> Result<u64> {
        self.ensure_query_is_executable()?;

        let (sql, params) = self.build_count_sql_with_params();

        let db = self.current_db()?;
        let timer = self.start_query_log(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        let rows = db
            .__raw_json_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context));
        Self::finish_query_log(timer, &rows, |rows| rows.len() as u64);
        let rows = rows?;
        Self::decode_count_value(rows.first().and_then(|row| row.get("count")))
    }

    /// Decode the `count` column of a rendered COUNT projection.
    ///
    /// `build_count_sql_with_params` always projects exactly one `count` column
    /// over exactly one row, so a missing or non-numeric value is a decode
    /// failure and not an empty result. Reporting zero for it would turn a
    /// broken read into a plausible-looking answer.
    fn decode_count_value(value: Option<&serde_json::Value>) -> Result<u64> {
        let Some(value) = value else {
            return Err(Error::query(
                "Database returned no 'count' column for the query count",
            ));
        };

        if let Some(count) = value.as_u64() {
            Ok(count)
        } else if let Some(count) = value.as_i64() {
            crate::internal::count_to_u64(count, "query count")
        } else {
            Err(Error::query(format!(
                "Unable to decode the query count as an integer (got {})",
                value
            )))
        }
    }

    pub async fn exists(self) -> Result<bool> {
        self.ensure_query_is_executable()?;

        let (sql, params) = self.build_exists_sql_with_params();

        let db = self.current_db()?;
        let timer = self.start_query_log(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        let rows = db
            .__raw_json_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context));
        Self::finish_query_log(timer, &rows, |rows| rows.len() as u64);
        let rows = rows?;

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
    fn decode_exists_flag(value: &serde_json::Value) -> Result<bool> {
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

    fn invalidate_model_state(rows_affected: u64) {
        if rows_affected > 0 {
            crate::QueryCache::global().invalidate_model(M::table_name());
            #[cfg(feature = "dirty-tracking")]
            crate::model::__invalidate_dirty_snapshots::<M>();
        }
    }

    pub async fn delete(self) -> Result<u64> {
        self.ensure_query_is_executable()?;
        self.ensure_mutation_query_is_safe("delete")?;
        self.ensure_mutation_has_explicit_filters("delete")?;

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        Self::ensure_rendered_filter_is_restrictive("delete", &where_sql)?;
        let sql = format!("DELETE FROM {} WHERE {}", table, where_sql);

        let db = self.current_db()?;
        let timer = self.start_query_log(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        let rows_affected = db
            .__execute_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context));
        Self::finish_query_log(timer, &rows_affected, |rows| *rows);
        let rows_affected = rows_affected?;
        Self::invalidate_model_state(rows_affected);
        Ok(rows_affected)
    }

    /// Delete every row in the table represented by this query.
    ///
    /// This is an explicit opt-in escape hatch for full-table deletion and is kept
    /// separate from `delete()` so accidental unfiltered bulk deletes remain blocked.
    pub async fn delete_all(self) -> Result<u64> {
        self.ensure_query_is_executable()?;
        self.ensure_mutation_query_is_safe("delete_all")?;
        self.ensure_mutation_has_no_explicit_filters("delete_all")?;

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let sql = format!("DELETE FROM {}", table);

        let db = self.current_db()?;
        let timer = self.start_query_log(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        let rows_affected = db
            .__execute_with_params(&sql, Vec::new())
            .await
            .map_err(|err| err.with_context(error_context));
        Self::finish_query_log(timer, &rows_affected, |rows| *rows);
        let rows_affected = rows_affected?;
        Self::invalidate_model_state(rows_affected);
        Ok(rows_affected)
    }

    /// Render the soft-delete UPDATE and the values its WHERE clause binds.
    ///
    /// The stamp is rendered for `db_type` — the backend the rest of the
    /// statement is rendered for — and not for the ambient one: the literal's
    /// shape differs per backend, so a statement bound for MySQL that carried
    /// the PostgreSQL rendering would be rejected by the server.
    fn build_soft_delete_sql(&self, db_type: DatabaseType) -> Result<(String, Vec<Value>)> {
        let table = db_sql::quote_ident(db_type, M::table_name());
        let deleted_at = db_sql::quote_ident(db_type, M::deleted_at_column());
        let now = Self::current_timestamp_sql(db_type);
        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        Self::ensure_rendered_filter_is_restrictive("soft_delete", &where_sql)?;
        let sql = format!(
            "UPDATE {} SET {} = {} WHERE {}",
            table, deleted_at, now, where_sql
        );

        Ok((sql, params))
    }

    pub async fn soft_delete(self) -> Result<u64> {
        self.ensure_query_is_executable()?;
        self.ensure_mutation_query_is_safe("soft_delete")?;

        if !M::soft_delete_enabled() {
            return Err(Error::invalid_query(
                "soft_delete() can only be used on models with soft delete enabled",
            ));
        }

        self.ensure_mutation_has_explicit_filters("soft_delete")?;

        let (sql, params) = self.build_soft_delete_sql(self.db_type_for_sql())?;

        let db = self.current_db()?;
        let timer = self.start_query_log(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        let rows_affected = db
            .__execute_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context));
        Self::finish_query_log(timer, &rows_affected, |rows| *rows);
        let rows_affected = rows_affected?;
        Self::invalidate_model_state(rows_affected);
        Ok(rows_affected)
    }

    pub async fn restore(self) -> Result<u64> {
        self.ensure_query_is_executable()?;
        self.ensure_mutation_query_is_safe("restore")?;

        if !M::soft_delete_enabled() {
            return Err(Error::invalid_query(
                "restore() can only be used on models with soft delete enabled",
            ));
        }

        self.ensure_mutation_has_explicit_filters("restore")?;

        // `restore()` only ever targets trashed rows, so the scope is forced here
        // instead of relying on the caller remembering `with_trashed()`: under the
        // default active-only scope the query would carry `deleted_at IS NULL` and
        // never match anything. Routing the guard through the scope also keeps it
        // inside the rendered condition tree, rather than concatenating
        // `AND deleted_at IS NOT NULL` onto an unparenthesized body where a
        // top-level OR would bind it to the last branch only.
        let mut query = self;
        query.only_trashed = true;
        query.include_trashed = false;

        let db_type = query.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let deleted_at = db_sql::quote_ident(db_type, M::deleted_at_column());
        let (where_sql, params) = query.build_where_clause_with_condition_for_db(db_type);
        Self::ensure_rendered_filter_is_restrictive("restore", &where_sql)?;
        let sql = format!(
            "UPDATE {} SET {} = NULL WHERE {}",
            table, deleted_at, where_sql
        );

        let db = query.current_db()?;
        let timer = query.start_query_log(&sql);
        let error_context = query.build_query_error_context(Some(&sql));
        let rows_affected = db
            .__execute_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context));
        Self::finish_query_log(timer, &rows_affected, |rows| *rows);
        let rows_affected = rows_affected?;
        Self::invalidate_model_state(rows_affected);
        Ok(rows_affected)
    }

    pub async fn force_delete(self) -> Result<u64> {
        self.ensure_query_is_executable()?;
        self.ensure_mutation_query_is_safe("force_delete")?;
        self.ensure_mutation_has_explicit_filters("force_delete")?;

        // `force_delete()` exists precisely to reach rows the default active-only
        // scope hides, so trashed rows are put back in scope regardless of what
        // the caller asked for.
        let mut query = self;
        query.include_trashed = true;
        query.only_trashed = false;

        let db_type = query.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let (where_sql, params) = query.build_where_clause_with_condition_for_db(db_type);
        Self::ensure_rendered_filter_is_restrictive("force_delete", &where_sql)?;
        let sql = format!("DELETE FROM {} WHERE {}", table, where_sql);

        let db = query.current_db()?;
        let timer = query.start_query_log(&sql);
        let error_context = query.build_query_error_context(Some(&sql));
        let rows_affected = db
            .__execute_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context));
        Self::finish_query_log(timer, &rows_affected, |rows| *rows);
        let rows_affected = rows_affected?;
        Self::invalidate_model_state(rows_affected);
        Ok(rows_affected)
    }

    pub async fn get_json(self) -> Result<Vec<serde_json::Value>> {
        self.ensure_query_is_executable()?;
        let (sql, params) = self.build_select_sql_with_params();
        let db = self.current_db()?;
        let timer = self.start_query_log(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        let rows = db
            .__raw_json_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context));
        Self::finish_query_log(timer, &rows, |rows| rows.len() as u64);
        rows
    }
}

/// The process-wide identities handed out by [`connection_identity`].
static CONNECTION_IDENTITIES: OnceLock<Mutex<ConnectionIdentities>> = OnceLock::new();

/// The identity assigned to every pooled connection a cache key has named.
#[derive(Default)]
struct ConnectionIdentities {
    /// The last identity handed out. Identities are never reused within a
    /// process, so a stale cache entry can never be mistaken for a live one.
    last_id: u64,
    /// Address of the connection's `Arc` allocation to its identity, alongside a
    /// `Weak` to that allocation.
    assigned: HashMap<usize, (Weak<InternalConnection>, u64)>,
}

impl ConnectionIdentities {
    /// The identity of `connection`, assigning one if it has none yet.
    ///
    /// The map is keyed by address, which is only trustworthy because of the
    /// `Weak` stored beside it: a `Weak` keeps its `Arc` allocation reserved even
    /// after the connection itself is dropped, so no second connection can ever
    /// be allocated at the address of an entry that is still on record. Dead
    /// entries are dropped before a new identity is handed out — that releases
    /// the address *and* the mapping together, so a connection that later lands
    /// there misses this map and is issued a fresh identity rather than
    /// inheriting the dropped connection's cached rows.
    fn identify(&mut self, connection: &Arc<InternalConnection>) -> u64 {
        let address = Arc::as_ptr(connection) as usize;
        if let Some((_, id)) = self.assigned.get(&address) {
            return *id;
        }

        self.assigned
            .retain(|_, (tracked, _)| tracked.strong_count() > 0);
        self.last_id += 1;
        self.assigned
            .insert(address, (Arc::downgrade(connection), self.last_id));
        self.last_id
    }
}

/// A process-unique identity for a pooled connection.
///
/// Stable for as long as the connection lives and never handed to another
/// connection afterwards, which is what a cache key needs: the address the
/// connection happens to occupy is neither.
fn connection_identity(connection: &Arc<InternalConnection>) -> u64 {
    CONNECTION_IDENTITIES
        .get_or_init(|| Mutex::new(ConnectionIdentities::default()))
        .lock()
        .identify(connection)
}

/// Record `table` as a cache tag, ignoring duplicates and unusable names.
fn push_table_tag(tables: &mut Vec<String>, table: &str) {
    let Some(table) = normalize_table_name(table) else {
        return;
    };

    if !tables.contains(&table) {
        tables.push(table);
    }
}

/// Reduce a rendered table reference to the bare name used as a cache tag.
///
/// Handles the quoting styles the SQL builders emit (`"users"`, `` `users` ``,
/// `[users]`) and drops any schema qualifier. Anything that is not a plain
/// identifier is rejected so keywords and expressions never become tags.
fn normalize_table_name(token: &str) -> Option<String> {
    const QUOTES: [char; 4] = ['"', '`', '[', ']'];
    let is_quote = |character: char| QUOTES.contains(&character);

    let qualified = token.trim_matches(|character| is_quote(character) || character == ';');
    let name = qualified
        .rsplit_once('.')
        .map_or(qualified, |(_, table)| table)
        .trim_matches(is_quote);

    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
    {
        return None;
    }

    Some(name.to_string())
}

/// Pull table names out of rendered SQL by reading the identifier that follows
/// each `FROM`/`JOIN` keyword.
///
/// This is a fallback, not the intended mechanism: joins report their table
/// directly off `JoinClause`, but union, CTE, and subquery operands only survive
/// as SQL text and have nothing structured left to ask. Over-collecting is
/// harmless — a spurious tag just makes invalidation more eager — while
/// under-collecting would keep serving stale rows, so every identifier-shaped
/// token is kept.
fn collect_tables_from_sql(sql: &str, tables: &mut Vec<String>) {
    const NOT_A_TABLE: [&str; 5] = ["select", "lateral", "only", "unnest", "values"];

    let is_separator =
        |character: char| character.is_whitespace() || matches!(character, ',' | '(' | ')');

    let mut expect_table = false;
    for token in sql.split(is_separator) {
        if token.is_empty() {
            continue;
        }

        if token.eq_ignore_ascii_case("from") || token.eq_ignore_ascii_case("join") {
            expect_table = true;
            continue;
        }

        if !expect_table {
            continue;
        }
        expect_table = false;

        if NOT_A_TABLE
            .into_iter()
            .any(|keyword| token.eq_ignore_ascii_case(keyword))
        {
            continue;
        }

        push_table_tag(tables, token);
    }
}

/// Collect the tables named inside a condition's SQL-carrying operands.
fn collect_condition_tables(condition: &crate::query::WhereCondition, tables: &mut Vec<String>) {
    match &condition.value {
        crate::query::ConditionValue::Subquery(query_sql)
        | crate::query::ConditionValue::RawExpr(query_sql) => {
            collect_tables_from_sql(query_sql, tables);
        }
        crate::query::ConditionValue::RawExprWithValues { preview_sql, .. } => {
            collect_tables_from_sql(preview_sql, tables);
        }
        _ => {}
    }
}

/// Collect the tables named inside an OR group and everything nested under it.
fn collect_or_group_tables(group: &crate::query::OrGroup, tables: &mut Vec<String>) {
    for condition in &group.conditions {
        collect_condition_tables(condition, tables);
    }

    for nested in &group.nested_groups {
        collect_or_group_tables(nested, tables);
    }
}

#[cfg(test)]
mod tests {
    use crate::config::DatabaseType;
    use crate::internal::InternalConnection;
    use crate::model::Model;
    use crate::query::{
        ConditionValue, FrameBound, FrameType, Operator, Order, QueryBuilder, WhereCondition,
        WindowFunction, WindowFunctionType,
    };
    use std::sync::Arc;

    #[tideorm::model(table = "cache_key_test_users")]
    struct CacheKeyTestUser {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
    }

    #[tideorm::model(table = "cache_key_test_posts")]
    struct CacheKeyTestPost {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        title: String,
    }

    #[tideorm::model(table = "cache_key_test_soft_delete_users", soft_delete)]
    struct CacheKeyTestSoftDeleteUser {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
        deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    }

    #[test]
    fn test_generate_cache_key_is_stable_for_equivalent_structured_queries() {
        let query_one = CacheKeyTestUser::query()
            .where_in("status", vec!["active", "pending"])
            .or_where(|group| {
                group
                    .where_eq("role", "admin")
                    .nested_and(|inner| inner.where_gt("score", 10).where_lt("score", 20))
            })
            .window(
                WindowFunction::new(
                    WindowFunctionType::Lag("score".to_string(), Some(1), Some("0".to_string())),
                    "previous_score",
                )
                .partition_by("team")
                .order_by("score", Order::Desc)
                .frame(
                    FrameType::Rows,
                    FrameBound::UnboundedPreceding,
                    FrameBound::CurrentRow,
                ),
            )
            .limit(10);

        let query_two = CacheKeyTestUser::query()
            .where_in("status", vec!["active", "pending"])
            .or_where(|group| {
                group
                    .where_eq("role", "admin")
                    .nested_and(|inner| inner.where_gt("score", 10).where_lt("score", 20))
            })
            .window(
                WindowFunction::new(
                    WindowFunctionType::Lag("score".to_string(), Some(1), Some("0".to_string())),
                    "previous_score",
                )
                .partition_by("team")
                .order_by("score", Order::Desc)
                .frame(
                    FrameType::Rows,
                    FrameBound::UnboundedPreceding,
                    FrameBound::CurrentRow,
                ),
            )
            .limit(10);

        assert_eq!(
            query_one.generate_cache_key(),
            query_two.generate_cache_key()
        );
    }

    #[test]
    fn test_generate_cache_key_changes_when_window_definition_changes() {
        let baseline = CacheKeyTestUser::query().window(
            WindowFunction::new(WindowFunctionType::Rank, "rank_alias")
                .order_by("score", Order::Desc),
        );
        let changed = CacheKeyTestUser::query().window(
            WindowFunction::new(WindowFunctionType::DenseRank, "rank_alias")
                .order_by("score", Order::Desc),
        );

        assert_ne!(baseline.generate_cache_key(), changed.generate_cache_key());
    }

    #[test]
    fn test_explicit_cache_key_is_namespaced_per_model() {
        let ttl = std::time::Duration::from_secs(60);
        let user_query = CacheKeyTestUser::query().cache_with_key("recent", ttl);
        let post_query = CacheKeyTestPost::query().cache_with_key("recent", ttl);

        assert_ne!(
            user_query.generate_cache_key(),
            post_query.generate_cache_key()
        );
    }

    #[test]
    fn test_rendered_filter_guard_rejects_constant_true_bodies() {
        type Guard = QueryBuilder<CacheKeyTestUser>;

        for body in ["", "TRUE", "true", "1 = 1", "(TRUE)"] {
            let err = Guard::ensure_rendered_filter_is_restrictive("delete", body).unwrap_err();
            let message = err.to_string();
            assert!(
                message.contains("unfiltered bulk mutations are blocked"),
                "body: {body}"
            );
        }

        let restrictive = Guard::ensure_rendered_filter_is_restrictive("delete", "\"id\" = $1");
        assert!(restrictive.is_ok());
    }

    #[test]
    fn test_empty_negative_list_does_not_count_as_an_explicit_filter() {
        // An empty candidate set for a negative membership test renders
        // constant-true, so it must not satisfy the explicit-filter requirement.
        // Reaching this by accident is easy -- a filter list that came back empty
        // from a form -- and the rendered-SQL check cannot catch it: sea-query
        // emits an empty `NOT IN` as the bound pair `? = ?`, and a soft-delete
        // model appends `deleted_at IS NULL` to whatever the caller declared.
        for query in [
            CacheKeyTestUser::query().where_not_in("id", Vec::<i64>::new()),
            CacheKeyTestUser::query().ne_all("name", Vec::<&str>::new()),
            CacheKeyTestUser::query()
                .ne_all("name", Vec::<&str>::new())
                .where_not_in("id", Vec::<i64>::new()),
        ] {
            let err = query
                .ensure_mutation_has_explicit_filters("delete")
                .unwrap_err();
            assert!(
                err.to_string()
                    .contains("unfiltered bulk mutations are blocked"),
                "vacuous filter was accepted: {err}"
            );
        }
    }

    #[test]
    fn test_a_real_filter_still_counts_alongside_a_vacuous_one() {
        // The guard rejects only queries where *every* declared filter is
        // vacuous; one real predicate is enough, and it still applies.
        assert!(
            CacheKeyTestUser::query()
                .where_eq("name", "alice")
                .where_not_in("id", Vec::<i64>::new())
                .ensure_mutation_has_explicit_filters("delete")
                .is_ok()
        );

        // A non-empty candidate set is a real filter on its own.
        assert!(
            CacheKeyTestUser::query()
                .where_not_in("id", vec![1i64])
                .ensure_mutation_has_explicit_filters("delete")
                .is_ok()
        );

        // The positive duals render constant-FALSE, which matches nothing and is
        // safe for a mutation, so they are deliberately still accepted.
        assert!(
            CacheKeyTestUser::query()
                .where_in("id", Vec::<i64>::new())
                .ensure_mutation_has_explicit_filters("delete")
                .is_ok()
        );
    }

    #[test]
    fn test_cache_tags_cover_joined_tables() {
        let tables = CacheKeyTestPost::query()
            .inner_join(
                "cache_key_test_users",
                "cache_key_test_posts.id",
                "cache_key_test_users.id",
            )
            .cache_tables();

        assert_eq!(
            tables,
            vec![
                "cache_key_test_posts".to_string(),
                "cache_key_test_users".to_string()
            ]
        );
    }

    #[test]
    fn test_write_to_joined_table_evicts_cached_join_result() {
        let cache = crate::cache::QueryCache::new();
        cache.enable();

        let joined = CacheKeyTestPost::query().inner_join(
            "cache_key_test_users",
            "cache_key_test_posts.id",
            "cache_key_test_users.id",
        );
        let joined_key = joined.generate_cache_key();
        cache
            .set_tagged(&joined_key, &["row"], None, &joined.cache_tables())
            .unwrap();

        let posts_only = CacheKeyTestPost::query();
        let posts_only_key = posts_only.generate_cache_key();
        cache
            .set_tagged(&posts_only_key, &["row"], None, &posts_only.cache_tables())
            .unwrap();

        assert!(cache.contains(&joined_key));
        assert!(cache.contains(&posts_only_key));

        cache.invalidate_model("cache_key_test_users");

        assert!(
            !cache.contains(&joined_key),
            "a write to a joined table must evict the cached join result"
        );
        assert!(
            cache.contains(&posts_only_key),
            "invalidation must stay targeted at the tables a query actually reads"
        );
    }

    #[test]
    fn test_cache_tags_cover_union_operand_tables() {
        let tables = CacheKeyTestPost::query()
            .union(CacheKeyTestUser::query())
            .cache_tables();

        assert!(
            tables.contains(&"cache_key_test_users".to_string()),
            "a union operand's table must be tagged for invalidation: {tables:?}"
        );
    }

    #[test]
    fn test_collect_tables_from_sql_reads_from_and_join_targets() {
        let mut tables = Vec::new();
        super::collect_tables_from_sql(
            "SELECT * FROM \"orders\" INNER JOIN `line_items` ON a = b \
             WHERE id IN (SELECT id FROM public.archived_orders)",
            &mut tables,
        );

        assert_eq!(
            tables,
            vec![
                "orders".to_string(),
                "line_items".to_string(),
                "archived_orders".to_string()
            ]
        );
    }

    #[test]
    fn test_collect_tables_from_sql_skips_derived_table_keywords() {
        let mut tables = Vec::new();
        super::collect_tables_from_sql("SELECT * FROM (SELECT * FROM \"users\") t", &mut tables);

        assert_eq!(tables, vec!["users".to_string()]);
    }

    #[test]
    fn test_exists_flag_decoding_reports_undecodable_values() {
        type Decoder = QueryBuilder<CacheKeyTestUser>;

        assert!(Decoder::decode_exists_flag(&serde_json::json!(true)).unwrap());
        assert!(!Decoder::decode_exists_flag(&serde_json::json!(false)).unwrap());
        assert!(Decoder::decode_exists_flag(&serde_json::json!(1)).unwrap());
        assert!(!Decoder::decode_exists_flag(&serde_json::json!(0)).unwrap());

        // An EXISTS query always returns one row, so guessing `true` from "a row
        // came back" would make every undecodable result a false positive.
        let err = Decoder::decode_exists_flag(&serde_json::json!(null)).unwrap_err();
        assert!(
            err.to_string().contains("Unable to decode"),
            "an undecodable EXISTS result must be reported: {err}"
        );
    }

    #[test]
    fn test_count_decoding_reports_missing_and_undecodable_values() {
        type Decoder = QueryBuilder<CacheKeyTestUser>;

        let seven = serde_json::json!(7);
        assert_eq!(Decoder::decode_count_value(Some(&seven)).unwrap(), 7);

        let missing = Decoder::decode_count_value(None).unwrap_err();
        assert!(
            missing.to_string().contains("no 'count' column"),
            "{missing}"
        );

        let text = serde_json::json!("many");
        let undecodable = Decoder::decode_count_value(Some(&text)).unwrap_err();
        assert!(
            undecodable.to_string().contains("Unable to decode"),
            "{undecodable}"
        );

        let negative_value = serde_json::json!(-1);
        let negative = Decoder::decode_count_value(Some(&negative_value)).unwrap_err();
        assert!(
            negative.to_string().contains("negative count"),
            "{negative}"
        );
    }

    #[test]
    fn test_union_bound_values_participate_in_the_cache_key() {
        let tenant_one =
            CacheKeyTestPost::query().union(CacheKeyTestUser::query().where_eq("id", 1));
        let tenant_two =
            CacheKeyTestPost::query().union(CacheKeyTestUser::query().where_eq("id", 2));

        assert_eq!(
            tenant_one.unions[0].query_sql, tenant_two.unions[0].query_sql,
            "a parameterized union operand renders the same SQL for either bound value"
        );
        assert_ne!(
            tenant_one.generate_cache_key(),
            tenant_two.generate_cache_key(),
            "two unions differing only in a bound value must not share a cache entry"
        );
    }

    #[test]
    fn test_cte_bound_values_participate_in_the_cache_key() {
        let tenant_one = CacheKeyTestPost::query()
            .with_query("tenant_users", CacheKeyTestUser::query().where_eq("id", 1));
        let tenant_two = CacheKeyTestPost::query()
            .with_query("tenant_users", CacheKeyTestUser::query().where_eq("id", 2));

        assert_eq!(
            tenant_one.ctes[0].query_sql, tenant_two.ctes[0].query_sql,
            "a parameterized CTE body renders the same SQL for either bound value"
        );
        assert_ne!(
            tenant_one.generate_cache_key(),
            tenant_two.generate_cache_key(),
            "two CTE bodies differing only in a bound value must not share a cache entry"
        );
    }

    /// A connection that never dialled anything, for identity bookkeeping only.
    fn disconnected_connection() -> Arc<InternalConnection> {
        Arc::new(InternalConnection {
            conn: Default::default(),
        })
    }

    #[test]
    fn test_connection_identity_is_stable_for_one_connection() {
        let connection = disconnected_connection();
        let same_connection = Arc::clone(&connection);

        assert_eq!(
            super::connection_identity(&connection),
            super::connection_identity(&same_connection),
            "one connection must keep one identity, or its own cache entries never hit"
        );
    }

    #[test]
    fn test_connection_identity_is_never_inherited_from_a_recycled_address() {
        let first = disconnected_connection();
        let first_address = Arc::as_ptr(&first) as usize;
        let first_identity = super::connection_identity(&first);
        drop(first);

        // A batch of identically-sized allocations is exactly what lands on a
        // just-freed block, which is how hashing `Arc::as_ptr` served one
        // tenant's cached rows to the next tenant to connect.
        let reopened: Vec<_> = (0..16).map(|_| disconnected_connection()).collect();

        for connection in &reopened {
            assert_ne!(
                Arc::as_ptr(connection) as usize,
                first_address,
                "an identity on record must keep its connection's address reserved"
            );
            assert_ne!(
                super::connection_identity(connection),
                first_identity,
                "a new connection must never inherit a dropped connection's cache identity"
            );
        }
    }

    /// The soft-delete stamp is a literal, so it has to be spelled the way the
    /// backend the statement is heading for spells it. Rendering it for the
    /// ambient backend instead put an RFC3339 literal — `T` separator, UTC
    /// offset — into statements bound for MySQL, which rejects both.
    #[test]
    fn test_soft_delete_stamp_renders_for_the_statement_backend() {
        // The stamp is the only quoted token in the statement: every filter
        // value travels beside it as a bound parameter.
        fn stamp_of(sql: &str) -> &str {
            sql.split('\'')
                .nth(1)
                .expect("the soft-delete statement embeds a quoted timestamp literal")
        }

        let query = CacheKeyTestSoftDeleteUser::query().where_eq("id", 1);

        let (mysql_sql, _) = query
            .build_soft_delete_sql(DatabaseType::MySQL)
            .expect("a filtered soft delete renders");
        let mysql_stamp = stamp_of(&mysql_sql);
        assert!(!mysql_stamp.contains('T'), "mysql stamp: {mysql_stamp}");
        assert!(!mysql_stamp.contains('+'), "mysql stamp: {mysql_stamp}");

        let (postgres_sql, _) = query
            .build_soft_delete_sql(DatabaseType::Postgres)
            .expect("a filtered soft delete renders");
        let postgres_stamp = stamp_of(&postgres_sql);
        assert!(
            postgres_stamp.contains('T'),
            "postgres stamp: {postgres_stamp}"
        );
        assert!(
            postgres_stamp.ends_with("+00:00"),
            "postgres stamp: {postgres_stamp}"
        );
    }

    #[test]
    fn test_unrepresentable_condition_is_rejected_instead_of_dropped() {
        let mut query = CacheKeyTestUser::query();
        query.conditions.push(WhereCondition {
            column: "id".to_string(),
            operator: Operator::Between,
            value: ConditionValue::Single(serde_json::json!(1)),
        });

        let err = query.ensure_conditions_are_representable().unwrap_err();
        assert!(err.to_string().contains("cannot be rendered as SQL"));
    }
}
