//! Fluent query builder
//!
//! This module provides a fluent, chainable query builder API for TideORM.
//!
//! ## Example
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//!
//! // Simple query
//! let users = User::query()
//!     .where_eq("active", true)
//!     .get()
//!     .await?;
//!
//! // Complex query
//! let users = User::query()
//!     .where_eq("status", "active")
//!     .where_not("role", "admin")
//!     .where_in("department", vec!["engineering", "design"])
//!     .where_like("email", "%@company.com")
//!     .order_by("created_at", Order::Desc)
//!     .limit(10)
//!     .offset(20)
//!     .get()
//!     .await?;
//!
//! // Counting
//! let count = User::query()
//!     .where_eq("active", true)
//!     .count()
//!     .await?;
//!
//! // First record
//! let first = User::query()
//!     .where_eq("email", "admin@example.com")
//!     .first()
//!     .await?;
//!
//! // Bulk delete
//! let deleted = User::query()
//!     .where_eq("status", "inactive")
//!     .delete()
//!     .await?;
//! println!("Deleted {} records", deleted);
//! ```
//!
//! ## Database-Specific Support
//!
//! TideORM supports database-specific features across PostgreSQL, MySQL, and SQLite:
//!
//! - **JSON Operations**: PostgreSQL uses `@>`, `?`, `@?` operators; MySQL uses `JSON_CONTAINS()`,
//!   `JSON_EXTRACT()`; SQLite uses JSON1 extension functions.
//! - **Array Operations**: Native array support in PostgreSQL; emulated via JSON arrays in MySQL/SQLite.
//! - **RETURNING clause**: Supported in PostgreSQL and SQLite 3.35+; not supported in MySQL.

use std::marker::PhantomData;

use crate::error::{Error, Result};
use crate::model::Model;

mod advanced;
mod db_sql;
mod sql;

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
    /// Equal to
    Eq,
    /// Not equal to
    NotEq,
    /// Greater than
    Gt,
    /// Greater than or equal to
    Gte,
    /// Less than
    Lt,
    /// Less than or equal to
    Lte,
    /// LIKE pattern matching
    Like,
    /// NOT LIKE
    NotLike,
    /// IN list
    In,
    /// NOT IN list
    NotIn,
    /// IS NULL
    IsNull,
    /// IS NOT NULL
    IsNotNull,
    /// BETWEEN
    Between,
    /// JSON contains (@>)
    JsonContains,
    /// JSON contained by (<@)
    JsonContainedBy,
    /// JSON key exists (?)
    JsonKeyExists,
    /// JSON key does not exist (?!)
    JsonKeyNotExists,
    /// JSON path exists (@?)
    JsonPathExists,
    /// JSON path does not exist (?!)
    JsonPathNotExists,
    /// Array contains (@>)
    ArrayContains,
    /// Array contained by (<@)
    ArrayContainedBy,
    /// Array overlaps (&&)
    ArrayOverlaps,
    /// Array contains any element (&& with ANY)
    ArrayContainsAny,
    /// Array contains all elements (&< with ALL)
    ArrayContainsAll,
    /// Subquery IN
    SubqueryIn,
    /// Subquery NOT IN
    SubqueryNotIn,
    /// Raw expression
    Raw,
    /// = ANY(array) - PostgreSQL optimization for IN
    EqAny,
    /// <> ALL(array) - PostgreSQL optimization for NOT IN
    NeAll,
}

/// A single where condition
#[derive(Debug, Clone)]
pub struct WhereCondition {
    /// Column name
    pub column: String,
    /// Comparison operator
    pub operator: Operator,
    /// Value to compare against
    pub value: ConditionValue,
}

/// Value for a where condition
#[derive(Debug, Clone)]
pub enum ConditionValue {
    /// Single value
    Single(serde_json::Value),
    /// List of values (for IN, NOT IN)
    List(Vec<serde_json::Value>),
    /// Range (for BETWEEN)
    Range(serde_json::Value, serde_json::Value),
    /// No value (for IS NULL, IS NOT NULL)
    None,
    /// Subquery SQL string
    Subquery(String),
    /// Raw SQL expression
    RawExpr(String),
}

// =============================================================================
// OR CLAUSE SUPPORT
// =============================================================================

/// Logical operator for combining conditions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOp {
    /// AND - all conditions must match
    And,
    /// OR - any condition must match
    Or,
}

impl LogicalOp {
    /// Convert to SQL keyword
    pub fn as_sql(&self) -> &'static str {
        match self {
            LogicalOp::And => "AND",
            LogicalOp::Or => "OR",
        }
    }
}

/// A group of conditions combined with a logical operator
///
/// This allows building complex WHERE clauses like:
/// `WHERE (status = 'active' OR status = 'pending') AND category = 'books'`
///
/// # Example
///
/// ```rust,ignore
/// // Simple OR group
/// let users = User::query()
///     .or_where(|q| q
///         .where_eq("role", "admin")
///         .where_eq("role", "moderator")
///     )
///     .get()
///     .await?;
/// // Generates: WHERE (role = 'admin' OR role = 'moderator')
///
/// // Complex nested conditions
/// let products = Product::query()
///     .where_eq("active", true)
///     .or_where(|q| q
///         .where_lt("price", 100)
///         .where_eq("featured", true)
///     )
///     .get()
///     .await?;
/// // Generates: WHERE active = true AND (price < 100 OR featured = true)
/// ```
#[derive(Debug, Clone)]
pub struct OrGroup {
    /// Conditions in this OR group
    pub conditions: Vec<WhereCondition>,
    /// Nested OR groups (for complex nesting)
    pub nested_groups: Vec<OrGroup>,
    /// Whether this group is combined with AND or OR with siblings
    pub combine_with: LogicalOp,
}

impl OrGroup {
    /// Create a new OR group
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
            nested_groups: Vec::new(),
            combine_with: LogicalOp::Or,
        }
    }

    /// Add a where equals condition to this OR group
    pub fn where_eq(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::Eq,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a where not equals condition
    pub fn where_not(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::NotEq,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a where greater than condition
    pub fn where_gt(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::Gt,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a where greater than or equal condition
    pub fn where_gte(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::Gte,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a where less than condition
    pub fn where_lt(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::Lt,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a where less than or equal condition
    pub fn where_lte(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::Lte,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a where LIKE condition
    pub fn where_like(mut self, column: &str, pattern: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::Like,
            value: ConditionValue::Single(serde_json::Value::String(pattern.to_string())),
        });
        self
    }

    /// Add a where NOT LIKE condition
    pub fn where_not_like(mut self, column: &str, pattern: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::NotLike,
            value: ConditionValue::Single(serde_json::Value::String(pattern.to_string())),
        });
        self
    }

    /// Add a where IN condition
    pub fn where_in<V: Into<serde_json::Value>>(mut self, column: &str, values: Vec<V>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::In,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add a where NOT IN condition
    pub fn where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::NotIn,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add a where IS NULL condition
    pub fn where_null(mut self, column: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::IsNull,
            value: ConditionValue::None,
        });
        self
    }

    /// Add a where IS NOT NULL condition
    pub fn where_not_null(mut self, column: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::IsNotNull,
            value: ConditionValue::None,
        });
        self
    }

    /// Add a where BETWEEN condition
    pub fn where_between(
        mut self,
        column: &str,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::Between,
            value: ConditionValue::Range(min.into(), max.into()),
        });
        self
    }

    /// Add a raw WHERE condition
    pub fn where_raw(mut self, raw_sql: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(raw_sql.to_string()),
        });
        self
    }

    /// Nest another OR group within this group
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let query = User::query()
    ///     .or_where(|q| q
    ///         .where_eq("status", "active")
    ///         .nested_or(|inner| inner
    ///             .where_eq("role", "admin")
    ///             .where_eq("role", "super_admin")
    ///         )
    ///     )
    ///     .get()
    ///     .await?;
    /// // Generates: WHERE (status = 'active' OR (role = 'admin' OR role = 'super_admin'))
    /// ```
    pub fn nested_or<F>(mut self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        let mut nested = OrGroup::new();
        nested.combine_with = LogicalOp::Or;
        nested = f(nested);
        self.nested_groups.push(nested);
        self
    }

    /// Nest an AND group within this OR group
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let query = User::query()
    ///     .or_where(|q| q
    ///         .nested_and(|inner| inner
    ///             .where_eq("status", "active")
    ///             .where_eq("verified", true)
    ///         )
    ///         .nested_and(|inner| inner
    ///             .where_eq("role", "admin")
    ///         )
    ///     )
    ///     .get()
    ///     .await?;
    /// // Generates: WHERE ((status = 'active' AND verified = true) OR (role = 'admin'))
    /// ```
    pub fn nested_and<F>(mut self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        let mut nested = OrGroup::new();
        nested.combine_with = LogicalOp::And;
        nested = f(nested);
        self.nested_groups.push(nested);
        self
    }

    /// Check if the group is empty
    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty() && self.nested_groups.is_empty()
    }

    /// Get the total number of conditions (including nested)
    pub fn condition_count(&self) -> usize {
        let nested_count: usize = self.nested_groups.iter().map(|g| g.condition_count()).sum();
        self.conditions.len() + nested_count
    }
}

