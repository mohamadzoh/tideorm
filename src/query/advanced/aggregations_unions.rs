use super::*;

impl<M: Model> QueryBuilder<M> {
    /// Add a GROUP BY clause
    #[must_use]
    pub fn group_by(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.group_by.push(column.column_name().to_string());
        self
    }

    /// Add multiple GROUP BY columns
    #[must_use]
    pub fn group_by_columns(mut self, columns: Vec<&str>) -> Self {
        for col in columns {
            self.group_by.push(col.to_string());
        }
        self
    }

    /// Add a HAVING clause (raw SQL condition)
    #[must_use]
    pub fn having(mut self, condition: &str) -> Self {
        if let Err(reason) =
            crate::query::db_sql::validate_having_sql_fragment("HAVING raw SQL", condition)
        {
            self.invalidate_query(reason);
        }

        self.having_conditions.push(condition.to_string());
        self.having_bindings.push(Vec::new());
        self
    }

    fn having_with_params(mut self, sql_template: String, params: Vec<serde_json::Value>) -> Self {
        self.having_conditions.push(sql_template);
        self.having_bindings.push(params);
        self
    }

    /// Add HAVING with COUNT condition
    #[must_use]
    pub fn having_count_gt(self, value: i64) -> Self {
        self.having_with_params("COUNT(*) > ?".to_string(), vec![value.into()])
    }

    /// Add HAVING with COUNT >= condition
    #[must_use]
    pub fn having_count_gte(self, value: i64) -> Self {
        self.having_with_params("COUNT(*) >= ?".to_string(), vec![value.into()])
    }

    /// Add HAVING with COUNT < condition
    #[must_use]
    pub fn having_count_lt(self, value: i64) -> Self {
        self.having_with_params("COUNT(*) < ?".to_string(), vec![value.into()])
    }

    /// Add HAVING with COUNT <= condition
    #[must_use]
    pub fn having_count_lte(self, value: i64) -> Self {
        self.having_with_params("COUNT(*) <= ?".to_string(), vec![value.into()])
    }

    /// Add HAVING with SUM condition
    #[must_use]
    pub fn having_sum_gt(self, column: impl crate::columns::IntoColumnName, value: f64) -> Self {
        let db_type = self.db_type_for_sql();
        let col = db_sql::quote_ident(db_type, column.column_name());
        self.having_with_params(format!("SUM({}) > ?", col), vec![value.into()])
    }

    /// Add HAVING with AVG condition
    #[must_use]
    pub fn having_avg_gt(self, column: impl crate::columns::IntoColumnName, value: f64) -> Self {
        let db_type = self.db_type_for_sql();
        let col = db_sql::quote_ident(db_type, column.column_name());
        self.having_with_params(format!("AVG({}) > ?", col), vec![value.into()])
    }

    /// Calculate SUM of a column
    pub async fn sum(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        let db_type = self.db_type_for_sql();
        let col = db_sql::quote_ident(db_type, column.column_name());
        let expr = db_sql::cast_to_float(db_type, &format!("SUM({})", col));
        self.aggregate_f64(&expr, "sum_result").await
    }

    /// Calculate AVG of a column
    pub async fn avg(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        let db_type = self.db_type_for_sql();
        let col = db_sql::quote_ident(db_type, column.column_name());
        let expr = db_sql::cast_to_float(db_type, &format!("AVG({})", col));
        self.aggregate_f64(&expr, "avg_result").await
    }

    /// Find MIN value of a column
    pub async fn min(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        let db_type = self.db_type_for_sql();
        let col = db_sql::quote_ident(db_type, column.column_name());
        let expr = db_sql::cast_to_float(db_type, &format!("MIN({})", col));
        self.aggregate_f64(&expr, "min_result").await
    }

    /// Find MAX value of a column
    pub async fn max(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        let db_type = self.db_type_for_sql();
        let col = db_sql::quote_ident(db_type, column.column_name());
        let expr = db_sql::cast_to_float(db_type, &format!("MAX({})", col));
        self.aggregate_f64(&expr, "max_result").await
    }

