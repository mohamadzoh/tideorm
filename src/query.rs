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

use std::marker::PhantomData;

use crate::error::Result;
use crate::model::Model;
use crate::internal::{
    EntityTrait, QueryFilter, QueryOrder, QuerySelect, Condition, 
    Expr, translate_error, FromQueryResult, Asterisk,
};

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
}

/// A single where condition
#[derive(Debug, Clone)]
#[allow(dead_code)]
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
    conditions: Vec<WhereCondition>,
    order_by: Vec<(String, Order)>,
    limit_value: Option<u64>,
    offset_value: Option<u64>,
    select_columns: Option<Vec<String>>,
    include_trashed: bool,
    only_trashed: bool,
    joins: Vec<JoinClause>,
    group_by: Vec<String>,
    having_conditions: Vec<String>,
}

impl<M: Model> QueryBuilder<M> {
    /// Create a new query builder
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
            conditions: Vec::new(),
            order_by: Vec::new(),
            limit_value: None,
            offset_value: None,
            select_columns: None,
            include_trashed: false,
            only_trashed: false,
            joins: Vec::new(),
            group_by: Vec::new(),
            having_conditions: Vec::new(),
        }
    }
    
    // =========================================================================
    // WHERE CLAUSES
    // =========================================================================
    
    /// Add a where equals condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_eq("active", true)
    /// ```
    pub fn where_eq(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::Eq,
            value: ConditionValue::Single(value.into()),
        });
        self
    }
    
    /// Add a where not equals condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_not("role", "admin")
    /// ```
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
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_like("email", "%@company.com")
    /// ```
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
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_in("role", vec!["admin", "moderator"])
    /// ```
    pub fn where_in<V: Into<serde_json::Value>>(mut self, column: &str, values: Vec<V>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::In,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self
    }
    
    /// Add a where NOT IN condition
    pub fn where_not_in<V: Into<serde_json::Value>>(mut self, column: &str, values: Vec<V>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::NotIn,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self
    }
    
    /// Add a where IS NULL condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_null("deleted_at")
    /// ```
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
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().where_between("age", 18, 65)
    /// ```
    pub fn where_between(
        mut self,
        column: &str,
        low: impl Into<serde_json::Value>,
        high: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
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
    pub fn where_json_contains(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
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
    pub fn where_json_contained_by(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
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
    pub fn where_array_contains<V: Into<serde_json::Value>>(mut self, column: &str, value: Vec<V>) -> Self {
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
    pub fn where_array_contained_by<V: Into<serde_json::Value>>(mut self, column: &str, value: Vec<V>) -> Self {
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
    pub fn where_array_overlaps<V: Into<serde_json::Value>>(mut self, column: &str, value: Vec<V>) -> Self {
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
    pub fn where_array_contains_any<V: Into<serde_json::Value>>(mut self, column: &str, value: Vec<V>) -> Self {
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
    pub fn where_array_contains_all<V: Into<serde_json::Value>>(mut self, column: &str, value: Vec<V>) -> Self {
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
    
    /// Add an ORDER BY clause
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query()
    ///     .order_by("created_at", Order::Desc)
    ///     .order_by("name", Order::Asc)
    /// ```
    pub fn order_by(mut self, column: &str, direction: Order) -> Self {
        self.order_by.push((column.to_string(), direction));
        self
    }
    
    /// Order by ascending
    pub fn order_asc(self, column: &str) -> Self {
        self.order_by(column, Order::Asc)
    }
    
    /// Order by descending
    pub fn order_desc(self, column: &str) -> Self {
        self.order_by(column, Order::Desc)
    }
    
    /// Order by latest (created_at DESC)
    pub fn latest(self) -> Self {
        self.order_desc("created_at")
    }
    
    /// Order by oldest (created_at ASC)
    pub fn oldest(self) -> Self {
        self.order_asc("created_at")
    }
    
    // =========================================================================
    // PAGINATION
    // =========================================================================
    
    /// Limit the number of results
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().limit(10)
    /// ```
    pub fn limit(mut self, n: u64) -> Self {
        self.limit_value = Some(n);
        self
    }
    
    /// Skip a number of results
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().offset(20)
    /// ```
    pub fn offset(mut self, n: u64) -> Self {
        self.offset_value = Some(n);
        self
    }
    
    /// Paginate results
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Page 3, 25 items per page
    /// User::query().page(3, 25)
    /// ```
    pub fn page(self, page: u64, per_page: u64) -> Self {
        let offset = (page.saturating_sub(1)) * per_page;
        self.limit(per_page).offset(offset)
    }
    
    /// Take only the first N records
    pub fn take(self, n: u64) -> Self {
        self.limit(n)
    }
    
    /// Skip the first N records
    pub fn skip(self, n: u64) -> Self {
        self.offset(n)
    }
    
    // =========================================================================
    // SELECT
    // =========================================================================
    
    /// Select specific columns
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query().select(vec!["id", "name", "email"])
    /// ```
    pub fn select(mut self, columns: Vec<&str>) -> Self {
        self.select_columns = Some(columns.into_iter().map(|s| s.to_string()).collect());
        self
    }
    
    // =========================================================================
    // JOIN OPERATIONS
    // =========================================================================
    
    /// Add an INNER JOIN clause
    ///
    /// Returns only rows with matches in both tables.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Join posts with users
    /// Post::query()
    ///     .inner_join("users", "posts.user_id", "users.id")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn inner_join(self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Inner, table, None, left_column, right_column)
    }
    
    /// Add an INNER JOIN clause with an alias
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Post::query()
    ///     .inner_join_as("users", "author", "posts.user_id", "author.id")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn inner_join_as(self, table: &str, alias: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Inner, table, Some(alias), left_column, right_column)
    }
    
    /// Add a LEFT JOIN clause
    ///
    /// Returns all rows from the left table, and matched rows from the right.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Get all users and their posts (if any)
    /// User::query()
    ///     .left_join("posts", "users.id", "posts.user_id")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn left_join(self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Left, table, None, left_column, right_column)
    }
    
    /// Add a LEFT JOIN clause with an alias
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::query()
    ///     .left_join_as("posts", "p", "users.id", "p.user_id")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn left_join_as(self, table: &str, alias: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Left, table, Some(alias), left_column, right_column)
    }
    
    /// Add a RIGHT JOIN clause
    ///
    /// Returns all rows from the right table, and matched rows from the left.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Get all posts and their users
    /// User::query()
    ///     .right_join("posts", "users.id", "posts.user_id")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn right_join(self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Right, table, None, left_column, right_column)
    }
    
    /// Add a RIGHT JOIN clause with an alias
    pub fn right_join_as(self, table: &str, alias: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Right, table, Some(alias), left_column, right_column)
    }
    
    /// Generic join method (internal)
    fn join(
        mut self,
        join_type: JoinType,
        table: &str,
        alias: Option<&str>,
        left_column: &str,
        right_column: &str,
    ) -> Self {
        self.joins.push(JoinClause {
            join_type,
            table: table.to_string(),
            alias: alias.map(|s| s.to_string()),
            left_column: left_column.to_string(),
            right_column: right_column.to_string(),
        });
        self
    }
    
    // =========================================================================
    // AGGREGATIONS
    // =========================================================================
    
    /// Add a GROUP BY clause
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Count posts by user
    /// Post::query()
    ///     .group_by("user_id")
    ///     .select_raw("user_id, COUNT(*) as post_count")
    ///     .get_raw()
    ///     .await?;
    /// ```
    pub fn group_by(mut self, column: &str) -> Self {
        self.group_by.push(column.to_string());
        self
    }
    
    /// Add multiple GROUP BY columns
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Order::query()
    ///     .group_by_columns(vec!["status", "category"])
    ///     .get()
    ///     .await?;
    /// ```
    pub fn group_by_columns(mut self, columns: Vec<&str>) -> Self {
        for col in columns {
            self.group_by.push(col.to_string());
        }
        self
    }
    
    /// Add a HAVING clause (raw SQL condition)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Post::query()
    ///     .group_by("user_id")
    ///     .having("COUNT(*) > 5")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn having(mut self, condition: &str) -> Self {
        self.having_conditions.push(condition.to_string());
        self
    }
    
    /// Add HAVING with COUNT condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Post::query()
    ///     .group_by("user_id")
    ///     .having_count_gt(5)
    ///     .get()
    ///     .await?;
    /// ```
    pub fn having_count_gt(self, value: i64) -> Self {
        self.having(&format!("COUNT(*) > {}", value))
    }
    
    /// Add HAVING with COUNT >= condition
    pub fn having_count_gte(self, value: i64) -> Self {
        self.having(&format!("COUNT(*) >= {}", value))
    }
    
    /// Add HAVING with COUNT < condition
    pub fn having_count_lt(self, value: i64) -> Self {
        self.having(&format!("COUNT(*) < {}", value))
    }
    
    /// Add HAVING with COUNT <= condition
    pub fn having_count_lte(self, value: i64) -> Self {
        self.having(&format!("COUNT(*) <= {}", value))
    }
    
    /// Add HAVING with SUM condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Order::query()
    ///     .group_by("customer_id")
    ///     .having_sum_gt("total", 1000.0)
    ///     .get()
    ///     .await?;
    /// ```
    pub fn having_sum_gt(self, column: &str, value: f64) -> Self {
        self.having(&format!("SUM(\"{}\") > {}", column, value))
    }
    
    /// Add HAVING with AVG condition
    pub fn having_avg_gt(self, column: &str, value: f64) -> Self {
        self.having(&format!("AVG(\"{}\") > {}", column, value))
    }
    
    /// Calculate SUM of a column
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let total = Order::query()
    ///     .where_eq("status", "completed")
    ///     .sum("amount")
    ///     .await?;
    /// ```
    pub async fn sum(self, column: &str) -> Result<f64> {
        self.aggregate_f64(&format!("CAST(SUM(\"{}\") AS FLOAT8)", column), "sum_result").await
    }
    
    /// Calculate AVG of a column
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let avg_price = Product::query()
    ///     .where_eq("category", "electronics")
    ///     .avg("price")
    ///     .await?;
    /// ```
    pub async fn avg(self, column: &str) -> Result<f64> {
        self.aggregate_f64(&format!("CAST(AVG(\"{}\") AS FLOAT8)", column), "avg_result").await
    }
    
    /// Find MIN value of a column
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let min_price = Product::query()
    ///     .where_eq("in_stock", true)
    ///     .min("price")
    ///     .await?;
    /// ```
    pub async fn min(self, column: &str) -> Result<f64> {
        self.aggregate_f64(&format!("CAST(MIN(\"{}\") AS FLOAT8)", column), "min_result").await
    }
    
    /// Find MAX value of a column
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let max_price = Product::query()
    ///     .max("price")
    ///     .await?;
    /// ```
    pub async fn max(self, column: &str) -> Result<f64> {
        self.aggregate_f64(&format!("CAST(MAX(\"{}\") AS FLOAT8)", column), "max_result").await
    }
    
    /// Count distinct values of a column
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let unique_categories = Product::query()
    ///     .count_distinct("category")
    ///     .await?;
    /// ```
    pub async fn count_distinct(self, column: &str) -> Result<u64> {
        #[derive(Debug, FromQueryResult)]
        struct CountResult {
            count_result: i64,
        }
        
        let conn = crate::database::db().__internal_connection();
        
        let mut select = M::Entity::find();
        
        // Apply WHERE conditions
        if !self.conditions.is_empty() || M::soft_delete_enabled() {
            let condition = self.build_sea_condition();
            select = select.filter(condition);
        }
        
        // Build COUNT(DISTINCT column) expression
        let count_expr = Expr::cust(format!("COUNT(DISTINCT \"{}\")", column));
        
        let result: Option<CountResult> = select
            .select_only()
            .column_as(count_expr, "count_result")
            .into_model::<CountResult>()
            .one(conn)
            .await
            .map_err(translate_error)?;
        
        Ok(result.map(|r| r.count_result as u64).unwrap_or(0))
    }
    
    /// Internal helper for f64 aggregations
    async fn aggregate_f64(self, expr_sql: &str, _alias: &str) -> Result<f64> {
        #[derive(Debug, FromQueryResult)]
        struct AggResult {
            agg_result: Option<f64>,
        }
        
        let conn = crate::database::db().__internal_connection();
        
        let mut select = M::Entity::find();
        
        // Apply WHERE conditions
        if !self.conditions.is_empty() || M::soft_delete_enabled() {
            let condition = self.build_sea_condition();
            select = select.filter(condition);
        }
        
        // Build aggregate expression
        let agg_expr = Expr::cust(expr_sql.to_string());
        
        let result: Option<AggResult> = select
            .select_only()
            .column_as(agg_expr, "agg_result")
            .into_model::<AggResult>()
            .one(conn)
            .await
            .map_err(translate_error)?;
        
        Ok(result.and_then(|r| r.agg_result).unwrap_or(0.0))
    }
    
    // =========================================================================
    // SOFT DELETE QUERIES
    // =========================================================================
    
    /// Include soft-deleted records in the query results
    ///
    /// By default, soft-deleted records (where `deleted_at` is not NULL) are excluded.
    /// Use this method to include them.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Get all users including soft-deleted ones
    /// let all_users = User::query()
    ///     .with_trashed()
    ///     .get()
    ///     .await?;
    /// ```
    pub fn with_trashed(mut self) -> Self {
        self.include_trashed = true;
        self.only_trashed = false;
        self
    }
    
    /// Only return soft-deleted records
    ///
    /// Returns only records where `deleted_at` is not NULL.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Get only soft-deleted users (trash bin)
    /// let trashed_users = User::query()
    ///     .only_trashed()
    ///     .get()
    ///     .await?;
    /// ```
    pub fn only_trashed(mut self) -> Self {
        self.only_trashed = true;
        self.include_trashed = false;
        self
    }
    
    /// Exclude soft-deleted records (default behavior)
    ///
    /// This is the default, but can be used to explicitly exclude soft-deleted
    /// records after calling `with_trashed()`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let active_users = User::query()
    ///     .without_trashed()
    ///     .get()
    ///     .await?;
    /// ```
    pub fn without_trashed(mut self) -> Self {
        self.include_trashed = false;
        self.only_trashed = false;
        self
    }
    
    // =========================================================================
    // SCOPES (Reusable query fragments)
    // =========================================================================
    
    /// Apply a scope function to modify the query
    ///
    /// Scopes are reusable query fragments that can be applied to any query.
    /// This allows you to define common query patterns once and reuse them.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Define reusable scopes as functions
    /// fn active<M: Model>(q: QueryBuilder<M>) -> QueryBuilder<M> {
    ///     q.where_eq("active", true)
    /// }
    ///
    /// fn recent<M: Model>(q: QueryBuilder<M>) -> QueryBuilder<M> {
    ///     q.order_desc("created_at").limit(10)
    /// }
    ///
    /// // Use scopes
    /// let users = User::query()
    ///     .scope(active)
    ///     .scope(recent)
    ///     .get()
    ///     .await?;
    /// ```
    pub fn scope<F>(self, f: F) -> Self
    where
        F: FnOnce(Self) -> Self,
    {
        f(self)
    }
    
    /// Apply a conditional scope
    ///
    /// Only applies the scope function if the condition is true.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let include_inactive = false;
    /// 
    /// let users = User::query()
    ///     .when(include_inactive, |q| q.with_trashed())
    ///     .get()
    ///     .await?;
    /// ```
    pub fn when<F>(self, condition: bool, f: F) -> Self
    where
        F: FnOnce(Self) -> Self,
    {
        if condition {
            f(self)
        } else {
            self
        }
    }
    
    /// Apply a scope based on an Option value
    ///
    /// If the option is Some, applies the scope function with the value.
    /// If None, returns the query unchanged.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let status_filter: Option<&str> = Some("active");
    /// 
    /// let users = User::query()
    ///     .when_some(status_filter, |q, status| q.where_eq("status", status))
    ///     .get()
    ///     .await?;
    /// ```
    pub fn when_some<T, F>(self, option: Option<T>, f: F) -> Self
    where
        F: FnOnce(Self, T) -> Self,
    {
        match option {
            Some(value) => f(self, value),
            None => self,
        }
    }
    
    // =========================================================================
    // INTERNAL: BUILD SEAORM CONDITIONS
    // =========================================================================
    
    /// Convert a JSON value to a SeaORM Value
    fn json_to_sea_value(value: &serde_json::Value) -> crate::internal::Value {
        use crate::internal::Value;
        match value {
            serde_json::Value::Null => Value::String(None),
            serde_json::Value::Bool(b) => Value::Bool(Some(*b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::BigInt(Some(i))
                } else if let Some(f) = n.as_f64() {
                    Value::Double(Some(f))
                } else {
                    Value::String(Some(Box::new(n.to_string())))
                }
            }
            serde_json::Value::String(s) => Value::String(Some(Box::new(s.clone()))),
            // For arrays and objects, serialize to string
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                Value::String(Some(Box::new(value.to_string())))
            }
        }
    }
    
    /// Build SeaORM Condition from our WhereConditions
    fn build_sea_condition(&self) -> Condition {
        use sea_orm::sea_query::{Alias, SimpleExpr};
        
        let mut condition = Condition::all();
        
        for where_cond in &self.conditions {
            let col = Expr::col(Alias::new(&where_cond.column));
            
            let expr: SimpleExpr = match &where_cond.operator {
                Operator::Eq => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        col.eq(Self::json_to_sea_value(val))
                    } else {
                        continue;
                    }
                }
                Operator::NotEq => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        col.ne(Self::json_to_sea_value(val))
                    } else {
                        continue;
                    }
                }
                Operator::Gt => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        col.gt(Self::json_to_sea_value(val))
                    } else {
                        continue;
                    }
                }
                Operator::Gte => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        col.gte(Self::json_to_sea_value(val))
                    } else {
                        continue;
                    }
                }
                Operator::Lt => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        col.lt(Self::json_to_sea_value(val))
                    } else {
                        continue;
                    }
                }
                Operator::Lte => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        col.lte(Self::json_to_sea_value(val))
                    } else {
                        continue;
                    }
                }
                Operator::Like => {
                    if let ConditionValue::Single(serde_json::Value::String(pattern)) = &where_cond.value {
                        col.like(pattern.as_str())
                    } else {
                        continue;
                    }
                }
                Operator::NotLike => {
                    if let ConditionValue::Single(serde_json::Value::String(pattern)) = &where_cond.value {
                        col.not_like(pattern.as_str())
                    } else {
                        continue;
                    }
                }
                Operator::In => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        let sea_values: Vec<_> = values.iter().map(Self::json_to_sea_value).collect();
                        col.is_in(sea_values)
                    } else {
                        continue;
                    }
                }
                Operator::NotIn => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        let sea_values: Vec<_> = values.iter().map(Self::json_to_sea_value).collect();
                        col.is_not_in(sea_values)
                    } else {
                        continue;
                    }
                }
                Operator::IsNull => {
                    col.is_null()
                }
                Operator::IsNotNull => {
                    col.is_not_null()
                }
                Operator::Between => {
                    if let ConditionValue::Range(low, high) = &where_cond.value {
                        col.between(Self::json_to_sea_value(low), Self::json_to_sea_value(high))
                    } else {
                        continue;
                    }
                }
                Operator::JsonContains => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        // Use PostgreSQL JSON containment: column @> value
                        let value_str = val.to_string();
                        Expr::cust(format!("\"{}\" @> '{}'", &where_cond.column, value_str.replace("'", "''")))
                    } else {
                        continue;
                    }
                }
                Operator::JsonContainedBy => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        // Use PostgreSQL JSON containment: column <@ value
                        let value_str = val.to_string();
                        Expr::cust(format!("\"{}\" <@ '{}'", &where_cond.column, value_str.replace("'", "''")))
                    } else {
                        continue;
                    }
                }
                Operator::JsonKeyExists => {
                    if let ConditionValue::Single(serde_json::Value::String(key)) = &where_cond.value {
                        // Use PostgreSQL JSON key existence: column ? key
                        Expr::cust(format!("\"{}\" ? '{}'", &where_cond.column, key.replace("'", "''")))
                    } else {
                        continue;
                    }
                }
                Operator::JsonKeyNotExists => {
                    if let ConditionValue::Single(serde_json::Value::String(key)) = &where_cond.value {
                        // Use PostgreSQL JSON key non-existence: NOT (column ? key)
                        Expr::cust(format!("NOT (\"{}\" ? '{}')", &where_cond.column, key.replace("'", "''")))
                    } else {
                        continue;
                    }
                }
                Operator::JsonPathExists => {
                    if let ConditionValue::Single(serde_json::Value::String(path)) = &where_cond.value {
                        // Use PostgreSQL JSON path existence: column @? path
                        Expr::cust(format!("\"{}\" @? '{}'", &where_cond.column, path.replace("'", "''")))
                    } else {
                        continue;
                    }
                }
                Operator::JsonPathNotExists => {
                    if let ConditionValue::Single(serde_json::Value::String(path)) = &where_cond.value {
                        // Use PostgreSQL JSON path non-existence: column ?! path
                        Expr::cust(format!("\"{}\" ?! '{}'", &where_cond.column, path.replace("'", "''")))
                    } else {
                        continue;
                    }
                }
                Operator::ArrayContains => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        // Use PostgreSQL array containment: column @> ARRAY[values]
                        let array_literal = format!("ARRAY[{}]", 
                            values.iter()
                                .map(|v| match v {
                                    serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                    serde_json::Value::Number(n) => n.to_string(),
                                    serde_json::Value::Bool(b) => b.to_string(),
                                    _ => v.to_string(),
                                })
                                .collect::<Vec<_>>()
                                .join(",")
                        );
                        Expr::cust(format!("\"{}\" @> {}", &where_cond.column, array_literal))
                    } else {
                        continue;
                    }
                }
                Operator::ArrayContainedBy => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        // Use PostgreSQL array containment: column <@ ARRAY[values]
                        let array_literal = format!("ARRAY[{}]", 
                            values.iter()
                                .map(|v| match v {
                                    serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                    serde_json::Value::Number(n) => n.to_string(),
                                    serde_json::Value::Bool(b) => b.to_string(),
                                    _ => v.to_string(),
                                })
                                .collect::<Vec<_>>()
                                .join(",")
                        );
                        Expr::cust(format!("\"{}\" <@ {}", &where_cond.column, array_literal))
                    } else {
                        continue;
                    }
                }
                Operator::ArrayOverlaps => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        // Use PostgreSQL array overlap: column && ARRAY[values]
                        let array_literal = format!("ARRAY[{}]", 
                            values.iter()
                                .map(|v| match v {
                                    serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                    serde_json::Value::Number(n) => n.to_string(),
                                    serde_json::Value::Bool(b) => b.to_string(),
                                    _ => v.to_string(),
                                })
                                .collect::<Vec<_>>()
                                .join(",")
                        );
                        Expr::cust(format!("\"{}\" && {}", &where_cond.column, array_literal))
                    } else {
                        continue;
                    }
                }
                Operator::ArrayContainsAny => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        // Use PostgreSQL array overlap for "contains any": column && ARRAY[values]
                        let array_literal = format!("ARRAY[{}]", 
                            values.iter()
                                .map(|v| match v {
                                    serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                    serde_json::Value::Number(n) => n.to_string(),
                                    serde_json::Value::Bool(b) => b.to_string(),
                                    _ => v.to_string(),
                                })
                                .collect::<Vec<_>>()
                                .join(",")
                        );
                        Expr::cust(format!("\"{}\" && {}", &where_cond.column, array_literal))
                    } else {
                        continue;
                    }
                }
                Operator::ArrayContainsAll => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        // Use PostgreSQL array containment: ARRAY[values] <@ column
                        // This checks if all values are contained in the column array
                        let array_literal = format!("ARRAY[{}]", 
                            values.iter()
                                .map(|v| match v {
                                    serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                    serde_json::Value::Number(n) => n.to_string(),
                                    serde_json::Value::Bool(b) => b.to_string(),
                                    _ => v.to_string(),
                                })
                                .collect::<Vec<_>>()
                                .join(",")
                        );
                        Expr::cust(format!("{} <@ \"{}\"", array_literal, &where_cond.column))
                    } else {
                        continue;
                    }
                }
            };
            
            condition = condition.add(expr);
        }
        
        // Apply soft delete filter if model supports it
        if M::soft_delete_enabled() {
            use sea_orm::sea_query::Alias;
            let deleted_at_col = Expr::col(Alias::new("deleted_at"));
            
            if self.only_trashed {
                // Only return soft-deleted records
                condition = condition.add(deleted_at_col.is_not_null());
            } else if !self.include_trashed {
                // Exclude soft-deleted records (default behavior)
                condition = condition.add(deleted_at_col.is_null());
            }
            // If include_trashed is true, don't add any filter
        }
        
        condition
    }
    
    /// Log the query if logging is enabled
    #[allow(dead_code)]
    fn log_query(&self, sql: &str) {
        // Check if query logging is enabled via environment variable
        if std::env::var("TIDE_LOG_QUERIES")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(false)
        {
            eprintln!("[TideORM Query] {}", sql);
        }
    }
    
    // =========================================================================
    // EXECUTION
    // =========================================================================
    
    /// Execute the query and get all matching records
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let users = User::query()
    ///     .where_eq("active", true)
    ///     .get()
    ///     .await?;
    /// ```
    pub async fn get(self) -> Result<Vec<M>> {
        use sea_orm::sea_query::Alias;
        
        let conn = crate::database::db().__internal_connection();
        
        // If we have JOINs or GROUP BY, use raw SQL
        if !self.joins.is_empty() || !self.group_by.is_empty() {
            return self.get_with_joins().await;
        }
        
        // Start with Entity::find()
        let mut select = M::Entity::find();
        
        // Apply WHERE conditions (including soft delete filter if applicable)
        // Always build conditions if there are explicit conditions or soft delete is enabled
        if !self.conditions.is_empty() || M::soft_delete_enabled() {
            let condition = self.build_sea_condition();
            select = select.filter(condition);
        }
        
        // Apply ORDER BY
        for (column, direction) in &self.order_by {
            let col_expr = Expr::col(Alias::new(column));
            select = match direction {
                Order::Asc => select.order_by_asc(col_expr),
                Order::Desc => select.order_by_desc(col_expr),
            };
        }
        
        // Apply LIMIT
        if let Some(limit) = self.limit_value {
            select = select.limit(limit);
        }
        
        // Apply OFFSET
        if let Some(offset) = self.offset_value {
            select = select.offset(offset);
        }
        
        // Execute and convert
        let results = select
            .all(conn)
            .await
            .map_err(translate_error)?;
        
        Ok(results.into_iter().map(M::from_sea_model).collect())
    }
    
    /// Execute query with JOINs using raw SQL
    async fn get_with_joins(self) -> Result<Vec<M>> {
        let sql = self.build_select_sql();
        
        // Execute using raw SQL
        let results = crate::database::Database::raw::<M>(&sql).await?;
        Ok(results)
    }
    
    /// Build SELECT SQL string with JOINs, GROUP BY, HAVING
    fn build_select_sql(&self) -> String {
        let table = M::table_name();
        let mut sql = String::new();
        
        // SELECT clause
        if let Some(ref columns) = self.select_columns {
            let cols: Vec<String> = columns.iter()
                .map(|c| {
                    if c.contains('.') || c.contains('(') || c.contains('*') {
                        c.clone()
                    } else {
                        format!("\"{}\".\"{}\""  , table, c)
                    }
                })
                .collect();
            sql.push_str(&format!("SELECT {} ", cols.join(", ")));
        } else {
            sql.push_str(&format!("SELECT \"{}\".*  ", table));
        }
        
        // FROM clause
        sql.push_str(&format!("FROM \"{}\" ", table));
        
        // JOIN clauses
        for join in &self.joins {
            let join_table = if let Some(ref alias) = join.alias {
                format!("\"{}\" AS \"{}\"", join.table, alias)
            } else {
                format!("\"{}\"", join.table)
            };
            
            sql.push_str(&format!(
                "{} {} ON {} = {} ",
                join.join_type.as_sql(),
                join_table,
                self.format_column(&join.left_column),
                self.format_column(&join.right_column)
            ));
        }
        
        // WHERE clause
        let where_sql = self.build_where_sql();
        if !where_sql.is_empty() {
            sql.push_str(&format!("WHERE {} ", where_sql));
        }
        
        // GROUP BY clause
        if !self.group_by.is_empty() {
            let group_cols: Vec<String> = self.group_by.iter()
                .map(|c| self.format_column(c))
                .collect();
            sql.push_str(&format!("GROUP BY {} ", group_cols.join(", ")));
        }
        
        // HAVING clause
        if !self.having_conditions.is_empty() {
            sql.push_str(&format!("HAVING {} ", self.having_conditions.join(" AND ")));
        }
        
        // ORDER BY clause
        if !self.order_by.is_empty() {
            let order_parts: Vec<String> = self.order_by.iter()
                .map(|(col, dir)| format!("{} {}", self.format_column(col), dir.as_str()))
                .collect();
            sql.push_str(&format!("ORDER BY {} ", order_parts.join(", ")));
        }
        
        // LIMIT clause
        if let Some(limit) = self.limit_value {
            sql.push_str(&format!("LIMIT {} ", limit));
        }
        
        // OFFSET clause
        if let Some(offset) = self.offset_value {
            sql.push_str(&format!("OFFSET {} ", offset));
        }
        
        sql.trim().to_string()
    }
    
    /// Format a column name, handling table.column syntax
    fn format_column(&self, column: &str) -> String {
        if column.contains('(') || column.contains('*') || column.contains('"') {
            // Already formatted or is an expression
            column.to_string()
        } else if column.contains('.') {
            // table.column format
            let parts: Vec<&str> = column.split('.').collect();
            if parts.len() == 2 {
                format!("\"{}\".\"{}\"", parts[0], parts[1])
            } else {
                column.to_string()
            }
        } else {
            // Simple column name
            format!("\"{}\"", column)
        }
    }
    
    /// Build WHERE clause SQL
    fn build_where_sql(&self) -> String {
        let mut conditions = Vec::new();
        
        for cond in &self.conditions {
            let col = self.format_column(&cond.column);
            
            let expr = match &cond.operator {
                Operator::Eq => {
                    if let ConditionValue::Single(val) = &cond.value {
                        format!("{} = {}", col, self.format_value(val))
                    } else { continue; }
                }
                Operator::NotEq => {
                    if let ConditionValue::Single(val) = &cond.value {
                        format!("{} != {}", col, self.format_value(val))
                    } else { continue; }
                }
                Operator::Gt => {
                    if let ConditionValue::Single(val) = &cond.value {
                        format!("{} > {}", col, self.format_value(val))
                    } else { continue; }
                }
                Operator::Gte => {
                    if let ConditionValue::Single(val) = &cond.value {
                        format!("{} >= {}", col, self.format_value(val))
                    } else { continue; }
                }
                Operator::Lt => {
                    if let ConditionValue::Single(val) = &cond.value {
                        format!("{} < {}", col, self.format_value(val))
                    } else { continue; }
                }
                Operator::Lte => {
                    if let ConditionValue::Single(val) = &cond.value {
                        format!("{} <= {}", col, self.format_value(val))
                    } else { continue; }
                }
                Operator::Like => {
                    if let ConditionValue::Single(serde_json::Value::String(pattern)) = &cond.value {
                        format!("{} LIKE '{}'", col, pattern.replace("'", "''"))
                    } else { continue; }
                }
                Operator::NotLike => {
                    if let ConditionValue::Single(serde_json::Value::String(pattern)) = &cond.value {
                        format!("{} NOT LIKE '{}'", col, pattern.replace("'", "''"))
                    } else { continue; }
                }
                Operator::In => {
                    if let ConditionValue::List(values) = &cond.value {
                        let vals: Vec<String> = values.iter().map(|v| self.format_value(v)).collect();
                        format!("{} IN ({})", col, vals.join(", "))
                    } else { continue; }
                }
                Operator::NotIn => {
                    if let ConditionValue::List(values) = &cond.value {
                        let vals: Vec<String> = values.iter().map(|v| self.format_value(v)).collect();
                        format!("{} NOT IN ({})", col, vals.join(", "))
                    } else { continue; }
                }
                Operator::IsNull => format!("{} IS NULL", col),
                Operator::IsNotNull => format!("{} IS NOT NULL", col),
                Operator::Between => {
                    if let ConditionValue::Range(low, high) = &cond.value {
                        format!("{} BETWEEN {} AND {}", col, self.format_value(low), self.format_value(high))
                    } else { continue; }
                }
                Operator::JsonContains => {
                    if let ConditionValue::Single(val) = &cond.value {
                        format!("{} @> '{}'", col, val.to_string().replace("'", "''"))
                    } else { continue; }
                }
                Operator::JsonContainedBy => {
                    if let ConditionValue::Single(val) = &cond.value {
                        format!("{} <@ '{}'", col, val.to_string().replace("'", "''"))
                    } else { continue; }
                }
                Operator::JsonKeyExists => {
                    if let ConditionValue::Single(serde_json::Value::String(key)) = &cond.value {
                        format!("{} ? '{}'", col, key.replace("'", "''"))
                    } else { continue; }
                }
                Operator::JsonKeyNotExists => {
                    if let ConditionValue::Single(serde_json::Value::String(key)) = &cond.value {
                        format!("NOT ({} ? '{}')", col, key.replace("'", "''"))
                    } else { continue; }
                }
                Operator::JsonPathExists => {
                    if let ConditionValue::Single(serde_json::Value::String(path)) = &cond.value {
                        format!("{} @? '{}'", col, path.replace("'", "''"))
                    } else { continue; }
                }
                Operator::JsonPathNotExists => {
                    if let ConditionValue::Single(serde_json::Value::String(path)) = &cond.value {
                        format!("NOT ({} @? '{}')", col, path.replace("'", "''"))
                    } else { continue; }
                }
                Operator::ArrayContains | Operator::ArrayContainsAll => {
                    if let ConditionValue::List(values) = &cond.value {
                        let array_lit = self.format_array_literal(values);
                        format!("{} @> {}", col, array_lit)
                    } else { continue; }
                }
                Operator::ArrayContainedBy => {
                    if let ConditionValue::List(values) = &cond.value {
                        let array_lit = self.format_array_literal(values);
                        format!("{} <@ {}", col, array_lit)
                    } else { continue; }
                }
                Operator::ArrayOverlaps | Operator::ArrayContainsAny => {
                    if let ConditionValue::List(values) = &cond.value {
                        let array_lit = self.format_array_literal(values);
                        format!("{} && {}", col, array_lit)
                    } else { continue; }
                }
            };
            
            conditions.push(expr);
        }
        
        // Add soft delete filter if applicable
        if M::soft_delete_enabled() {
            if self.only_trashed {
                conditions.push("\"deleted_at\" IS NOT NULL".to_string());
            } else if !self.include_trashed {
                conditions.push("\"deleted_at\" IS NULL".to_string());
            }
        }
        
        conditions.join(" AND ")
    }
    
    /// Format a JSON value for SQL
    fn format_value(&self, value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Null => "NULL".to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                format!("'{}'", value.to_string().replace("'", "''"))
            }
        }
    }
    
    /// Format an array literal for PostgreSQL
    fn format_array_literal(&self, values: &[serde_json::Value]) -> String {
        let elements: Vec<String> = values.iter()
            .map(|v| match v {
                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => v.to_string(),
            })
            .collect();
        format!("ARRAY[{}]", elements.join(","))
    }

    /// Execute the query and get the first matching record
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let user = User::query()
    ///     .where_eq("email", "admin@example.com")
    ///     .first()
    ///     .await?;
    /// ```
    pub async fn first(self) -> Result<Option<M>> {
        use sea_orm::sea_query::Alias;
        
        let conn = crate::database::db().__internal_connection();
        
        // If we have JOINs or GROUP BY, use raw SQL
        if !self.joins.is_empty() || !self.group_by.is_empty() {
            let results = self.limit(1).get_with_joins().await?;
            return Ok(results.into_iter().next());
        }
        
        // Start with Entity::find()
        let mut select = M::Entity::find();
        
        // Apply WHERE conditions (including soft delete filter if applicable)
        if !self.conditions.is_empty() || M::soft_delete_enabled() {
            let condition = self.build_sea_condition();
            select = select.filter(condition);
        }
        
        // Apply ORDER BY
        for (column, direction) in &self.order_by {
            let col_expr = Expr::col(Alias::new(column));
            select = match direction {
                Order::Asc => select.order_by_asc(col_expr),
                Order::Desc => select.order_by_desc(col_expr),
            };
        }
        
        // Limit to 1
        select = select.limit(1);
        
        // Apply OFFSET
        if let Some(offset) = self.offset_value {
            select = select.offset(offset);
        }
        
        // Execute and convert
        let result = select
            .one(conn)
            .await
            .map_err(translate_error)?;
        
        Ok(result.map(M::from_sea_model))
    }
    
    /// Execute the query and get the first record or fail
    ///
    /// # Errors
    ///
    /// Returns `Error::NotFound` if no record matches.
    pub async fn first_or_fail(self) -> Result<M> {
        self.first().await?.ok_or_else(|| {
            crate::error::Error::not_found(format!(
                "No {} found matching query",
                M::table_name()
            ))
        })
    }
    
    /// Count matching records using efficient SQL COUNT
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let count = User::query()
    ///     .where_eq("active", true)
    ///     .count()
    ///     .await?;
    /// ```
    pub async fn count(self) -> Result<u64> {
        #[derive(Debug, FromQueryResult)]
        struct CountResult {
            count: i64,
        }
        
        let conn = crate::database::db().__internal_connection();
        
        // Start with Entity::find()
        let mut select = M::Entity::find();
        
        // Apply WHERE conditions (including soft delete filter if applicable)
        if !self.conditions.is_empty() || M::soft_delete_enabled() {
            let condition = self.build_sea_condition();
            select = select.filter(condition);
        }
        
        // Use SELECT COUNT(*) for efficiency
        let result: Option<CountResult> = select
            .select_only()
            .column_as(Expr::col(Asterisk).count(), "count")
            .into_model::<CountResult>()
            .one(conn)
            .await
            .map_err(translate_error)?;
        
        Ok(result.map(|r| r.count as u64).unwrap_or(0))
    }
    
    /// Check if any records exist
    pub async fn exists(self) -> Result<bool> {
        Ok(self.count().await? > 0)
    }
    
    /// Delete all matching records using efficient bulk delete
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let deleted = User::query()
    ///     .where_eq("status", "inactive")
    ///     .delete()
    ///     .await?;
    /// println!("Deleted {} records", deleted);
    /// ```
    pub async fn delete(self) -> Result<u64> {
        let conn = crate::database::db().__internal_connection();
        
        // Start with Entity::delete_many()
        let mut delete = M::Entity::delete_many();
        
        // Apply WHERE conditions (including soft delete filter if applicable)
        if !self.conditions.is_empty() || M::soft_delete_enabled() {
            let condition = self.build_sea_condition();
            delete = delete.filter(condition);
        }
        
        // Execute bulk delete
        let result = delete
            .exec(conn)
            .await
            .map_err(translate_error)?;
        
        Ok(result.rows_affected)
    }
}

impl<M: Model> Default for QueryBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}
