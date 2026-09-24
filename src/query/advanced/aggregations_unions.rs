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

/// One aggregate that [`QueryBuilder::aggregates`] computes.
///
/// The constructors take a column name or a typed column, as the
/// single-aggregate terminals do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Aggregate {
    /// `COUNT(*)`: the number of rows.
    Count,
    /// `COUNT(DISTINCT column)`.
    CountDistinct(String),
    /// `SUM(column)`.
    Sum(String),
    /// `AVG(column)`.
    Avg(String),
    /// `MIN(column)`.
    Min(String),
    /// `MAX(column)`.
    Max(String),
}

impl Aggregate {
    /// `COUNT(*)`: the number of rows.
    pub fn count() -> Self {
        Self::Count
    }

    /// `COUNT(DISTINCT column)`.
    pub fn count_distinct(column: impl crate::columns::IntoColumnName) -> Self {
        Self::CountDistinct(column.column_name().to_string())
    }

    /// `SUM(column)`.
    pub fn sum(column: impl crate::columns::IntoColumnName) -> Self {
        Self::Sum(column.column_name().to_string())
    }

    /// `AVG(column)`.
    pub fn avg(column: impl crate::columns::IntoColumnName) -> Self {
        Self::Avg(column.column_name().to_string())
    }

    /// `MIN(column)`.
    pub fn min(column: impl crate::columns::IntoColumnName) -> Self {
        Self::Min(column.column_name().to_string())
    }

    /// `MAX(column)`.
    pub fn max(column: impl crate::columns::IntoColumnName) -> Self {
        Self::Max(column.column_name().to_string())
    }

    /// The select-list expression, with columns rendered by `format_column`.
    /// The numeric ones are cast to a float, as the single terminals are.
    fn render(&self, db_type: DatabaseType, format_column: &dyn Fn(&str) -> String) -> String {
        let (function, column) = match self {
            Self::Count => return "COUNT(*)".to_string(),
            Self::CountDistinct(column) => {
                return format!("COUNT(DISTINCT {})", format_column(column));
            }
            Self::Sum(column) => ("SUM", column),
            Self::Avg(column) => ("AVG", column),
            Self::Min(column) => ("MIN", column),
            Self::Max(column) => ("MAX", column),
        };
        db_sql::cast_to_float(db_type, &format!("{}({})", function, format_column(column)))
    }
}

impl<M: Model> QueryBuilder<M> {
    /// Add a GROUP BY clause
    #[must_use]
    pub fn group_by(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.group_by.push(column.column_name().to_string());
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
    /// row instead of the rows being aggregated, `DISTINCT` has to collapse the
    /// rows before they are counted, as `count()` does, and UNION/CTE bodies
    /// cannot be expressed by a plain `FROM <table>` aggregate at all.
    fn aggregate_needs_derived_table(&self) -> bool {
        !self.unions.is_empty()
            || !self.ctes.is_empty()
            || self.limit_value.is_some()
            || self.offset_value.is_some()
            || self.lock_for_update
            || self.is_distinct()
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
        self.build_projected_aggregate_sql(db_type, |format_column| {
            vec![format!(
                "{} AS {}",
                render_expression(&format_column(column)),
                quoted_alias
            )]
        })
    }

    /// `SELECT <projections>` over the query's rows, read from a derived table
    /// when its modifiers shape those rows. `projections` is handed the column
    /// formatter for whichever of the two forms is rendered.
    fn build_projected_aggregate_sql(
        &self,
        db_type: DatabaseType,
        projections: impl FnOnce(&dyn Fn(&str) -> String) -> Vec<String>,
    ) -> (String, Vec<Value>) {
        if self.aggregate_needs_derived_table() {
            let (inner_sql, params) = self.build_select_sql_with_params_for_db(db_type);
            let select_list =
                projections(&|column| Self::format_derived_aggregate_column(db_type, column));
            return (
                format!(
                    "SELECT {} FROM ({}) AS {}",
                    select_list.join(", "),
                    inner_sql,
                    db_sql::quote_ident(db_type, AGGREGATE_SUBQUERY_ALIAS)
                ),
                params,
            );
        }

        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        let select_list = projections(&|column| Self::format_aggregate_column(db_type, column));
        let mut sql = format!("SELECT {} ", select_list.join(", "));
        self.append_from_and_join_sql(&mut sql, db_type);
        if !where_sql.is_empty() {
            sql.push_str(&format!("WHERE {}", where_sql));
        }

        (sql.trim_end().to_string(), params)
    }

    /// Run a scalar aggregate and return the value of its single `alias`
    /// column; `None` when the backend returned no such column.
    ///
    /// The query is validated like every other terminal, so a condition that
    /// cannot be rendered is rejected instead of silently widening the rows the
    /// aggregate covers.
    async fn execute_scalar_aggregate(
        &self,
        terminal: &str,
        db_type: DatabaseType,
        column: &str,
        alias: &str,
        render_expression: impl Fn(&str) -> String,
    ) -> Result<Option<serde_json::Value>> {
        self.ensure_query_is_executable()?;
        self.ensure_scalar_aggregate_is_representable(terminal)?;

        let (sql, params) =
            self.build_aggregate_sql_with_params_for_db(db_type, column, alias, render_expression);
        let rows = self.fetch_json(&sql, params).await?;

        Ok(rows.first().and_then(|row| row.get(alias)).cloned())
    }

    /// Calculate SUM of a column
    ///
    /// Joins, CTEs, unions, and `limit()`/`offset()` are honoured; `group_by()`,
    /// `having()`, and `window()` are rejected because a grouped aggregate has no
    /// single scalar answer. An aggregate over no rows is `0.0`.
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
        let value = self
            .execute_scalar_aggregate(
                "count_distinct()",
                self.db_type_for_sql(),
                column.column_name(),
                COUNT_RESULT_ALIAS,
                |column_sql| format!("COUNT(DISTINCT {})", column_sql),
            )
            .await?;

        Self::decode_count_value(value.as_ref(), COUNT_RESULT_ALIAS)
    }

