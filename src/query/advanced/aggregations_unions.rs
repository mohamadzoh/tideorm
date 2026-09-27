use super::*;

use crate::config::DatabaseType;
use crate::error::Error;
use crate::internal::Value;

/// Alias of the column a SUM or AVG terminal selects.
const AGGREGATE_RESULT_ALIAS: &str = "agg_result";

/// Alias of the single column `count_distinct()` selects.
const COUNT_RESULT_ALIAS: &str = "count_result";

/// Alias of the derived table an aggregate over limited/compound input reads from.
const AGGREGATE_SUBQUERY_ALIAS: &str = "tideorm_aggregate_subquery";

/// Decode an aggregate result as `T`.
///
/// Drivers hand back an exact numeric result (PostgreSQL `numeric`, MySQL
/// `DECIMAL`, which `SUM` and `AVG` of an integer column produce) as text, so a
/// result that does not decode as it is is retried as `retry`: the same value
/// with its numeric text read as the numbers it spells.
fn decode_aggregate<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
    retry: Option<serde_json::Value>,
    what: &str,
) -> Result<T> {
    let error = match serde_json::from_value::<T>(value.clone()) {
        Ok(decoded) => return Ok(decoded),
        Err(error) => error,
    };
    if let Some(decoded) = retry.and_then(|value| serde_json::from_value::<T>(value).ok()) {
        return Ok(decoded);
    }

    let hint = if value.is_null()
        || value
            .as_array()
            .is_some_and(|v| v.iter().any(|v| v.is_null()))
    {
        "; an aggregate over no rows is NULL, so read it as an Option"
    } else {
        ""
    };
    Err(Error::query(format!(
        "{} returned {}, which does not decode as {}: {}{}",
        what,
        value,
        std::any::type_name::<T>(),
        error,
        hint
    )))
}

/// The number a numeric text spells: an integer when it is one, so an exact
/// total stays exact, and a float otherwise. `None` for anything else.
fn number_from_text(value: &serde_json::Value) -> Option<serde_json::Value> {
    let text = value.as_str()?.trim();
    text.parse::<i64>()
        .map(serde_json::Value::from)
        .or_else(|_| text.parse::<u64>().map(serde_json::Value::from))
        .ok()
        .or_else(|| {
            text.parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(serde_json::Value::Number)
        })
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
        Self::CountDistinct(crate::columns::column_reference(&column, None))
    }

    /// `SUM(column)`.
    pub fn sum(column: impl crate::columns::IntoColumnName) -> Self {
        Self::Sum(crate::columns::column_reference(&column, None))
    }

    /// `AVG(column)`.
    pub fn avg(column: impl crate::columns::IntoColumnName) -> Self {
        Self::Avg(crate::columns::column_reference(&column, None))
    }

    /// `MIN(column)`.
    pub fn min(column: impl crate::columns::IntoColumnName) -> Self {
        Self::Min(crate::columns::column_reference(&column, None))
    }

    /// `MAX(column)`.
    pub fn max(column: impl crate::columns::IntoColumnName) -> Self {
        Self::Max(crate::columns::column_reference(&column, None))
    }

    /// Keep the groups where this aggregate is greater than `value`, in
    /// [`QueryBuilder::having`]: `.having(Aggregate::sum("revenue").gt(5_000))`.
    pub fn gt(self, value: impl serde::Serialize) -> AggregateCondition {
        self.compare(">", value)
    }

    /// Keep the groups where this aggregate is at least `value`.
    pub fn gte(self, value: impl serde::Serialize) -> AggregateCondition {
        self.compare(">=", value)
    }

    /// Keep the groups where this aggregate is less than `value`.
    pub fn lt(self, value: impl serde::Serialize) -> AggregateCondition {
        self.compare("<", value)
    }

    /// Keep the groups where this aggregate is at most `value`.
    pub fn lte(self, value: impl serde::Serialize) -> AggregateCondition {
        self.compare("<=", value)
    }

    /// Keep the groups where this aggregate equals `value`.
    pub fn eq(self, value: impl serde::Serialize) -> AggregateCondition {
        self.compare("=", value)
    }

    /// Keep the groups where this aggregate differs from `value`.
    pub fn ne(self, value: impl serde::Serialize) -> AggregateCondition {
        self.compare("<>", value)
    }

    fn compare(self, operator: &'static str, value: impl serde::Serialize) -> AggregateCondition {
        AggregateCondition {
            aggregate: self,
            operator,
            value: crate::query::checked_filter_value(value),
        }
    }

    /// The select-list expression, with columns rendered by `format_column`.
    fn render(&self, format_column: &dyn Fn(&str) -> String) -> String {
        match self {
            Self::Count => "COUNT(*)".to_string(),
            Self::CountDistinct(column) => format!("COUNT(DISTINCT {})", format_column(column)),
            Self::Sum(column) => sum_expression(&format_column(column)),
            Self::Avg(column) => format!("AVG({})", format_column(column)),
            Self::Min(column) => format!("MIN({})", format_column(column)),
            Self::Max(column) => format!("MAX({})", format_column(column)),
        }
    }

    /// The column a `MIN` or `MAX` reads, whose type its result shares.
    fn extreme_column(&self) -> Option<&str> {
        match self {
            Self::Min(column) | Self::Max(column) => Some(column),
            _ => None,
        }
    }
}