impl Default for OrGroup {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// OR BRANCH BUILDER (Fluent API for chained OR conditions)
// =============================================================================

/// A single branch in an OR expression
///
/// Each branch contains conditions that are combined with AND logic,
/// and branches are combined with OR logic.
///
/// # Example
///
/// ```rust,ignore
/// // Two branches: (role = 'admin' AND active = true) OR (role = 'moderator' AND verified = true)
/// let branch1 = OrBranch::new()
///     .where_eq("role", "admin")
///     .where_eq("active", true);
/// let branch2 = OrBranch::new()
///     .where_eq("role", "moderator")
///     .where_eq("verified", true);
/// ```
#[derive(Debug, Clone)]
pub struct OrBranch {
    /// Conditions within this branch (combined with AND)
    pub conditions: Vec<WhereCondition>,
}

impl OrBranch {
    /// Create a new empty OR branch
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
        }
    }

    /// Add a where equals condition to this branch
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

    /// Add a where not equals condition
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

    /// Add a where greater than condition
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

    /// Add a where greater than or equal condition
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

    /// Add a where less than condition
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

    /// Add a where less than or equal condition
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

    /// Add a where LIKE condition
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

    /// Add a where NOT LIKE condition
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

    /// Add a where IN condition
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

    /// Add a where NOT IN condition
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

    /// Add a where IS NULL condition
    pub fn where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNull,
            value: ConditionValue::None,
        });
        self
    }

    /// Add a where IS NOT NULL condition
    pub fn where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNotNull,
            value: ConditionValue::None,
        });
        self
    }

    /// Add a where BETWEEN condition
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

    /// Add a raw WHERE condition
    pub fn where_raw(mut self, raw_sql: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(raw_sql.to_string()),
        });
        self
    }

    /// Check if the branch is empty
    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty()
    }

    /// Get the number of conditions in this branch
    pub fn len(&self) -> usize {
        self.conditions.len()
    }
}

impl Default for OrBranch {
    fn default() -> Self {
        Self::new()
    }
}

/// Fluent builder for chaining OR conditions with AND modifiers
///
/// This builder allows creating complex OR expressions where each OR branch
/// can have multiple AND conditions.
///
/// # Example
///
/// ```rust,ignore
/// // Complex OR with AND conditions in each branch:
/// // WHERE active = true AND (
/// //     (role = 'admin' AND verified = true) OR
/// //     (role = 'moderator' AND age > 25) OR
/// //     role = 'superuser'
/// // )
/// let users = User::query()
///     .where_eq("active", true)
///     .or_where_eq("role", "admin").and_where_eq("verified", true)
///     .or_where_eq("role", "moderator").and_where_gt("age", 25)
///     .or_where_eq("role", "superuser")
///     .end_or()
///     .get()
///     .await?;
/// ```
#[derive(Debug)]
pub struct OrBranchBuilder<M: Model> {
    /// The query builder we're building on
    query: QueryBuilder<M>,
    /// All completed branches
    branches: Vec<OrBranch>,
    /// Current branch being built
    current_branch: OrBranch,
}

impl<M: Model> OrBranchBuilder<M> {
    /// Create a new OR branch builder from a QueryBuilder
    pub fn new(query: QueryBuilder<M>) -> Self {
        Self {
            query,
            branches: Vec::new(),
            current_branch: OrBranch::new(),
        }
    }

    /// Start a new OR branch with an equals condition
    ///
    /// This finishes the current branch (if any) and starts a new one.
    pub fn or_where_eq(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        // Save current branch if it has conditions
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        // Start new branch with the condition
        self.current_branch = OrBranch::new().where_eq(column, value);
        self
    }

    /// Start a new OR branch with a not equals condition
    pub fn or_where_not(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_not(column, value);
        self
    }

    /// Start a new OR branch with a greater than condition
    pub fn or_where_gt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_gt(column, value);
        self
    }

    /// Start a new OR branch with a greater than or equal condition
    pub fn or_where_gte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_gte(column, value);
        self
    }

    /// Start a new OR branch with a less than condition
    pub fn or_where_lt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_lt(column, value);
        self
    }

    /// Start a new OR branch with a less than or equal condition
    pub fn or_where_lte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_lte(column, value);
        self
    }

    /// Start a new OR branch with a LIKE condition
    pub fn or_where_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_like(column, pattern);
        self
    }

    /// Start a new OR branch with an IN condition
    pub fn or_where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_in(column, values);
        self
    }

    /// Start a new OR branch with a NOT IN condition
    pub fn or_where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_not_in(column, values);
        self
    }

    /// Start a new OR branch with an IS NULL condition
    pub fn or_where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_null(column);
        self
    }

    /// Start a new OR branch with an IS NOT NULL condition
    pub fn or_where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_not_null(column);
        self
    }

    /// Start a new OR branch with a BETWEEN condition
    pub fn or_where_between(
        mut self,
        column: impl crate::columns::IntoColumnName,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_between(column, min, max);
        self
    }

    /// Start a new OR branch with a raw SQL condition
    pub fn or_where_raw(mut self, raw_sql: &str) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_raw(raw_sql);
        self
    }

    // =========================================================================
    // AND modifiers for current branch
    // =========================================================================

    /// Add an AND equals condition to the current OR branch
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // (role = 'admin' AND active = true) OR (role = 'moderator')
    /// User::query()
    ///     .or_where_eq("role", "admin").and_where_eq("active", true)
    ///     .or_where_eq("role", "moderator")
    ///     .end_or()
    /// ```
    pub fn and_where_eq(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_eq(column, value);
        self
    }

    /// Add an AND not equals condition to the current OR branch
    pub fn and_where_not(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_not(column, value);
        self
    }

    /// Add an AND greater than condition to the current OR branch
    pub fn and_where_gt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_gt(column, value);
        self
    }

    /// Add an AND greater than or equal condition to the current OR branch
    pub fn and_where_gte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_gte(column, value);
        self
    }

    /// Add an AND less than condition to the current OR branch
    pub fn and_where_lt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_lt(column, value);
        self
    }

    /// Add an AND less than or equal condition to the current OR branch
    pub fn and_where_lte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_lte(column, value);
        self
    }

    /// Add an AND LIKE condition to the current OR branch
    pub fn and_where_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        self.current_branch = self.current_branch.where_like(column, pattern);
        self
    }

    /// Add an AND NOT LIKE condition to the current OR branch
    pub fn and_where_not_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        self.current_branch = self.current_branch.where_not_like(column, pattern);
        self
    }

    /// Add an AND IN condition to the current OR branch
    pub fn and_where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.current_branch = self.current_branch.where_in(column, values);
        self
    }

    /// Add an AND NOT IN condition to the current OR branch
    pub fn and_where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.current_branch = self.current_branch.where_not_in(column, values);
        self
    }

    /// Add an AND IS NULL condition to the current OR branch
    pub fn and_where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.current_branch = self.current_branch.where_null(column);
        self
    }

    /// Add an AND IS NOT NULL condition to the current OR branch
    pub fn and_where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.current_branch = self.current_branch.where_not_null(column);
        self
    }

    /// Add an AND BETWEEN condition to the current OR branch
    pub fn and_where_between(
        mut self,
        column: impl crate::columns::IntoColumnName,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_between(column, min, max);
        self
    }

    /// Add an AND raw SQL condition to the current OR branch
    pub fn and_where_raw(mut self, raw_sql: &str) -> Self {
        self.current_branch = self.current_branch.where_raw(raw_sql);
        self
    }

    // =========================================================================
    // Finish and return to QueryBuilder
    // =========================================================================

    /// Finish building OR branches and return to the QueryBuilder
    ///
    /// This converts all branches into an OrGroup and adds it to the query.
    pub fn end_or(mut self) -> QueryBuilder<M> {
        // Save current branch if it has conditions
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }

        // Convert branches to OrGroup format
        if !self.branches.is_empty() {
            let mut or_group = OrGroup::new();

            for branch in self.branches {
                if branch.conditions.len() == 1 {
                    // Single condition - add directly to OR group
                    if let Some(condition) = branch.conditions.into_iter().next() {
                        or_group.conditions.push(condition);
                    }
                } else {
                    // Multiple conditions - create nested AND group
                    let mut nested = OrGroup::new();
                    nested.combine_with = LogicalOp::And;
                    nested.conditions = branch.conditions;
                    or_group.nested_groups.push(nested);
                }
            }

            self.query.or_groups.push(or_group);
        }

        self.query
    }

    /// Get the number of branches currently built
    pub fn branch_count(&self) -> usize {
        let current = if self.current_branch.is_empty() { 0 } else { 1 };
        self.branches.len() + current
    }

    /// Get the total number of conditions across all branches
    pub fn total_conditions(&self) -> usize {
        let mut total: usize = self.branches.iter().map(|b| b.len()).sum();
        total += self.current_branch.len();
        total
    }
}

/// Type of JOIN operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinType {
    /// INNER JOIN - returns rows with matches in both tables
    Inner,
    /// LEFT JOIN - returns all rows from left table, matched rows from right
    Left,
    /// RIGHT JOIN - returns all rows from right table, matched rows from left
    Right,
}

impl JoinType {
    /// Convert to SQL keyword
    pub fn as_sql(&self) -> &'static str {
        match self {
            JoinType::Inner => "INNER JOIN",
            JoinType::Left => "LEFT JOIN",
            JoinType::Right => "RIGHT JOIN",
        }
    }
}

/// A JOIN clause
#[derive(Debug, Clone)]
pub struct JoinClause {
    /// Type of join
    pub join_type: JoinType,
    /// Table to join
    pub table: String,
    /// Optional alias for the joined table
    pub alias: Option<String>,
    /// ON condition: left column
    pub left_column: String,
    /// ON condition: right column
    pub right_column: String,
}