    /// Count distinct values of a column
    pub async fn count_distinct(self, column: impl crate::columns::IntoColumnName) -> Result<u64> {
        use crate::database::Connection;

        #[derive(Debug, FromQueryResult)]
        struct CountResult {
            count_result: i64,
        }

        self.ensure_query_is_valid()?;

        let db_type = self.db_type_for_sql();
        let col = db_sql::quote_ident(db_type, column.column_name());

        let db = self.current_db()?;
        let preview = self.build_sql_preview();
        let error_context = self.build_query_error_context(Some(&preview));

        let mut select = M::Entity::find();

        if !self.conditions.is_empty() || !self.or_groups.is_empty() || M::soft_delete_enabled() {
            let condition = self.build_sea_condition();
            select = select.filter(condition);
        }

        let count_expr = Expr::cust(format!("COUNT(DISTINCT {})", col));

        let result: Option<CountResult> = match db.__get_connection()? {
            crate::database::ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(
                    select
                        .select_only()
                        .column_as(count_expr, "count_result")
                        .into_model::<CountResult>()
                        .one(conn.connection()),
                )
                .await
            }
            crate::database::ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(
                    select
                        .select_only()
                        .column_as(count_expr, "count_result")
                        .into_model::<CountResult>()
                        .one(tx.as_ref()),
                )
                .await
            }
        }
        .map_err(translate_error)
        .map_err(|err| err.with_context(error_context))?;

        result
            .map(|r| crate::internal::count_to_u64(r.count_result, "COUNT(DISTINCT ...)"))
            .transpose()
            .map(|count| count.unwrap_or(0))
    }

    /// Internal helper for f64 aggregations
    async fn aggregate_f64(self, expr_sql: &str, _alias: &str) -> Result<f64> {
        use crate::database::Connection;

        #[derive(Debug, FromQueryResult)]
        struct AggResult {
            agg_result: Option<f64>,
        }

        self.ensure_query_is_valid()?;

        let db = self.current_db()?;
        let preview = self.build_sql_preview();
        let error_context = self.build_query_error_context(Some(&preview));

        let mut select = M::Entity::find();

        if !self.conditions.is_empty() || !self.or_groups.is_empty() || M::soft_delete_enabled() {
            let condition = self.build_sea_condition();
            select = select.filter(condition);
        }

        let agg_expr = Expr::cust(expr_sql.to_string());

        let result: Option<AggResult> = match db.__get_connection()? {
            crate::database::ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(
                    select
                        .select_only()
                        .column_as(agg_expr, "agg_result")
                        .into_model::<AggResult>()
                        .one(conn.connection()),
                )
                .await
            }
            crate::database::ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(
                    select
                        .select_only()
                        .column_as(agg_expr, "agg_result")
                        .into_model::<AggResult>()
                        .one(tx.as_ref()),
                )
                .await
            }
        }
        .map_err(translate_error)
        .map_err(|err| err.with_context(error_context))?;

        Ok(result.and_then(|r| r.agg_result).unwrap_or(0.0))
    }

    // =========================================================================
    // UNION OPERATIONS
    // =========================================================================

    /// Add a UNION with another query
    ///
    /// UNION combines the results of two queries and removes duplicates.
    #[must_use]
    pub fn union<N: Model>(mut self, other: QueryBuilder<N>) -> Self {
        self.unions.push(UnionClause {
            union_type: UnionType::Union,
            query_sql: other.build_base_select_sql(),
        });
        self
    }

    /// Add a UNION ALL with another query
    ///
    /// UNION ALL combines all results including duplicates (faster than UNION).
    #[must_use]
    pub fn union_all<N: Model>(mut self, other: QueryBuilder<N>) -> Self {
        self.unions.push(UnionClause {
            union_type: UnionType::UnionAll,
            query_sql: other.build_base_select_sql(),
        });
        self
    }

    /// Add a raw UNION query
    ///
    /// Trusted SQL only. Do not pass user-controlled input; prefer `union()` with a
    /// `QueryBuilder` whenever possible.
    #[must_use]
    pub fn union_raw(mut self, sql: &str) -> Self {
        if let Err(reason) = crate::query::db_sql::validate_subquery_sql(sql) {
            self.invalidate_query(format!("invalid subquery for union_raw(): {}", reason));
        }

        self.unions.push(UnionClause {
            union_type: UnionType::Union,
            query_sql: sql.to_string(),
        });
        self
    }

    /// Add a raw UNION ALL query
    ///
    /// Trusted SQL only. Do not pass user-controlled input; prefer `union_all()` with a
    /// `QueryBuilder` whenever possible.
    #[must_use]
    pub fn union_all_raw(mut self, sql: &str) -> Self {
        if let Err(reason) = crate::query::db_sql::validate_subquery_sql(sql) {
            self.invalidate_query(format!("invalid subquery for union_all_raw(): {}", reason));
        }

        self.unions.push(UnionClause {
            union_type: UnionType::UnionAll,
            query_sql: sql.to_string(),
        });
        self
    }
}