/// An aggregate compared with a value, which [`QueryBuilder::having`] keeps
/// the groups by; built with [`Aggregate::gt`] and its siblings. The value is
/// bound, so it may come from a request.
#[derive(Debug, Clone, PartialEq)]
pub struct AggregateCondition {
    aggregate: Aggregate,
    operator: &'static str,
    /// The value compared with, or why it cannot be compared.
    value: std::result::Result<serde_json::Value, String>,
}

/// What [`QueryBuilder::having`] keeps a group by: a comparison of an
/// aggregate, or a raw SQL condition.
#[derive(Debug, Clone, PartialEq)]
pub enum HavingCondition {
    /// An aggregate compared with a bound value.
    Aggregate(AggregateCondition),
    /// A raw SQL condition. **Trusted SQL only**: it is checked for statement
    /// separators and comments, never escaped.
    Raw(String),
}

impl From<AggregateCondition> for HavingCondition {
    fn from(condition: AggregateCondition) -> Self {
        Self::Aggregate(condition)
    }
}

impl From<&str> for HavingCondition {
    fn from(sql: &str) -> Self {
        Self::Raw(sql.to_string())
    }
}

impl From<&String> for HavingCondition {
    fn from(sql: &String) -> Self {
        Self::Raw(sql.clone())
    }
}

impl From<String> for HavingCondition {
    fn from(sql: String) -> Self {
        Self::Raw(sql)
    }
}

/// `SUM` of an already rendered column; the sum of no rows is 0, not NULL.
fn sum_expression(column_sql: &str) -> String {
    format!("COALESCE(SUM({}), 0)", column_sql)
}

impl<M: Model> QueryBuilder<M> {
    /// Add a GROUP BY clause
    #[must_use]
    pub fn group_by(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.group_by.push(crate::columns::column_reference(
            &column,
            Some(M::table_name()),
        ));
        self
    }