/// Aggregate function types
#[derive(Debug, Clone)]
pub enum AggregateFunction {
    /// COUNT aggregate
    Count,
    /// COUNT(DISTINCT column)
    CountDistinct(String),
    /// SUM aggregate
    Sum(String),
    /// AVG aggregate
    Avg(String),
    /// MIN aggregate
    Min(String),
    /// MAX aggregate
    Max(String),
}

// =============================================================================
// UNION TYPES
// =============================================================================

/// Type of UNION operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnionType {
    /// UNION - combines results and removes duplicates
    Union,
    /// UNION ALL - combines all results including duplicates
    UnionAll,
}

impl UnionType {
    /// Convert to SQL keyword
    pub fn as_sql(&self) -> &'static str {
        match self {
            UnionType::Union => "UNION",
            UnionType::UnionAll => "UNION ALL",
        }
    }
}

/// A UNION clause containing the union type and the SQL query to union
#[derive(Debug, Clone)]
pub struct UnionClause {
    /// Type of union (UNION or UNION ALL)
    pub union_type: UnionType,
    /// SQL query string to union with
    pub query_sql: String,
}

// =============================================================================
// WINDOW FUNCTION TYPES
// =============================================================================

/// Window function frame boundary
#[derive(Debug, Clone)]
pub enum FrameBound {
    /// UNBOUNDED PRECEDING
    UnboundedPreceding,
    /// UNBOUNDED FOLLOWING
    UnboundedFollowing,
    /// CURRENT ROW
    CurrentRow,
    /// N PRECEDING
    Preceding(u64),
    /// N FOLLOWING
    Following(u64),
}

impl FrameBound {
    /// Convert to SQL string
    pub fn as_sql(&self) -> String {
        match self {
            FrameBound::UnboundedPreceding => "UNBOUNDED PRECEDING".to_string(),
            FrameBound::UnboundedFollowing => "UNBOUNDED FOLLOWING".to_string(),
            FrameBound::CurrentRow => "CURRENT ROW".to_string(),
            FrameBound::Preceding(n) => format!("{} PRECEDING", n),
            FrameBound::Following(n) => format!("{} FOLLOWING", n),
        }
    }
}

/// Window function frame type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    /// ROWS frame
    Rows,
    /// RANGE frame
    Range,
    /// GROUPS frame (PostgreSQL 11+, SQLite 3.28+)
    Groups,
}

impl FrameType {
    /// Convert to SQL keyword
    pub fn as_sql(&self) -> &'static str {
        match self {
            FrameType::Rows => "ROWS",
            FrameType::Range => "RANGE",
            FrameType::Groups => "GROUPS",
        }
    }
}

/// Window function type
#[derive(Debug, Clone)]
pub enum WindowFunctionType {
    /// ROW_NUMBER()
    RowNumber,
    /// RANK()
    Rank,
    /// DENSE_RANK()
    DenseRank,
    /// NTILE(n)
    Ntile(u32),
    /// LAG(column, offset, default)
    Lag(String, Option<i32>, Option<String>),
    /// LEAD(column, offset, default)
    Lead(String, Option<i32>, Option<String>),
    /// FIRST_VALUE(column)
    FirstValue(String),
    /// LAST_VALUE(column)
    LastValue(String),
    /// NTH_VALUE(column, n)
    NthValue(String, u32),
    /// SUM(column) OVER
    Sum(String),
    /// AVG(column) OVER
    Avg(String),
    /// COUNT(*) OVER or COUNT(column) OVER
    Count(Option<String>),
    /// MIN(column) OVER
    Min(String),
    /// MAX(column) OVER
    Max(String),
    /// Custom window function expression
    Custom(String),
}

impl WindowFunctionType {
    /// Convert to SQL string (function part only)
    pub fn as_sql(&self) -> String {
        match self {
            WindowFunctionType::RowNumber => "ROW_NUMBER()".to_string(),
            WindowFunctionType::Rank => "RANK()".to_string(),
            WindowFunctionType::DenseRank => "DENSE_RANK()".to_string(),
            WindowFunctionType::Ntile(n) => format!("NTILE({})", n),
            WindowFunctionType::Lag(col, offset, default) => {
                let mut s = format!("LAG(\"{}\"", col);
                if let Some(o) = offset {
                    s.push_str(&format!(", {}", o));
                    if let Some(d) = default {
                        s.push_str(&format!(", {}", d));
                    }
                }
                s.push(')');
                s
            }
            WindowFunctionType::Lead(col, offset, default) => {
                let mut s = format!("LEAD(\"{}\"", col);
                if let Some(o) = offset {
                    s.push_str(&format!(", {}", o));
                    if let Some(d) = default {
                        s.push_str(&format!(", {}", d));
                    }
                }
                s.push(')');
                s
            }
            WindowFunctionType::FirstValue(col) => format!("FIRST_VALUE(\"{}\")", col),
            WindowFunctionType::LastValue(col) => format!("LAST_VALUE(\"{}\")", col),
            WindowFunctionType::NthValue(col, n) => format!("NTH_VALUE(\"{}\", {})", col, n),
            WindowFunctionType::Sum(col) => format!("SUM(\"{}\")", col),
            WindowFunctionType::Avg(col) => format!("AVG(\"{}\")", col),
            WindowFunctionType::Count(col) => match col {
                Some(c) => format!("COUNT(\"{}\")", c),
                None => "COUNT(*)".to_string(),
            },
            WindowFunctionType::Min(col) => format!("MIN(\"{}\")", col),
            WindowFunctionType::Max(col) => format!("MAX(\"{}\")", col),
            WindowFunctionType::Custom(expr) => expr.clone(),
        }
    }
}

/// A complete window function definition
#[derive(Debug, Clone)]
pub struct WindowFunction {
    /// The window function type/expression
    pub function: WindowFunctionType,
    /// PARTITION BY columns
    pub partition_by: Vec<String>,
    /// ORDER BY columns with direction
    pub order_by: Vec<(String, Order)>,
    /// Frame type (ROWS, RANGE, GROUPS)
    pub frame_type: Option<FrameType>,
    /// Frame start bound
    pub frame_start: Option<FrameBound>,
    /// Frame end bound  
    pub frame_end: Option<FrameBound>,
    /// Alias for the result column
    pub alias: String,
}

impl WindowFunction {
    /// Create a new window function
    pub fn new(function: WindowFunctionType, alias: &str) -> Self {
        Self {
            function,
            partition_by: Vec::new(),
            order_by: Vec::new(),
            frame_type: None,
            frame_start: None,
            frame_end: None,
            alias: alias.to_string(),
        }
    }

    /// Add PARTITION BY column
    pub fn partition_by(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.partition_by.push(column.column_name().to_string());
        self
    }

    /// Add ORDER BY column
    pub fn order_by(
        mut self,
        column: impl crate::columns::IntoColumnName,
        direction: Order,
    ) -> Self {
        self.order_by
            .push((column.column_name().to_string(), direction));
        self
    }

    /// Set frame specification
    pub fn frame(mut self, frame_type: FrameType, start: FrameBound, end: FrameBound) -> Self {
        self.frame_type = Some(frame_type);
        self.frame_start = Some(start);
        self.frame_end = Some(end);
        self
    }

    /// Convert to SQL string
    pub fn to_sql(&self) -> String {
        let mut sql = self.function.as_sql();
        sql.push_str(" OVER (");

        let mut clauses = Vec::new();

        if !self.partition_by.is_empty() {
            let cols: Vec<String> = self
                .partition_by
                .iter()
                .map(|c| format!("\"{}\"", c))
                .collect();
            clauses.push(format!("PARTITION BY {}", cols.join(", ")));
        }

        if !self.order_by.is_empty() {
            let orders: Vec<String> = self
                .order_by
                .iter()
                .map(|(col, dir)| format!("\"{}\" {}", col, dir.as_str()))
                .collect();
            clauses.push(format!("ORDER BY {}", orders.join(", ")));
        }

        if let (Some(frame_type), Some(start)) = (&self.frame_type, &self.frame_start) {
            let frame_sql = if let Some(end) = &self.frame_end {
                format!(
                    "{} BETWEEN {} AND {}",
                    frame_type.as_sql(),
                    start.as_sql(),
                    end.as_sql()
                )
            } else {
                format!("{} {}", frame_type.as_sql(), start.as_sql())
            };
            clauses.push(frame_sql);
        }

        sql.push_str(&clauses.join(" "));
        sql.push_str(&format!(") AS \"{}\"", self.alias));

        sql
    }
}

// =============================================================================
// CTE (Common Table Expression) TYPES
// =============================================================================

/// A Common Table Expression (CTE) definition
#[derive(Debug, Clone)]
pub struct CTE {
    /// CTE name
    pub name: String,
    /// Column names (optional)
    pub columns: Option<Vec<String>>,
    /// CTE query SQL
    pub query_sql: String,
    /// Whether this is a recursive CTE
    pub recursive: bool,
}

impl CTE {
    /// Create a new CTE
    pub fn new(name: &str, query_sql: String) -> Self {
        Self {
            name: name.to_string(),
            columns: None,
            query_sql,
            recursive: false,
        }
    }

    /// Create a new CTE with column names
    pub fn with_columns(name: &str, columns: Vec<&str>, query_sql: String) -> Self {
        Self {
            name: name.to_string(),
            columns: Some(columns.into_iter().map(|s| s.to_string()).collect()),
            query_sql,
            recursive: false,
        }
    }

