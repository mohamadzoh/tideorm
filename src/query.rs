//! Fluent query builder
//!
//! This module provides a fluent, chainable query builder API for TideORM.

use std::marker::PhantomData;

use crate::model::Model;

mod advanced;
mod builder;
mod db_sql;
mod filters;
mod sql;
mod structure;

pub use filters::{
    ConditionValue, LogicalOp, Operator, OrBranch, OrBranchBuilder, OrGroup, Order,
    WhereCondition,
};
pub use structure::{
    AggregateFunction, CTE, FrameBound, FrameType, JoinClause, JoinResultConsolidator, JoinType,
    QueryFragment, UnionClause, UnionType, WindowFunction, WindowFunctionType,
};

/// Fluent query builder for TideORM models.
#[derive(Debug, Clone)]
pub struct QueryBuilder<M: Model> {
    _marker: PhantomData<M>,
    /// WHERE conditions combined with AND logic.
    pub conditions: Vec<WhereCondition>,
    /// OR groups for complex boolean expressions.
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
    unions: Vec<UnionClause>,
    window_functions: Vec<WindowFunction>,
    ctes: Vec<CTE>,
    cache_options: Option<crate::cache::CacheOptions>,
    cache_key: Option<String>,
}

impl<M: Model> QueryBuilder<M> {
    /// Create a new QueryBuilder from a QueryFragment.
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
#[path = "testing/query_tests.rs"]
mod tests;