    /// Keep the groups that meet `condition`: an aggregate compared with a
    /// bound value, or a raw SQL condition. Several `having` calls must all
    /// hold.
    ///
    /// ```ignore
    /// Sale::query()
    ///     .select_raw("region, SUM(revenue) AS revenue")
    ///     .group_by("region")
    ///     .having(Aggregate::sum("revenue").gt(5_000))
    ///     .having(Aggregate::count().gte(3))
    ///     .get_json()
    ///     .await?;
    /// ```
    ///
    /// A raw condition, `having("COUNT(*) > 2")`, is **trusted SQL only**.
    #[must_use]
    pub fn having(mut self, condition: impl Into<HavingCondition>) -> Self {
        match condition.into() {
            HavingCondition::Aggregate(condition) => {
                let db_type = self.db_type_for_sql();
                // The model's own columns are written with its table, so a
                // join added after this call cannot make them ambiguous.
                let expression = condition.aggregate.render(&|column| {
                    let column = match M::canonical_column_parts(column.trim()) {
                        (None, name) if M::column_names().contains(&name) => {
                            format!("{}.{}", M::table_name(), name)
                        }
                        (Some(table), name) => format!("{}.{}", table, name),
                        (None, name) => name.to_string(),
                    };
                    db_sql::format_column(db_type, &column)
                });
                // A count is a number; a sum, an average, a minimum or a
                // maximum compares as the column it aggregates.
                let column_type = match &condition.aggregate {
                    Aggregate::Count | Aggregate::CountDistinct(_) => None,
                    Aggregate::Sum(column)
                    | Aggregate::Avg(column)
                    | Aggregate::Min(column)
                    | Aggregate::Max(column) => self.column_type(column),
                };
                let value = match &condition.value {
                    Ok(value) => crate::internal::json_to_column_value(value, column_type.as_ref()),
                    Err(reason) => {
                        self.invalidate_query(format!("having(): {}", reason));
                        return self;
                    }
                };
                self.having_with_params(
                    format!("{} {} ?", expression, condition.operator),
                    vec![value],
                )
            }
            HavingCondition::Raw(condition) => {
                if let Err(reason) =
                    crate::query::db_sql::validate_having_sql_fragment("HAVING raw SQL", &condition)
                {
                    self.invalidate_query(reason);
                }

                self.having_conditions.push(condition);
                self.having_bindings.push(Vec::new());
                self
            }
        }
    }

    fn having_with_params(mut self, sql_template: String, params: Vec<Value>) -> Self {
        self.having_conditions.push(sql_template);
        self.having_bindings.push(params);
        self
    }

    /// Keep the groups of more than `value` rows; `having(Aggregate::count().gt(value))`.
    #[must_use]
    pub fn having_count_gt(self, value: i64) -> Self {
        self.having(Aggregate::count().gt(value))
    }

    /// Keep the groups whose sum of `column` exceeds `value`;
    /// `having(Aggregate::sum(column).gt(value))`.
    #[must_use]
    pub fn having_sum_gt(self, column: impl crate::columns::IntoColumnName, value: f64) -> Self {
        self.having(Aggregate::sum(column).gt(value))
    }

    /// Keep the groups whose average of `column` exceeds `value`;
    /// `having(Aggregate::avg(column).gt(value))`.
    #[must_use]
    pub fn having_avg_gt(self, column: impl crate::columns::IntoColumnName, value: f64) -> Self {
        self.having(Aggregate::avg(column).gt(value))
    }

    /// Render a column reference used inside an aggregate or HAVING expression.
    ///
    /// The reference is canonicalised from a Rust field name to its database
    /// column, qualified with the model's table on a query with joins, and
    /// quoted segment by segment (`"orders"."total"`, never the bogus
    /// `"orders.total"`). Rendering stays strict: anything that is not a plain
    /// identifier reference is quoted as one identifier rather than passed
    /// through as raw SQL.
    fn format_aggregate_column(&self, db_type: DatabaseType, column: &str) -> String {
        db_sql::format_column(
            db_type,
            self.canonical_model_identifier(column.trim()).as_ref(),
        )
    }