    /// Compute several aggregates over the same rows in one statement.
    ///
    /// Returns one value per entry of `aggregates`, in the same order. Counts
    /// are whole numbers, and a `SUM`, `AVG`, `MIN` or `MAX` over no rows is
    /// `0.0`, as the single-aggregate terminals report it. Carries the same
    /// modifier rules as [`sum()`](Self::sum), so unlike
    /// [`count()`](Self::count), [`Aggregate::Count`] counts only the rows
    /// `limit()` and `offset()` leave.
    ///
    /// ```ignore
    /// let stats = Sale::query()
    ///     .where_eq("region", "EU")
    ///     .aggregates(&[Aggregate::count(), Aggregate::sum("amount"), Aggregate::max("amount")])
    ///     .await?;
    /// let (orders, revenue, largest) = (stats[0], stats[1], stats[2]);
    /// ```
    pub async fn aggregates(self, aggregates: &[Aggregate]) -> Result<Vec<f64>> {
        if aggregates.is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_query_is_executable()?;
        self.ensure_scalar_aggregate_is_representable("aggregates()")?;

        let db_type = self.db_type_for_sql();
        let aliases: Vec<String> = (0..aggregates.len())
            .map(|index| format!("aggregate_{index}"))
            .collect();
        let (sql, params) = self.build_projected_aggregate_sql(db_type, |format_column| {
            aggregates
                .iter()
                .zip(&aliases)
                .map(|(aggregate, alias)| {
                    format!(
                        "{} AS {}",
                        aggregate.render(db_type, format_column),
                        db_sql::quote_ident(db_type, alias)
                    )
                })
                .collect()
        });
        let rows = self.fetch_json(&sql, params).await?;
        let row = rows.first();

        aggregates
            .iter()
            .zip(&aliases)
            .map(|(aggregate, alias)| {
                let value = row.and_then(|row| row.get(alias.as_str()));
                match aggregate {
                    Aggregate::Count | Aggregate::CountDistinct(_) => {
                        Self::decode_count_value(value, alias).map(|count| count as f64)
                    }
                    _ => match value {
                        None => Err(Error::query(format!(
                            "Database returned no '{}' column for aggregates()",
                            alias
                        ))),
                        Some(serde_json::Value::Null) => Ok(0.0),
                        Some(value) => aggregate_value_as_f64(value).ok_or_else(|| {
                            Error::query(format!(
                                "Unable to decode the {:?} result as a number (got {})",
                                aggregate, value
                            ))
                        }),
                    },
                }
            })
            .collect()
    }

    /// Run one of the `f64` aggregates.
    ///
    /// `function` is one of the four hardcoded aggregate names used by the public
    /// terminals above; it is never caller-controlled, so interpolating it is safe.
    /// A NULL result — the aggregate of no rows — is `0.0`; a missing or
    /// non-numeric one is a decode failure.
    async fn aggregate_f64(&self, function: &str, column: &str) -> Result<f64> {
        let db_type = self.db_type_for_sql();
        let value = self
            .execute_scalar_aggregate(
                &format!("{}()", function.to_ascii_lowercase()),
                db_type,
                column,
                AGGREGATE_RESULT_ALIAS,
                |column_sql| {
                    db_sql::cast_to_float(db_type, &format!("{}({})", function, column_sql))
                },
            )
            .await?;

        match value {
            None => Err(Error::query(format!(
                "Database returned no '{}' column for {}()",
                AGGREGATE_RESULT_ALIAS, function
            ))),
            Some(serde_json::Value::Null) => Ok(0.0),
            Some(value) => aggregate_value_as_f64(&value).ok_or_else(|| {
                Error::query(format!(
                    "Unable to decode the {}() result as a number (got {})",
                    function, value
                ))
            }),
        }
    }

    /// Render a compound-select operand as parameterized SQL.
    ///
    /// The operand string is concatenated into the outer statement and executed,
    /// so every builder-supplied value stays a bound parameter instead of
    /// becoming a hand-escaped inline literal. The operand is rendered for the
    /// outer statement's backend so both halves agree on identifier quoting and
    /// placeholder style.
    fn compound_operand<N: Model>(
        &self,
        union_type: UnionType,
        other: &QueryBuilder<N>,
    ) -> UnionClause {
        let db_type = self.db_type_for_sql();
        let (query_sql, params) = other.build_compound_operand_sql_for_db(db_type);
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
