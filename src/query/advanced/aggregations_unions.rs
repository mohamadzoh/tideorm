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

/// The operand of a numeric HAVING comparison (a count, sum or average): a
/// JSON number as is, or numeric text as the number it spells, so a text
/// parameter never makes the database compare the aggregate as text. SQLite
/// has no decimal type and gets an integer or a REAL. `None` for anything else.
fn numeric_having_value(value: &serde_json::Value, db_type: DatabaseType) -> Option<Value> {
    use rust_decimal::prelude::ToPrimitive;
    if value.is_number() {
        return Some(crate::internal::json_to_db_value(value));
    }
    let decimal = value
        .as_str()?
        .trim()
        .parse::<rust_decimal::Decimal>()
        .ok()?;
    Some(match db_type {
        DatabaseType::SQLite => match decimal.to_i64().filter(|_| decimal.fract().is_zero()) {
            Some(integer) => Value::from(integer),
            None => Value::from(decimal.to_f64()?),
        },
        _ => Value::Decimal(Some(decimal)),
    })
}

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

    /// The column this aggregate compares values of, with the operation an
    /// error names: randomized ciphertext cannot be ordered, summed or
    /// deduplicated, so none of these may read an encrypted column.
    fn compared_column(&self) -> Option<(&str, &'static str)> {
        match self {
            Self::Min(column) | Self::Max(column) => Some((column, "MIN/MAX")),
            Self::CountDistinct(column) => Some((column, "DISTINCT")),
            Self::Sum(column) | Self::Avg(column) => Some((column, "SUM/AVG")),
            Self::Count => None,
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
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().group_by(User::ACTIVE).validate().unwrap();
    /// ```
    #[must_use]
    pub fn group_by(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.clauses.group_by.push(Self::own_column(&column));
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
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().having(Aggregate::count().gt(1)).validate().unwrap();
    /// ```
    #[must_use]
    pub fn having(mut self, condition: impl Into<HavingCondition>) -> Self {
        match condition.into() {
            HavingCondition::Aggregate(condition) => {
                // Checked against encrypted columns when the query is validated,
                // once every join this HAVING may read is known.
                if let Some((column, operation)) = condition.aggregate.compared_column() {
                    self.clauses
                        .having_comparisons
                        .push((column.to_string(), operation));
                }
                let db_type = self.db_type_for_sql();
                // The model's own columns are written with its table, so a
                // join added after this call cannot make them ambiguous.
                let expression = condition.aggregate.render(&|column| {
                    db_sql::format_column(db_type, &self.model_identifier(column.trim(), true))
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
                    Ok(value) if value.is_null() => {
                        let operator = match condition.operator {
                            "=" => "IS NULL",
                            "<>" => "IS NOT NULL",
                            _ => {
                                self.invalidate_query(
                                    "HAVING ordering comparisons cannot use NULL".to_string(),
                                );
                                return self;
                            }
                        };
                        return self.having_with_params(format!("{expression} {operator}"), vec![]);
                    }
                    Ok(value)
                        if matches!(
                            condition.aggregate,
                            Aggregate::Count
                                | Aggregate::CountDistinct(_)
                                | Aggregate::Sum(_)
                                | Aggregate::Avg(_)
                        ) =>
                    {
                        let Some(value) = numeric_having_value(value, db_type) else {
                            self.invalidate_query(
                                "numeric HAVING comparisons require a number".to_string(),
                            );
                            return self;
                        };
                        value
                    }
                    Ok(value) => crate::internal::json_to_column_value(value, column_type.as_ref()),
                    Err(reason) => {
                        self.invalidate_query(format!("having(): {}", reason));
                        return self;
                    }
                };
                if let (Some(column), Ok(value)) =
                    (condition.aggregate.extreme_column(), &condition.value)
                {
                    self.clauses.having_typed_bindings.insert(
                        self.clauses.having_conditions.len(),
                        (column.to_string(), value.clone()),
                    );
                }
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

                self.clauses.having_conditions.push(condition);
                self.clauses.having_bindings.push(Vec::new());
                self
            }
        }
    }

    fn having_with_params(mut self, sql_template: String, params: Vec<Value>) -> Self {
        self.clauses.having_conditions.push(sql_template);
        self.clauses.having_bindings.push(params);
        self
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

    /// The name a derived table over this query exposes `column` under, or
    /// `None` when the query's projection leaves it out.
    ///
    /// A selected column is exposed under the name it is selected as
    /// (`amount AS total` exposes `amount` as `total`), and a result column
    /// can be named directly. A query that keeps the model's projection, or
    /// selects a wildcard, exposes a model column under its database name.
    pub(in crate::query) fn derived_output_name(&self, column: &str) -> Option<String> {
        let column = column.trim();
        let (table, name) = match M::canonical_column_parts(column) {
            (Some(table), name) => (table, name),
            (None, name) => (M::table_name(), name),
        };
        let outputs = self.projection_outputs();
        if !column.contains('.')
            && let Some((output, _)) = outputs.iter().find(|(output, _)| output == column)
        {
            return Some(output.clone());
        }
        let selected = outputs.iter().find(|(_, source)| {
            source
                .as_ref()
                .is_some_and(|(source_table, source_column)| {
                    let source_column = match source_table == M::table_name() {
                        true => M::column_named(source_column),
                        false => source_column,
                    };
                    source_table == table && source_column == name
                })
        });
        if let Some((output, _)) = selected {
            return Some(output.clone());
        }
        (!self.projection_is_closed() && table == M::table_name()).then(|| name.to_string())
    }

    /// Whether the query's rows are defined by what it selects: DISTINCT and
    /// UNION preserve the projection, so an aggregate over them reads its
    /// output names, which take precedence over model fields.
    fn rows_fixed_by_projection(&self) -> bool {
        self.is_distinct() || !self.clauses.unions.is_empty()
    }

    /// Reject builder state that a single-scalar aggregate cannot represent.
    ///
    /// `group_by()`/`having()` make an aggregate return one row *per group* and a
    /// window function is a per-row projection; neither collapses into the one
    /// number these terminals return. Mirrors `ensure_mutation_query_is_safe()`:
    /// name the incompatible modifier and fail loudly rather than silently
    /// answering with the ungrouped aggregate.
    fn ensure_scalar_aggregate_is_representable(&self, terminal: &str) -> Result<()> {
        let modifier = if !self.clauses.group_by.is_empty() {
            "group_by()"
        } else if !self.clauses.having_conditions.is_empty() {
            "having()"
        } else if !self.clauses.window_functions.is_empty() {
            "window()"
        } else {
            return Ok(());
        };

        Err(Error::query(format!(
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
        !self.clauses.unions.is_empty()
            || !self.clauses.ctes.is_empty()
            || self.clauses.limit_value.is_some()
            || self.clauses.offset_value.is_some()
            || self.clauses.lock_for_update
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
        projections: impl Fn(&dyn Fn(&str) -> String) -> Vec<String>,
    ) -> Result<(String, Vec<Value>)> {
        let rows_fixed_by_projection = self.rows_fixed_by_projection();
        let invalid = std::cell::RefCell::new(None);
        projections(&|column| {
            let column = column.trim();
            let is_output_alias = rows_fixed_by_projection
                && !column.contains('.')
                && self
                    .projection_outputs()
                    .iter()
                    .any(|(name, _)| name == column);
            if !is_output_alias {
                let result = Self::validate_order_or_group_value(
                    "aggregate column",
                    column,
                    false,
                    Some(&self.known_qualifiers()),
                );
                if let Err(reason) = result {
                    invalid.borrow_mut().get_or_insert(reason);
                }
            }
            String::new()
        });
        if let Some(reason) = invalid.into_inner() {
            return Err(Error::query(reason));
        }
        if !self.aggregate_needs_derived_table() {
            let select_list = projections(&|column| self.format_aggregate_column(db_type, column));
            return Ok(self.build_plain_select_sql(db_type, &select_list.join(", ")));
        }

        let (select_list, (inner_sql, params)) = if rows_fixed_by_projection {
            let unselected = std::cell::RefCell::new(None::<String>);
            let select_list = projections(&|column| {
                let column = column.trim();
                let Some(name) = self.derived_output_name(column) else {
                    unselected
                        .borrow_mut()
                        .get_or_insert_with(|| column.to_string());
                    return String::new();
                };
                db_sql::quote_ident(db_type, &name)
            });
            if let Some(column) = unselected.into_inner() {
                return Err(Error::query(format!(
                    "an aggregate over a distinct() or union() query reads the columns the query selects, and '{}' is not one of them; select() it, or aggregate before distinct()/union()",
                    column
                )));
            }
            (
                select_list,
                self.build_select_sql_with_params_for_db(db_type),
            )
        } else {
            let occupied = self.projection_outputs();
            let input_alias = |index: usize| {
                db_sql::unused_name(&format!("tideorm_aggregate_input_{index}"), |name| {
                    occupied.iter().any(|(output, _)| output == name)
                        || M::column_names().contains(&name)
                })
            };
            let inputs = std::cell::RefCell::new(Vec::<String>::new());
            let select_list = projections(&|column| {
                let column = column.trim();
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

            // Keep the original projection: ORDER BY may resolve its aliases,
            // even when an alias has the same name as another model column.
            let mut inner = self.clone();
            inner.clauses.raw_select_expressions.extend(
                inputs
                    .into_inner()
                    .iter()
                    .enumerate()
                    .map(|(index, column)| {
                        format!(
                            "{} AS {}",
                            self.format_aggregate_column(db_type, column),
                            db_sql::quote_ident(db_type, &input_alias(index))
                        )
                    }),
            );
            (
                select_list,
                inner.build_select_sql_with_params_for_db(db_type),
            )
        };
        Ok((
            db_sql::select_from_derived(
                db_type,
                &select_list.join(", "),
                &inner_sql,
                AGGREGATE_SUBQUERY_ALIAS,
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
        column: &str,
        alias: &str,
        reads_as_column: bool,
        render_expression: impl Fn(&str) -> String,
    ) -> Result<Option<serde_json::Value>> {
        self.ensure_query_is_executable()?;
        self.ensure_scalar_aggregate_is_representable(terminal)?;

        let (sql, params) = self.build_aggregate_sql_with_params_for_db(
            self.db_type_for_sql(),
            column,
            alias,
            render_expression,
        )?;
        let (output_types, sources) =
            self.extreme_outputs(reads_as_column.then_some((alias, column)));
        let rows = self
            .fetch_aggregate_json(&sql, params, &output_types, &sources)
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
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).sum::<i64>(User::ID).await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn sum<T: serde::de::DeserializeOwned>(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> Result<T> {
        let column = Self::own_column(&column);
        self.ensure_encrypted_column_is_comparable(&column, "SUM")?;
        self.decoded_aggregate(
            "sum()",
            &column,
            AGGREGATE_RESULT_ALIAS,
            false,
            sum_expression,
        )
        .await
    }

    /// Average a column, read as `T` (usually `f64`); `None` over no rows.
    ///
    /// Carries the same modifier rules as [`sum()`](Self::sum).
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).avg::<f64>(User::ID).await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn avg<T: serde::de::DeserializeOwned>(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> Result<Option<T>> {
        let column = Self::own_column(&column);
        self.ensure_encrypted_column_is_comparable(&column, "AVG")?;
        self.decoded_aggregate("avg()", &column, AGGREGATE_RESULT_ALIAS, false, |column| {
            format!("AVG({})", column)
        })
        .await
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
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).min::<i64>(User::ID).await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn min<T: serde::de::DeserializeOwned>(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> Result<Option<T>> {
        self.extreme("MIN", &Self::own_column(&column)).await
    }

    /// The largest value of a column, read as `T`; `None` over no rows.
    ///
    /// Works like [`min()`](Self::min).
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).max::<i64>(User::ID).await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn max<T: serde::de::DeserializeOwned>(
        self,
        column: impl crate::columns::IntoColumnName,
    ) -> Result<Option<T>> {
        self.extreme("MAX", &Self::own_column(&column)).await
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
        self.ensure_encrypted_column_is_comparable(column, function)?;
        let terminal = format!("{}()", function.to_ascii_lowercase());
        let alias = Self::extreme_alias(column).unwrap_or(AGGREGATE_RESULT_ALIAS);
        self.decoded_aggregate(&terminal, column, alias, true, |column| {
            format!("{}({})", function, column)
        })
        .await
    }

    /// Resolve the column actually read by an aggregate. DISTINCT and UNION
    /// preserve projections, so their aliases take precedence over model fields.
    pub(in crate::query) fn aggregate_source(&self, column: &str) -> Option<(String, String)> {
        if self.rows_fixed_by_projection() {
            let output = self.derived_output_name(column)?;
            if let Some((_, source)) = self
                .projection_outputs()
                .into_iter()
                .find(|(name, _)| *name == output)
            {
                return source;
            }
        }
        if let Some(column) = M::own_column_name(column) {
            return Some((M::table_name().to_string(), column.to_string()));
        }
        column
            .rsplit_once('.')
            .map(|(table, field)| (table.to_string(), field.to_string()))
    }

    pub(in crate::query) fn ensure_encrypted_column_is_comparable(
        &self,
        column: &str,
        operation: &str,
    ) -> Result<()> {
        // Derived queries read output aliases; ordinary aggregates read source columns.
        let output = if self.rows_fixed_by_projection() {
            self.derived_output_name(column)
        } else if self.aggregate_source(column).is_none() {
            Some(column.to_string())
        } else {
            None
        };
        if let Some(output) = output
            && let Some((name, source)) = self
                .projection_outputs()
                .into_iter()
                .find(|(name, _)| *name == output)
        {
            return Self::reject_encrypted_source(
                self.projected_encrypted_column(&name, source.as_ref())?,
                operation,
            );
        }
        if let Some((table, field)) = self.aggregate_source(column) {
            self.ensure_encrypted_source_is_comparable(&table, &field, operation)?;
        }
        Ok(())
    }

    /// GROUP BY and HAVING read inputs before DISTINCT is applied. A model
    /// column takes precedence over an identically named output alias; aliases
    /// remain usable when they do not shadow an input column.
    pub(in crate::query) fn ensure_encrypted_input_is_comparable(
        &self,
        column: &str,
        operation: &str,
    ) -> Result<()> {
        let (table, column) = Self::simple_column_reference(column).unwrap_or(("", column));
        if !table.is_empty() {
            return self.ensure_encrypted_source_is_comparable(table, column, operation);
        }
        if let Some(column) = M::own_column_name(column) {
            return self.ensure_encrypted_source_is_comparable(M::table_name(), column, operation);
        }
        // An unqualified joined input also takes precedence over an output alias.
        let mut joined_input = false;
        for join in &self.clauses.joins {
            if self.joined_column_names(join).contains(&column) {
                joined_input = true;
                self.ensure_encrypted_source_is_comparable(join.qualifier(), column, operation)?;
            }
        }
        if joined_input {
            return Ok(());
        }
        if let Some((name, source)) = self
            .projection_outputs()
            .into_iter()
            .find(|(name, _)| name == column)
        {
            Self::reject_encrypted_source(
                self.projected_encrypted_column(&name, source.as_ref())?,
                operation,
            )?;
        }
        Ok(())
    }

    pub(in crate::query) fn ensure_encrypted_source_is_comparable(
        &self,
        table: &str,
        field: &str,
        operation: &str,
    ) -> Result<()> {
        Self::reject_encrypted_source(self.encrypted_source_column(table, field)?, operation)
    }

    pub(in crate::query) fn reject_encrypted_source(
        source: Option<crate::model::EncryptedColumn>,
        operation: &str,
    ) -> Result<()> {
        match source {
            Some(source) => Err(source.refuse(
                operation,
                "randomized ciphertext cannot compare plaintext values; read the values and                  compare them after decryption",
            )),
            None => Ok(()),
        }
    }

    fn aggregate_column_type(&self, column: &str) -> Option<crate::orm::ColumnType> {
        if self.rows_fixed_by_projection()
            && let Some(output) = self.derived_output_name(column)
            && let Some(column_type) = self.projected_column_type(&output)
        {
            return Some(column_type);
        }
        let (table, field) = self.aggregate_source(column)?;
        self.column_type(&format!("{table}.{field}"))
    }

    /// How the `MIN`/`MAX` outputs in `extremes`, each an alias and the column
    /// it reads, are read back: decoded by the column's type, the model's or
    /// a joined model's, so a second `MAX` of a timestamp, under an alias of
    /// its own, still reads as one, and decrypted as that column.
    #[allow(clippy::type_complexity)]
    fn extreme_outputs<'a>(
        &self,
        extremes: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> (Vec<(String, crate::orm::ColumnType)>, Vec<(String, String)>) {
        let mut output_types = Vec::new();
        let mut sources = Vec::new();
        for (alias, column) in extremes {
            if let Some(column_type) = self.aggregate_column_type(column) {
                output_types.push((alias.to_string(), column_type));
            }
            sources.push((alias.to_string(), column.to_string()));
        }
        (output_types, sources)
    }

    /// The name a `MIN`/`MAX` over `column` is selected as: the model's own
    /// column name when it is one of the model's columns, so the value is
    /// decoded by that column's type.
    fn extreme_alias(column: &str) -> Option<&'static str> {
        crate::internal::column_type_of::<M>(column)?;
        M::own_column_name(column)
    }

    /// Run a scalar aggregate selected as `alias` and decode its value as `T`.
    async fn decoded_aggregate<T: serde::de::DeserializeOwned>(
        &self,
        terminal: &str,
        column: &str,
        alias: &str,
        reads_as_column: bool,
        render_expression: impl Fn(&str) -> String,
    ) -> Result<T> {
        let value = self
            .execute_scalar_aggregate(terminal, column, alias, reads_as_column, render_expression)
            .await?
            .ok_or_else(|| {
                Error::query(format!(
                    "Database returned no '{}' column for {}",
                    alias, terminal
                ))
            })?;
        let retry = number_from_text(&value);
        decode_aggregate(value, retry, terminal)
    }

    /// Count distinct values of a column
    ///
    /// Encrypted model columns are rejected because randomized ciphertext
    /// cannot count distinct plaintext values.
    ///
    /// Carries the same modifier rules as [`sum()`](Self::sum).
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).count_distinct(User::NAME).await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// ```
    pub async fn count_distinct(self, column: impl crate::columns::IntoColumnName) -> Result<u64> {
        let column = Self::own_column(&column);
        self.ensure_encrypted_column_is_comparable(&column, "DISTINCT")?;
        let value = self
            .execute_scalar_aggregate(
                "count_distinct()",
                &column,
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
    ///
    /// Run this example with the `sqlite` feature.
    ///
    /// ```
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User,)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id, name, active) VALUES (1, 'Ada', 1)").await?;
    /// let result = User::query().where_eq(User::ID, 1).aggregates::<serde_json::Value>(&[Aggregate::count()]).await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
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
        for (column, operation) in aggregates.iter().filter_map(Aggregate::compared_column) {
            self.ensure_encrypted_column_is_comparable(column, operation)?;
        }

        let db_type = self.db_type_for_sql();
        // A MIN or MAX is selected as its model column's name, the first time
        // that name comes up, so it is decoded as the model decodes the column.
        let mut aliases: Vec<String> = Vec::with_capacity(aggregates.len());
        for (index, aggregate) in aggregates.iter().enumerate() {
            let base = aggregate
                .extreme_column()
                .and_then(Self::extreme_alias)
                .filter(|name| !aliases.iter().any(|alias| alias == name))
                .map_or_else(|| format!("aggregate_{index}"), str::to_string);
            let alias =
                db_sql::unused_name(&base, |name| aliases.iter().any(|alias| alias == name));
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
        let (output_types, sources) =
            self.extreme_outputs(aggregates.iter().zip(&aliases).filter_map(
                |(aggregate, alias)| {
                    aggregate
                        .extreme_column()
                        .map(|column| (alias.as_str(), column))
                },
            ));
        let rows = self
            .fetch_aggregate_json(&sql, params, &output_types, &sources)
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
        let retry = aliases
            .iter()
            .zip(&values)
            .map(|(alias, value)| {
                let reads_numbers = !matches!(
                    output_types
                        .iter()
                        .find(|(output, _)| output == alias)
                        .map(|(_, column_type)| column_type),
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
        self.absorb_operand_error("operand", method, other);
        let db_type = self.db_type_for_sql();
        let (query_sql, params) = other.build_compound_operand_sql_for_db(db_type);
        let mut operand = UnionClause::with_params(union_type, query_sql, params);
        // An operand with a raw arm of its own reads `table.*`, in the table's
        // column order, as a raw arm does; mixing arms is refused inside it too.
        operand.raw = other.reads_table_wildcard();
        if let Err(error) = other.ensure_union_projections_are_valid() {
            self.invalidate_query(error.to_string());
        }
        // Such an operand's columns are as unknown as a raw arm's.
        if !operand.raw {
            match other.compound_projection() {
                Ok(projection) => operand.projection = Some(projection),
                Err(error) => self.invalidate_query(error.to_string()),
            }
        }
        self.clauses.unions.push(operand);
        self
    }

    /// Add a UNION with another query
    ///
    /// UNION combines the results of two queries and removes duplicates.
    /// Explicitly bound operands must resolve to the same database and
    /// transaction as the outer query, as with subqueries and CTEs.
    /// Encrypted projections are rejected because randomized ciphertext cannot
    /// deduplicate plaintext values; use compatible [`union_all()`](Self::union_all)
    /// projections instead.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().union(User::query().where_eq(User::ACTIVE, false)).validate().unwrap();
    /// ```
    #[must_use]
    pub fn union<N: Model>(self, other: QueryBuilder<N>) -> Self {
        self.push_compound_operand(UnionType::Union, "union", &other)
    }

    /// Add a UNION ALL with another query
    ///
    /// UNION ALL combines all results including duplicates (faster than UNION).
    /// Explicitly bound operands must resolve to the same database and
    /// transaction as the outer query.
    /// If a projection includes encrypted columns, every operand must have the
    /// same encryption table and column at each output position. Use explicit
    /// columns or the default model projection so these scopes can be checked.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().union_all(User::query().where_eq(User::ACTIVE, false)).validate().unwrap();
    /// ```
    #[must_use]
    pub fn union_all<N: Model>(self, other: QueryBuilder<N>) -> Self {
        self.push_compound_operand(UnionType::UnionAll, "union_all", &other)
    }

    /// Add a raw UNION query
    ///
    /// Trusted SQL only. Do not pass user-controlled input; prefer `union()` with a
    /// `QueryBuilder` whenever possible.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().union_raw("SELECT * FROM users").validate().unwrap();
    /// ```
    #[must_use]
    pub fn union_raw(self, sql: &str) -> Self {
        self.push_raw_union(UnionType::Union, "union_raw", sql)
    }

    /// Add a raw UNION ALL query
    ///
    /// Trusted SQL only. Do not pass user-controlled input; prefer `union_all()` with a
    /// `QueryBuilder` whenever possible.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().union_all_raw("SELECT * FROM users").validate().unwrap();
    /// ```
    #[must_use]
    pub fn union_all_raw(self, sql: &str) -> Self {
        self.push_raw_union(UnionType::UnionAll, "union_all_raw", sql)
    }

    fn push_raw_union(mut self, union_type: UnionType, method: &str, sql: &str) -> Self {
        if let Err(reason) = crate::query::db_sql::validate_subquery_sql(sql) {
            self.invalidate_query(format!("invalid subquery for {method}(): {reason}"));
        }

        self.clauses
            .unions
            .push(UnionClause::new(union_type, sql.to_string()));
        self
    }
}