    /// Mark this CTE as recursive
    pub fn recursive(mut self) -> Self {
        self.recursive = true;
        self
    }

    /// Convert to SQL string (the CTE definition, not including WITH)
    pub fn to_sql(&self) -> String {
        let mut sql = format!("\"{}\"", self.name);

        if let Some(ref cols) = self.columns {
            let col_list: Vec<String> = cols.iter().map(|c| format!("\"{}\"", c)).collect();
            sql.push_str(&format!(" ({})", col_list.join(", ")));
        }

        sql.push_str(&format!(" AS ({})", self.query_sql));

        sql
    }
}

// =============================================================================
// QUERY FRAGMENT (SeaORM consolidate() support)
// =============================================================================

/// A reusable query fragment that can be applied to multiple queries
///
/// Query fragments allow you to consolidate common query conditions, ordering,
/// and other clauses into a reusable unit. This is useful for:
///
/// - Defining reusable scopes (e.g., "active users", "recent posts")
/// - Applying consistent filtering across multiple queries
/// - Composing complex queries from smaller building blocks
///
/// # Example
///
/// ```rust,ignore
/// // Create a reusable "active and verified" filter
/// let active_verified = User::query()
///     .where_eq("status", "active")
///     .where_eq("verified", true)
///     .consolidate();
///
/// // Apply to different queries
/// let recent_active = User::query()
///     .apply(&active_verified)
///     .order_by("created_at", Order::Desc)
///     .limit(10)
///     .get()
///     .await?;
///
/// let active_count = User::query()
///     .apply(&active_verified)
///     .count()
///     .await?;
/// ```
#[derive(Debug, Clone)]
pub struct QueryFragment<M: Model> {
    _marker: PhantomData<M>,
    /// WHERE conditions to apply
    pub conditions: Vec<WhereCondition>,
    /// ORDER BY clauses to apply
    pub order_by: Vec<(String, Order)>,
    /// GROUP BY columns
    pub group_by: Vec<String>,
    /// HAVING conditions
    pub having_conditions: Vec<String>,
    /// JOIN clauses
    pub joins: Vec<JoinClause>,
    /// Deferred query validation error
    pub invalid_query_reason: Option<String>,
    /// Soft delete: include trashed records
    pub include_trashed: bool,
    /// Soft delete: only show trashed records
    pub only_trashed: bool,
}

impl<M: Model> Default for QueryFragment<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: Model> QueryFragment<M> {
    /// Create an empty query fragment
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
            conditions: Vec::new(),
            order_by: Vec::new(),
            group_by: Vec::new(),
            having_conditions: Vec::new(),
            joins: Vec::new(),
            invalid_query_reason: None,
            include_trashed: false,
            only_trashed: false,
        }
    }

    /// Check if this fragment has any conditions or clauses
    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty()
            && self.order_by.is_empty()
            && self.group_by.is_empty()
            && self.having_conditions.is_empty()
            && self.joins.is_empty()
            && self.invalid_query_reason.is_none()
    }

    /// Get the number of conditions in this fragment
    pub fn condition_count(&self) -> usize {
        self.conditions.len()
    }
}

// =============================================================================
// JOIN RESULT CONSOLIDATION (SeaORM 2.0 SelectThree::consolidate() equivalent)
// =============================================================================

/// Consolidates flat join results into nested structures
///
/// When you query with joins, you get flat tuples like:
/// `[(order1, customer1, line1), (order1, customer1, line2), (order2, customer2, line3)]`
///
/// This consolidator groups them into nested structures:
/// `[(order1, customer1, [line1, line2]), (order2, customer2, [line3])]`
///
/// # Example
///
/// ```rust,ignore
/// use tideorm::query::JoinResultConsolidator;
///
/// // Flat join results
/// let flat: Vec<(Order, Customer, LineItem)> = Order::query()
///     .inner_join("customers", "orders.customer_id", "customers.id")
///     .inner_join("lineitems", "orders.id", "lineitems.order_id")
///     .get_tuples()
///     .await?;
///
/// // Consolidate by order id, then customer id
/// let nested: Vec<(Order, Vec<(Customer, Vec<LineItem>)>)> =
///     JoinResultConsolidator::consolidate_three(
///         flat,
///         |o| o.id,      // Group by order id
///         |c| c.id,      // Then by customer id
///     );
/// ```
pub struct JoinResultConsolidator;

impl JoinResultConsolidator {
    /// Consolidate two-way join results: `Vec<(A, B)>` -> `Vec<(A, Vec<B>)>`
    ///
    /// Groups all B records under their parent A record.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let flat: Vec<(User, Post)> = results;
    /// let nested: Vec<(User, Vec<Post>)> =
    ///     JoinResultConsolidator::consolidate_two(flat, |u| u.id);
    /// ```
    pub fn consolidate_two<A, B, K, F>(items: Vec<(A, B)>, key_fn: F) -> Vec<(A, Vec<B>)>
    where
        A: Clone,
        K: Eq + std::hash::Hash,
        F: Fn(&A) -> K,
    {
        use std::collections::HashMap;

        let mut groups: HashMap<K, (A, Vec<B>)> = HashMap::new();
        let mut order: Vec<K> = Vec::new();

        for (a, b) in items {
            let key = key_fn(&a);
            if let Some((_, bs)) = groups.get_mut(&key) {
                bs.push(b);
            } else {
                order.push(key_fn(&a));
                groups.insert(key, (a, vec![b]));
            }
        }

        // Preserve original order
        order
            .into_iter()
            .filter_map(|k| groups.remove(&k))
            .collect()
    }

    /// Consolidate two-way join results with optional second item: `Vec<(A, Option<B>)>` -> `Vec<(A, Vec<B>)>`
    ///
    /// Handles LEFT JOIN results where B might be NULL.
    pub fn consolidate_two_optional<A, B, K, F>(
        items: Vec<(A, Option<B>)>,
        key_fn: F,
    ) -> Vec<(A, Vec<B>)>
    where
        A: Clone,
        K: Eq + std::hash::Hash,
        F: Fn(&A) -> K,
    {
        use std::collections::HashMap;

        let mut groups: HashMap<K, (A, Vec<B>)> = HashMap::new();
        let mut order: Vec<K> = Vec::new();

        for (a, maybe_b) in items {
            let key = key_fn(&a);
            if let Some((_, bs)) = groups.get_mut(&key) {
                if let Some(b) = maybe_b {
                    bs.push(b);
                }
            } else {
                order.push(key_fn(&a));
                let bs = maybe_b.into_iter().collect();
                groups.insert(key, (a, bs));
            }
        }

        order
            .into_iter()
            .filter_map(|k| groups.remove(&k))
            .collect()
    }

    /// Consolidate three-way join results: `Vec<(A, B, C)>` -> `Vec<(A, Vec<(B, Vec<C>)>)>`
    ///
    /// Groups C records under B, then B groups under A.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Flat: [(order1, line1, product1), (order1, line1, product2), (order1, line2, product3)]
    /// // Nested: [(order1, [(line1, [product1, product2]), (line2, [product3])])]
    /// let nested = JoinResultConsolidator::consolidate_three(
    ///     flat,
    ///     |o| o.id,  // Group by order
    ///     |l| l.id,  // Then by line item
    /// );
    /// ```
    #[allow(clippy::type_complexity)]
    pub fn consolidate_three<A, B, C, KA, KB, FA, FB>(
        items: Vec<(A, B, C)>,
        key_a: FA,
        key_b: FB,
    ) -> Vec<(A, Vec<(B, Vec<C>)>)>
    where
        A: Clone,
        B: Clone,
        KA: Eq + std::hash::Hash + Clone,
        KB: Eq + std::hash::Hash + Clone,
        FA: Fn(&A) -> KA,
        FB: Fn(&B) -> KB,
    {
        use std::collections::HashMap;

        // First, group by A
        let mut a_groups: HashMap<KA, (A, HashMap<KB, (B, Vec<C>)>, Vec<KB>)> = HashMap::new();
        let mut a_order: Vec<KA> = Vec::new();

        for (a, b, c) in items {
            let ka = key_a(&a);
            let kb = key_b(&b);

            if let Some((_, b_groups, b_order)) = a_groups.get_mut(&ka) {
                if let Some((_, cs)) = b_groups.get_mut(&kb) {
                    cs.push(c);
                } else {
                    b_order.push(kb.clone());
                    b_groups.insert(kb, (b, vec![c]));
                }
            } else {
                a_order.push(ka.clone());
                let mut b_groups = HashMap::new();
                let b_order = vec![kb.clone()];
                b_groups.insert(kb, (b, vec![c]));
                a_groups.insert(ka, (a, b_groups, b_order));
            }
        }

        // Convert to nested structure preserving order
        a_order
            .into_iter()
            .filter_map(|ka| {
                a_groups.remove(&ka).map(|(a, mut b_groups, b_order)| {
                    let bs: Vec<(B, Vec<C>)> = b_order
                        .into_iter()
                        .filter_map(|kb| b_groups.remove(&kb))
                        .collect();
                    (a, bs)
                })
            })
            .collect()
    }

