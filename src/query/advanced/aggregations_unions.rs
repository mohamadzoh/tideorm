use super::*;

use crate::config::DatabaseType;
use crate::error::Error;
use crate::internal::Value;

/// Alias of the single column every `f64` aggregate terminal selects.
const AGGREGATE_RESULT_ALIAS: &str = "agg_result";

/// Alias of the single column `count_distinct()` selects.
const COUNT_RESULT_ALIAS: &str = "count_result";

/// Alias of the derived table an aggregate over limited/compound input reads from.
const AGGREGATE_SUBQUERY_ALIAS: &str = "tideorm_aggregate_subquery";

/// Decode the single scalar an aggregate query returned.
///
/// The aggregate expression is always wrapped in `CAST(.. AS FLOAT8/DOUBLE/REAL)`,
/// so backends hand it back as a JSON number; a decimal column can still
/// round-trip through its string form, which is accepted too.
fn aggregate_value_as_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
}

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
        let col = Self::format_aggregate_column(db_type, column.column_name());
        self.having_with_params(format!("SUM({}) > ?", col), vec![value.into()])
    }

    /// Add HAVING with AVG condition
    #[must_use]
    pub fn having_avg_gt(self, column: impl crate::columns::IntoColumnName, value: f64) -> Self {
        let db_type = self.db_type_for_sql();
        let col = Self::format_aggregate_column(db_type, column.column_name());
        self.having_with_params(format!("AVG({}) > ?", col), vec![value.into()])
    }

    /// Render a column reference used inside an aggregate or HAVING expression.
    ///
    /// A qualified `table.column` reference is split so each segment is quoted on
    /// its own (`"orders"."total"`) instead of collapsing into the single bogus
    /// identifier `"orders.total"`, and a bare reference is canonicalised from its
    /// Rust field name to the database column name. Rendering stays strict:
    /// anything that is not a plain identifier reference is quoted as one
    /// identifier rather than passed through as raw SQL.
    fn format_aggregate_column(db_type: DatabaseType, column: &str) -> String {
        let trimmed = column.trim();
        db_sql::format_column(
            db_type,
            M::canonical_column_name(trimmed).unwrap_or(trimmed),
        )
    }

    /// Render an aggregate column against the derived table it is read from.
    ///
    /// A derived table exposes its columns unqualified, so the original table
    /// qualifier has to be dropped before quoting.
    fn format_derived_aggregate_column(db_type: DatabaseType, column: &str) -> String {
        let trimmed = column.trim();
        let unqualified = trimmed.rsplit_once('.').map_or(trimmed, |(_, name)| name);
        Self::format_aggregate_column(db_type, unqualified)
    }

    /// Reject builder state that a single-scalar aggregate cannot represent.
    ///
    /// `group_by()`/`having()` make an aggregate return one row *per group* and a
    /// window function is a per-row projection; neither collapses into the one
    /// number these terminals return. Mirrors `ensure_mutation_query_is_safe()`:
    /// name the incompatible modifier and fail loudly rather than silently
    /// answering with the ungrouped aggregate.
    fn ensure_scalar_aggregate_is_representable(&self, terminal: &str) -> Result<()> {
        let modifier = if !self.group_by.is_empty() {
            "group_by()"
        } else if !self.having_conditions.is_empty() {
            "having()"
        } else if !self.window_functions.is_empty() {
            "window()"
        } else {
            return Ok(());
        };

        Err(Error::invalid_query(format!(
            "{} returns a single scalar and does not support {}; that modifier produces one row per group or per input row, so read those rows with select_raw() and get() instead",
            terminal, modifier
        )))
    }

    /// True when modifiers shape the aggregate's *input rows* and therefore have to
    /// be materialised in a derived table before the aggregate function runs.
    ///
    /// `LIMIT`/`OFFSET` placed next to the aggregate would bound the single result
    /// row instead of the rows being aggregated, and UNION/CTE bodies cannot be
    /// expressed by a plain `FROM <table>` aggregate at all.
    fn aggregate_needs_derived_table(&self) -> bool {
        !self.unions.is_empty()
            || !self.ctes.is_empty()
            || self.limit_value.is_some()
            || self.offset_value.is_some()
    }

    /// True when the aggregate carries no modifier beyond WHERE and can therefore
    /// run through the typed entity path unchanged.
    fn aggregate_uses_typed_path(&self) -> bool {
        self.joins.is_empty() && !self.aggregate_needs_derived_table()
    }

    /// Render a scalar aggregate through the same pipeline `count()` uses, so
    /// joins, CTEs, unions, ordering, and limit/offset are all honoured.
    pub(crate) fn build_aggregate_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
        column: &str,
        alias: &str,
        render_expression: impl Fn(&str) -> String,
    ) -> (String, Vec<Value>) {
        let quoted_alias = db_sql::quote_ident(db_type, alias);

        if self.aggregate_needs_derived_table() {
            let (inner_sql, params) = self.build_select_sql_with_params_for_db(db_type);
            return (
                format!(
                    "SELECT {} AS {} FROM ({}) AS {}",
                    render_expression(&Self::format_derived_aggregate_column(db_type, column)),
                    quoted_alias,
                    inner_sql,
                    db_sql::quote_ident(db_type, AGGREGATE_SUBQUERY_ALIAS)
                ),
                params,
            );
        }

        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        let mut sql = format!(
            "SELECT {} AS {} ",
            render_expression(&Self::format_aggregate_column(db_type, column)),
            quoted_alias
        );
        self.append_from_and_join_sql(&mut sql, db_type);
        if !where_sql.is_empty() {
            sql.push_str(&format!("WHERE {}", where_sql));
        }

        (sql.trim_end().to_string(), params)
    }

    /// Execute a rendered scalar aggregate and return its single value, if any.
    async fn execute_scalar_aggregate(
        &self,
        db_type: DatabaseType,
        column: &str,
        alias: &str,
        render_expression: impl Fn(&str) -> String,
    ) -> Result<Option<serde_json::Value>> {
        let (sql, params) =
            self.build_aggregate_sql_with_params_for_db(db_type, column, alias, render_expression);
        let error_context = self.build_query_error_context(Some(&sql));

        let rows = self
            .current_db()?
            .__raw_json_with_params(&sql, params)
            .await
            .map_err(|err| err.with_context(error_context))?;

        Ok(rows
            .first()
            .and_then(|row| row.get(alias))
            .filter(|value| !value.is_null())
            .cloned())
    }

    /// Calculate SUM of a column
    ///
    /// Joins, CTEs, unions, and `limit()`/`offset()` are honoured; `group_by()`,
    /// `having()`, and `window()` are rejected because a grouped aggregate has no
    /// single scalar answer.
    pub async fn sum(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        self.aggregate_f64("SUM", column.column_name()).await
    }

    /// Calculate AVG of a column
    ///
    /// Carries the same modifier rules as [`sum()`](Self::sum).
    pub async fn avg(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        self.aggregate_f64("AVG", column.column_name()).await
    }

    /// Find MIN value of a column
    ///
    /// Carries the same modifier rules as [`sum()`](Self::sum).
    pub async fn min(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        self.aggregate_f64("MIN", column.column_name()).await
    }

    /// Find MAX value of a column
    ///
    /// Carries the same modifier rules as [`sum()`](Self::sum).
    pub async fn max(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        self.aggregate_f64("MAX", column.column_name()).await
    }

    /// Count distinct values of a column
    ///
    /// Carries the same modifier rules as [`sum()`](Self::sum).
    pub async fn count_distinct(self, column: impl crate::columns::IntoColumnName) -> Result<u64> {
        use crate::database::Connection;

        #[derive(Debug, FromQueryResult)]
        struct CountResult {
            count_result: i64,
        }

        self.ensure_query_is_valid()?;
        self.ensure_scalar_aggregate_is_representable("count_distinct()")?;

        let column = column.column_name();
        let db_type = self.db_type_for_sql();
        let render_expression = |column_sql: &str| format!("COUNT(DISTINCT {})", column_sql);

        if !self.aggregate_uses_typed_path() {
            let value = self
                .execute_scalar_aggregate(db_type, column, COUNT_RESULT_ALIAS, render_expression)
                .await?;

            let Some(value) = value else {
                return Ok(0);
            };

            return if let Some(count) = value.as_u64() {
                Ok(count)
            } else if let Some(count) = value.as_i64() {
                crate::internal::count_to_u64(count, "COUNT(DISTINCT ...)")
            } else {
                Ok(0)
            };
        }

        let db = self.current_db()?;
        let preview = self.build_sql_preview();
        let error_context = self.build_query_error_context(Some(&preview));

        let mut select = M::Entity::find();

        if !self.conditions.is_empty() || !self.or_groups.is_empty() || M::soft_delete_enabled() {
            let condition = self.build_sea_condition();
            select = select.filter(condition);
        }

        let count_expr = Expr::cust(render_expression(&Self::format_aggregate_column(
            db_type, column,
        )));

        let result: Option<CountResult> = match db.__get_connection()? {
            crate::database::ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(
                    select
                        .select_only()
                        .column_as(count_expr, COUNT_RESULT_ALIAS)
                        .into_model::<CountResult>()
                        .one(conn.connection()),
                )
                .await
            }
            crate::database::ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(
                    select
                        .select_only()
                        .column_as(count_expr, COUNT_RESULT_ALIAS)
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
    ///
    /// `function` is one of the four hardcoded aggregate names used by the public
    /// terminals above; it is never caller-controlled, so interpolating it is safe.
    async fn aggregate_f64(&self, function: &str, column: &str) -> Result<f64> {
        use crate::database::Connection;

        #[derive(Debug, FromQueryResult)]
        struct AggResult {
            agg_result: Option<f64>,
        }

        self.ensure_query_is_valid()?;
        self.ensure_scalar_aggregate_is_representable(&format!(
            "{}()",
            function.to_ascii_lowercase()
        ))?;

        let db_type = self.db_type_for_sql();
        let render_expression = |column_sql: &str| {
            db_sql::cast_to_float(db_type, &format!("{}({})", function, column_sql))
        };

        if !self.aggregate_uses_typed_path() {
            let value = self
                .execute_scalar_aggregate(
                    db_type,
                    column,
                    AGGREGATE_RESULT_ALIAS,
                    render_expression,
                )
                .await?;
            return Ok(value
                .as_ref()
                .and_then(aggregate_value_as_f64)
                .unwrap_or(0.0));
        }

        let db = self.current_db()?;
        let preview = self.build_sql_preview();
        let error_context = self.build_query_error_context(Some(&preview));

        let mut select = M::Entity::find();

        if !self.conditions.is_empty() || !self.or_groups.is_empty() || M::soft_delete_enabled() {
            let condition = self.build_sea_condition();
            select = select.filter(condition);
        }

        let agg_expr = Expr::cust(render_expression(&Self::format_aggregate_column(
            db_type, column,
        )));

        let result: Option<AggResult> = match db.__get_connection()? {
            crate::database::ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(
                    select
                        .select_only()
                        .column_as(agg_expr, AGGREGATE_RESULT_ALIAS)
                        .into_model::<AggResult>()
                        .one(conn.connection()),
                )
                .await
            }
            crate::database::ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(
                    select
                        .select_only()
                        .column_as(agg_expr, AGGREGATE_RESULT_ALIAS)
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

    /// Render a compound-select operand as parameterized SQL.
    ///
    /// The operand string is concatenated into the outer statement and executed,
    /// so it goes through the parameterized renderer rather than the debug
    /// preview renderer: every builder-supplied value stays a bound parameter
    /// instead of becoming a hand-escaped inline literal. The operand is
    /// rendered for the outer statement's backend so both halves agree on
    /// identifier quoting and placeholder style.
    fn compound_operand<N: Model>(
        &self,
        union_type: UnionType,
        other: &QueryBuilder<N>,
    ) -> UnionClause {
        let db_type = self.db_type_for_sql();
        let (query_sql, params) = other.build_base_select_sql_with_params_for_db(db_type);
        UnionClause::with_params(union_type, query_sql, params)
    }

    /// Add a UNION with another query
    ///
    /// UNION combines the results of two queries and removes duplicates.
    #[must_use]
    pub fn union<N: Model>(mut self, other: QueryBuilder<N>) -> Self {
        let clause = self.compound_operand(UnionType::Union, &other);
        self.unions.push(clause);
        self
    }

    /// Add a UNION ALL with another query
    ///
    /// UNION ALL combines all results including duplicates (faster than UNION).
    #[must_use]
    pub fn union_all<N: Model>(mut self, other: QueryBuilder<N>) -> Self {
        let clause = self.compound_operand(UnionType::UnionAll, &other);
        self.unions.push(clause);
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

        self.unions
            .push(UnionClause::new(UnionType::Union, sql.to_string()));
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

        self.unions
            .push(UnionClause::new(UnionType::UnionAll, sql.to_string()));
        self
    }
}
