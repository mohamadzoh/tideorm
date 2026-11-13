//! The project rules a query is held to (`crate::rules`), checked as it runs.

use super::builder::{DISTINCT_SELECT_MARKER, raw_order_by_expression};
use super::{ConditionValue, HavingClause, Operand, Operator, QueryBuilder, WindowFunctionType};
use crate::error::{Error, Result};
use crate::model::Model;
use crate::rules::{self, RuleScope};

impl<M: Model> QueryBuilder<M> {
    /// The query held to `scope`'s rules rather than all of them.
    pub(crate) fn with_rule_scope(mut self, scope: RuleScope) -> Self {
        self.clauses.rule_scope = scope;
        self
    }

    /// Refuse a query that breaks the project's query rules: raw SQL, too
    /// many joins or too long an `IN` list. Its rows are checked by the reads
    /// alone ([`ensure_row_limit`](Self::ensure_row_limit)).
    pub(crate) fn ensure_query_follows_rules(&self) -> Result<()> {
        if let Some(refusal) = self.raw_sql_refusal() {
            return Err(refusal);
        }
        if !self.clauses.rule_scope.checks_shape() {
            return Ok(());
        }
        let rules = M::__rules();
        let model = rules::short_name(std::any::type_name::<M>());
        let joins = self.clauses.joins.len();
        if let Some(max) = rules.max_joins.filter(|max| joins > *max as usize) {
            return Err(Error::query(format!(
                "this query of {model} joins {joins} tables, and the project rules allow {max} \
                 (max_joins in tideorm.toml)"
            )));
        }
        if let Some(max) = rules.max_in_values {
            let longest = self
                .all_conditions()
                .into_iter()
                .filter(|condition| matches!(condition.operator, Operator::In | Operator::NotIn))
                .filter_map(|condition| match &condition.value {
                    ConditionValue::List(values) => Some(values.len()),
                    _ => None,
                })
                .max()
                .unwrap_or(0);
            if longest > max as usize {
                return Err(Error::query(format!(
                    "this query of {model} lists {longest} values in one where_in, and the project \
                     rules allow {max} (max_in_values in tideorm.toml): use a subquery or a join"
                )));
            }
        }
        Ok(())
    }

    /// Refuse a read of more rows than the project rules allow a query.
    pub(crate) fn ensure_row_limit(&self) -> Result<()> {
        if !self.clauses.rule_scope.checks_rows() {
            return Ok(());
        }
        match self.clauses.limit_value {
            Some(limit) => {
                rules::ensure_rows_allowed::<M>(format_args!("limit({limit})"), Some(limit))
            }
            None => rules::ensure_rows_allowed::<M>("this query", None),
        }
    }

    /// Why the project rules refuse raw SQL in this query or in one it
    /// embeds; `None` when it may run.
    pub(crate) fn raw_sql_refusal(&self) -> Option<Error> {
        if !self.clauses.rule_scope.checks_raw_sql() || !rules::raw_sql_forbidden_anywhere() {
            return None;
        }
        if M::__rules().forbid_raw_sql
            && let Some(what) = self.raw_sql_used()
        {
            return Some(rules::raw_sql_refused(
                what,
                Some(std::any::type_name::<M>()),
            ));
        }
        let clauses = &self.clauses;
        let conditions = self.all_conditions();
        let subqueries = conditions
            .iter()
            .filter_map(|condition| match &condition.value {
                ConditionValue::Subquery(subquery) => subquery.raw_sql_refusal(),
                _ => None,
            });
        let projections = clauses
            .subquery_select_expressions
            .iter()
            .filter_map(|select| select.query.raw_sql_refusal());
        let unions = clauses
            .unions
            .iter()
            .filter_map(|union| match &union.operand {
                Operand::Query(query) => query.raw_sql_refusal(),
                Operand::Sql { .. } => None,
            });
        let ctes = clauses
            .ctes
            .iter()
            .filter_map(|cte| cte.query.as_ref()?.raw_sql_refusal());
        subqueries
            .chain(projections)
            .chain(unions)
            .chain(ctes)
            .next()
    }

    /// The builder call that wrote raw SQL into this query's own clauses.
    fn raw_sql_used(&self) -> Option<&'static str> {
        let clauses = &self.clauses;
        let raw_condition = self.all_conditions().into_iter().any(|condition| {
            matches!(
                condition.value,
                ConditionValue::RawExpr(_) | ConditionValue::RawTemplate { .. }
            )
        });
        let windows = || {
            clauses
                .window_functions
                .iter()
                .map(|window| &window.function)
        };
        [
            (raw_condition, "where_raw()"),
            (
                clauses
                    .raw_select_expressions
                    .iter()
                    .any(|expression| expression != DISTINCT_SELECT_MARKER),
                "select_raw()",
            ),
            (
                clauses
                    .order_by
                    .iter()
                    .any(|(term, _)| raw_order_by_expression(term).is_some()),
                "order_by_raw()",
            ),
            (
                clauses
                    .having_conditions
                    .iter()
                    .any(|having| matches!(having, HavingClause::Raw { .. })),
                "having_raw()",
            ),
            (
                clauses
                    .unions
                    .iter()
                    .any(|union| matches!(union.operand, Operand::Sql { .. })),
                "union_raw()",
            ),
            (
                clauses.ctes.iter().any(|cte| cte.query.is_none()),
                "a CTE written as SQL (CTE::new)",
            ),
            (
                windows().any(|function| matches!(function, WindowFunctionType::Custom(_))),
                "WindowFunctionType::Custom",
            ),
            (
                windows().any(|function| {
                    matches!(
                        function,
                        WindowFunctionType::Lag(_, _, Some(_))
                            | WindowFunctionType::Lead(_, _, Some(_))
                    )
                }),
                "lag_with_raw_default()/lead_with_raw_default()",
            ),
        ]
        .into_iter()
        .find_map(|(used, what)| used.then_some(what))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/query_rules_tests.rs"]
mod tests;
