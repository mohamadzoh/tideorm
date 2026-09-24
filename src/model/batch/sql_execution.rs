use super::*;

use crate::internal::push_param;

impl<M: Model> BatchUpdateBuilder<M> {
    /// Bind `value` with the type of the column it is assigned to.
    fn column_value(column: &str, value: &serde_json::Value) -> crate::internal::Value {
        crate::internal::json_to_column_value(
            value,
            crate::internal::column_type_of::<M>(column).as_ref(),
        )
    }

    /// `arithmetic` as the value of `column`, rounded back to an integer when
    /// the column holds one.
    ///
    /// Scaling by an `f64` yields a floating-point result. PostgreSQL and MySQL
    /// round it when storing it in an integer column, but SQLite stores the
    /// REAL as it is, after which the model can no longer read the row.
    fn keep_integral(
        column: &str,
        db_type: crate::config::DatabaseType,
        arithmetic: String,
    ) -> String {
        use crate::orm::ColumnType;

        let integral = matches!(
            crate::internal::column_type_of::<M>(column),
            Some(
                ColumnType::TinyInteger
                    | ColumnType::SmallInteger
                    | ColumnType::Integer
                    | ColumnType::BigInteger
                    | ColumnType::TinyUnsigned
                    | ColumnType::SmallUnsigned
                    | ColumnType::Unsigned
                    | ColumnType::BigUnsigned
            )
        );
        if integral && db_type == crate::config::DatabaseType::SQLite {
            format!("CAST(ROUND({}) AS INTEGER)", arithmetic)
        } else {
            arithmetic
        }
    }

