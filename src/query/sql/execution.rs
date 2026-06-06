use super::*;

mod hash_helpers;
mod mutation_safety;

use hash_helpers::{hash_having_clause, hash_or_group, hash_where_condition, hash_window_function};

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

    fn generate_cache_key(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        if let Some(key) = &self.cache_key {
            return key.clone();
        }

        let mut hasher = DefaultHasher::new();
        M::table_name().hash(&mut hasher);

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

        for union in &self.unions {
            union.query_sql.hash(&mut hasher);
            union.union_type.as_sql().hash(&mut hasher);
        }

        for cte in &self.ctes {
            cte.name.hash(&mut hasher);
            cte.query_sql.hash(&mut hasher);
            cte.recursive.hash(&mut hasher);
            cte.columns.hash(&mut hasher);
        }

        for window_function in &self.window_functions {
            hash_window_function(window_function, &mut hasher);
        }

        let hash = hasher.finish();
        crate::cache::QueryCache::global().generate_key(M::table_name(), hash)
    }

    pub async fn get(self) -> Result<Vec<M>> {
        self.ensure_query_is_valid()?;

        let cache_key = if self.cache_options.is_some() {
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
        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        let results = self
            .current_db()?
            .__raw_with_params::<M>(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context))?;

        if let (Some(key), Some(options)) = (cache_key, &self.cache_options) {
            let _ = crate::cache::QueryCache::global().set(
                &key,
                &results,
                Some(options.ttl),
                M::table_name(),
            );
        }

        Ok(results)
    }

    pub async fn first(self) -> Result<Option<M>> {
        self.ensure_query_is_valid()?;
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
        self.ensure_query_is_valid()?;

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
        self.ensure_query_is_valid()?;

        let (sql, params) = self.build_count_sql_with_params();

        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        let rows = self
            .current_db()?
            .__raw_json_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context))?;
        let count = rows
            .first()
            .and_then(|row| row.get("count"))
            .map(|value| {
                if let Some(count) = value.as_u64() {
                    Ok(count)
                } else if let Some(count) = value.as_i64() {
                    crate::internal::count_to_u64(count, "query count")
                } else {
                    Ok(0)
                }
            })
            .transpose()?
            .unwrap_or(0);

        Ok(count)
    }

    pub async fn exists(self) -> Result<bool> {
        self.ensure_query_is_valid()?;

        let (sql, params) = self.build_exists_sql_with_params();

        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        let rows = self
            .current_db()?
            .__raw_json_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context))?;

        let exists_value = rows.first().and_then(|row| row.get("exists_result"));
        if let Some(value) = exists_value {
            if let Some(exists) = value.as_bool() {
                return Ok(exists);
            }
            if let Some(exists) = value.as_u64() {
                return Ok(exists != 0);
            }
            if let Some(exists) = value.as_i64() {
                return Ok(exists != 0);
            }
        }

        Ok(!rows.is_empty())
    }

    fn invalidate_model_state(rows_affected: u64) {
        if rows_affected > 0 {
            crate::QueryCache::global().invalidate_model(M::table_name());
            #[cfg(feature = "dirty-tracking")]
            crate::model::__invalidate_dirty_snapshots::<M>();
        }
    }

    pub async fn delete(self) -> Result<u64> {
        self.ensure_query_is_valid()?;
        self.ensure_mutation_query_is_safe("delete")?;
        self.ensure_mutation_has_explicit_filters("delete")?;

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        let sql = if where_sql.is_empty() {
            format!("DELETE FROM {}", table)
        } else {
            format!("DELETE FROM {} WHERE {}", table, where_sql)
        };

        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        let rows_affected = self
            .current_db()?
            .__execute_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context))?;
        Self::invalidate_model_state(rows_affected);
        Ok(rows_affected)
    }

    /// Delete every row in the table represented by this query.
    ///
    /// This is an explicit opt-in escape hatch for full-table deletion and is kept
    /// separate from `delete()` so accidental unfiltered bulk deletes remain blocked.
    pub async fn delete_all(self) -> Result<u64> {
        self.ensure_query_is_valid()?;
        self.ensure_mutation_query_is_safe("delete_all")?;
        self.ensure_mutation_has_no_explicit_filters("delete_all")?;

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let sql = format!("DELETE FROM {}", table);

        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        let rows_affected = self
            .current_db()?
            .__execute_with_params(&sql, Vec::new())
            .await
            .map_err(|err| err.with_context(error_context))?;
        Self::invalidate_model_state(rows_affected);
        Ok(rows_affected)
    }

    pub async fn soft_delete(self) -> Result<u64> {
        self.ensure_query_is_valid()?;
        self.ensure_mutation_query_is_safe("soft_delete")?;

        if !M::soft_delete_enabled() {
            return Err(Error::invalid_query(
                "soft_delete() can only be used on models with soft delete enabled",
            ));
        }

        self.ensure_mutation_has_explicit_filters("soft_delete")?;

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let deleted_at = db_sql::quote_ident(db_type, M::deleted_at_column());
        let now = Self::current_timestamp_sql();
        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        let sql = if where_sql.is_empty() {
            format!("UPDATE {} SET {} = {}", table, deleted_at, now)
        } else {
            format!(
                "UPDATE {} SET {} = {} WHERE {}",
                table, deleted_at, now, where_sql
            )
        };

        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        let rows_affected = self
            .current_db()?
            .__execute_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context))?;
        Self::invalidate_model_state(rows_affected);
        Ok(rows_affected)
    }

    pub async fn restore(self) -> Result<u64> {
        self.ensure_query_is_valid()?;
        self.ensure_mutation_query_is_safe("restore")?;

        if !M::soft_delete_enabled() {
            return Err(Error::invalid_query(
                "restore() can only be used on models with soft delete enabled",
            ));
        }

        self.ensure_mutation_has_explicit_filters("restore")?;

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let deleted_at = db_sql::quote_ident(db_type, M::deleted_at_column());
        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        let sql = if where_sql.is_empty() {
            format!(
                "UPDATE {} SET {} = NULL WHERE {} IS NOT NULL",
                table, deleted_at, deleted_at
            )
        } else {
            format!(
                "UPDATE {} SET {} = NULL WHERE {} AND {} IS NOT NULL",
                table, deleted_at, where_sql, deleted_at
            )
        };

        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        let rows_affected = self
            .current_db()?
            .__execute_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context))?;
        Self::invalidate_model_state(rows_affected);
        Ok(rows_affected)
    }

    pub async fn force_delete(self) -> Result<u64> {
        self.ensure_query_is_valid()?;
        self.ensure_mutation_query_is_safe("force_delete")?;
        self.ensure_mutation_has_explicit_filters("force_delete")?;

        let db_type = self.db_type_for_sql();
        let table = db_sql::quote_ident(db_type, M::table_name());
        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        let sql = if where_sql.is_empty() {
            format!("DELETE FROM {}", table)
        } else {
            format!("DELETE FROM {} WHERE {}", table, where_sql)
        };

        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        let rows_affected = self
            .current_db()?
            .__execute_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context))?;
        Self::invalidate_model_state(rows_affected);
        Ok(rows_affected)
    }

    pub async fn get_json(self) -> Result<Vec<serde_json::Value>> {
        self.ensure_query_is_valid()?;
        let (sql, params) = self.build_select_sql_with_params();
        self.log_query(&sql);
        let error_context = self.build_query_error_context(Some(&sql));
        self.current_db()?
            .__raw_json_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context))
    }
}

#[cfg(test)]
mod tests {
    use crate::model::Model;
    use crate::query::{FrameBound, FrameType, Order, WindowFunction, WindowFunctionType};

    #[tideorm::model(table = "cache_key_test_users")]
    struct CacheKeyTestUser {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
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
}
