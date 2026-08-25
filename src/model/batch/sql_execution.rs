use super::*;

impl<M: Model> BatchUpdateBuilder<M> {
    fn build_assignment_sql(
        column: &str,
        value: &UpdateValue,
        db_type: crate::config::DatabaseType,
        params: &mut Vec<crate::internal::Value>,
    ) -> Result<String> {
        let col = Self::quote_update_column(column, db_type)?;

        match value {
            UpdateValue::Value(value) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::json_to_db_value(value),
                );
                Ok(format!("{} = {}", col, placeholder))
            }
            UpdateValue::UnsafeRaw(expression) => Ok(format!("{} = {}", col, expression)),
            UpdateValue::Increment(by) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::Value::BigInt(Some(*by)),
                );
                Ok(format!("{} = {} + {}", col, col, placeholder))
            }
            UpdateValue::Decrement(by) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::Value::BigInt(Some(*by)),
                );
                Ok(format!("{} = {} - {}", col, col, placeholder))
            }
            UpdateValue::Multiply(by) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::Value::Double(Some(*by)),
                );
                Ok(format!("{} = {} * {}", col, col, placeholder))
            }
            UpdateValue::Divide(by) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::Value::Double(Some(*by)),
                );
                Ok(format!("{} = {} / {}", col, col, placeholder))
            }
            UpdateValue::ArrayAppend(value) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::json_to_db_value(value),
                );
                Ok(match db_type {
                    crate::config::DatabaseType::Postgres => {
                        format!("{} = array_append({}, {})", col, col, placeholder)
                    }
                    crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                        format!("{} = JSON_ARRAY_APPEND({}, '$', {})", col, col, placeholder)
                    }
                    crate::config::DatabaseType::SQLite => {
                        format!("{} = json_insert({}, '$[#]', {})", col, col, placeholder)
                    }
                })
            }
            UpdateValue::ArrayRemove(value) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::json_to_db_value(value),
                );
                Ok(match db_type {
                    crate::config::DatabaseType::Postgres => {
                        format!("{} = array_remove({}, {})", col, col, placeholder)
                    }
                    crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                        format!(
                            "{} = JSON_REMOVE({}, JSON_UNQUOTE(JSON_SEARCH({}, 'one', {})))",
                            col, col, col, placeholder
                        )
                    }
                    crate::config::DatabaseType::SQLite => {
                        format!(
                            "{} = (SELECT json_group_array(value) FROM json_each({}) WHERE value != {})",
                            col, col, placeholder
                        )
                    }
                })
            }
            UpdateValue::JsonSet(path, value) => {
                let segments = Self::validate_json_path(path)?;
                let path_placeholder = match db_type {
                    crate::config::DatabaseType::Postgres => crate::internal::push_param(
                        db_type,
                        params,
                        crate::internal::Value::String(Some(Self::postgres_json_path_literal(
                            &segments,
                        ))),
                    ),
                    crate::config::DatabaseType::MySQL
                    | crate::config::DatabaseType::MariaDB
                    | crate::config::DatabaseType::SQLite => crate::internal::push_param(
                        db_type,
                        params,
                        crate::internal::Value::String(Some(path.clone())),
                    ),
                };
                let json_text = serde_json::to_string(value)?;
                let value_placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::Value::String(Some(json_text)),
                );

                Ok(match db_type {
                    crate::config::DatabaseType::Postgres => format!(
                        "{} = jsonb_set({}, {}::text[], CAST({} AS jsonb))",
                        col, col, path_placeholder, value_placeholder
                    ),
                    crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                        format!(
                            "{} = JSON_SET({}, {}, CAST({} AS JSON))",
                            col, col, path_placeholder, value_placeholder
                        )
                    }
                    crate::config::DatabaseType::SQLite => {
                        format!(
                            "{} = json_set({}, {}, json({}))",
                            col, col, path_placeholder, value_placeholder
                        )
                    }
                })
            }
            UpdateValue::Coalesce(default) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::json_to_db_value(default),
                );
                Ok(format!("{} = COALESCE({}, {})", col, col, placeholder))
            }
        }
    }

    fn build_set_clause_with_params_for_db(
        &self,
        db_type: crate::config::DatabaseType,
    ) -> Result<(Vec<String>, Vec<crate::internal::Value>)> {
        let mut params = Vec::new();
        let mut set_parts = Vec::with_capacity(self.updates.len());

        // `updates` is a HashMap, so its iteration order varies run to run.
        // Render assignments in column order instead: identical logical updates
        // then produce byte-identical SQL, which keeps server-side prepared
        // statement caches and slow-query fingerprints usable. Parameters are
        // pushed in the same pass, so they stay aligned with the placeholders.
        let mut ordered_updates: Vec<(&String, &UpdateValue)> = self.updates.iter().collect();
        ordered_updates.sort_unstable_by_key(|(column, _)| *column);

        for (column, value) in ordered_updates {
            let value = crate::model::__prepare_batch_update_value::<M>(column, value.clone())?;
            set_parts.push(Self::build_assignment_sql(
                column,
                &value,
                db_type,
                &mut params,
            )?);
        }

        Ok((set_parts, params))
    }

    pub(crate) fn ensure_backend_supports_returning(
        db_type: crate::config::DatabaseType,
    ) -> Result<()> {
        if !db_type.supports_returning() {
            return Err(Error::query(format!(
                "{} does not support RETURNING clause",
                db_type
            )));
        }

        Ok(())
    }

    fn build_where_query(&self) -> QueryBuilder<M> {
        // Batch updates keep soft-deleted rows in scope by default so a bulk
        // restore can reach them; `without_trashed()` opts back into the normal
        // active-only scope.
        let mut query = if self.include_trashed {
            QueryBuilder::new().with_trashed()
        } else {
            QueryBuilder::new()
        };
        let mut or_conditions = Vec::new();

        for condition in &self.conditions {
            if let Some(column) = condition.column.strip_prefix("__OR__") {
                let mut or_condition = condition.clone();
                or_condition.column = column.to_string();
                or_conditions.push(or_condition);
            } else {
                query.conditions.push(condition.clone());
            }
        }

        if !or_conditions.is_empty() {
            query.or_groups.push(OrGroup {
                conditions: or_conditions,
                nested_groups: Vec::new(),
                combine_with: LogicalOp::Or,
            });
        }

        query
    }

    /// Whether the backend accepts `LIMIT` directly on an `UPDATE` statement.
    fn backend_supports_update_limit(db_type: crate::config::DatabaseType) -> bool {
        matches!(
            db_type,
            crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB
        )
    }

    /// Quoted primary-key column used to cap an `UPDATE` on backends that
    /// cannot take `LIMIT` directly.
    fn limit_scope_primary_key(db_type: crate::config::DatabaseType) -> Result<String> {
        match M::primary_key_names() {
            [column] => Ok(Self::quote_identifier(column, db_type)),
            columns => Err(Error::invalid_query(format!(
                "limit() is not supported for '{}' on {}: that backend cannot cap an UPDATE \
                 directly, so the row limit has to be scoped through a primary-key subquery, \
                 which needs a single primary key column (found {})",
                M::table_name(),
                db_type,
                columns.len()
            ))),
        }
    }

    /// `returning()` only has a terminal that can hand rows back:
    /// `execute_returning()`. Refuse rather than silently discard the request.
    fn ensure_returning_terminal(&self) -> Result<()> {
        if self.returning {
            return Err(Error::invalid_query(
                "returning() requires execute_returning(); execute() only reports the number of \
                 affected rows",
            ));
        }

        Ok(())
    }

    /// Render the `UPDATE` statement (without any `RETURNING` clause) and its
    /// parameters for `db_type`.
    fn build_update_statement(
        &self,
        db_type: crate::config::DatabaseType,
    ) -> Result<(String, Vec<crate::internal::Value>)> {
        let (set_parts, mut params) = self.build_set_clause_with_params_for_db(db_type)?;

        let query = self.build_where_query();
        let (mut where_sql, where_params) = query.build_where_clause_with_condition_for_db(db_type);

        if matches!(db_type, crate::config::DatabaseType::Postgres) {
            where_sql = Self::offset_postgres_placeholders(&where_sql, params.len());
        }
        params.extend(where_params);

        let table = Self::quote_identifier(M::table_name(), db_type);
        let mut sql = format!("UPDATE {} SET {}", table, set_parts.join(", "));

        match self.limit_value {
            // Postgres and SQLite reject `UPDATE .. LIMIT`, so the cap is
            // enforced by scoping the update to a bounded primary-key subquery.
            // `limit()` is a blast-radius control; dropping it on those
            // backends would widen exactly what the caller asked to contain.
            Some(limit) if !Self::backend_supports_update_limit(db_type) => {
                let primary_key = Self::limit_scope_primary_key(db_type)?;
                sql.push_str(&format!(
                    " WHERE {} IN (SELECT {} FROM {}",
                    primary_key, primary_key, table
                ));
                if !where_sql.is_empty() {
                    sql.push_str(" WHERE ");
                    sql.push_str(&where_sql);
                }
                // `limit` is a `u64`, so it can only ever render as digits.
                sql.push_str(&format!(" LIMIT {})", limit));
            }
            limit => {
                if !where_sql.is_empty() {
                    sql.push_str(" WHERE ");
                    sql.push_str(&where_sql);
                }
                if let Some(limit) = limit {
                    sql.push_str(&format!(" LIMIT {}", limit));
                }
            }
        }

        Ok((sql, params))
    }

    /// Run the update and report how many rows it changed.
    ///
    /// This is a single `UPDATE` statement: model callbacks, validations, and
    /// automatic timestamp columns are **not** applied, and no rows are loaded.
    /// Use it for bulk maintenance writes; use `model.save()` when the model's
    /// own lifecycle matters.
    ///
    /// Returns `Ok(0)` without touching the database when no assignment was
    /// staged. Errors when the builder carries no explicit filter, or when
    /// [`returning()`](Self::returning) was requested — that flag needs
    /// [`execute_returning`](Self::execute_returning), and silently dropping it
    /// would hide the caller's intent.
    ///
    /// On success the query cache for this table is invalidated.
    pub async fn execute(self) -> Result<u64> {
        if self.updates.is_empty() {
            return Ok(0);
        }

        self.ensure_explicit_filters("update")?;
        self.ensure_returning_terminal()?;

        // Resolve the dialect from the very handle that will run the statement.
        // `require_db()` only ever sees the global connection, so a batch update
        // inside `some_db.transaction(..)` with no global connection used to
        // fail before it rendered any SQL — and could pick the wrong dialect
        // when the scoped handle spoke a different backend.
        let db = crate::database::__current_db()?;
        let db_type = db.backend();

        let (sql, params) = self.build_update_statement(db_type)?;

        let rows_affected = db.__execute_with_params(&sql, params).await?;
        if rows_affected > 0 {
            crate::QueryCache::global().invalidate_model(M::table_name());
            #[cfg(feature = "dirty-tracking")]
            crate::model::__invalidate_dirty_snapshots::<M>();
        }
        Ok(rows_affected)
    }

    /// Run the update and return the rows it wrote.
    ///
    /// Appends `RETURNING *`, so it needs a backend that supports it:
    /// PostgreSQL, MariaDB, and SQLite do, plain MySQL does not and is rejected
    /// with an error before anything runs. Calling
    /// [`returning()`](Self::returning) first is optional here — this terminal
    /// always returns rows.
    ///
    /// Returns an empty vector without touching the database when no assignment
    /// was staged, and errors when the builder carries no explicit filter. Like
    /// [`execute`](Self::execute), no callbacks or validations run.
    pub async fn execute_returning(self) -> Result<Vec<M>> {
        if self.updates.is_empty() {
            return Ok(vec![]);
        }

        self.ensure_explicit_filters("update")?;

        let db = crate::database::__current_db()?;
        let db_type = db.backend();
        Self::ensure_backend_supports_returning(db_type)?;

        let (mut sql, params) = self.build_update_statement(db_type)?;
        sql.push_str(" RETURNING *");

        let models = db.__raw_with_params::<M>(&sql, params).await?;
        if !models.is_empty() {
            crate::QueryCache::global().invalidate_model(M::table_name());
            #[cfg(feature = "dirty-tracking")]
            {
                crate::model::__invalidate_dirty_snapshots::<M>();
                let _ = crate::model::__remember_dirty_snapshots(&models);
            }
        }
        Ok(models)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // The macro-generated entity module emits `Result<_, DbErr>`, so it must not see
    // tideorm's own one-parameter `Result<T>` alias that `use super::*` brings in here.
    use std::result::Result;

    #[tideorm::model(table = "batch_sql_execution_users")]
    struct BatchSqlUser {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
        age: i32,
    }

    #[tideorm::model(table = "batch_sql_execution_soft_delete_users", soft_delete)]
    struct BatchSqlSoftDeleteUser {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
        deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    }

    #[test]
    fn empty_negative_list_does_not_count_as_an_explicit_filter() {
        // Same hazard as the QueryBuilder mutation terminals: an empty candidate
        // set for a negative membership test renders constant-true, so counting
        // conditions would let a caller whose filter list came back empty rewrite
        // every row in the table.
        let err = BatchUpdateBuilder::<BatchSqlUser>::new()
            .set("name", "updated")
            .where_not_in("id", Vec::<i64>::new())
            .ensure_explicit_filters("update")
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("unfiltered bulk mutations are blocked"),
            "vacuous filter was accepted: {err}"
        );

        // One real predicate alongside it is still enough.
        assert!(
            BatchUpdateBuilder::<BatchSqlUser>::new()
                .set("name", "updated")
                .where_eq("name", "alice")
                .where_not_in("id", Vec::<i64>::new())
                .ensure_explicit_filters("update")
                .is_ok()
        );
    }

    fn filtered_builder() -> BatchUpdateBuilder<BatchSqlUser> {
        BatchUpdateBuilder::<BatchSqlUser>::new()
            .set("name", "updated")
            .set("age", 30)
            .where_eq("id", 1)
    }

    fn filtered_soft_delete_builder() -> BatchUpdateBuilder<BatchSqlSoftDeleteUser> {
        BatchUpdateBuilder::<BatchSqlSoftDeleteUser>::new()
            .set("name", "updated")
            .where_eq("id", 1)
    }

    #[test]
    fn batch_update_includes_trashed_rows_by_default() {
        let (sql, _) = filtered_soft_delete_builder()
            .build_update_statement(crate::config::DatabaseType::SQLite)
            .expect("statement should build");

        assert!(
            !sql.contains("deleted_at"),
            "the default scope must not filter soft-deleted rows: {sql}"
        );
    }

    #[test]
    fn batch_update_without_trashed_restores_the_active_only_scope() {
        let (sql, _) = filtered_soft_delete_builder()
            .without_trashed()
            .build_update_statement(crate::config::DatabaseType::SQLite)
            .expect("statement should build");

        assert!(
            sql.contains(r#""deleted_at" IS NULL"#),
            "without_trashed() must scope out soft-deleted rows: {sql}"
        );
    }

    #[test]
    fn batch_update_with_trashed_is_the_default_scope() {
        let (default_sql, _) = filtered_soft_delete_builder()
            .build_update_statement(crate::config::DatabaseType::SQLite)
            .expect("statement should build");
        let (explicit_sql, _) = filtered_soft_delete_builder()
            .without_trashed()
            .with_trashed()
            .build_update_statement(crate::config::DatabaseType::SQLite)
            .expect("statement should build");

        assert_eq!(default_sql, explicit_sql);
    }

    #[test]
    fn batch_update_set_clause_order_is_deterministic() {
        for _ in 0..16 {
            let (sql, _) = filtered_builder()
                .build_update_statement(crate::config::DatabaseType::SQLite)
                .expect("statement should build");

            assert!(
                sql.starts_with(r#"UPDATE "batch_sql_execution_users" SET "age" = ?, "name" = ?"#),
                "unexpected sql: {sql}"
            );
        }
    }

    #[test]
    fn batch_update_limit_is_scoped_by_primary_key_subquery_on_postgres() {
        let (sql, params) = filtered_builder()
            .limit(5)
            .build_update_statement(crate::config::DatabaseType::Postgres)
            .expect("statement should build");

        assert!(
            sql.contains(
                r#"WHERE "id" IN (SELECT "id" FROM "batch_sql_execution_users" WHERE "id" = $3 LIMIT 5)"#
            ),
            "unexpected sql: {sql}"
        );
        assert_eq!(params.len(), 3);
    }

    #[test]
    fn batch_update_limit_is_scoped_by_primary_key_subquery_on_sqlite() {
        let (sql, _) = filtered_builder()
            .limit(5)
            .build_update_statement(crate::config::DatabaseType::SQLite)
            .expect("statement should build");

        assert!(
            sql.contains(
                r#"WHERE "id" IN (SELECT "id" FROM "batch_sql_execution_users" WHERE "id" = ? LIMIT 5)"#
            ),
            "unexpected sql: {sql}"
        );
    }

    #[test]
    fn batch_update_limit_uses_the_native_clause_on_mysql() {
        let (sql, _) = filtered_builder()
            .limit(5)
            .build_update_statement(crate::config::DatabaseType::MySQL)
            .expect("statement should build");

        assert!(sql.ends_with(" LIMIT 5"), "unexpected sql: {sql}");
        assert!(!sql.contains("SELECT"), "unexpected sql: {sql}");
    }

    #[test]
    fn batch_update_without_limit_keeps_a_plain_where_clause() {
        let (sql, _) = filtered_builder()
            .build_update_statement(crate::config::DatabaseType::Postgres)
            .expect("statement should build");

        assert!(
            sql.ends_with(r#" WHERE "id" = $3"#),
            "unexpected sql: {sql}"
        );
    }

    #[test]
    fn batch_update_execute_rejects_a_returning_flag_it_cannot_honour() {
        let err = filtered_builder()
            .returning()
            .ensure_returning_terminal()
            .unwrap_err();

        assert!(
            err.to_string().contains("requires execute_returning()"),
            "unexpected error: {err}"
        );
        assert!(filtered_builder().ensure_returning_terminal().is_ok());
    }
}