    /// The name a derived table exposes a projected column under: its
    /// database column name, without a table qualifier.
    pub(in crate::query) fn derived_output_name(&self, column: &str) -> String {
        let canonical = self.canonical_model_identifier(column.trim());
        let name = canonical
            .rsplit_once('.')
            .map_or(canonical.as_ref(), |(_, name)| name);
        name.to_string()
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
    ) -> Result<(String, Vec<Value>)> {
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
    ///
    /// In a derived table bounded by a limit, an offset, a lock or a CTE, the
    /// inner query projects exactly the columns the aggregates read, under
    /// aliases of their own, so a joined table's column is read from that
    /// table. The rows of a `distinct()` or `union()` query are defined by
    /// what it selects, so an aggregate there reads a selected column by name,
    /// and a joined column the query does not select is refused rather than
    /// read from the model's column of the same name.
    fn build_projected_aggregate_sql(
        &self,
        db_type: DatabaseType,
        projections: impl FnOnce(&dyn Fn(&str) -> String) -> Vec<String>,
    ) -> Result<(String, Vec<Value>)> {
        if !self.aggregate_needs_derived_table() {
            let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
            let select_list = projections(&|column| self.format_aggregate_column(db_type, column));
            let mut sql = format!("SELECT {} ", select_list.join(", "));
            self.append_from_and_join_sql(&mut sql, db_type);
            if !where_sql.is_empty() {
                sql.push_str(&format!("WHERE {}", where_sql));
            }
            return Ok((sql.trim_end().to_string(), params));
        }

        let rows_fixed_by_projection = self.is_distinct() || !self.unions.is_empty();
        let input_alias = |index: usize| format!("tideorm_aggregate_input_{index}");
        let inputs = std::cell::RefCell::new(Vec::<String>::new());
        let select_list = projections(&|column| {
            let column = column.trim();
            if rows_fixed_by_projection {
                inputs.borrow_mut().push(column.to_string());
                return db_sql::quote_ident(db_type, &self.derived_output_name(column));
            }
            let mut inputs = inputs.borrow_mut();
            let index = inputs
                .iter()
                .position(|input| input == column)
                .unwrap_or_else(|| {
                    inputs.push(column.to_string());
                    inputs.len() - 1
                });
            db_sql::quote_ident(db_type, &input_alias(index))
        });
        let inputs = inputs.into_inner();

        let mut inner = self.clone();
        if rows_fixed_by_projection {
            let selected = self.select_columns.as_deref().unwrap_or_default();
            if let Some(column) = inputs.iter().find(|column| {
                matches!(M::canonical_column_parts(column), (Some(table), _) if table != M::table_name())
                    && !selected.iter().any(|selected| selected.trim() == column.as_str())
            }) {
                return Err(Error::invalid_query(format!(
                    "an aggregate over a distinct() or union() query reads the columns the query selects, and '{}' is not one of them; select() it, or aggregate before distinct()/union()",
                    column
                )));
            }
        } else if !inputs.is_empty() {
            inner.select_columns = None;
            inner.subquery_select_expressions.clear();
            inner.raw_select_expressions = inputs
                .iter()
                .enumerate()
                .map(|(index, column)| {
                    format!(
                        "{} AS {}",
                        self.format_aggregate_column(db_type, column),
                        db_sql::quote_ident(db_type, &input_alias(index))
                    )
                })
                .collect();
        }

        let (inner_sql, params) = inner.build_select_sql_with_params_for_db(db_type);
        Ok((
            format!(
                "SELECT {} FROM ({}) AS {}",
                select_list.join(", "),
                inner_sql,
                db_sql::quote_ident(db_type, AGGREGATE_SUBQUERY_ALIAS)
            ),
            params,
        ))
    }

    /// Run a scalar aggregate and return the value of its single `alias`
    /// column; `None` when the backend returned no such column.
    ///
    /// The query is validated like every other terminal, so a condition that
    /// cannot be rendered is rejected instead of silently widening the rows the
    /// aggregate covers.
    ///
    /// `reads_as_column` is set for a `MIN`/`MAX`, whose value is one of the
    /// column's and is decoded by its type; a sum or a count is a number
    /// whatever it adds up, so a `SUM` of a boolean is not read as one.
    async fn execute_scalar_aggregate(
        &self,
        terminal: &str,
        db_type: DatabaseType,
        column: &str,
        alias: &str,
        reads_as_column: bool,
        render_expression: impl Fn(&str) -> String,
    ) -> Result<Option<serde_json::Value>> {
        self.ensure_query_is_executable()?;
        self.ensure_scalar_aggregate_is_representable(terminal)?;

        let (sql, params) =
            self.build_aggregate_sql_with_params_for_db(db_type, column, alias, render_expression)?;
        let output_types = if reads_as_column {
            self.extreme_output_type(alias, column)
        } else {
            Vec::new()
        };
        let rows = self
            .fetch_json_with_types(&sql, params, &output_types)
            .await?;

        Ok(rows.first().and_then(|row| row.get(alias)).cloned())
    }

    /// Sum a column, read as `T`: the sum of no rows is zero.
    ///
    /// `T` is whatever the sum should be read as, usually inferred: an integer
    /// type for an integer column, `Decimal` for an exact total of a decimal
    /// column, `f64` for an approximate one. The sum is never read through a
    /// float first, so an integer total past 2^53 and a decimal total stay
    /// exact.
    ///
    /// ```ignore
    /// let revenue: Decimal = Order::query().where_eq("paid", true).sum("total").await?;
    /// ```
    ///
    /// Joins, CTEs, unions, and `limit()`/`offset()` are honoured; `group_by()`,
    /// `having()`, and `window()` are rejected because a grouped aggregate has no
    /// single scalar answer.
    pub async fn sum<T: serde::de::DeserializeOwned>(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> Result<T> {
        let value = self
            .scalar_aggregate(
                "sum()",
                &crate::columns::column_reference(&column, Some(M::table_name())),
                AGGREGATE_RESULT_ALIAS,
                false,
                sum_expression,
            )
            .await?;
        let retry = number_from_text(&value);
        decode_aggregate(value, retry, "sum()")
    }

    /// Average a column, read as `T` (usually `f64`); `None` over no rows.
    ///
    /// Carries the same modifier rules as [`sum()`](Self::sum).
    pub async fn avg<T: serde::de::DeserializeOwned>(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> Result<Option<T>> {
        let value = self
            .scalar_aggregate(
                "avg()",
                &crate::columns::column_reference(&column, Some(M::table_name())),
                AGGREGATE_RESULT_ALIAS,
                false,
                |column| format!("AVG({})", column),
            )
            .await?;
        let retry = number_from_text(&value);
        decode_aggregate(value, retry, "avg()")
    }

    /// The smallest value of a column, read as `T`; `None` over no rows.
    ///
    /// Any column type works — a number, a text, a date — and `T` is read the
    /// way the model reads that column:
    ///
    /// ```ignore
    /// let first_signup: Option<DateTime<Utc>> = User::query().min("created_at").await?;
    /// ```
    ///
    /// Carries the same modifier rules as [`sum()`](Self::sum).
    pub async fn min<T: serde::de::DeserializeOwned>(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> Result<Option<T>> {
        self.extreme(
            "MIN",
            &crate::columns::column_reference(&column, Some(M::table_name())),
        )
        .await
    }

    /// The largest value of a column, read as `T`; `None` over no rows.
    ///
    /// Works like [`min()`](Self::min).
    pub async fn max<T: serde::de::DeserializeOwned>(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> Result<Option<T>> {
        self.extreme(
            "MAX",
            &crate::columns::column_reference(&column, Some(M::table_name())),
        )
        .await
    }

    /// Run a `MIN` or `MAX`, aliased as the model column it reads so the
    /// result is decoded as the model decodes that column.
    ///
    /// `function` is one of the two hardcoded names above, never caller input.
    async fn extreme<T: serde::de::DeserializeOwned>(
        &self,
        function: &str,
        column: &str,
    ) -> Result<Option<T>> {
        let terminal = format!("{}()", function.to_ascii_lowercase());
        let alias = Self::extreme_alias(column).unwrap_or(AGGREGATE_RESULT_ALIAS);
        let value = self
            .scalar_aggregate(&terminal, column, alias, true, |column| {
                format!("{}({})", function, column)
            })
            .await?;
        let retry = number_from_text(&value);
        decode_aggregate(value, retry, &terminal)
    }

    /// The type a `MIN`/`MAX` of `column`, selected as `alias`, is decoded by:
    /// the column's own, the model's or a joined model's, so a second `MAX`
    /// of a timestamp, under an alias of its own, still reads as one.
    fn extreme_output_type(
        &self,
        alias: &str,
        column: &str,
    ) -> Vec<(String, crate::orm::ColumnType)> {
        self.column_type(column)
            .map(|column_type| vec![(alias.to_string(), column_type)])
            .unwrap_or_default()
    }

    /// The name a `MIN`/`MAX` over `column` is selected as: the model's own
    /// column name when it is one of the model's columns, so the value is
    /// decoded by that column's type.
    fn extreme_alias(column: &str) -> Option<&'static str> {
        crate::internal::column_type_of::<M>(column)?;
        let name = column.rsplit_once('.').map_or(column, |(_, name)| name);
        M::canonical_column_name(name.trim())
    }

    /// Run a scalar aggregate selected as `alias` and return its value.
    async fn scalar_aggregate(
        &self,
        terminal: &str,
        column: &str,
        alias: &str,
        reads_as_column: bool,
        render_expression: impl Fn(&str) -> String,
    ) -> Result<serde_json::Value> {
        self.execute_scalar_aggregate(
            terminal,
            self.db_type_for_sql(),
            column,
            alias,
            reads_as_column,
            render_expression,
        )
        .await?
        .ok_or_else(|| {
            Error::query(format!(
                "Database returned no '{}' column for {}",
                alias, terminal
            ))
        })
    }

    /// Count distinct values of a column
    ///
    /// Carries the same modifier rules as [`sum()`](Self::sum).
    pub async fn count_distinct(self, column: impl crate::columns::IntoColumnName) -> Result<u64> {
        let value = self
            .execute_scalar_aggregate(
                "count_distinct()",
                self.db_type_for_sql(),
                &crate::columns::column_reference(&column, Some(M::table_name())),
                COUNT_RESULT_ALIAS,
                false,
                |column_sql| format!("COUNT(DISTINCT {})", column_sql),
            )
            .await?;

        Self::decode_count_value(value.as_ref(), COUNT_RESULT_ALIAS)
    }

    /// Compute several aggregates over the same rows in one statement.
    ///
    /// The results are read into `T` in the order of `aggregates`: a tuple
    /// with one type per aggregate, or a `Vec`. Each value is read as the
    /// matching single-aggregate terminal reads it, so a `SUM` over no rows is
    /// zero and an `AVG`, `MIN` or `MAX` over no rows is NULL, which needs an
    /// `Option`. Carries the same modifier rules as [`sum()`](Self::sum), so
    /// unlike [`count()`](Self::count), [`Aggregate::Count`] counts only the
    /// rows `limit()` and `offset()` leave.
    ///
    /// ```ignore
    /// let (orders, revenue, largest): (u64, Decimal, Option<Decimal>) = Sale::query()
    ///     .where_eq("region", "EU")
    ///     .aggregates(&[Aggregate::count(), Aggregate::sum("amount"), Aggregate::max("amount")])
    ///     .await?;
    /// ```
    pub async fn aggregates<T: serde::de::DeserializeOwned>(
        self,
        aggregates: &[Aggregate],
    ) -> Result<T> {
        if aggregates.is_empty() {
            return decode_aggregate(serde_json::Value::Array(Vec::new()), None, "aggregates()");
        }
        self.ensure_query_is_executable()?;
        self.ensure_scalar_aggregate_is_representable("aggregates()")?;

        let db_type = self.db_type_for_sql();
        // A MIN or MAX is selected as its model column's name, the first time
        // that name comes up, so it is decoded as the model decodes the column.
        let mut aliases: Vec<String> = Vec::with_capacity(aggregates.len());
        for (index, aggregate) in aggregates.iter().enumerate() {
            let alias = aggregate
                .extreme_column()
                .and_then(Self::extreme_alias)
                .filter(|name| !aliases.iter().any(|alias| alias == name))
                .map_or_else(|| format!("aggregate_{index}"), str::to_string);
            aliases.push(alias);
        }
        let (sql, params) = self.build_projected_aggregate_sql(db_type, |format_column| {
            aggregates
                .iter()
                .zip(&aliases)
                .map(|(aggregate, alias)| {
                    format!(
                        "{} AS {}",
                        aggregate.render(format_column),
                        db_sql::quote_ident(db_type, alias)
                    )
                })
                .collect()
        })?;
        // Every MIN or MAX reads as its column.
        let output_types: Vec<(String, crate::orm::ColumnType)> = aggregates
            .iter()
            .zip(&aliases)
            .filter_map(|(aggregate, alias)| {
                aggregate.extreme_column().map(|column| (column, alias))
            })
            .flat_map(|(column, alias)| self.extreme_output_type(alias, column))
            .collect();
        let rows = self
            .fetch_json_with_types(&sql, params, &output_types)
            .await?;
        let row = rows.first();

        let values = aliases
            .iter()
            .map(|alias| {
                row.and_then(|row| row.get(alias.as_str()))
                    .cloned()
                    .ok_or_else(|| {
                        Error::query(format!(
                            "Database returned no '{}' column for aggregates()",
                            alias
                        ))
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        // The MIN or MAX of a text column is text, even when it spells a number.
        let retry = aggregates
            .iter()
            .zip(&values)
            .map(|(aggregate, value)| {
                let reads_numbers = !matches!(
                    aggregate
                        .extreme_column()
                        .and_then(crate::internal::column_type_of::<M>),
                    Some(
                        crate::orm::ColumnType::String(_)
                            | crate::orm::ColumnType::Text
                            | crate::orm::ColumnType::Char(_)
                            | crate::orm::ColumnType::Enum { .. }
                    )
                );
                reads_numbers
                    .then(|| number_from_text(value))
                    .flatten()
                    .unwrap_or_else(|| value.clone())
            })
            .collect();
        decode_aggregate(
            serde_json::Value::Array(values),
            Some(serde_json::Value::Array(retry)),
            "aggregates()",
        )
    }

    /// Render a compound-select operand as parameterized SQL.
    ///
    /// The operand string is concatenated into the outer statement and executed,
    /// so every builder-supplied value stays a bound parameter instead of
    /// becoming a hand-escaped inline literal. The operand is rendered for the
    /// outer statement's backend so both halves agree on identifier quoting and
    /// placeholder style.
    ///
    /// The operand is validated like any subquery: one this query could not
    /// run on its own (an unknown column, an ordering expression outside
    /// `order_by_raw()`, `page(0, n)`) invalidates this query instead of being
    /// spliced in as written.
    fn push_compound_operand<N: Model>(
        mut self,
        union_type: UnionType,
        method: &str,
        other: &QueryBuilder<N>,
    ) -> Self {
        if let Err(err) = other.ensure_query_is_executable() {
            self.invalidate_query(format!("invalid operand for {}(): {}", method, err));
        }
        let db_type = self.db_type_for_sql();
        let (query_sql, params) = other.build_compound_operand_sql_for_db(db_type);
        self.unions
            .push(UnionClause::with_params(union_type, query_sql, params));
        self
    }

    /// Add a UNION with another query
    ///
    /// UNION combines the results of two queries and removes duplicates.
    #[must_use]
    pub fn union<N: Model>(self, other: QueryBuilder<N>) -> Self {
        self.push_compound_operand(UnionType::Union, "union", &other)
    }

    /// Add a UNION ALL with another query
    ///
    /// UNION ALL combines all results including duplicates (faster than UNION).
    #[must_use]
    pub fn union_all<N: Model>(self, other: QueryBuilder<N>) -> Self {
        self.push_compound_operand(UnionType::UnionAll, "union_all", &other)
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
