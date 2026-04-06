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

        for (column, value) in &self.updates {
            set_parts.push(Self::build_assignment_sql(
                column,
                value,
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
        let mut query = QueryBuilder::new().with_trashed();
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

    pub async fn execute(self) -> Result<u64> {
        if self.updates.is_empty() {
            return Ok(0);
        }

        self.ensure_explicit_filters("update")?;

        let _ = self.returning;

        let db_type = crate::database::require_db()?.backend();
        let (set_parts, mut params) = self.build_set_clause_with_params_for_db(db_type)?;

        let query = self.build_where_query();
        let (mut where_sql, where_params) = query.build_where_clause_with_condition_for_db(db_type);

        if matches!(db_type, crate::config::DatabaseType::Postgres) {
            where_sql = Self::offset_postgres_placeholders(&where_sql, params.len());
        }
        params.extend(where_params);

        let table = Self::quote_identifier(M::table_name(), db_type);
        let mut sql = format!("UPDATE {} SET {}", table, set_parts.join(", "));

        if !where_sql.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_sql);
        }

        if let Some(limit) = self.limit_value {
            if matches!(
                db_type,
                crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB
            ) {
                sql.push_str(&format!(" LIMIT {}", limit));
            }
        }

        let rows_affected = crate::Database::execute_with_params(&sql, params).await?;
        if rows_affected > 0 {
            crate::QueryCache::global().invalidate_model(M::table_name());
            #[cfg(feature = "dirty-tracking")]
            crate::model::__invalidate_dirty_snapshots::<M>();
        }
        Ok(rows_affected)
    }

    pub async fn execute_returning(self) -> Result<Vec<M>> {
        if self.updates.is_empty() {
            return Ok(vec![]);
        }

        self.ensure_explicit_filters("update")?;

        let db_type = crate::database::require_db()?.backend();
        Self::ensure_backend_supports_returning(db_type)?;

        let (set_parts, mut params) = self.build_set_clause_with_params_for_db(db_type)?;

        let query = self.build_where_query();
        let (mut where_sql, where_params) = query.build_where_clause_with_condition_for_db(db_type);

        if matches!(db_type, crate::config::DatabaseType::Postgres) {
            where_sql = Self::offset_postgres_placeholders(&where_sql, params.len());
        }
        params.extend(where_params);

        let table = Self::quote_identifier(M::table_name(), db_type);
        let mut sql = format!("UPDATE {} SET {}", table, set_parts.join(", "));

        if !where_sql.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_sql);
        }

        sql.push_str(" RETURNING *");

        let models = crate::Database::raw_with_params::<M>(&sql, params).await?;
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
