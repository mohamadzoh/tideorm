use super::*;

use crate::config::DatabaseType;
use crate::error::Error;
use crate::internal::Value;

mod terminals;
mod unions;

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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[must_use = "an aggregate computes nothing until `aggregates()` or `having()` takes it"]
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
    pub fn gt(self, value: impl Into<crate::DbValue>) -> AggregateCondition {
        self.compare(">", value)
    }

    /// Keep the groups where this aggregate is at least `value`.
    pub fn gte(self, value: impl Into<crate::DbValue>) -> AggregateCondition {
        self.compare(">=", value)
    }

    /// Keep the groups where this aggregate is less than `value`.
    pub fn lt(self, value: impl Into<crate::DbValue>) -> AggregateCondition {
        self.compare("<", value)
    }

    /// Keep the groups where this aggregate is at most `value`.
    pub fn lte(self, value: impl Into<crate::DbValue>) -> AggregateCondition {
        self.compare("<=", value)
    }

    /// Keep the groups where this aggregate equals `value`.
    pub fn eq(self, value: impl Into<crate::DbValue>) -> AggregateCondition {
        self.compare("=", value)
    }

    /// Keep the groups where this aggregate differs from `value`.
    pub fn ne(self, value: impl Into<crate::DbValue>) -> AggregateCondition {
        self.compare("<>", value)
    }

    fn compare(
        self,
        operator: &'static str,
        value: impl Into<crate::DbValue>,
    ) -> AggregateCondition {
        AggregateCondition {
            aggregate: self,
            operator,
            value: crate::query::comparable_value(value),
        }
    }

    /// The select-list expression, with columns rendered by `format_column`.
    pub(super) fn render(&self, format_column: &dyn Fn(&str) -> String) -> String {
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
    pub(super) fn extreme_column(&self) -> Option<&str> {
        match self {
            Self::Min(column) | Self::Max(column) => Some(column),
            _ => None,
        }
    }

    /// The column this aggregate compares values of, with the operation an
    /// error names: randomized ciphertext cannot be ordered, summed or
    /// deduplicated, so none of these may read an encrypted column.
    pub(super) fn compared_column(&self) -> Option<(&str, &'static str)> {
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
#[must_use = "a HAVING condition filters nothing until `having()` takes it"]
pub struct AggregateCondition {
    pub(super) aggregate: Aggregate,
    pub(super) operator: &'static str,
    /// The value compared with, or why it cannot be compared.
    pub(super) value: std::result::Result<Value, String>,
}

/// What [`QueryBuilder::having`] keeps a group by: a comparison of an
/// aggregate, or a raw SQL condition, which
/// [`having_raw`](QueryBuilder::having_raw) adds by name.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
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
    pub fn group_by(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.clauses.group_by.push(Self::own_column(&column));
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
            return db_sql::built(
                db_type,
                &self.plain_select(db_type, select_list.join(", "))?,
            );
        }

        let (select_list, inner) = if rows_fixed_by_projection {
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
            (select_list, self.select_statement(db_type)?)
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
            (select_list, inner.select_statement(db_type)?)
        };
        let select =
            db_sql::select_from_derived(select_list.join(", "), inner, AGGREGATE_SUBQUERY_ALIAS);
        db_sql::built(db_type, &select)
    }
}