    fn build_assignment_sql(
        column: &str,
        value: &UpdateValue,
        db_type: crate::config::DatabaseType,
        params: &mut Vec<crate::internal::Value>,
    ) -> Result<String> {
        let col = Self::quote_update_column(column, db_type)?;

        match value {
            UpdateValue::Value(value) => {
                let placeholder = push_param(db_type, params, Self::column_value(column, value));
                Ok(format!("{} = {}", col, placeholder))
            }
            UpdateValue::UnsafeRaw(expression) => Ok(format!("{} = {}", col, expression)),
            UpdateValue::Increment(by) => {
                let placeholder =
                    push_param(db_type, params, crate::internal::Value::BigInt(Some(*by)));
                Ok(format!("{} = {} + {}", col, col, placeholder))
            }
            UpdateValue::Decrement(by) => {
                let placeholder =
                    push_param(db_type, params, crate::internal::Value::BigInt(Some(*by)));
                Ok(format!("{} = {} - {}", col, col, placeholder))
            }
            UpdateValue::Multiply(by) => {
                let placeholder =
                    push_param(db_type, params, crate::internal::Value::Double(Some(*by)));
                let product = format!("{} * {}", col, placeholder);
                Ok(format!(
                    "{} = {}",
                    col,
                    Self::keep_integral(column, db_type, product)
                ))
            }
            UpdateValue::Divide(by) => {
                let placeholder =
                    push_param(db_type, params, crate::internal::Value::Double(Some(*by)));
                let quotient = format!("{} / {}", col, placeholder);
                Ok(format!(
                    "{} = {}",
                    col,
                    Self::keep_integral(column, db_type, quotient)
                ))
            }
            UpdateValue::ArrayAppend(value) => {
                let placeholder =
                    push_param(db_type, params, crate::internal::json_to_db_value(value));
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
                let placeholder =
                    push_param(db_type, params, crate::internal::json_to_db_value(value));
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
                let bound_path = match db_type {
                    crate::config::DatabaseType::Postgres => {
                        Self::postgres_json_path_literal(&segments)
                    }
                    crate::config::DatabaseType::MySQL
                    | crate::config::DatabaseType::MariaDB
                    | crate::config::DatabaseType::SQLite => path.clone(),
                };
                let path_placeholder = push_param(
                    db_type,
                    params,
                    crate::internal::Value::String(Some(bound_path)),
                );
                let json_text = serde_json::to_string(value)?;
                let value_placeholder = push_param(
                    db_type,
                    params,
                    crate::internal::Value::String(Some(json_text)),
                );

                Ok(match db_type {
                    crate::config::DatabaseType::Postgres => format!(
                        "{} = jsonb_set({}, {}::text[], CAST({} AS jsonb))",
                        col,
                        crate::query::db_sql::postgres_jsonb(&col),
                        path_placeholder,
                        value_placeholder
                    ),
                    // `JSON_EXTRACT(?, '$')` reads the text as JSON on both
                    // servers; MariaDB has no `CAST(.. AS JSON)`.
                    crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                        format!(
                            "{} = JSON_SET({}, {}, JSON_EXTRACT({}, '$'))",
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
                let placeholder = push_param(db_type, params, Self::column_value(column, default));
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
            return Err(Error::backend_not_supported(
                format!(
                    "execute_returning() is not supported on {}: it needs UPDATE .. RETURNING",
                    db_type
                ),
                db_type.to_string(),
            ));
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

        query.conditions = self.conditions.clone();
        if !self.or_group.is_empty() {
            query.or_groups.push(self.or_group.clone());
        }

        query
    }

    /// A batch update's filters must pass the query builder's validation, and
    /// at least one of them must be able to exclude a row; these are the same
    /// guards the `QueryBuilder` mutation terminals use.
    pub(crate) fn ensure_explicit_filters(&self, operation: &str) -> Result<()> {
        let query = self.build_where_query();
        query.ensure_query_is_executable()?;
        query.ensure_mutation_has_explicit_filters(operation)
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
            [column] => Ok(quote_ident(db_type, column)),
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
            where_sql =
                crate::query::db_sql::offset_postgres_placeholders(&where_sql, params.len());
        }
        params.extend(where_params);

        let table = crate::query::db_sql::quote_table::<M>(db_type);
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
        QueryBuilder::<M>::invalidate_model_state(rows_affected);
        Ok(rows_affected)
    }

    /// Run the update and return the rows it wrote.
    ///
    /// Appends `RETURNING`, so it needs a backend whose `UPDATE` takes one:
    /// PostgreSQL and SQLite. MySQL has no `RETURNING`, and MariaDB has
    /// `UPDATE .. RETURNING` only from 13.0, so both are refused with an error
    /// before anything runs, whatever the server's version. Calling
    /// [`returning()`](Self::returning) first is optional here — this terminal
    /// always returns rows.
    ///
    /// Returns an empty vector without touching the database when no assignment
    /// was staged, and errors when the builder carries no explicit filter. Like
    /// [`execute`](Self::execute), no callbacks or validations run.
    ///
    /// The update and the decoding of the rows it returns are not one unit: if
    /// a row does not decode into the model, the error comes back but the rows
    /// are already written, and retrying repeats a non-idempotent change such
    /// as `increment`. Run it inside `Database::transaction` when that matters.
    pub async fn execute_returning(self) -> Result<Vec<M>> {
        if self.updates.is_empty() {
            return Ok(vec![]);
        }

        self.ensure_explicit_filters("update")?;

        let db = crate::database::__current_db()?;
        let db_type = db.backend();
        Self::ensure_backend_supports_returning(db_type)?;
        // The rows are written before they are decoded, as with `update()`.
        crate::internal::ensure_fields_storable::<M, _>(&db.__get_connection()?.executor())?;

        let (mut sql, params) = self.build_update_statement(db_type)?;
        sql.push_str(" RETURNING ");
        sql.push_str(&crate::query::db_sql::model_columns_sql::<M>(db_type, None));

        let models = db.__raw_with_params::<M>(&sql, params).await?;
        QueryBuilder::<M>::invalidate_model_state(models.len() as u64);
        #[cfg(feature = "dirty-tracking")]
        crate::model::__remember_dirty_snapshots(&models);
        Ok(models)
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/model_batch_sql_execution_tests.rs"]
mod tests;
