#![allow(missing_docs)]

use super::QueryBuilder;
use crate::internal::Value;
use crate::model::Model;

/// Emit the shared `where_*` condition-builder method family as an inherent impl
/// on a builder that owns a `conditions: Vec<WhereCondition>` field. `OrGroup`
/// and `OrBranch` share this family verbatim; keep it single-source here rather
/// than hand-copying 17 methods per type.
macro_rules! impl_or_where_condition_methods {
    ($ty:ty) => {
        impl $ty {
            #[must_use]
            pub fn where_eq(
                mut self,
                column: impl crate::columns::IntoColumnName,
                value: impl Into<serde_json::Value>,
            ) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::Eq,
                    value: ConditionValue::Single(value.into()),
                });
                self
            }

            #[must_use]
            pub fn where_not(
                mut self,
                column: impl crate::columns::IntoColumnName,
                value: impl Into<serde_json::Value>,
            ) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::NotEq,
                    value: ConditionValue::Single(value.into()),
                });
                self
            }

            #[must_use]
            pub fn where_gt(
                mut self,
                column: impl crate::columns::IntoColumnName,
                value: impl Into<serde_json::Value>,
            ) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::Gt,
                    value: ConditionValue::Single(value.into()),
                });
                self
            }

            #[must_use]
            pub fn where_gte(
                mut self,
                column: impl crate::columns::IntoColumnName,
                value: impl Into<serde_json::Value>,
            ) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::Gte,
                    value: ConditionValue::Single(value.into()),
                });
                self
            }

            #[must_use]
            pub fn where_lt(
                mut self,
                column: impl crate::columns::IntoColumnName,
                value: impl Into<serde_json::Value>,
            ) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::Lt,
                    value: ConditionValue::Single(value.into()),
                });
                self
            }

            #[must_use]
            pub fn where_lte(
                mut self,
                column: impl crate::columns::IntoColumnName,
                value: impl Into<serde_json::Value>,
            ) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::Lte,
                    value: ConditionValue::Single(value.into()),
                });
                self
            }

            #[must_use]
            pub fn where_like(
                mut self,
                column: impl crate::columns::IntoColumnName,
                pattern: &str,
            ) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::Like,
                    value: ConditionValue::Single(serde_json::Value::String(pattern.to_string())),
                });
                self
            }

            #[must_use]
            pub fn where_contains(
                mut self,
                column: impl crate::columns::IntoColumnName,
                value: &str,
            ) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::LikeEscaped,
                    value: ConditionValue::Single(serde_json::Value::String(format!(
                        "%{}%",
                        crate::columns::escape_like_literal(value)
                    ))),
                });
                self
            }

            #[must_use]
            pub fn where_starts_with(
                mut self,
                column: impl crate::columns::IntoColumnName,
                value: &str,
            ) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::LikeEscaped,
                    value: ConditionValue::Single(serde_json::Value::String(format!(
                        "{}%",
                        crate::columns::escape_like_literal(value)
                    ))),
                });
                self
            }

            #[must_use]
            pub fn where_ends_with(
                mut self,
                column: impl crate::columns::IntoColumnName,
                value: &str,
            ) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::LikeEscaped,
                    value: ConditionValue::Single(serde_json::Value::String(format!(
                        "%{}",
                        crate::columns::escape_like_literal(value)
                    ))),
                });
                self
            }

            #[must_use]
            pub fn where_not_like(
                mut self,
                column: impl crate::columns::IntoColumnName,
                pattern: &str,
            ) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::NotLike,
                    value: ConditionValue::Single(serde_json::Value::String(pattern.to_string())),
                });
                self
            }

            #[must_use]
            pub fn where_in<V: Into<serde_json::Value>>(
                mut self,
                column: impl crate::columns::IntoColumnName,
                values: Vec<V>,
            ) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::In,
                    value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
                });
                self
            }

            #[must_use]
            pub fn where_not_in<V: Into<serde_json::Value>>(
                mut self,
                column: impl crate::columns::IntoColumnName,
                values: Vec<V>,
            ) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::NotIn,
                    value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
                });
                self
            }

            #[must_use]
            pub fn where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::IsNull,
                    value: ConditionValue::None,
                });
                self
            }

            #[must_use]
            pub fn where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::IsNotNull,
                    value: ConditionValue::None,
                });
                self
            }

            #[must_use]
            pub fn where_between(
                mut self,
                column: impl crate::columns::IntoColumnName,
                min: impl Into<serde_json::Value>,
                max: impl Into<serde_json::Value>,
            ) -> Self {
                self.conditions.push(WhereCondition {
                    column: column.column_name().to_string(),
                    operator: Operator::Between,
                    value: ConditionValue::Range(min.into(), max.into()),
                });
                self
            }

            #[must_use]
            pub fn where_raw(mut self, raw_sql: &str) -> Self {
                self.conditions.push(WhereCondition {
                    column: String::new(),
                    operator: Operator::Raw,
                    value: ConditionValue::RawExpr(raw_sql.to_string()),
                });
                self
            }
        }
    };
}