    /// Consolidate with optional third item (for LEFT JOINs)
    #[allow(clippy::type_complexity)]
    pub fn consolidate_three_optional<A, B, C, KA, KB, FA, FB>(
        items: Vec<(A, B, Option<C>)>,
        key_a: FA,
        key_b: FB,
    ) -> Vec<(A, Vec<(B, Vec<C>)>)>
    where
        A: Clone,
        B: Clone,
        KA: Eq + std::hash::Hash + Clone,
        KB: Eq + std::hash::Hash + Clone,
        FA: Fn(&A) -> KA,
        FB: Fn(&B) -> KB,
    {
        use std::collections::HashMap;

        let mut a_groups: HashMap<KA, (A, HashMap<KB, (B, Vec<C>)>, Vec<KB>)> = HashMap::new();
        let mut a_order: Vec<KA> = Vec::new();

        for (a, b, maybe_c) in items {
            let ka = key_a(&a);
            let kb = key_b(&b);

            if let Some((_, b_groups, b_order)) = a_groups.get_mut(&ka) {
                if let Some((_, cs)) = b_groups.get_mut(&kb) {
                    if let Some(c) = maybe_c {
                        cs.push(c);
                    }
                } else {
                    b_order.push(kb.clone());
                    let cs = maybe_c.into_iter().collect();
                    b_groups.insert(kb, (b, cs));
                }
            } else {
                a_order.push(ka.clone());
                let mut b_groups = HashMap::new();
                let b_order = vec![kb.clone()];
                let cs = maybe_c.into_iter().collect();
                b_groups.insert(kb, (b, cs));
                a_groups.insert(ka, (a, b_groups, b_order));
            }
        }

        a_order
            .into_iter()
            .filter_map(|ka| {
                a_groups.remove(&ka).map(|(a, mut b_groups, b_order)| {
                    let bs: Vec<(B, Vec<C>)> = b_order
                        .into_iter()
                        .filter_map(|kb| b_groups.remove(&kb))
                        .collect();
                    (a, bs)
                })
            })
            .collect()
    }
}

/// Fluent query builder for TideORM models
///
/// The query builder provides a chainable API for constructing database queries.
/// All methods return `Self` to allow chaining.
///
/// # Example
///
/// ```rust,ignore
/// let users = User::query()
///     .where_eq("status", "active")
///     .order_by("name", Order::Asc)
///     .limit(10)
///     .get(&db)
///     .await?;
/// ```
#[derive(Debug, Clone)]
pub struct QueryBuilder<M: Model> {
    _marker: PhantomData<M>,
    /// WHERE conditions (combined with AND)
    pub conditions: Vec<WhereCondition>,
    /// OR groups for complex conditions
    pub or_groups: Vec<OrGroup>,
    order_by: Vec<(String, Order)>,
    limit_value: Option<u64>,
    offset_value: Option<u64>,
    select_columns: Option<Vec<String>>,
    raw_select_expressions: Vec<String>,
    include_trashed: bool,
    only_trashed: bool,
    joins: Vec<JoinClause>,
    invalid_query_reason: Option<String>,
    group_by: Vec<String>,
    having_conditions: Vec<String>,
    /// UNION clauses
    unions: Vec<UnionClause>,
    /// Window functions to include in SELECT
    window_functions: Vec<WindowFunction>,
    /// Common Table Expressions (CTEs)
    ctes: Vec<CTE>,
    /// Cache options for this query
    cache_options: Option<crate::cache::CacheOptions>,
    /// Custom cache key (overrides generated key)
    cache_key: Option<String>,
}

