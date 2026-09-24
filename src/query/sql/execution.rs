use super::*;

use crate::internal::InternalConnection;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, OnceLock, Weak};

mod hash_helpers;
mod mutation_safety;

use hash_helpers::{hash_bound_values, hash_having_clause, hash_or_group, hash_where_condition};

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

    /// Cache this query's models for `ttl` in the global [`QueryCache`](crate::QueryCache).
    ///
    /// The cache starts disabled: until `QueryCache::global().enable()` or
    /// `QueryCache::init_global(..)` turns it on, nothing is cached, and the
    /// first such query logs a warning saying so.
    ///
    /// A TideORM write drops the cached reads of its table. Inside a
    /// transaction it drops them again once the transaction commits, since
    /// other requests read, and may cache, the replaced rows until then, and a
    /// read that a write overtook is not cached at all. Reads inside a
    /// transaction, and locked reads, bypass the cache. Writes TideORM does not
    /// make, from another process or straight through the driver, are only
    /// seen once the TTL expires.
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
    pub(crate) fn ensure_query_is_executable(&self) -> Result<()> {
        self.ensure_query_is_valid()?;
        self.ensure_conditions_are_representable()
    }

    /// Run `statement` with query logging, attaching this query's error context
    /// to a failure.
    async fn logged<T>(
        &self,
        sql: &str,
        statement: impl Future<Output = Result<T>>,
        row_count: impl FnOnce(&T) -> u64,
    ) -> Result<T> {
        let timer = self.start_query_log(sql);
        let result = crate::logging::logged_by_caller(statement)
            .await
            .map_err(|err| err.with_context(self.build_query_error_context(Some(sql))));
        Self::finish_query_log(timer, &result, row_count);
        result
    }

    /// Run a row-returning statement and hand the rows back as JSON objects,
    /// with `M`'s own columns decoded as `M` reads them.
    pub(in crate::query) async fn fetch_json(
        &self,
        sql: &str,
        params: Vec<Value>,
    ) -> Result<Vec<serde_json::Value>> {
        let db = self.current_db()?;
        let rows = db.__raw_json_typed(sql, params, crate::internal::column_type_of::<M>);
        self.logged(sql, rows, |rows| rows.len() as u64).await
    }

    /// Run a mutation and drop every cached result it may have made stale.
    async fn run_mutation(&self, sql: &str, params: Vec<Value>) -> Result<u64> {
        let db = self.current_db()?;
        let rows_affected = self
            .logged(sql, db.__execute_with_params(sql, params), |rows| *rows)
            .await?;
        Self::invalidate_model_state(rows_affected);
        Ok(rows_affected)
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
            database.__get_connection(),
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

        for subquery in &self.subquery_select_expressions {
            collect_tables_from_sql(&subquery.query_sql, &mut tables);
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

        self.order_by.hash(&mut hasher);
        self.limit_value.hash(&mut hasher);
        self.offset_value.hash(&mut hasher);
        self.include_trashed.hash(&mut hasher);
        self.only_trashed.hash(&mut hasher);
        self.select_columns.hash(&mut hasher);
        self.raw_select_expressions.hash(&mut hasher);

        // `select_subquery()`, `union()` and `with_query()` render their operand
        // as *parameterized* SQL, so two operands differing only in a bound value
        // are byte-identical strings: the values have to be hashed alongside the
        // text or the two queries share one cache entry.
        for subquery in &self.subquery_select_expressions {
            subquery.query_sql.hash(&mut hasher);
            subquery.alias.hash(&mut hasher);
            hash_bound_values(&subquery.params, &mut hasher);
        }

        self.joins.hash(&mut hasher);
        self.group_by.hash(&mut hasher);

        for (having, bindings) in self.having_clauses() {
            hash_having_clause(having, bindings, &mut hasher);
        }

        for union in &self.unions {
            union.query_sql.hash(&mut hasher);
            union.union_type.hash(&mut hasher);
            hash_bound_values(&union.params, &mut hasher);
        }

        for cte in &self.ctes {
            cte.name.hash(&mut hasher);
            cte.query_sql.hash(&mut hasher);
            cte.recursive.hash(&mut hasher);
            cte.columns.hash(&mut hasher);
            hash_bound_values(&cte.params, &mut hasher);
        }

        self.window_functions.hash(&mut hasher);

        let hash = hasher.finish();
        crate::cache::QueryCache::global().generate_key(M::table_name(), hash)
    }

    pub async fn get(self) -> Result<Vec<M>> {
        self.ensure_query_is_executable()?;
        self.ensure_projection_covers_model()?;

        // A read performed inside a transaction never touches the process-global
        // cache: the payload would outlive a rollback for the rest of its TTL, and
        // a hit would hide rows the transaction itself has already written.
        let cache_fill =
            if self.cache_options.is_some() && !self.lock_for_update && !self.runs_in_transaction()
            {
                let cache = crate::cache::QueryCache::global();
                cache.warn_once_if_disabled();
                let key = self.generate_cache_key();
                if let Some(cached) = cache.get::<Vec<M>>(&key) {
                    #[cfg(feature = "dirty-tracking")]
                    crate::model::__remember_dirty_snapshots(&cached);
                    return Ok(cached);
                }
                // Taken before the read: a write that lands while it runs may
                // replace the rows it returns.
                Some((key, cache.fill_point()))
            } else {
                None
            };

        let (sql, params) = self.build_select_sql_with_params();
        let db = self.current_db()?;
        let results = self
            .logged(&sql, db.__raw_with_params::<M>(&sql, params), |rows| {
                rows.len() as u64
            })
            .await?;

        if let (Some((key, fill_point)), Some(options)) = (cache_fill, &self.cache_options) {
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
    pub async fn first(self) -> Result<Option<M>> {
        let results = self.limit(1).get().await?;
        Ok(results.into_iter().next())
    }

    /// [`first()`](Self::first), with a not-found error when nothing matches.
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

    /// Count the rows the query matches.
    ///
    /// Ordering, `limit()` and `offset()` are ignored, so a paged query counts
    /// its total across every page. The aggregates honour them.
    pub async fn count(self) -> Result<u64> {
        self.ensure_query_is_executable()?;

        let (sql, params) = self.build_count_sql_with_params();
        let rows = self.fetch_json(&sql, params).await?;
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

    pub async fn exists(self) -> Result<bool> {
        self.ensure_query_is_executable()?;

        let (sql, params) = self.build_exists_sql_with_params();
        let rows = self.fetch_json(&sql, params).await?;

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

    /// Drop the cached reads and dirty-tracking snapshots a write to this
    /// model's table has made stale.
    pub(crate) fn invalidate_model_state(rows_affected: u64) {
        if rows_affected > 0 {
            crate::QueryCache::global().invalidate_model(M::table_name());
            #[cfg(feature = "dirty-tracking")]
            crate::model::__invalidate_dirty_snapshots::<M>();
        }
    }

    /// Render `DELETE FROM <table> WHERE ..` for this query's filters.
    fn build_delete_sql(&self, operation: &str) -> Result<(String, Vec<Value>)> {
        let db_type = self.db_type_for_sql();
        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        Self::ensure_rendered_filter_is_restrictive(operation, &where_sql)?;
        let sql = format!(
            "DELETE FROM {} WHERE {}",
            db_sql::quote_table::<M>(db_type),
            where_sql
        );

        Ok((sql, params))
    }

    pub async fn delete(self) -> Result<u64> {
        self.ensure_query_is_executable()?;
        self.ensure_mutation_query_is_safe("delete")?;
        self.ensure_mutation_has_explicit_filters("delete")?;

        let (sql, params) = self.build_delete_sql("delete")?;
        self.run_mutation(&sql, params).await
    }

    /// Delete every row in the table represented by this query.
    ///
    /// This is an explicit opt-in escape hatch for full-table deletion and is kept
    /// separate from `delete()` so accidental unfiltered bulk deletes remain blocked.
    pub async fn delete_all(self) -> Result<u64> {
        self.ensure_query_is_executable()?;
        self.ensure_mutation_query_is_safe("delete_all")?;
        self.ensure_mutation_has_no_explicit_filters("delete_all")?;

        let sql = format!(
            "DELETE FROM {}",
            db_sql::quote_table::<M>(self.db_type_for_sql())
        );
        self.run_mutation(&sql, Vec::new()).await
    }

    /// Render the soft-delete UPDATE and the values its WHERE clause binds.
    ///
    /// The stamp is rendered for `db_type` — the backend the rest of the
    /// statement is rendered for — and not for the ambient one: the literal's
    /// shape differs per backend, so a statement bound for MySQL that carried
    /// the PostgreSQL rendering would be rejected by the server.
    fn build_soft_delete_sql(&self, db_type: DatabaseType) -> Result<(String, Vec<Value>)> {
        let table = db_sql::quote_table::<M>(db_type);
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
        self.run_mutation(&sql, params).await
    }

    pub async fn restore(self) -> Result<u64> {
        self.ensure_query_is_executable()?;
        self.ensure_mutation_query_is_safe("restore")?;

        if !M::soft_delete_enabled() {
            return Err(Error::invalid_query(
                "restore() can only be used on models with soft delete enabled",
            ));
        }

        self.ensure_trash_mutation_has_filters("restore")?;

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
        let (where_sql, params) = query.build_where_clause_with_condition_for_db(db_type);
        Self::ensure_rendered_filter_is_restrictive("restore", &where_sql)?;
        let sql = format!(
            "UPDATE {} SET {} = NULL WHERE {}",
            db_sql::quote_table::<M>(db_type),
            db_sql::quote_ident(db_type, M::deleted_at_column()),
            where_sql
        );

        query.run_mutation(&sql, params).await
    }

    pub async fn force_delete(self) -> Result<u64> {
        self.ensure_query_is_executable()?;
        self.ensure_mutation_query_is_safe("force_delete")?;
        self.ensure_trash_mutation_has_filters("force_delete")?;

        // `force_delete()` exists precisely to reach rows the default active-only
        // scope hides, so trashed rows are put back in scope — unless the caller
        // asked for trashed rows only, which must not widen to live ones.
        let mut query = self;
        if !query.only_trashed {
            query.include_trashed = true;
        }

        let (sql, params) = query.build_delete_sql("force_delete")?;
        query.run_mutation(&sql, params).await
    }

    pub async fn get_json(self) -> Result<Vec<serde_json::Value>> {
        self.ensure_query_is_executable()?;
        let (sql, params) = self.build_select_sql_with_params();
        self.fetch_json(&sql, params).await
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
        crate::query::ConditionValue::RawExpr(query_sql) => {
            collect_tables_from_sql(query_sql, tables);
        }
        crate::query::ConditionValue::RawExprWithValues { sql, .. } => {
            collect_tables_from_sql(sql, tables);
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
#[path = "../../../tests/unit/query_execution_tests.rs"]
mod tests;