mod or_branch;
mod or_branch_builder;
mod or_group;

pub use or_branch::OrBranch;
pub use or_branch_builder::OrBranchBuilder;
pub use or_group::OrGroup;

/// Sort order for queries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Ascending order (A-Z, 1-9)
    Asc,
    /// Descending order (Z-A, 9-1)
    Desc,
}

impl Order {
    /// Convert to SQL string
    pub fn as_str(&self) -> &'static str {
        match self {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        }
    }
}

/// Comparison operators for where clauses
#[derive(Debug, Clone)]
pub enum Operator {
    Eq,
    NotEq,
    Gt,
    Gte,
    Lt,
    Lte,
    Like,
    LikeEscaped,
    NotLike,
    In,
    NotIn,
    IsNull,
    IsNotNull,
    Between,
    JsonContains,
    JsonContainedBy,
    JsonKeyExists,
    JsonKeyNotExists,
    JsonPathExists,
    JsonPathNotExists,
    ArrayContains,
    ArrayContainedBy,
    ArrayOverlaps,
    ArrayContainsAny,
    ArrayContainsAll,
    SubqueryIn,
    SubqueryNotIn,
    Raw,
    EqAny,
    NeAll,
}

/// A single where condition
#[derive(Debug, Clone)]
pub struct WhereCondition {
    pub column: String,
    pub operator: Operator,
    pub value: ConditionValue,
}

/// Value for a where condition
#[derive(Debug, Clone)]
pub enum ConditionValue {
    Single(serde_json::Value),
    List(Vec<serde_json::Value>),
    Range(serde_json::Value, serde_json::Value),
    None,
    Subquery(String),
    RawExpr(String),
    /// A builder-generated SQL fragment that carries its own bound parameters.
    ///
    /// Unlike [`ConditionValue::RawExpr`], which is emitted verbatim through
    /// `Expr::cust`, this variant keeps the operand's values out of the SQL text
    /// entirely. It exists for the operands the builder generates itself —
    /// `where_in_subquery`, `where_exists`, the `has_related` family — where the
    /// only alternative is hand-escaping user data into an inline literal.
    RawExprWithValues {
        /// The executable fragment.
        ///
        /// It must already use the *target backend's own* placeholder marker
        /// (`$1..$n` on PostgreSQL, `?` on MySQL/MariaDB/SQLite), because
        /// `Expr::cust_with_values` only renumbers tokens matching that marker
        /// into the surrounding statement; a mismatched marker binds nothing.
        sql: String,
        /// The parameters bound to `sql`, in placeholder order.
        values: Vec<Value>,
        /// An inline-literal rendering of the same fragment.
        ///
        /// Used only by the non-executable debug preview (and by the operand
        /// strings the preview renderer feeds to UNION/CTE clauses). It must
        /// never be executed with `values` bound alongside it.
        preview_sql: String,
    },
}

/// Logical operator for combining conditions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOp {
    And,
    Or,
}

impl LogicalOp {
    pub fn as_sql(&self) -> &'static str {
        match self {
            LogicalOp::And => "AND",
            LogicalOp::Or => "OR",
        }
    }
}