impl<M: Model> QueryBuilder<M> {
    /// Create a new query builder
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
            conditions: Vec::new(),
            or_groups: Vec::new(),
            order_by: Vec::new(),
            limit_value: None,
            offset_value: None,
            select_columns: None,
            raw_select_expressions: Vec::new(),
            include_trashed: false,
            only_trashed: false,
            joins: Vec::new(),
            invalid_query_reason: None,
            group_by: Vec::new(),
            having_conditions: Vec::new(),
            unions: Vec::new(),
            window_functions: Vec::new(),
            ctes: Vec::new(),
            cache_options: None,
            cache_key: None,
        }
    }

    // =========================================================================
    // QUERY FRAGMENT SUPPORT (SeaORM 2.0 consolidate feature)
    // =========================================================================

    /// Consolidate current query conditions into a reusable QueryFragment
    ///
    /// This extracts the current WHERE conditions, ORDER BY clauses, JOINs,
    /// GROUP BY, and HAVING clauses into a reusable fragment that can be
    /// applied to other queries.
    ///
    /// Note: This does NOT include LIMIT, OFFSET, UNION, CTEs, window functions,
    /// or SELECT columns, as these are typically query-specific.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Create a reusable scope for active, verified users
    /// let active_scope = User::query()
    ///     .where_eq("status", "active")
    ///     .where_eq("verified", true)
    ///     .consolidate();
    ///
    /// // Use in different queries
    /// let admins = User::query()
    ///     .apply(&active_scope)
    ///     .where_eq("role", "admin")
    ///     .get()
    ///     .await?;
    ///
    /// let count = User::query()
    ///     .apply(&active_scope)
    ///     .count()
    ///     .await?;
    /// ```
    pub fn consolidate(&self) -> QueryFragment<M> {
        QueryFragment {
            _marker: PhantomData,
            conditions: self.conditions.clone(),
            order_by: self.order_by.clone(),
            group_by: self.group_by.clone(),
            having_conditions: self.having_conditions.clone(),
            joins: self.joins.clone(),
            invalid_query_reason: self.invalid_query_reason.clone(),
            include_trashed: self.include_trashed,
            only_trashed: self.only_trashed,
        }
    }

    /// Apply a QueryFragment to this query builder
    ///
    /// This merges all conditions and clauses from the fragment into
    /// the current query. Conditions are combined with AND logic.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let active_scope = User::query()
    ///     .where_eq("status", "active")
    ///     .consolidate();
    ///
    /// let results = User::query()
    ///     .apply(&active_scope)
    ///     .where_eq("role", "editor")
    ///     .order_by("name", Order::Asc)
    ///     .get()
    ///     .await?;
    /// ```
    pub fn apply(mut self, fragment: &QueryFragment<M>) -> Self {
        // Merge conditions
        self.conditions.extend(fragment.conditions.clone());

        // Merge order_by (fragment's ordering takes precedence if not already set)
        if self.order_by.is_empty() {
            self.order_by.extend(fragment.order_by.clone());
        }

        // Merge group_by
        self.group_by.extend(fragment.group_by.clone());

        // Merge having_conditions
        self.having_conditions
            .extend(fragment.having_conditions.clone());

        // Merge joins
        self.joins.extend(fragment.joins.clone());

        if self.invalid_query_reason.is_none() {
            self.invalid_query_reason = fragment.invalid_query_reason.clone();
        }

        // Apply soft delete flags (OR logic - if either wants them included)
        if fragment.include_trashed {
            self.include_trashed = true;
        }
        if fragment.only_trashed {
            self.only_trashed = true;
        }

        self
    }

    fn invalidate_query(&mut self, reason: String) {
        if self.invalid_query_reason.is_none() {
            self.invalid_query_reason = Some(reason);
        }
    }

    fn ensure_query_is_valid(&self) -> Result<()> {
        if let Some(reason) = &self.invalid_query_reason {
            return Err(Error::invalid_query(reason.clone()));
        }

        Ok(())
    }

    fn validate_join_clause(
        table: &str,
        alias: Option<&str>,
        left_column: &str,
        right_column: &str,
    ) -> std::result::Result<(), String> {
        db_sql::validate_identifier("JOIN table", table)?;

        if let Some(alias) = alias {
            db_sql::validate_identifier("JOIN alias", alias)?;
        }

        db_sql::validate_join_column(left_column)?;
        db_sql::validate_join_column(right_column)?;

        Ok(())
    }

    /// Create a new QueryBuilder from a QueryFragment
    ///
    /// This is a convenient way to start a new query from an existing fragment.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let active_scope = User::query()
    ///     .where_eq("status", "active")
    ///     .consolidate();
    ///
    /// // Start a new query from the scope
    /// let results = QueryBuilder::<User>::from_fragment(&active_scope)
    ///     .limit(10)
    ///     .get()
    ///     .await?;
    /// ```
    pub fn from_fragment(fragment: &QueryFragment<M>) -> Self {
        Self::new().apply(fragment)
    }

    // =========================================================================
    // WHERE CLAUSES
    // =========================================================================

    /// Add a where equals condition
    ///
    /// Accepts either a string column name or a typed column reference.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // String-based (runtime checked)
    /// User::query().where_eq("active", true)
    ///
    /// // Typed column (compile-time checked)
    /// User::query().where_eq(User::columns.active, true)
    /// ```
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

    /// Add a where not equals condition
    ///
    /// Accepts either a string column name or a typed column reference.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // String-based
    /// User::query().where_not("role", "admin")
    ///
    /// // Typed column
    /// User::query().where_not(User::columns.role, "admin")
    /// ```
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

    /// Add a where greater than condition
    ///
    /// Accepts either a string column name or a typed column reference.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_gt("age", 18)
    /// User::query().where_gt(User::columns.age, 18)
    /// ```
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

    /// Add a where greater than or equal condition
    ///
    /// Accepts either a string column name or a typed column reference.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_gte("age", 18)
    /// User::query().where_gte(User::columns.age, 18)
    /// ```
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

    /// Add a where less than condition
    ///
    /// Accepts either a string column name or a typed column reference.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_lt("age", 65)
    /// User::query().where_lt(User::columns.age, 65)
    /// ```
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

    /// Add a where less than or equal condition
    ///
    /// Accepts either a string column name or a typed column reference.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_lte("age", 65)
    /// User::query().where_lte(User::columns.age, 65)
    /// ```
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

    /// Add a where LIKE condition
    ///
    /// Accepts either a string column name or a typed column reference.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_like("email", "%@company.com")
    /// User::query().where_like(User::columns.email, "%@company.com")
    /// ```
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

    /// Add a where NOT LIKE condition
    ///
    /// Accepts either a string column name or a typed column reference.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_not_like("email", "%spam%")
    /// User::query().where_not_like(User::columns.email, "%spam%")
    /// ```
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

    /// Add a where IN condition
    ///
    /// Accepts either a string column name or a typed column reference.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_in("role", vec!["admin", "moderator"])
    /// User::query().where_in(User::columns.role, vec!["admin", "moderator"])
    /// ```
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

    /// Add a where NOT IN condition
    ///
    /// Accepts either a string column name or a typed column reference.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_not_in("status", vec!["banned", "suspended"])
    /// User::query().where_not_in(User::columns.status, vec!["banned", "suspended"])
    /// ```
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

    // =========================================================================
    // OR CLAUSE METHODS
    // =========================================================================

    /// Add an OR group to the query
    ///
    /// Conditions within the closure are combined with OR logic,
    /// and the entire group is combined with the rest of the query using AND.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users who are either admin OR moderator
    /// let users = User::query()
    ///     .or_where(|q| q
    ///         .where_eq("role", "admin")
    ///         .where_eq("role", "moderator")
    ///     )
    ///     .get()
    ///     .await?;
    /// // Generates: WHERE (role = 'admin' OR role = 'moderator')
    ///
    /// // Combined with other conditions
    /// let users = User::query()
    ///     .where_eq("active", true)
    ///     .or_where(|q| q
    ///         .where_eq("role", "admin")
    ///         .where_eq("role", "moderator")
    ///     )
    ///     .get()
    ///     .await?;
    /// // Generates: WHERE active = true AND (role = 'admin' OR role = 'moderator')
    /// ```
    pub fn or_where<F>(mut self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        let group = f(OrGroup::new());
        if !group.is_empty() {
            self.or_groups.push(group);
        }
        self
    }

    /// Add an OR condition directly (simple shorthand)
    ///
    /// This is a shorthand for adding two conditions combined with OR.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users where role is admin OR status is active
    /// let users = User::query()
    ///     .where_eq("role", "admin")
    ///     .or_where_eq("status", "active")
    ///     .get()
    ///     .await?;
    /// // Generates: WHERE role = 'admin' OR status = 'active'
    /// ```
    pub fn or_where_eq(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Eq,
            value: ConditionValue::Single(value.into()),
        });
        self.or_groups.push(group);
        self
    }

    /// Add an OR NOT EQUAL condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let users = User::query()
    ///     .where_eq("role", "admin")
    ///     .or_where_not("status", "banned")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn or_where_not(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::NotEq,
            value: ConditionValue::Single(value.into()),
        });
        self.or_groups.push(group);
        self
    }

    /// Add an OR GREATER THAN condition
    pub fn or_where_gt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Gt,
            value: ConditionValue::Single(value.into()),
        });
        self.or_groups.push(group);
        self
    }

    /// Add an OR GREATER THAN OR EQUAL condition
    pub fn or_where_gte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Gte,
            value: ConditionValue::Single(value.into()),
        });
        self.or_groups.push(group);
        self
    }

    /// Add an OR LESS THAN condition
    pub fn or_where_lt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Lt,
            value: ConditionValue::Single(value.into()),
        });
        self.or_groups.push(group);
        self
    }

    /// Add an OR LESS THAN OR EQUAL condition
    pub fn or_where_lte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Lte,
            value: ConditionValue::Single(value.into()),
        });
        self.or_groups.push(group);
        self
    }

    /// Add an OR LIKE condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let users = User::query()
    ///     .where_like("email", "%@gmail.com")
    ///     .or_where_like("email", "%@yahoo.com")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn or_where_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Like,
            value: ConditionValue::Single(serde_json::Value::String(pattern.to_string())),
        });
        self.or_groups.push(group);
        self
    }

    /// Add an OR IN condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let products = Product::query()
    ///     .where_in("category", vec!["electronics"])
    ///     .or_where_in("category", vec!["books", "games"])
    ///     .get()
    ///     .await?;
    /// ```
    pub fn or_where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::In,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self.or_groups.push(group);
        self
    }

    /// Add an OR NOT IN condition
    pub fn or_where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::NotIn,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self.or_groups.push(group);
        self
    }

    /// Add an OR IS NULL condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let users = User::query()
    ///     .where_eq("verified", true)
    ///     .or_where_null("email")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn or_where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNull,
            value: ConditionValue::None,
        });
        self.or_groups.push(group);
        self
    }

    /// Add an OR IS NOT NULL condition
    pub fn or_where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNotNull,
            value: ConditionValue::None,
        });
        self.or_groups.push(group);
        self
    }

    /// Add an OR BETWEEN condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let products = Product::query()
    ///     .where_between("price", 0, 50)
    ///     .or_where_between("price", 200, 500)
    ///     .get()
    ///     .await?;
    /// // Finds products priced $0-50 OR $200-500
    /// ```
    pub fn or_where_between(
        mut self,
        column: impl crate::columns::IntoColumnName,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Between,
            value: ConditionValue::Range(min.into(), max.into()),
        });
        self.or_groups.push(group);
        self
    }

    /// Add an OR raw SQL condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let users = User::query()
    ///     .where_eq("active", true)
    ///     .or_where_raw("created_at > NOW() - INTERVAL '7 days'")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn or_where_raw(mut self, raw_sql: &str) -> Self {
        let mut group = OrGroup::new();
        group.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(raw_sql.to_string()),
        });
        self.or_groups.push(group);
        self
    }

    // =========================================================================
    // FLUENT OR BRANCH BUILDER
    // =========================================================================

    /// Start building a fluent OR expression with chained AND conditions
    ///
    /// This method begins an OR expression builder that allows you to chain
    /// multiple conditions where conditions after `or_where_*` are part of
    /// the same OR branch, and `and_where_*` adds to that branch.
    ///
    /// Use `.end_or()` to finish the OR expression and return to the QueryBuilder.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Complex OR with AND conditions in each branch:
    /// // WHERE active = true AND (
    /// //     (role = 'admin' AND verified = true) OR
    /// //     (role = 'moderator' AND age > 25) OR
    /// //     role = 'superuser'
    /// // )
    /// let users = User::query()
    ///     .where_eq("active", true)
    ///     .begin_or()
    ///         .or_where_eq("role", "admin").and_where_eq("verified", true)
    ///         .or_where_eq("role", "moderator").and_where_gt("age", 25)
    ///         .or_where_eq("role", "superuser")
    ///     .end_or()
    ///     .get()
    ///     .await?;
    /// ```
    ///
    /// # Flow
    ///
    /// - `.begin_or()` - starts the OR builder
    /// - `.or_where_*()` - starts a new OR branch with a condition
    /// - `.and_where_*()` - adds an AND condition to the current branch
    /// - `.end_or()` - finishes and returns to QueryBuilder
    pub fn begin_or(self) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self)
    }

    /// Start a fluent OR expression with an initial equals condition
    ///
    /// Shorthand for `.begin_or().or_where_eq(column, value)`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // WHERE active = true AND (
    /// //     (role = 'admin' AND verified = true) OR
    /// //     (role = 'moderator')
    /// // )
    /// let users = User::query()
    ///     .where_eq("active", true)
    ///     .begin_or_where_eq("role", "admin").and_where_eq("verified", true)
    ///     .or_where_eq("role", "moderator")
    ///     .end_or()
    ///     .get()
    ///     .await?;
    /// ```
    pub fn begin_or_where_eq(
        self,
        column: &str,
        value: impl Into<serde_json::Value>,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_eq(column, value)
    }

    /// Start a fluent OR expression with an initial greater than condition
    pub fn begin_or_where_gt(
        self,
        column: &str,
        value: impl Into<serde_json::Value>,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_gt(column, value)
    }

    /// Start a fluent OR expression with an initial greater than or equal condition
    pub fn begin_or_where_gte(
        self,
        column: &str,
        value: impl Into<serde_json::Value>,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_gte(column, value)
    }

    /// Start a fluent OR expression with an initial less than condition
    pub fn begin_or_where_lt(
        self,
        column: &str,
        value: impl Into<serde_json::Value>,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_lt(column, value)
    }

    /// Start a fluent OR expression with an initial less than or equal condition
    pub fn begin_or_where_lte(
        self,
        column: &str,
        value: impl Into<serde_json::Value>,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_lte(column, value)
    }

    /// Start a fluent OR expression with an initial LIKE condition
    pub fn begin_or_where_like(self, column: &str, pattern: &str) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_like(column, pattern)
    }

    /// Start a fluent OR expression with an initial IN condition
    pub fn begin_or_where_in<V: Into<serde_json::Value>>(
        self,
        column: &str,
        values: Vec<V>,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_in(column, values)
    }

    /// Start a fluent OR expression with an initial IS NULL condition
    pub fn begin_or_where_null(self, column: &str) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_null(column)
    }

    /// Start a fluent OR expression with an initial IS NOT NULL condition
    pub fn begin_or_where_not_null(self, column: &str) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_not_null(column)
    }

    /// Start a fluent OR expression with an initial BETWEEN condition
    pub fn begin_or_where_between(
        self,
        column: &str,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_between(column, min, max)
    }

    /// Add a WHERE column = ANY(array) condition (PostgreSQL optimization)
    ///
    /// This is an optimized version of `where_in` for PostgreSQL that uses
    /// the `= ANY()` operator. This is more efficient for large arrays as
    /// the query plan can be cached and reused with different array values.
    ///
    /// For other databases, this falls back to standard IN clause behavior.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users with specific IDs - uses = ANY() on PostgreSQL
    /// let users = User::query()
    ///     .eq_any("id", vec![1, 2, 3, 4, 5])
    ///     .get()
    ///     .await?;
    ///
    /// // Generates on PostgreSQL: WHERE "id" = ANY(ARRAY[1, 2, 3, 4, 5])
    /// // Generates on others: WHERE "id" IN (1, 2, 3, 4, 5)
    ///
    /// // Find users by roles
    /// let users = User::query()
    ///     .eq_any("role", vec!["admin", "moderator", "editor"])
    ///     .get()
    ///     .await?;
    /// ```
    pub fn eq_any<V: Into<serde_json::Value>>(mut self, column: &str, values: Vec<V>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::EqAny,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add a WHERE column <> ALL(array) condition - inverse of eq_any (PostgreSQL optimization)
    ///
    /// This is an optimized version of `where_not_in` for PostgreSQL that uses
    /// the `<> ALL()` operator.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users NOT with specific IDs
    /// let users = User::query()
    ///     .ne_all("id", vec![1, 2, 3])
    ///     .get()
    ///     .await?;
    ///
    /// // Generates on PostgreSQL: WHERE "id" <> ALL(ARRAY[1, 2, 3])
    /// // Generates on others: WHERE "id" NOT IN (1, 2, 3)
    /// ```
    pub fn ne_all<V: Into<serde_json::Value>>(mut self, column: &str, values: Vec<V>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::NeAll,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add a WHERE condition using a strongly-typed column
    ///
    /// This method accepts conditions generated from `Column<T>` typed columns,
    /// providing compile-time type safety for column operations.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use tideorm::prelude::*;
    ///
    /// // Define typed columns
    /// mod user_cols {
    ///     use tideorm::columns::*;
    ///     pub const ID: Column<i64> = Column::new("id");
    ///     pub const NAME: Column<String> = Column::new("name");
    ///     pub const AGE: Column<Option<i32>> = Column::new("age");
    /// }
    ///
    /// // Type-safe queries - compiler catches type errors
    /// let users = User::query()
    ///     .where_col(user_cols::NAME.eq("Alice"))          // OK: String == &str
    ///     .where_col(user_cols::AGE.gt(18))                // OK: Option<i32> > i32
    ///     // .where_col(user_cols::NAME.eq(123))           // COMPILE ERROR!
    ///     // .where_col(user_cols::AGE.like("%test%"))     // COMPILE ERROR!
    ///     .get()
    ///     .await?;
    /// ```
    pub fn where_col(mut self, condition: crate::columns::ColumnCondition) -> Self {
        let operator = match condition.operator {
            crate::columns::ColumnOperator::Eq => Operator::Eq,
            crate::columns::ColumnOperator::NotEq => Operator::NotEq,
            crate::columns::ColumnOperator::Gt => Operator::Gt,
            crate::columns::ColumnOperator::Gte => Operator::Gte,
            crate::columns::ColumnOperator::Lt => Operator::Lt,
            crate::columns::ColumnOperator::Lte => Operator::Lte,
            crate::columns::ColumnOperator::Like => Operator::Like,
            crate::columns::ColumnOperator::NotLike => Operator::NotLike,
            crate::columns::ColumnOperator::In => Operator::In,
            crate::columns::ColumnOperator::NotIn => Operator::NotIn,
            crate::columns::ColumnOperator::IsNull => Operator::IsNull,
            crate::columns::ColumnOperator::IsNotNull => Operator::IsNotNull,
            crate::columns::ColumnOperator::Between => Operator::Between,
        };

        let value = match condition.operator {
            crate::columns::ColumnOperator::IsNull | crate::columns::ColumnOperator::IsNotNull => {
                ConditionValue::None
            }
            crate::columns::ColumnOperator::In | crate::columns::ColumnOperator::NotIn => {
                if let serde_json::Value::Array(arr) = condition.value {
                    ConditionValue::List(arr)
                } else {
                    ConditionValue::List(vec![condition.value])
                }
            }
            crate::columns::ColumnOperator::Between => {
                if let serde_json::Value::Array(arr) = condition.value {
                    if arr.len() >= 2 {
                        ConditionValue::Range(arr[0].clone(), arr[1].clone())
                    } else {
                        ConditionValue::Single(serde_json::Value::Null)
                    }
                } else {
                    ConditionValue::Single(condition.value)
                }
            }
            _ => ConditionValue::Single(condition.value),
        };

        self.conditions.push(WhereCondition {
            column: condition.column,
            operator,
            value,
        });
        self
    }

    // =========================================================================
    // SUBQUERIES
    // =========================================================================

    /// Add a WHERE IN (subquery) condition
    ///
    /// Use another query builder as a subquery for the IN clause.
    /// The subquery should select a single column that matches the type of the column.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find all users who have posted in the last 30 days
    /// let active_users = User::query()
    ///     .where_in_subquery("id",
    ///         Post::query()
    ///             .select(vec!["user_id"])
    ///             .where_gte("created_at", thirty_days_ago)
    ///     )
    ///     .get()
    ///     .await?;
    ///
    /// // Find users not in any team
    /// let solo_users = User::query()
    ///     .where_not_in_subquery("id",
    ///         TeamMember::query()
    ///             .select(vec!["user_id"])
    ///     )
    ///     .get()
    ///     .await?;
    /// ```
    pub fn where_in_subquery<N: Model>(mut self, column: &str, subquery: QueryBuilder<N>) -> Self {
        let subquery_sql = subquery.to_subquery_sql();
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::SubqueryIn,
            value: ConditionValue::Subquery(subquery_sql),
        });
        self
    }

    /// Add a WHERE NOT IN (subquery) condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users who haven't ordered anything
    /// let inactive_users = User::query()
    ///     .where_not_in_subquery("id",
    ///         Order::query().select(vec!["user_id"])
    ///     )
    ///     .get()
    ///     .await?;
    /// ```
    pub fn where_not_in_subquery<N: Model>(
        mut self,
        column: &str,
        subquery: QueryBuilder<N>,
    ) -> Self {
        let subquery_sql = subquery.to_subquery_sql();
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::SubqueryNotIn,
            value: ConditionValue::Subquery(subquery_sql),
        });
        self
    }

    /// Add a WHERE EXISTS (subquery) condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users who have at least one post
    /// let users_with_posts = User::query()
    ///     .where_exists(
    ///         Post::query().where_raw("posts.user_id = users.id")
    ///     )
    ///     .get()
    ///     .await?;
    /// ```
    pub fn where_exists<N: Model>(mut self, subquery: QueryBuilder<N>) -> Self {
        let subquery_sql = subquery.to_subquery_sql();
        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(format!("EXISTS ({})", subquery_sql)),
        });
        self
    }

    /// Add a WHERE NOT EXISTS (subquery) condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users without any posts
    /// let users_without_posts = User::query()
    ///     .where_not_exists(
    ///         Post::query().where_raw("posts.user_id = users.id")
    ///     )
    ///     .get()
    ///     .await?;
    /// ```
    pub fn where_not_exists<N: Model>(mut self, subquery: QueryBuilder<N>) -> Self {
        let subquery_sql = subquery.to_subquery_sql();
        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(format!("NOT EXISTS ({})", subquery_sql)),
        });
        self
    }

    /// Check if related records exist matching a condition
    ///
    /// This generates an EXISTS subquery to find records that have related records
    /// matching the specified condition. It's a cleaner API than manually constructing
    /// EXISTS queries.
    ///
    /// # Arguments
    ///
    /// * `related_table` - The related table name
    /// * `foreign_key` - The foreign key column on the related table
    /// * `local_key` - The local key column (usually primary key)
    /// * `condition_column` - Column in the related table to filter on
    /// * `condition_value` - Value to filter the related records by
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find all cakes that have a fruit named "Mango"
    /// let cakes = Cake::query()
    ///     .has_related("fruits", "cake_id", "id", "name", "Mango")
    ///     .get()
    ///     .await?;
    ///
    /// // Generates: SELECT * FROM cakes WHERE EXISTS(
    /// //   SELECT 1 FROM fruits WHERE fruits.cake_id = cakes.id AND fruits.name = 'Mango'
    /// // )
    ///
    /// // Find users who have at least one active post
    /// let users = User::query()
    ///     .has_related("posts", "user_id", "id", "status", "active")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn has_related(
        mut self,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
        condition_column: &str,
        condition_value: impl Into<serde_json::Value>,
    ) -> Self {
        let table = M::table_name();
        let value = condition_value.into();
        let value_sql = match &value {
            serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => "NULL".to_string(),
            _ => value.to_string(),
        };

        let exists_sql = format!(
            "EXISTS (SELECT 1 FROM \"{}\" WHERE \"{}\".\"{}\" = \"{}\".\"{}\" AND \"{}\".\"{}\" = {})",
            related_table,
            related_table,
            foreign_key,
            table,
            local_key,
            related_table,
            condition_column,
            value_sql
        );

        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(exists_sql),
        });
        self
    }

    /// Check if related records do NOT exist matching a condition
    ///
    /// The inverse of `has_related` - finds records that do NOT have matching related records.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find all cakes that don't have any fruit named "Mango"
    /// let cakes = Cake::query()
    ///     .has_no_related("fruits", "cake_id", "id", "name", "Mango")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn has_no_related(
        mut self,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
        condition_column: &str,
        condition_value: impl Into<serde_json::Value>,
    ) -> Self {
        let table = M::table_name();
        let value = condition_value.into();
        let value_sql = match &value {
            serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => "NULL".to_string(),
            _ => value.to_string(),
        };

        let not_exists_sql = format!(
            "NOT EXISTS (SELECT 1 FROM \"{}\" WHERE \"{}\".\"{}\" = \"{}\".\"{}\" AND \"{}\".\"{}\" = {})",
            related_table,
            related_table,
            foreign_key,
            table,
            local_key,
            related_table,
            condition_column,
            value_sql
        );

        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(not_exists_sql),
        });
        self
    }

    /// Check if any related records exist (without condition)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find all users who have at least one post
    /// let users = User::query()
    ///     .has_any_related("posts", "user_id", "id")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn has_any_related(
        mut self,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
    ) -> Self {
        let table = M::table_name();

        let exists_sql = format!(
            "EXISTS (SELECT 1 FROM \"{}\" WHERE \"{}\".\"{}\" = \"{}\".\"{}\")",
            related_table, related_table, foreign_key, table, local_key
        );

        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(exists_sql),
        });
        self
    }

    /// Check if NO related records exist
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find all users who have no posts
    /// let users = User::query()
    ///     .has_no_related_at_all("posts", "user_id", "id")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn has_no_related_at_all(
        mut self,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
    ) -> Self {
        let table = M::table_name();

        let not_exists_sql = format!(
            "NOT EXISTS (SELECT 1 FROM \"{}\" WHERE \"{}\".\"{}\" = \"{}\".\"{}\")",
            related_table, related_table, foreign_key, table, local_key
        );

        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(not_exists_sql),
        });
        self
    }

    /// Convert this query builder to a subquery SQL string
    ///
    /// Used internally for subquery conditions.
    pub fn to_subquery_sql(&self) -> String {
        self.build_select_sql()
    }

    // =========================================================================
    // RAW EXPRESSIONS
    // =========================================================================

    /// Add a raw WHERE condition
    ///
    /// Use this when you need complex SQL conditions that can't be expressed
    /// with the standard query builder methods.
    ///
    /// âš ï¸ **Warning**: Raw SQL is not escaped. Only use with trusted input.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Complex date calculation
    /// let users = User::query()
    ///     .where_raw("created_at > NOW() - INTERVAL '30 days'")
    ///     .get()
    ///     .await?;
    ///
    /// // Subquery in raw form
    /// let users = User::query()
    ///     .where_raw("id IN (SELECT user_id FROM posts WHERE published = true)")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn where_raw(mut self, raw_sql: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(raw_sql.to_string()),
        });
        self
    }

    /// Add a raw WHERE condition with a column comparison
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Custom comparison
    /// let users = User::query()
    ///     .where_column_raw("email", "LIKE '%' || name || '%'")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn where_column_raw(mut self, column: &str, raw_expr: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(raw_expr.to_string()),
        });
        self
    }

    /// Add a raw SELECT expression
    ///
    /// Use this to add calculated columns or complex expressions to the select.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Add a calculated column
    /// let results = User::query()
    ///     .select_raw("id, name, (SELECT COUNT(*) FROM posts WHERE posts.user_id = users.id) AS post_count")
    ///     .get_raw()
    ///     .await?;
    ///
    /// // Add aggregate with alias
    /// let results = Order::query()
    ///     .group_by("user_id")
    ///     .select_raw("user_id, SUM(total) as total_spent, COUNT(*) as order_count")
    ///     .get_raw()
    ///     .await?;
    /// ```
    pub fn select_raw(mut self, raw_select: &str) -> Self {
        self.raw_select_expressions.push(raw_select.to_string());
        self
    }

    /// Add a scalar subquery as a SELECT expression
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Add post count as a column
    /// let users = User::query()
    ///     .select_subquery(
    ///         Post::query()
    ///             .select_raw("COUNT(*)")
    ///             .where_raw("posts.user_id = users.id"),
    ///         "post_count"
    ///     )
    ///     .get_raw()
    ///     .await?;
    /// ```
    pub fn select_subquery<N: Model>(mut self, subquery: QueryBuilder<N>, alias: &str) -> Self {
        let subquery_sql = subquery.to_subquery_sql();
        self.raw_select_expressions
            .push(format!("({}) AS \"{}\"", subquery_sql, alias));
        self
    }

    /// Add a where IS NULL condition
    ///
    /// Accepts either a string column name or a typed column reference.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_null("deleted_at")
    /// User::query().where_null(User::columns.deleted_at)
    /// ```
    pub fn where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNull,
            value: ConditionValue::None,
        });
        self
    }

    /// Add a where IS NOT NULL condition
    ///
    /// Accepts either a string column name or a typed column reference.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_not_null("email_verified_at")
    /// User::query().where_not_null(User::columns.email_verified_at)
    /// ```
    pub fn where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNotNull,
            value: ConditionValue::None,
        });
        self
    }

    /// Add a where BETWEEN condition
    ///
    /// Accepts either a string column name or a typed column reference.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_between("age", 18, 65)
    /// User::query().where_between(User::columns.age, 18, 65)
    /// ```
    pub fn where_between(
        mut self,
        column: impl crate::columns::IntoColumnName,
        low: impl Into<serde_json::Value>,
        high: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Between,
            value: ConditionValue::Range(low.into(), high.into()),
        });
        self
    }

    // =========================================================================
    // JSON OPERATIONS
    // =========================================================================

    /// Add a JSON contains condition (column @> value)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users with metadata containing {"role": "admin"}
    /// User::query().where_json_contains("metadata", serde_json::json!({"role": "admin"}))
    /// ```
    pub fn where_json_contains(
        mut self,
        column: &str,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonContains,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a JSON contained by condition (column <@ value)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users whose preferences are contained in the given object
    /// User::query().where_json_contained_by("preferences", serde_json::json!({"theme": "dark", "lang": "en"}))
    /// ```
    pub fn where_json_contained_by(
        mut self,
        column: &str,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonContainedBy,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a JSON key exists condition (column ? key)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users with a "verified" key in their metadata
    /// User::query().where_json_key_exists("metadata", "verified")
    /// ```
    pub fn where_json_key_exists(mut self, column: &str, key: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonKeyExists,
            value: ConditionValue::Single(serde_json::Value::String(key.to_string())),
        });
        self
    }

    /// Add a JSON key does not exist condition (column ?! key)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users without a "banned" key in their metadata
    /// User::query().where_json_key_not_exists("metadata", "banned")
    /// ```
    pub fn where_json_key_not_exists(mut self, column: &str, key: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonKeyNotExists,
            value: ConditionValue::Single(serde_json::Value::String(key.to_string())),
        });
        self
    }

    /// Add a JSON path exists condition (column @? path)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users with settings.theme = "dark"
    /// User::query().where_json_path_exists("settings", "$.theme")
    /// ```
    pub fn where_json_path_exists(mut self, column: &str, path: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonPathExists,
            value: ConditionValue::Single(serde_json::Value::String(path.to_string())),
        });
        self
    }

    /// Add a JSON path does not exist condition (column ?! path)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users without settings.notifications.email
    /// User::query().where_json_path_not_exists("settings", "$.notifications.email")
    /// ```
    pub fn where_json_path_not_exists(mut self, column: &str, path: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonPathNotExists,
            value: ConditionValue::Single(serde_json::Value::String(path.to_string())),
        });
        self
    }

    // =========================================================================
    // ARRAY OPERATIONS
    // =========================================================================

    /// Add an array contains condition (column @> value)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users with role "admin" in their roles array
    /// User::query().where_array_contains("roles", vec!["admin"])
    /// ```
    pub fn where_array_contains<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        value: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::ArrayContains,
            value: ConditionValue::List(value.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add an array contained by condition (column <@ value)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users whose tags are contained in the given list
    /// User::query().where_array_contained_by("tags", vec!["tech", "news", "sports"])
    /// ```
    pub fn where_array_contained_by<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        value: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::ArrayContainedBy,
            value: ConditionValue::List(value.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add an array overlaps condition (column && value)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users with overlapping interests
    /// User::query().where_array_overlaps("interests", vec!["coding", "music"])
    /// ```
    pub fn where_array_overlaps<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        value: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::ArrayOverlaps,
            value: ConditionValue::List(value.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add an array contains any element condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users with any of the specified skills
    /// User::query().where_array_contains_any("skills", vec!["rust", "python", "javascript"])
    /// ```
    pub fn where_array_contains_any<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        value: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::ArrayContainsAny,
            value: ConditionValue::List(value.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add an array contains all elements condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find users with all specified permissions
    /// User::query().where_array_contains_all("permissions", vec!["read", "write"])
    /// ```
    pub fn where_array_contains_all<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        value: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::ArrayContainsAll,
            value: ConditionValue::List(value.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    // =========================================================================
    // ORDERING
    // =========================================================================
}

#[cfg(test)]
mod tests;
