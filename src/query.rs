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

use crate::config::DatabaseType;
use crate::error::{Error, Result};
use crate::model::Model;
use crate::tide_debug;
use crate::internal::{
    EntityTrait, QueryFilter, QueryOrder, QuerySelect, Condition, 
    Expr, translate_error, FromQueryResult, Asterisk, ConnectionTrait, Statement, DbBackend,
    ExprTrait,
};

// =============================================================================
// DATABASE-SPECIFIC SQL GENERATION
// =============================================================================

/// Helper module for generating database-specific SQL expressions
mod db_sql {
    use crate::config::DatabaseType;
    
    /// Get the identifier quote character for the database
    pub fn quote_char(db_type: DatabaseType) -> char {
        match db_type {
            DatabaseType::Postgres | DatabaseType::SQLite => '"',
            DatabaseType::MySQL | DatabaseType::MariaDB => '`',
        }
    }
    
    /// Quote an identifier (column or table name)
    pub fn quote_ident(db_type: DatabaseType, name: &str) -> String {
        let q = quote_char(db_type);
        format!("{}{}{}", q, name, q)
    }
    
    /// Generate JSON contains expression
    /// 
    /// - PostgreSQL: `column @> 'value'`
    /// - MySQL: `JSON_CONTAINS(column, 'value')`
    /// - SQLite: `json_type(column) IS NOT NULL AND json(column) LIKE '%value%'` (fallback)
    pub fn json_contains(db_type: DatabaseType, column: &str, value: &str) -> String {
        let escaped_value = value.replace("'", "''");
        match db_type {
            DatabaseType::Postgres => {
                format!("\"{}\" @> '{}'", column, escaped_value)
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                format!("JSON_CONTAINS(`{}`, '{}')", column, escaped_value)
            }
            DatabaseType::SQLite => {
                // SQLite JSON1 extension - use json_each for containment check
                // This is a simplified version; for complex JSON, more elaborate SQL is needed
                format!(
                    "EXISTS (SELECT 1 FROM json_each(\"{}\") WHERE value = '{}')",
                    column, escaped_value.trim_matches('"')
                )
            }
        }
    }
    
    /// Generate JSON contained by expression
    ///
    /// - PostgreSQL: `column <@ 'value'`
    /// - MySQL: `JSON_CONTAINS('value', column)`
    /// - SQLite: Limited support via JSON1
    pub fn json_contained_by(db_type: DatabaseType, column: &str, value: &str) -> String {
        let escaped_value = value.replace("'", "''");
        match db_type {
            DatabaseType::Postgres => {
                format!("\"{}\" <@ '{}'", column, escaped_value)
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                format!("JSON_CONTAINS('{}', `{}`)", escaped_value, column)
            }
            DatabaseType::SQLite => {
                // SQLite fallback - less precise
                format!(
                    "json_type(\"{}\") IS NOT NULL AND '{}' LIKE '%' || \"{}\" || '%'",
                    column, escaped_value, column
                )
            }
        }
    }
    
    /// Generate JSON key exists expression
    ///
    /// - PostgreSQL: `column ? 'key'`
    /// - MySQL: `JSON_CONTAINS_PATH(column, 'one', '$.key')`
    /// - SQLite: `json_extract(column, '$.key') IS NOT NULL`
    pub fn json_key_exists(db_type: DatabaseType, column: &str, key: &str) -> String {
        let escaped_key = key.replace("'", "''");
        match db_type {
            DatabaseType::Postgres => {
                format!("\"{}\" ? '{}'", column, escaped_key)
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                format!("JSON_CONTAINS_PATH(`{}`, 'one', '$.{}')", column, escaped_key)
            }
            DatabaseType::SQLite => {
                format!("json_extract(\"{}\", '$.{}') IS NOT NULL", column, escaped_key)
            }
        }
    }
    
    /// Generate JSON key not exists expression
    pub fn json_key_not_exists(db_type: DatabaseType, column: &str, key: &str) -> String {
        let escaped_key = key.replace("'", "''");
        match db_type {
            DatabaseType::Postgres => {
                format!("NOT (\"{}\" ? '{}')", column, escaped_key)
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                format!("NOT JSON_CONTAINS_PATH(`{}`, 'one', '$.{}')", column, escaped_key)
            }
            DatabaseType::SQLite => {
                format!("json_extract(\"{}\", '$.{}') IS NULL", column, escaped_key)
            }
        }
    }
    
    /// Generate JSON path exists expression
    ///
    /// - PostgreSQL: `column @? 'path'`
    /// - MySQL: `JSON_CONTAINS_PATH(column, 'one', 'path')`
    /// - SQLite: `json_extract(column, 'path') IS NOT NULL`
    pub fn json_path_exists(db_type: DatabaseType, column: &str, path: &str) -> String {
        let escaped_path = path.replace("'", "''");
        match db_type {
            DatabaseType::Postgres => {
                format!("\"{}\" @? '{}'", column, escaped_path)
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                format!("JSON_CONTAINS_PATH(`{}`, 'one', '{}')", column, escaped_path)
            }
            DatabaseType::SQLite => {
                format!("json_extract(\"{}\", '{}') IS NOT NULL", column, escaped_path)
            }
        }
    }
    
    /// Generate JSON path not exists expression
    pub fn json_path_not_exists(db_type: DatabaseType, column: &str, path: &str) -> String {
        let escaped_path = path.replace("'", "''");
        match db_type {
            DatabaseType::Postgres => {
                format!("NOT (\"{}\" @? '{}')", column, escaped_path)
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                format!("NOT JSON_CONTAINS_PATH(`{}`, 'one', '{}')", column, escaped_path)
            }
            DatabaseType::SQLite => {
                format!("json_extract(\"{}\", '{}') IS NULL", column, escaped_path)
            }
        }
    }
    
    /// Generate array contains expression
    ///
    /// - PostgreSQL: `column @> ARRAY[values]`
    /// - MySQL: Uses JSON_CONTAINS with JSON array
    /// - SQLite: Uses json_each for array element checking
    pub fn array_contains(db_type: DatabaseType, column: &str, values: &[String]) -> String {
        match db_type {
            DatabaseType::Postgres => {
                format!("\"{}\" @> ARRAY[{}]", column, values.join(","))
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                // MySQL stores arrays as JSON arrays
                let json_array = format!("[{}]", values.iter()
                    .map(|v| if v.starts_with("'") { v.clone() } else { format!("\"{}\"", v.trim_matches('\'')) })
                    .collect::<Vec<_>>()
                    .join(","));
                format!("JSON_CONTAINS(`{}`, '{}')", column, json_array.replace("'", "''"))
            }
            DatabaseType::SQLite => {
                // SQLite: Check all values exist in the JSON array
                let conditions: Vec<String> = values.iter()
                    .map(|v| {
                        let clean_val = v.trim_matches('\'');
                        format!(
                            "EXISTS (SELECT 1 FROM json_each(\"{}\") WHERE value = '{}')",
                            column, clean_val.replace("'", "''")
                        )
                    })
                    .collect();
                format!("({})", conditions.join(" AND "))
            }
        }
    }
    
    /// Generate array contained by expression
    pub fn array_contained_by(db_type: DatabaseType, column: &str, values: &[String]) -> String {
        match db_type {
            DatabaseType::Postgres => {
                format!("\"{}\" <@ ARRAY[{}]", column, values.join(","))
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                let json_array = format!("[{}]", values.iter()
                    .map(|v| if v.starts_with("'") { v.clone() } else { format!("\"{}\"", v.trim_matches('\'')) })
                    .collect::<Vec<_>>()
                    .join(","));
                format!("JSON_CONTAINS('{}', `{}`)", json_array.replace("'", "''"), column)
            }
            DatabaseType::SQLite => {
                // All elements in column array must be in the provided values
                let value_list = values.iter()
                    .map(|v| format!("'{}'", v.trim_matches('\'').replace("'", "''")))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "NOT EXISTS (SELECT 1 FROM json_each(\"{}\") WHERE value NOT IN ({}))",
                    column, value_list
                )
            }
        }
    }
    
    /// Generate array overlaps expression (any element matches)
    pub fn array_overlaps(db_type: DatabaseType, column: &str, values: &[String]) -> String {
        match db_type {
            DatabaseType::Postgres => {
                format!("\"{}\" && ARRAY[{}]", column, values.join(","))
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                // Check if any value exists in the JSON array
                let conditions: Vec<String> = values.iter()
                    .map(|v| {
                        let clean_val = v.trim_matches('\'');
                        format!("JSON_CONTAINS(`{}`, '\"{}\"')", column, clean_val.replace("'", "''"))
                    })
                    .collect();
                format!("({})", conditions.join(" OR "))
            }
            DatabaseType::SQLite => {
                // Any element matches
                let conditions: Vec<String> = values.iter()
                    .map(|v| {
                        let clean_val = v.trim_matches('\'');
                        format!(
                            "EXISTS (SELECT 1 FROM json_each(\"{}\") WHERE value = '{}')",
                            column, clean_val.replace("'", "''")
                        )
                    })
                    .collect();
                format!("({})", conditions.join(" OR "))
            }
        }
    }
    
    /// Format a column identifier for the database
    pub fn format_column(db_type: DatabaseType, column: &str) -> String {
        if column.contains('(') || column.contains('*') {
            // Already formatted or is an expression
            column.to_string()
        } else if column.contains('.') {
            // table.column format
            let parts: Vec<&str> = column.split('.').collect();
            if parts.len() == 2 {
                let q = quote_char(db_type);
                format!("{0}{1}{0}.{0}{2}{0}", q, parts[0], parts[1])
            } else {
                column.to_string()
            }
        } else {
            quote_ident(db_type, column)
        }
    }
    
    /// Format an array literal for the database
    #[allow(dead_code)]
    pub fn format_array_literal(db_type: DatabaseType, values: &[serde_json::Value]) -> String {
        let elements: Vec<String> = values.iter()
            .map(|v| match v {
                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => v.to_string(),
            })
            .collect();
        
        match db_type {
            DatabaseType::Postgres => format!("ARRAY[{}]", elements.join(",")),
            DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => {
                // Store as JSON array
                format!("'[{}]'", elements.iter()
                    .map(|e| if e.starts_with("'") {
                        format!("\"{}\"", e.trim_matches('\''))
                    } else {
                        e.clone()
                    })
                    .collect::<Vec<_>>()
                    .join(","))
            }
        }
    }
    
    /// Generate aggregate function with proper casting for the database
    pub fn cast_to_float(db_type: DatabaseType, expr: &str) -> String {
        match db_type {
            DatabaseType::Postgres => format!("CAST({} AS FLOAT8)", expr),
            DatabaseType::MySQL | DatabaseType::MariaDB => format!("CAST({} AS DOUBLE)", expr),
            DatabaseType::SQLite => format!("CAST({} AS REAL)", expr),
        }
    }
    
    /// Generate = ANY(array) expression (PostgreSQL optimization for IN)
    ///
    /// On PostgreSQL, `col = ANY(ARRAY[...])` is often faster than `col IN (...)`
    /// because the query plan can be cached and reused with different arrays.
    /// On other databases, falls back to standard IN clause.
    pub fn eq_any(db_type: DatabaseType, column: &str, values: &[String]) -> String {
        match db_type {
            DatabaseType::Postgres => {
                format!("{} = ANY(ARRAY[{}])", column, values.join(","))
            }
            DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => {
                // Fall back to standard IN clause
                format!("{} IN ({})", column, values.join(","))
            }
        }
    }
    
    /// Generate <> ALL(array) expression (PostgreSQL optimization for NOT IN)
    ///
    /// On PostgreSQL, `col <> ALL(ARRAY[...])` is often faster than `col NOT IN (...)`
    /// On other databases, falls back to standard NOT IN clause.
    pub fn ne_all(db_type: DatabaseType, column: &str, values: &[String]) -> String {
        match db_type {
            DatabaseType::Postgres => {
                format!("{} <> ALL(ARRAY[{}])", column, values.join(","))
            }
            DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => {
                // Fall back to standard NOT IN clause
                format!("{} NOT IN ({})", column, values.join(","))
            }
        }
    }
}

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
    pub fn where_not_in<V: Into<serde_json::Value>>(mut self, column: &str, values: Vec<V>) -> Self {
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
        let nested_count: usize = self.nested_groups.iter()
            .map(|g| g.condition_count())
            .sum();
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
    pub fn where_eq(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Eq,
            value: ConditionValue::Single(value.into()),
        });
        self
    }
    
    /// Add a where not equals condition
    pub fn where_not(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::NotEq,
            value: ConditionValue::Single(value.into()),
        });
        self
    }
    
    /// Add a where greater than condition
    pub fn where_gt(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Gt,
            value: ConditionValue::Single(value.into()),
        });
        self
    }
    
    /// Add a where greater than or equal condition
    pub fn where_gte(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Gte,
            value: ConditionValue::Single(value.into()),
        });
        self
    }
    
    /// Add a where less than condition
    pub fn where_lt(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Lt,
            value: ConditionValue::Single(value.into()),
        });
        self
    }
    
    /// Add a where less than or equal condition
    pub fn where_lte(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Lte,
            value: ConditionValue::Single(value.into()),
        });
        self
    }
    
    /// Add a where LIKE condition
    pub fn where_like(mut self, column: impl crate::columns::IntoColumnName, pattern: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Like,
            value: ConditionValue::Single(serde_json::Value::String(pattern.to_string())),
        });
        self
    }
    
    /// Add a where NOT LIKE condition
    pub fn where_not_like(mut self, column: impl crate::columns::IntoColumnName, pattern: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::NotLike,
            value: ConditionValue::Single(serde_json::Value::String(pattern.to_string())),
        });
        self
    }
    
    /// Add a where IN condition
    pub fn where_in<V: Into<serde_json::Value>>(mut self, column: impl crate::columns::IntoColumnName, values: Vec<V>) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::In,
            value: ConditionValue::List(values.into_iter().map(|v| v.into()).collect()),
        });
        self
    }
    
    /// Add a where NOT IN condition
    pub fn where_not_in<V: Into<serde_json::Value>>(mut self, column: impl crate::columns::IntoColumnName, values: Vec<V>) -> Self {
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
    pub fn or_where_eq(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        // Save current branch if it has conditions
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        // Start new branch with the condition
        self.current_branch = OrBranch::new().where_eq(column, value);
        self
    }

    /// Start a new OR branch with a not equals condition
    pub fn or_where_not(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_not(column, value);
        self
    }

    /// Start a new OR branch with a greater than condition
    pub fn or_where_gt(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_gt(column, value);
        self
    }

    /// Start a new OR branch with a greater than or equal condition
    pub fn or_where_gte(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_gte(column, value);
        self
    }

    /// Start a new OR branch with a less than condition
    pub fn or_where_lt(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_lt(column, value);
        self
    }

    /// Start a new OR branch with a less than or equal condition
    pub fn or_where_lte(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_lte(column, value);
        self
    }

    /// Start a new OR branch with a LIKE condition
    pub fn or_where_like(mut self, column: impl crate::columns::IntoColumnName, pattern: &str) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_like(column, pattern);
        self
    }

    /// Start a new OR branch with an IN condition
    pub fn or_where_in<V: Into<serde_json::Value>>(mut self, column: impl crate::columns::IntoColumnName, values: Vec<V>) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_in(column, values);
        self
    }

    /// Start a new OR branch with a NOT IN condition
    pub fn or_where_not_in<V: Into<serde_json::Value>>(mut self, column: impl crate::columns::IntoColumnName, values: Vec<V>) -> Self {
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
    pub fn and_where_eq(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        self.current_branch = self.current_branch.where_eq(column, value);
        self
    }

    /// Add an AND not equals condition to the current OR branch
    pub fn and_where_not(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        self.current_branch = self.current_branch.where_not(column, value);
        self
    }

    /// Add an AND greater than condition to the current OR branch
    pub fn and_where_gt(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        self.current_branch = self.current_branch.where_gt(column, value);
        self
    }

    /// Add an AND greater than or equal condition to the current OR branch
    pub fn and_where_gte(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        self.current_branch = self.current_branch.where_gte(column, value);
        self
    }

    /// Add an AND less than condition to the current OR branch
    pub fn and_where_lt(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        self.current_branch = self.current_branch.where_lt(column, value);
        self
    }

    /// Add an AND less than or equal condition to the current OR branch
    pub fn and_where_lte(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        self.current_branch = self.current_branch.where_lte(column, value);
        self
    }

    /// Add an AND LIKE condition to the current OR branch
    pub fn and_where_like(mut self, column: impl crate::columns::IntoColumnName, pattern: &str) -> Self {
        self.current_branch = self.current_branch.where_like(column, pattern);
        self
    }

    /// Add an AND NOT LIKE condition to the current OR branch
    pub fn and_where_not_like(mut self, column: impl crate::columns::IntoColumnName, pattern: &str) -> Self {
        self.current_branch = self.current_branch.where_not_like(column, pattern);
        self
    }

    /// Add an AND IN condition to the current OR branch
    pub fn and_where_in<V: Into<serde_json::Value>>(mut self, column: impl crate::columns::IntoColumnName, values: Vec<V>) -> Self {
        self.current_branch = self.current_branch.where_in(column, values);
        self
    }
    
    /// Add an AND NOT IN condition to the current OR branch
    pub fn and_where_not_in<V: Into<serde_json::Value>>(mut self, column: impl crate::columns::IntoColumnName, values: Vec<V>) -> Self {
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
    pub fn order_by(mut self, column: impl crate::columns::IntoColumnName, direction: Order) -> Self {
        self.order_by.push((column.column_name().to_string(), direction));
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
            let cols: Vec<String> = self.partition_by.iter()
                .map(|c| format!("\"{}\"", c))
                .collect();
            clauses.push(format!("PARTITION BY {}", cols.join(", ")));
        }
        
        if !self.order_by.is_empty() {
            let orders: Vec<String> = self.order_by.iter()
                .map(|(col, dir)| format!("\"{}\" {}", col, dir.as_str()))
                .collect();
            clauses.push(format!("ORDER BY {}", orders.join(", ")));
        }
        
        if let (Some(frame_type), Some(start)) = (&self.frame_type, &self.frame_start) {
            let frame_sql = if let Some(end) = &self.frame_end {
                format!("{} BETWEEN {} AND {}", frame_type.as_sql(), start.as_sql(), end.as_sql())
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
            let col_list: Vec<String> = cols.iter()
                .map(|c| format!("\"{}\"", c))
                .collect();
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
        order.into_iter()
            .filter_map(|k| groups.remove(&k))
            .collect()
    }
    
    /// Consolidate two-way join results with optional second item: `Vec<(A, Option<B>)>` -> `Vec<(A, Vec<B>)>`
    ///
    /// Handles LEFT JOIN results where B might be NULL.
    pub fn consolidate_two_optional<A, B, K, F>(items: Vec<(A, Option<B>)>, key_fn: F) -> Vec<(A, Vec<B>)>
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
        
        order.into_iter()
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
        a_order.into_iter()
            .filter_map(|ka| {
                a_groups.remove(&ka).map(|(a, mut b_groups, b_order)| {
                    let bs: Vec<(B, Vec<C>)> = b_order.into_iter()
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
        
        a_order.into_iter()
            .filter_map(|ka| {
                a_groups.remove(&ka).map(|(a, mut b_groups, b_order)| {
                    let bs: Vec<(B, Vec<C>)> = b_order.into_iter()
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
        self.having_conditions.extend(fragment.having_conditions.clone());
        
        // Merge joins
        self.joins.extend(fragment.joins.clone());
        
        // Apply soft delete flags (OR logic - if either wants them included)
        if fragment.include_trashed {
            self.include_trashed = true;
        }
        if fragment.only_trashed {
            self.only_trashed = true;
        }
        
        self
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
    pub fn where_eq(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
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
    pub fn where_not(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
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
    pub fn where_gt(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
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
    pub fn where_gte(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
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
    pub fn where_lt(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
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
    pub fn where_lte(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
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
    pub fn where_like(mut self, column: impl crate::columns::IntoColumnName, pattern: &str) -> Self {
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
    pub fn where_not_like(mut self, column: impl crate::columns::IntoColumnName, pattern: &str) -> Self {
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
    pub fn where_in<V: Into<serde_json::Value>>(mut self, column: impl crate::columns::IntoColumnName, values: Vec<V>) -> Self {
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
    pub fn where_not_in<V: Into<serde_json::Value>>(mut self, column: impl crate::columns::IntoColumnName, values: Vec<V>) -> Self {
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
    pub fn or_where_eq(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
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
    pub fn or_where_not(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
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
    pub fn or_where_gt(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
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
    pub fn or_where_gte(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
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
    pub fn or_where_lt(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
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
    pub fn or_where_lte(mut self, column: impl crate::columns::IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
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
    pub fn or_where_like(mut self, column: impl crate::columns::IntoColumnName, pattern: &str) -> Self {
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
    pub fn or_where_in<V: Into<serde_json::Value>>(mut self, column: impl crate::columns::IntoColumnName, values: Vec<V>) -> Self {
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
    pub fn or_where_not_in<V: Into<serde_json::Value>>(mut self, column: impl crate::columns::IntoColumnName, values: Vec<V>) -> Self {
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
    pub fn begin_or_where_eq(self, column: &str, value: impl Into<serde_json::Value>) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_eq(column, value)
    }
    
    /// Start a fluent OR expression with an initial greater than condition
    pub fn begin_or_where_gt(self, column: &str, value: impl Into<serde_json::Value>) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_gt(column, value)
    }
    
    /// Start a fluent OR expression with an initial greater than or equal condition
    pub fn begin_or_where_gte(self, column: &str, value: impl Into<serde_json::Value>) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_gte(column, value)
    }
    
    /// Start a fluent OR expression with an initial less than condition
    pub fn begin_or_where_lt(self, column: &str, value: impl Into<serde_json::Value>) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_lt(column, value)
    }
    
    /// Start a fluent OR expression with an initial less than or equal condition
    pub fn begin_or_where_lte(self, column: &str, value: impl Into<serde_json::Value>) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_lte(column, value)
    }
    
    /// Start a fluent OR expression with an initial LIKE condition
    pub fn begin_or_where_like(self, column: &str, pattern: &str) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self).or_where_like(column, pattern)
    }
    
    /// Start a fluent OR expression with an initial IN condition
    pub fn begin_or_where_in<V: Into<serde_json::Value>>(self, column: &str, values: Vec<V>) -> OrBranchBuilder<M> {
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
    pub fn where_not_in_subquery<N: Model>(mut self, column: &str, subquery: QueryBuilder<N>) -> Self {
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
            related_table, foreign_key,
            table, local_key,
            related_table, condition_column, value_sql
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
            related_table, foreign_key,
            table, local_key,
            related_table, condition_column, value_sql
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
            related_table,
            related_table, foreign_key,
            table, local_key
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
            related_table,
            related_table, foreign_key,
            table, local_key
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
    /// ⚠️ **Warning**: Raw SQL is not escaped. Only use with trusted input.
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
        self.raw_select_expressions.push(format!("({}) AS \"{}\"", subquery_sql, alias));
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
    ///     .order_by(User::columns.name, Order::Asc)  // Works with typed columns too
    /// ```
    pub fn order_by(mut self, column: impl crate::columns::IntoColumnName, direction: Order) -> Self {
        self.order_by.push((column.column_name().to_string(), direction));
        self
    }
    
    /// Order by ascending
    pub fn order_asc(self, column: impl crate::columns::IntoColumnName) -> Self {
        self.order_by(column, Order::Asc)
    }
    
    /// Order by descending
    pub fn order_desc(self, column: impl crate::columns::IntoColumnName) -> Self {
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
    
    /// Select columns from this table and also from a linked/joined table 
    ///
    /// This is useful for partial model queries where you want to include
    /// data from related tables without loading the full models.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Select cake fields and also bakery name through a link
    /// let results: Vec<(i64, String, Option<String>)> = Cake::query()
    ///     .select_with_linked(
    ///         vec!["id", "name"],           // Local columns
    ///         "bakeries",                    // Linked table
    ///         "bakery_id",                   // Local FK
    ///         "id",                          // Remote PK
    ///         vec!["name as bakery_name"]    // Remote columns
    ///     )
    ///     .get_raw()
    ///     .await?;
    /// ```
    pub fn select_with_linked(
        mut self,
        local_columns: Vec<&str>,
        linked_table: &str,
        local_fk: &str,
        remote_pk: &str,
        linked_columns: Vec<&str>,
    ) -> Self {
        // Set local columns with table prefix
        let table_name = M::table_name();
        let mut all_columns: Vec<String> = local_columns
            .iter()
            .map(|c| format!("{}.{}", table_name, c))
            .collect();
        
        // Add linked columns with table prefix
        for col in linked_columns {
            all_columns.push(format!("{}.{}", linked_table, col));
        }
        
        self.select_columns = Some(all_columns);
        
        // Add the join
        self.joins.push(JoinClause {
            join_type: JoinType::Left,
            table: linked_table.to_string(),
            alias: None,
            left_column: format!("{}.{}", table_name, local_fk),
            right_column: format!("{}.{}", linked_table, remote_pk),
        });
        
        self
    }
    
    /// Select all columns from this table plus specific columns from a linked table 
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Get all user fields plus their profile picture
    /// let results = User::query()
    ///     .select_also_linked(
    ///         "profiles",
    ///         "id",
    ///         "user_id", 
    ///         vec!["picture", "bio"]
    ///     )
    ///     .get_with_extra::<(String, String)>()
    ///     .await?;
    /// ```
    pub fn select_also_linked(
        mut self,
        linked_table: &str,
        local_pk: &str,
        remote_fk: &str,
        linked_columns: Vec<&str>,
    ) -> Self {
        let table_name = M::table_name();
        
        // Start with all local columns
        let local_cols: Vec<String> = M::column_names()
            .iter()
            .map(|c| format!("{}.{}", table_name, c))
            .collect();
        
        // Add linked columns
        let mut all_columns = local_cols;
        for col in linked_columns {
            all_columns.push(format!("{}.{}", linked_table, col));
        }
        
        self.select_columns = Some(all_columns);
        
        // Add the join
        self.joins.push(JoinClause {
            join_type: JoinType::Left,
            table: linked_table.to_string(),
            alias: None,
            left_column: format!("{}.{}", table_name, local_pk),
            right_column: format!("{}.{}", linked_table, remote_fk),
        });
        
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
    pub fn group_by(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.group_by.push(column.column_name().to_string());
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
    pub fn having_sum_gt(self, column: impl crate::columns::IntoColumnName, value: f64) -> Self {
        let db_type = crate::database::try_db()
            .map(|db| db.backend())
            .unwrap_or(DatabaseType::Postgres);
        let col = db_sql::quote_ident(db_type, column.column_name());
        self.having(&format!("SUM({}) > {}", col, value))
    }
    
    /// Add HAVING with AVG condition
    pub fn having_avg_gt(self, column: impl crate::columns::IntoColumnName, value: f64) -> Self {
        let db_type = crate::database::try_db()
            .map(|db| db.backend())
            .unwrap_or(DatabaseType::Postgres);
        let col = db_sql::quote_ident(db_type, column.column_name());
        self.having(&format!("AVG({}) > {}", col, value))
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
    pub async fn sum(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        let db_type = crate::database::try_db()
            .map(|db| db.backend())
            .unwrap_or(DatabaseType::Postgres);
        let col = db_sql::quote_ident(db_type, column.column_name());
        let expr = db_sql::cast_to_float(db_type, &format!("SUM({})", col));
        self.aggregate_f64(&expr, "sum_result").await
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
    pub async fn avg(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        let db_type = crate::database::try_db()
            .map(|db| db.backend())
            .unwrap_or(DatabaseType::Postgres);
        let col = db_sql::quote_ident(db_type, column.column_name());
        let expr = db_sql::cast_to_float(db_type, &format!("AVG({})", col));
        self.aggregate_f64(&expr, "avg_result").await
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
    pub async fn min(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        let db_type = crate::database::try_db()
            .map(|db| db.backend())
            .unwrap_or(DatabaseType::Postgres);
        let col = db_sql::quote_ident(db_type, column.column_name());
        let expr = db_sql::cast_to_float(db_type, &format!("MIN({})", col));
        self.aggregate_f64(&expr, "min_result").await
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
    pub async fn max(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        let db_type = crate::database::try_db()
            .map(|db| db.backend())
            .unwrap_or(DatabaseType::Postgres);
        let col = db_sql::quote_ident(db_type, column.column_name());
        let expr = db_sql::cast_to_float(db_type, &format!("MAX({})", col));
        self.aggregate_f64(&expr, "max_result").await
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
    pub async fn count_distinct(self, column: impl crate::columns::IntoColumnName) -> Result<u64> {
        #[derive(Debug, FromQueryResult)]
        struct CountResult {
            count_result: i64,
        }
        
        let db_type = crate::database::try_db()
            .map(|db| db.backend())
            .unwrap_or(DatabaseType::Postgres);
        let col = db_sql::quote_ident(db_type, column.column_name());
        
        let conn = crate::database::require_db()?.__internal_connection();
        
        let mut select = M::Entity::find();
        
        // Apply WHERE conditions
        if !self.conditions.is_empty() || !self.or_groups.is_empty() || M::soft_delete_enabled() {
            let condition = self.build_sea_condition();
            select = select.filter(condition);
        }
        
        // Build COUNT(DISTINCT column) expression
        let count_expr = Expr::cust(format!("COUNT(DISTINCT {})", col));
        
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
        
        let conn = crate::database::require_db()?.__internal_connection();
        
        let mut select = M::Entity::find();
        
        // Apply WHERE conditions
        if !self.conditions.is_empty() || !self.or_groups.is_empty() || M::soft_delete_enabled() {
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
    // UNION OPERATIONS
    // =========================================================================
    
    /// Add a UNION with another query
    ///
    /// UNION combines the results of two queries and removes duplicates.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Get active users combined with admin users (no duplicates)
    /// let users = User::query()
    ///     .where_eq("active", true)
    ///     .union(
    ///         User::query().where_eq("role", "admin")
    ///     )
    ///     .get()
    ///     .await?;
    /// ```
    pub fn union<N: Model>(mut self, other: QueryBuilder<N>) -> Self {
        self.unions.push(UnionClause {
            union_type: UnionType::Union,
            query_sql: other.build_base_select_sql(),
        });
        self
    }
    
    /// Add a UNION ALL with another query
    ///
    /// UNION ALL combines all results including duplicates (faster than UNION).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Combine all orders from different status (including duplicates)
    /// let orders = Order::query()
    ///     .where_eq("status", "pending")
    ///     .union_all(
    ///         Order::query().where_eq("status", "processing")
    ///     )
    ///     .union_all(
    ///         Order::query().where_eq("status", "shipped")
    ///     )
    ///     .get()
    ///     .await?;
    /// ```
    pub fn union_all<N: Model>(mut self, other: QueryBuilder<N>) -> Self {
        self.unions.push(UnionClause {
            union_type: UnionType::UnionAll,
            query_sql: other.build_base_select_sql(),
        });
        self
    }
    
    /// Add a raw UNION query
    ///
    /// Use when you need to union with a complex SQL query.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let results = User::query()
    ///     .where_eq("active", true)
    ///     .union_raw("SELECT * FROM archived_users WHERE year = 2023")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn union_raw(mut self, sql: &str) -> Self {
        self.unions.push(UnionClause {
            union_type: UnionType::Union,
            query_sql: sql.to_string(),
        });
        self
    }
    
    /// Add a raw UNION ALL query
    pub fn union_all_raw(mut self, sql: &str) -> Self {
        self.unions.push(UnionClause {
            union_type: UnionType::UnionAll,
            query_sql: sql.to_string(),
        });
        self
    }
    
    // =========================================================================
    // WINDOW FUNCTIONS
    // =========================================================================
    
    /// Add a window function to the SELECT clause
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Add row numbers partitioned by category
    /// let products = Product::query()
    ///     .window(
    ///         WindowFunction::new(WindowFunctionType::RowNumber, "row_num")
    ///             .partition_by("category")
    ///             .order_by("price", Order::Desc)
    ///     )
    ///     .get_raw()
    ///     .await?;
    /// ```
    pub fn window(mut self, window_fn: WindowFunction) -> Self {
        self.window_functions.push(window_fn);
        self
    }
    
    /// Add ROW_NUMBER() window function
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Number rows within each category by price
    /// let products = Product::query()
    ///     .row_number("row_num", Some("category"), "price", Order::Desc)
    ///     .get_raw()
    ///     .await?;
    /// ```
    pub fn row_number(
        mut self,
        alias: &str,
        partition_by: Option<&str>,
        order_by: &str,
        order: Order,
    ) -> Self {
        let mut wf = WindowFunction::new(WindowFunctionType::RowNumber, alias)
            .order_by(order_by, order);
        if let Some(partition) = partition_by {
            wf = wf.partition_by(partition);
        }
        self.window_functions.push(wf);
        self
    }
    
    /// Add RANK() window function
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Rank employees by salary within department
    /// let employees = Employee::query()
    ///     .rank("salary_rank", Some("department_id"), "salary", Order::Desc)
    ///     .get_raw()
    ///     .await?;
    /// ```
    pub fn rank(
        mut self,
        alias: &str,
        partition_by: Option<&str>,
        order_by: &str,
        order: Order,
    ) -> Self {
        let mut wf = WindowFunction::new(WindowFunctionType::Rank, alias)
            .order_by(order_by, order);
        if let Some(partition) = partition_by {
            wf = wf.partition_by(partition);
        }
        self.window_functions.push(wf);
        self
    }
    
    /// Add DENSE_RANK() window function
    ///
    /// Similar to RANK() but without gaps in ranking values.
    pub fn dense_rank(
        mut self,
        alias: &str,
        partition_by: Option<&str>,
        order_by: &str,
        order: Order,
    ) -> Self {
        let mut wf = WindowFunction::new(WindowFunctionType::DenseRank, alias)
            .order_by(order_by, order);
        if let Some(partition) = partition_by {
            wf = wf.partition_by(partition);
        }
        self.window_functions.push(wf);
        self
    }
    
    /// Add LAG() window function
    ///
    /// Access data from a previous row.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Get previous order total for comparison
    /// let orders = Order::query()
    ///     .lag("previous_total", "total", 1, None, "user_id", "created_at", Order::Asc)
    ///     .get_raw()
    ///     .await?;
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn lag(
        mut self,
        alias: &str,
        column: &str,
        offset: i32,
        default: Option<&str>,
        partition_by: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        let wf = WindowFunction::new(
            WindowFunctionType::Lag(column.to_string(), Some(offset), default.map(|s| s.to_string())),
            alias,
        )
            .partition_by(partition_by)
            .order_by(order_by, order);
        self.window_functions.push(wf);
        self
    }
    
    /// Add LEAD() window function
    ///
    /// Access data from a following row.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Get next appointment date
    /// let appointments = Appointment::query()
    ///     .lead("next_date", "date", 1, None, "patient_id", "date", Order::Asc)
    ///     .get_raw()
    ///     .await?;
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn lead(
        mut self,
        alias: &str,
        column: &str,
        offset: i32,
        default: Option<&str>,
        partition_by: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        let wf = WindowFunction::new(
            WindowFunctionType::Lead(column.to_string(), Some(offset), default.map(|s| s.to_string())),
            alias,
        )
            .partition_by(partition_by)
            .order_by(order_by, order);
        self.window_functions.push(wf);
        self
    }
    
    /// Add running SUM() window function
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Calculate running total of sales
    /// let sales = Sale::query()
    ///     .running_sum("running_total", "amount", "date", Order::Asc)
    ///     .get_raw()
    ///     .await?;
    /// ```
    pub fn running_sum(
        mut self,
        alias: &str,
        column: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        let wf = WindowFunction::new(WindowFunctionType::Sum(column.to_string()), alias)
            .order_by(order_by, order)
            .frame(FrameType::Rows, FrameBound::UnboundedPreceding, FrameBound::CurrentRow);
        self.window_functions.push(wf);
        self
    }
    
    /// Add running AVG() window function
    pub fn running_avg(
        mut self,
        alias: &str,
        column: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        let wf = WindowFunction::new(WindowFunctionType::Avg(column.to_string()), alias)
            .order_by(order_by, order)
            .frame(FrameType::Rows, FrameBound::UnboundedPreceding, FrameBound::CurrentRow);
        self.window_functions.push(wf);
        self
    }
    
    /// Add NTILE() window function
    ///
    /// Distribute rows into specified number of groups.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Divide products into 4 price quartiles
    /// let products = Product::query()
    ///     .ntile("price_quartile", 4, "price", Order::Asc)
    ///     .get_raw()
    ///     .await?;
    /// ```
    pub fn ntile(
        mut self,
        alias: &str,
        buckets: u32,
        order_by: &str,
        order: Order,
    ) -> Self {
        let wf = WindowFunction::new(WindowFunctionType::Ntile(buckets), alias)
            .order_by(order_by, order);
        self.window_functions.push(wf);
        self
    }
    
    /// Add FIRST_VALUE() window function
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Get the first order date for each customer
    /// let orders = Order::query()
    ///     .first_value("first_order_date", "created_at", "customer_id", "created_at", Order::Asc)
    ///     .get_raw()
    ///     .await?;
    /// ```
    pub fn first_value(
        mut self,
        alias: &str,
        column: &str,
        partition_by: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        let wf = WindowFunction::new(WindowFunctionType::FirstValue(column.to_string()), alias)
            .partition_by(partition_by)
            .order_by(order_by, order);
        self.window_functions.push(wf);
        self
    }
    
    /// Add LAST_VALUE() window function
    pub fn last_value(
        mut self,
        alias: &str,
        column: &str,
        partition_by: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        let wf = WindowFunction::new(WindowFunctionType::LastValue(column.to_string()), alias)
            .partition_by(partition_by)
            .order_by(order_by, order)
            // Need to extend frame to see last value
            .frame(FrameType::Rows, FrameBound::UnboundedPreceding, FrameBound::UnboundedFollowing);
        self.window_functions.push(wf);
        self
    }
    
    // =========================================================================
    // COMMON TABLE EXPRESSIONS (CTEs)
    // =========================================================================
    
    /// Add a CTE (WITH clause) to the query
    ///
    /// CTEs allow you to define temporary named result sets that can be
    /// referenced within the main query.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Define a CTE for high-value orders
    /// let orders = Order::query()
    ///     .with_cte(CTE::new(
    ///         "high_value_orders",
    ///         "SELECT * FROM orders WHERE total > 1000".to_string()
    ///     ))
    ///     .where_raw("id IN (SELECT id FROM high_value_orders)")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn with_cte(mut self, cte: CTE) -> Self {
        self.ctes.push(cte);
        self
    }
    
    /// Add a CTE from another query builder
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Create CTE from a query builder
    /// let active_users_query = User::query()
    ///     .where_eq("active", true)
    ///     .select(vec!["id", "name", "email"]);
    ///
    /// let posts = Post::query()
    ///     .with_query("active_users", active_users_query)
    ///     .inner_join("active_users", "posts.user_id", "active_users.id")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn with_query<N: Model>(mut self, name: &str, query: QueryBuilder<N>) -> Self {
        self.ctes.push(CTE::new(name, query.build_base_select_sql()));
        self
    }
    
    /// Add a CTE with column aliases
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let stats = Sale::query()
    ///     .with_cte_columns(
    ///         "daily_stats",
    ///         vec!["sale_date", "total_sales", "order_count"],
    ///         "SELECT DATE(created_at), SUM(amount), COUNT(*) FROM sales GROUP BY DATE(created_at)"
    ///     )
    ///     .where_raw("date IN (SELECT sale_date FROM daily_stats WHERE total_sales > 10000)")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn with_cte_columns(mut self, name: &str, columns: Vec<&str>, sql: &str) -> Self {
        self.ctes.push(CTE::with_columns(name, columns, sql.to_string()));
        self
    }
    
    /// Add a recursive CTE
    ///
    /// Recursive CTEs are useful for hierarchical or tree-structured data.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Get all employees in a management hierarchy
    /// let employees = Employee::query()
    ///     .with_recursive_cte(
    ///         "employee_tree",
    ///         vec!["id", "name", "manager_id", "level"],
    ///         // Base case: top-level managers
    ///         "SELECT id, name, manager_id, 0 FROM employees WHERE manager_id IS NULL",
    ///         // Recursive case: employees under managers
    ///         "SELECT e.id, e.name, e.manager_id, t.level + 1 
    ///          FROM employees e 
    ///          INNER JOIN employee_tree t ON e.manager_id = t.id"
    ///     )
    ///     .where_raw("id IN (SELECT id FROM employee_tree)")
    ///     .get()
    ///     .await?;
    /// ```
    pub fn with_recursive_cte(
        mut self,
        name: &str,
        columns: Vec<&str>,
        base_case: &str,
        recursive_case: &str,
    ) -> Self {
        let full_sql = format!("{} UNION ALL {}", base_case, recursive_case);
        let cte = CTE::with_columns(name, columns, full_sql).recursive();
        self.ctes.push(cte);
        self
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
                    Value::String(Some(n.to_string()))
                }
            }
            serde_json::Value::String(s) => Value::String(Some(s.clone())),
            // For arrays and objects, serialize to string
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                Value::String(Some(value.to_string()))
            }
        }
    }
    
    /// Build SeaORM Condition from our WhereConditions
    fn build_sea_condition(&self) -> Condition {
        use sea_orm::sea_query::{Alias, SimpleExpr};
        
        // Get database type for database-specific SQL generation
        let db_type = crate::database::try_db()
            .map(|db| db.backend())
            .unwrap_or(DatabaseType::Postgres);
        
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
                // JSON operations - database specific
                Operator::JsonContains => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        let value_str = val.to_string();
                        Expr::cust(db_sql::json_contains(db_type, &where_cond.column, &value_str))
                    } else {
                        continue;
                    }
                }
                Operator::JsonContainedBy => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        let value_str = val.to_string();
                        Expr::cust(db_sql::json_contained_by(db_type, &where_cond.column, &value_str))
                    } else {
                        continue;
                    }
                }
                Operator::JsonKeyExists => {
                    if let ConditionValue::Single(serde_json::Value::String(key)) = &where_cond.value {
                        Expr::cust(db_sql::json_key_exists(db_type, &where_cond.column, key))
                    } else {
                        continue;
                    }
                }
                Operator::JsonKeyNotExists => {
                    if let ConditionValue::Single(serde_json::Value::String(key)) = &where_cond.value {
                        Expr::cust(db_sql::json_key_not_exists(db_type, &where_cond.column, key))
                    } else {
                        continue;
                    }
                }
                Operator::JsonPathExists => {
                    if let ConditionValue::Single(serde_json::Value::String(path)) = &where_cond.value {
                        Expr::cust(db_sql::json_path_exists(db_type, &where_cond.column, path))
                    } else {
                        continue;
                    }
                }
                Operator::JsonPathNotExists => {
                    if let ConditionValue::Single(serde_json::Value::String(path)) = &where_cond.value {
                        Expr::cust(db_sql::json_path_not_exists(db_type, &where_cond.column, path))
                    } else {
                        continue;
                    }
                }
                // Array operations - database specific
                Operator::ArrayContains => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        Expr::cust(db_sql::array_contains(db_type, &where_cond.column, &array_vals))
                    } else {
                        continue;
                    }
                }
                Operator::ArrayContainedBy => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        Expr::cust(db_sql::array_contained_by(db_type, &where_cond.column, &array_vals))
                    } else {
                        continue;
                    }
                }
                Operator::ArrayOverlaps | Operator::ArrayContainsAny => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        Expr::cust(db_sql::array_overlaps(db_type, &where_cond.column, &array_vals))
                    } else {
                        continue;
                    }
                }
                Operator::ArrayContainsAll => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        // For "contains all", we use array_contains which checks all values exist
                        Expr::cust(db_sql::array_contains(db_type, &where_cond.column, &array_vals))
                    } else {
                        continue;
                    }
                }
                // Subquery operations
                Operator::SubqueryIn => {
                    if let ConditionValue::Subquery(subquery_sql) = &where_cond.value {
                        let col_quoted = db_sql::quote_ident(db_type, &where_cond.column);
                        Expr::cust(format!("{} IN ({})", col_quoted, subquery_sql))
                    } else {
                        continue;
                    }
                }
                Operator::SubqueryNotIn => {
                    if let ConditionValue::Subquery(subquery_sql) = &where_cond.value {
                        let col_quoted = db_sql::quote_ident(db_type, &where_cond.column);
                        Expr::cust(format!("{} NOT IN ({})", col_quoted, subquery_sql))
                    } else {
                        continue;
                    }
                }
                Operator::Raw => {
                    if let ConditionValue::RawExpr(raw_sql) = &where_cond.value {
                        if where_cond.column.is_empty() {
                            // Pure raw condition (like EXISTS, raw WHERE)
                            Expr::cust(raw_sql.clone())
                        } else {
                            // Column with raw expression
                            let col_quoted = db_sql::quote_ident(db_type, &where_cond.column);
                            Expr::cust(format!("{} {}", col_quoted, raw_sql))
                        }
                    } else {
                        continue;
                    }
                }
                // = ANY(array) optimization for PostgreSQL
                Operator::EqAny => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                serde_json::Value::Null => "NULL".to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        let col_quoted = db_sql::quote_ident(db_type, &where_cond.column);
                        Expr::cust(db_sql::eq_any(db_type, &col_quoted, &array_vals))
                    } else {
                        continue;
                    }
                }
                // <> ALL(array) optimization for PostgreSQL  
                Operator::NeAll => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                serde_json::Value::Null => "NULL".to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        let col_quoted = db_sql::quote_ident(db_type, &where_cond.column);
                        Expr::cust(db_sql::ne_all(db_type, &col_quoted, &array_vals))
                    } else {
                        continue;
                    }
                }
            };
            
            condition = condition.add(expr);
        }
        
        // Process OR groups
        for or_group in &self.or_groups {
            let or_condition = self.build_or_group_condition(or_group, db_type);
            condition = condition.add(or_condition);
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
    
    /// Build a SeaORM Condition from an OrGroup
    #[allow(clippy::only_used_in_recursion)]
    fn build_or_group_condition(&self, group: &OrGroup, db_type: DatabaseType) -> Condition {
        use sea_orm::sea_query::{Alias, SimpleExpr};
        
        let mut or_condition = match group.combine_with {
            LogicalOp::Or => Condition::any(),
            LogicalOp::And => Condition::all(),
        };
        
        // Add conditions from this group
        for where_cond in &group.conditions {
            let col = Expr::col(Alias::new(&where_cond.column));
            
            let expr: SimpleExpr = match &where_cond.operator {
                Operator::Eq => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        col.eq(Self::json_to_sea_value(val))
                    } else { continue; }
                }
                Operator::NotEq => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        col.ne(Self::json_to_sea_value(val))
                    } else { continue; }
                }
                Operator::Gt => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        col.gt(Self::json_to_sea_value(val))
                    } else { continue; }
                }
                Operator::Gte => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        col.gte(Self::json_to_sea_value(val))
                    } else { continue; }
                }
                Operator::Lt => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        col.lt(Self::json_to_sea_value(val))
                    } else { continue; }
                }
                Operator::Lte => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        col.lte(Self::json_to_sea_value(val))
                    } else { continue; }
                }
                Operator::Like => {
                    if let ConditionValue::Single(serde_json::Value::String(pattern)) = &where_cond.value {
                        col.like(pattern.as_str())
                    } else { continue; }
                }
                Operator::NotLike => {
                    if let ConditionValue::Single(serde_json::Value::String(pattern)) = &where_cond.value {
                        col.not_like(pattern.as_str())
                    } else { continue; }
                }
                Operator::In => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        let sea_values: Vec<_> = values.iter().map(Self::json_to_sea_value).collect();
                        col.is_in(sea_values)
                    } else { continue; }
                }
                Operator::NotIn => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        let sea_values: Vec<_> = values.iter().map(Self::json_to_sea_value).collect();
                        col.is_not_in(sea_values)
                    } else { continue; }
                }
                Operator::IsNull => col.is_null(),
                Operator::IsNotNull => col.is_not_null(),
                Operator::Between => {
                    if let ConditionValue::Range(low, high) = &where_cond.value {
                        col.between(Self::json_to_sea_value(low), Self::json_to_sea_value(high))
                    } else { continue; }
                }
                Operator::Raw => {
                    if let ConditionValue::RawExpr(raw_sql) = &where_cond.value {
                        if where_cond.column.is_empty() {
                            Expr::cust(raw_sql.clone())
                        } else {
                            let col_quoted = db_sql::quote_ident(db_type, &where_cond.column);
                            Expr::cust(format!("{} {}", col_quoted, raw_sql))
                        }
                    } else { continue; }
                }
                // JSON operations
                Operator::JsonContains => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        Expr::cust(db_sql::json_contains(db_type, &where_cond.column, &val.to_string()))
                    } else { continue; }
                }
                Operator::JsonContainedBy => {
                    if let ConditionValue::Single(val) = &where_cond.value {
                        Expr::cust(db_sql::json_contained_by(db_type, &where_cond.column, &val.to_string()))
                    } else { continue; }
                }
                Operator::JsonKeyExists => {
                    if let ConditionValue::Single(serde_json::Value::String(key)) = &where_cond.value {
                        Expr::cust(db_sql::json_key_exists(db_type, &where_cond.column, key))
                    } else { continue; }
                }
                Operator::JsonKeyNotExists => {
                    if let ConditionValue::Single(serde_json::Value::String(key)) = &where_cond.value {
                        Expr::cust(db_sql::json_key_not_exists(db_type, &where_cond.column, key))
                    } else { continue; }
                }
                Operator::JsonPathExists => {
                    if let ConditionValue::Single(serde_json::Value::String(path)) = &where_cond.value {
                        Expr::cust(db_sql::json_path_exists(db_type, &where_cond.column, path))
                    } else { continue; }
                }
                Operator::JsonPathNotExists => {
                    if let ConditionValue::Single(serde_json::Value::String(path)) = &where_cond.value {
                        Expr::cust(db_sql::json_path_not_exists(db_type, &where_cond.column, path))
                    } else { continue; }
                }
                // Array operations
                Operator::ArrayContains | Operator::ArrayContainsAll => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        Expr::cust(db_sql::array_contains(db_type, &where_cond.column, &array_vals))
                    } else { continue; }
                }
                Operator::ArrayContainedBy => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        Expr::cust(db_sql::array_contained_by(db_type, &where_cond.column, &array_vals))
                    } else { continue; }
                }
                Operator::ArrayOverlaps | Operator::ArrayContainsAny => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        Expr::cust(db_sql::array_overlaps(db_type, &where_cond.column, &array_vals))
                    } else { continue; }
                }
                // Subquery operations
                Operator::SubqueryIn => {
                    if let ConditionValue::Subquery(subquery_sql) = &where_cond.value {
                        let col_quoted = db_sql::quote_ident(db_type, &where_cond.column);
                        Expr::cust(format!("{} IN ({})", col_quoted, subquery_sql))
                    } else { continue; }
                }
                Operator::SubqueryNotIn => {
                    if let ConditionValue::Subquery(subquery_sql) = &where_cond.value {
                        let col_quoted = db_sql::quote_ident(db_type, &where_cond.column);
                        Expr::cust(format!("{} NOT IN ({})", col_quoted, subquery_sql))
                    } else { continue; }
                }
                // PostgreSQL optimizations
                Operator::EqAny => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                serde_json::Value::Null => "NULL".to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        let col_quoted = db_sql::quote_ident(db_type, &where_cond.column);
                        Expr::cust(db_sql::eq_any(db_type, &col_quoted, &array_vals))
                    } else { continue; }
                }
                Operator::NeAll => {
                    if let ConditionValue::List(values) = &where_cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                serde_json::Value::Null => "NULL".to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        let col_quoted = db_sql::quote_ident(db_type, &where_cond.column);
                        Expr::cust(db_sql::ne_all(db_type, &col_quoted, &array_vals))
                    } else { continue; }
                }
            };
            
            or_condition = or_condition.add(expr);
        }
        
        // Recursively process nested groups
        for nested_group in &group.nested_groups {
            let nested_condition = self.build_or_group_condition(nested_group, db_type);
            or_condition = or_condition.add(nested_condition);
        }
        
        or_condition
    }
    
    /// Log the query if logging is enabled
    #[allow(dead_code)]
    fn log_query(&self, sql: &str) {
        // Check if query logging is enabled via environment variable
        if std::env::var("TIDE_LOG_QUERIES")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(false)
        {
            tide_debug!("Query: {}", sql);
        }
        
        // Also log via QueryLogger if enabled
        if crate::logging::QueryLogger::is_enabled() {
            let entry = crate::logging::QueryLogEntry::new(sql)
                .with_table(M::table_name());
            crate::logging::QueryLogger::log(entry);
        }
    }
    
    /// Get debug information for this query without executing it
    ///
    /// Returns detailed information about the query including table, conditions,
    /// ordering, and the generated SQL.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let debug = User::query()
    ///     .where_eq("active", true)
    ///     .where_gt("age", 18)
    ///     .order_by("created_at", Order::Desc)
    ///     .debug();
    ///
    /// println!("{}", debug);
    /// // Output shows table, conditions, SQL preview, etc.
    /// ```
    pub fn debug(&self) -> crate::logging::QueryDebugInfo {
        use crate::logging::QueryDebugInfo;
        
        let mut info = QueryDebugInfo::new(M::table_name());
        
        // Add conditions
        for condition in &self.conditions {
            let op_str = match &condition.operator {
                Operator::Eq => "=",
                Operator::NotEq => "!=",
                Operator::Gt => ">",
                Operator::Gte => ">=",
                Operator::Lt => "<",
                Operator::Lte => "<=",
                Operator::Like => "LIKE",
                Operator::NotLike => "NOT LIKE",
                Operator::In => "IN",
                Operator::NotIn => "NOT IN",
                Operator::IsNull => "IS NULL",
                Operator::IsNotNull => "IS NOT NULL",
                Operator::Between => "BETWEEN",
                Operator::JsonContains => "@>",
                Operator::JsonContainedBy => "<@",
                Operator::JsonKeyExists => "?",
                Operator::JsonKeyNotExists => "?!",
                Operator::JsonPathExists => "@?",
                Operator::JsonPathNotExists => "NOT @?",
                Operator::ArrayContains => "@>",
                Operator::ArrayContainedBy => "<@",
                Operator::ArrayOverlaps => "&&",
                Operator::ArrayContainsAny => "&& ANY",
                Operator::ArrayContainsAll => "&& ALL",
                Operator::SubqueryIn => "IN (subquery)",
                Operator::SubqueryNotIn => "NOT IN (subquery)",
                Operator::Raw => "RAW",
                Operator::EqAny => "= ANY(array)",
                Operator::NeAll => "<> ALL(array)",
            };
            
            let value_str = match &condition.value {
                ConditionValue::Single(v) => format!("{:?}", v),
                ConditionValue::List(list) => format!("{:?}", list),
                ConditionValue::Range(start, end) => format!("{:?}..{:?}", start, end),
                ConditionValue::None => "NULL".to_string(),
                ConditionValue::Subquery(sub) => format!("({})", sub),
                ConditionValue::RawExpr(expr) => expr.clone(),
            };
            
            info.add_condition(format!("{} {} {}", condition.column, op_str, value_str));
        }
        
        // Add order by
        for (column, order) in &self.order_by {
            info.add_order_by(format!("{} {}", column, order.as_str()));
        }
        
        // Add group by
        info.group_by = self.group_by.clone();
        
        // Add limit/offset
        info.limit = self.limit_value;
        info.offset = self.offset_value;
        
        // Add select columns
        if let Some(ref cols) = self.select_columns {
            info.select = cols.clone();
        }
        
        // Add joins
        for join in &self.joins {
            info.joins.push(format!(
                "{:?} JOIN {} ON {} = {}",
                join.join_type,
                join.table,
                join.left_column,
                join.right_column
            ));
        }
        
        // Build SQL preview
        info.sql = self.build_sql_preview();
        
        info
    }
    
    /// Build a SQL preview string for debugging
    ///
    /// Returns a human-readable representation of the query that will be executed.
    /// This is useful for debugging and testing query construction.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let query = User::query()
    ///     .where_eq("active", true)
    ///     .begin_or()
    ///         .or_where_eq("role", "admin")
    ///         .or_where_eq("role", "moderator")
    ///     .end_or();
    ///
    /// println!("Query: {}", query.build_sql_preview());
    /// // Output: SELECT * FROM users WHERE active = true AND (role = 'admin' OR role = 'moderator')
    /// ```
    pub fn build_sql_preview(&self) -> String {
        let mut sql = String::new();
        
        // SELECT clause
        match &self.select_columns {
            Some(cols) if !cols.is_empty() => {
                sql.push_str("SELECT ");
                sql.push_str(&cols.join(", "));
                sql.push_str(" FROM ");
            }
            _ => {
                sql.push_str("SELECT * FROM ");
            }
        }
        
        sql.push_str(M::table_name());
        
        // JOINs
        for join in &self.joins {
            sql.push_str(&format!(
                " {:?} JOIN {} ON {} = {}",
                join.join_type,
                join.table,
                join.left_column,
                join.right_column
            ));
        }
        
        // WHERE
        if !self.conditions.is_empty() {
            sql.push_str(" WHERE ");
            let conditions: Vec<String> = self.conditions.iter()
                .map(|cond| {
                    let op_str = match &cond.operator {
                        Operator::Eq => "= ?",
                        Operator::NotEq => "!= ?",
                        Operator::Gt => "> ?",
                        Operator::Gte => ">= ?",
                        Operator::Lt => "< ?",
                        Operator::Lte => "<= ?",
                        Operator::Like | Operator::NotLike => "LIKE ?",
                        Operator::In | Operator::NotIn => "IN (?)",
                        Operator::IsNull => "IS NULL",
                        Operator::IsNotNull => "IS NOT NULL",
                        Operator::Between => "BETWEEN ? AND ?",
                        Operator::JsonContains | Operator::ArrayContains => "@> ?",
                        Operator::JsonContainedBy | Operator::ArrayContainedBy => "<@ ?",
                        Operator::JsonKeyExists => "? ?",
                        Operator::JsonKeyNotExists => "?! ?",
                        Operator::JsonPathExists => "@? ?",
                        Operator::JsonPathNotExists => "NOT @? ?",
                        Operator::ArrayOverlaps => "&& ?",
                        Operator::ArrayContainsAny => "&& ANY(?)",
                        Operator::ArrayContainsAll => "&& ALL(?)",
                        Operator::SubqueryIn => "IN (SELECT ...)",
                        Operator::SubqueryNotIn => "NOT IN (SELECT ...)",
                        Operator::Raw => "...",
                        Operator::EqAny => "= ANY(ARRAY[?])",
                        Operator::NeAll => "<> ALL(ARRAY[?])",
                    };
                    format!("{} {}", cond.column, op_str)
                })
                .collect();
            sql.push_str(&conditions.join(" AND "));
            
            // Add OR groups to preview
            if !self.or_groups.is_empty() {
                for or_group in &self.or_groups {
                    let or_preview = self.build_or_group_preview(or_group);
                    if !or_preview.is_empty() {
                        sql.push_str(&format!(" AND ({})", or_preview));
                    }
                }
            }
        } else if !self.or_groups.is_empty() {
            // Only OR groups, no regular conditions
            sql.push_str(" WHERE ");
            let or_previews: Vec<String> = self.or_groups.iter()
                .map(|g| format!("({})", self.build_or_group_preview(g)))
                .filter(|s| s != "()")
                .collect();
            sql.push_str(&or_previews.join(" AND "));
        }
        
        // GROUP BY
        if !self.group_by.is_empty() {
            sql.push_str(" GROUP BY ");
            sql.push_str(&self.group_by.join(", "));
        }
        
        // ORDER BY
        if !self.order_by.is_empty() {
            sql.push_str(" ORDER BY ");
            let orders: Vec<String> = self.order_by.iter()
                .map(|(col, ord)| format!("{} {}", col, ord.as_str()))
                .collect();
            sql.push_str(&orders.join(", "));
        }
        
        // LIMIT/OFFSET
        if let Some(limit) = self.limit_value {
            sql.push_str(&format!(" LIMIT {}", limit));
        }
        if let Some(offset) = self.offset_value {
            sql.push_str(&format!(" OFFSET {}", offset));
        }
        
        sql
    }
    
    /// Build a preview string for an OR group
    #[allow(clippy::only_used_in_recursion)]
    fn build_or_group_preview(&self, group: &OrGroup) -> String {
        let mut parts: Vec<String> = Vec::new();
        
        for cond in &group.conditions {
            let op_str = match &cond.operator {
                Operator::Eq => "= ?",
                Operator::NotEq => "!= ?",
                Operator::Gt => "> ?",
                Operator::Gte => ">= ?",
                Operator::Lt => "< ?",
                Operator::Lte => "<= ?",
                Operator::Like | Operator::NotLike => "LIKE ?",
                Operator::In | Operator::NotIn => "IN (?)",
                Operator::IsNull => "IS NULL",
                Operator::IsNotNull => "IS NOT NULL",
                Operator::Between => "BETWEEN ? AND ?",
                Operator::Raw => "...",
                _ => "?",
            };
            parts.push(format!("{} {}", cond.column, op_str));
        }
        
        // Include nested groups
        for nested in &group.nested_groups {
            let nested_preview = self.build_or_group_preview(nested);
            if !nested_preview.is_empty() {
                parts.push(format!("({})", nested_preview));
            }
        }
        
        let joiner = match group.combine_with {
            LogicalOp::Or => " OR ",
            LogicalOp::And => " AND ",
        };
        
        parts.join(joiner)
    }

    // =========================================================================
    // CACHING
    // =========================================================================
    
    /// Enable caching for this query with the specified TTL
    ///
    /// Results will be cached for the specified duration. Subsequent identical
    /// queries will return cached results instead of hitting the database.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::time::Duration;
    ///
    /// // Cache results for 5 minutes
    /// let users = User::query()
    ///     .where_eq("active", true)
    ///     .cache(Duration::from_secs(300))
    ///     .get()
    ///     .await?;
    /// ```
    pub fn cache(mut self, ttl: std::time::Duration) -> Self {
        self.cache_options = Some(crate::cache::CacheOptions::new(ttl));
        self
    }
    
    /// Enable caching with a custom cache key
    ///
    /// Use this when you want to control the cache key for easier invalidation.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Cache with a meaningful key
    /// let admins = User::query()
    ///     .where_eq("role", "admin")
    ///     .cache_with_key("admin_users", Duration::from_secs(600))
    ///     .get()
    ///     .await?;
    ///
    /// // Later, invalidate the cache
    /// QueryCache::global().invalidate("admin_users");
    /// ```
    pub fn cache_with_key(mut self, key: &str, ttl: std::time::Duration) -> Self {
        self.cache_key = Some(key.to_string());
        self.cache_options = Some(crate::cache::CacheOptions::new(ttl));
        self
    }
    
    /// Enable caching with custom options
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let options = CacheOptions::new(Duration::from_secs(300))
    ///     .with_key("active_users")
    ///     .with_tags(&["users", "active"]);
    ///
    /// let users = User::query()
    ///     .where_eq("active", true)
    ///     .cache_with_options(options)
    ///     .get()
    ///     .await?;
    /// ```
    pub fn cache_with_options(mut self, options: crate::cache::CacheOptions) -> Self {
        self.cache_options = Some(options);
        self
    }
    
    /// Disable caching for this query (even if global caching is enabled)
    pub fn no_cache(mut self) -> Self {
        self.cache_options = None;
        self.cache_key = None;
        self
    }
    
    /// Generate a cache key for this query
    fn generate_cache_key(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        // If custom key is set, use it
        if let Some(ref key) = self.cache_key {
            return key.clone();
        }
        
        // Generate key from query components
        let mut hasher = DefaultHasher::new();
        
        // Table name
        M::table_name().hash(&mut hasher);
        
        // Conditions
        for cond in &self.conditions {
            cond.column.hash(&mut hasher);
            format!("{:?}", cond.operator).hash(&mut hasher);
            format!("{:?}", cond.value).hash(&mut hasher);
        }
        
        // Order
        for (col, ord) in &self.order_by {
            col.hash(&mut hasher);
            ord.as_str().hash(&mut hasher);
        }
        
        // Limit/Offset
        self.limit_value.hash(&mut hasher);
        self.offset_value.hash(&mut hasher);
        
        // Joins
        for join in &self.joins {
            join.table.hash(&mut hasher);
        }
        
        // Group by
        for col in &self.group_by {
            col.hash(&mut hasher);
        }
        
        let hash = hasher.finish();
        crate::cache::QueryCache::global().generate_key(M::table_name(), hash)
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
        
        // Check cache first if caching is enabled
        let cache_key = if self.cache_options.is_some() {
            let key = self.generate_cache_key();
            if let Some(cached) = crate::cache::QueryCache::global().get::<Vec<M>>(&key) {
                return Ok(cached);
            }
            Some(key)
        } else {
            None
        };
        
        let conn = crate::database::require_db()?.__internal_connection();
        
        // If we have JOINs or GROUP BY, use raw SQL
        if !self.joins.is_empty() || !self.group_by.is_empty() {
            let results = self.clone().get_with_joins().await?;
            
            // Cache results if caching is enabled
            if let (Some(key), Some(options)) = (&cache_key, &self.cache_options) {
                let _ = crate::cache::QueryCache::global().set(
                    key,
                    &results,
                    Some(options.ttl),
                    M::table_name(),
                );
            }
            
            return Ok(results);
        }
        
        // Start with Entity::find()
        let mut select = M::Entity::find();
        
        // Apply WHERE conditions (including soft delete filter if applicable)
        // Always build conditions if there are explicit conditions, or_groups, or soft delete is enabled
        if !self.conditions.is_empty() || !self.or_groups.is_empty() || M::soft_delete_enabled() {
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
        
        let results: Vec<M> = results.into_iter().map(M::from_sea_model).collect();
        
        // Cache results if caching is enabled
        if let (Some(key), Some(options)) = (cache_key, &self.cache_options) {
            let _ = crate::cache::QueryCache::global().set(
                &key,
                &results,
                Some(options.ttl),
                M::table_name(),
            );
        }
        
        Ok(results)
    }
    
    /// Execute query with JOINs using raw SQL
    async fn get_with_joins(self) -> Result<Vec<M>> {
        let sql = self.build_select_sql();
        
        // Execute using raw SQL
        let results = crate::database::Database::raw::<M>(&sql).await?;
        Ok(results)
    }
    
    /// Build base SELECT SQL string without CTEs and UNIONs
    /// Used by CTEs and UNIONs to get the core query
    fn build_base_select_sql(&self) -> String {
        let table = M::table_name();
        let mut sql = String::new();
        
        // SELECT clause
        if !self.raw_select_expressions.is_empty() {
            // Use raw select expressions
            let mut exprs = self.raw_select_expressions.clone();
            // Add window functions
            for wf in &self.window_functions {
                exprs.push(wf.to_sql());
            }
            sql.push_str(&format!("SELECT {} ", exprs.join(", ")));
        } else if let Some(ref columns) = self.select_columns {
            let mut cols: Vec<String> = columns.iter()
                .map(|c| {
                    if c.contains('.') || c.contains('(') || c.contains('*') {
                        c.clone()
                    } else {
                        format!("\"{}\".\"{}\""  , table, c)
                    }
                })
                .collect();
            // Add window functions
            for wf in &self.window_functions {
                cols.push(wf.to_sql());
            }
            sql.push_str(&format!("SELECT {} ", cols.join(", ")));
        } else {
            let mut select_parts = vec![format!("\"{}\".*", table)];
            // Add window functions
            for wf in &self.window_functions {
                select_parts.push(wf.to_sql());
            }
            sql.push_str(&format!("SELECT {} ", select_parts.join(", ")));
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
        
        sql.trim().to_string()
    }
    
    /// Build SELECT SQL string with CTEs, JOINs, GROUP BY, HAVING, UNIONs
    fn build_select_sql(&self) -> String {
        let mut sql = String::new();
        
        // WITH clause (CTEs)
        if !self.ctes.is_empty() {
            let has_recursive = self.ctes.iter().any(|c| c.recursive);
            if has_recursive {
                sql.push_str("WITH RECURSIVE ");
            } else {
                sql.push_str("WITH ");
            }
            let cte_parts: Vec<String> = self.ctes.iter()
                .map(|c| c.to_sql())
                .collect();
            sql.push_str(&cte_parts.join(", "));
            sql.push(' ');
        }
        
        // Main query
        sql.push_str(&self.build_base_select_sql());
        
        // UNION clauses (applied before ORDER BY, LIMIT, OFFSET)
        for union in &self.unions {
            sql.push_str(&format!(" {} {}", union.union_type.as_sql(), union.query_sql));
        }
        
        // ORDER BY clause (applied to the entire UNION result)
        if !self.order_by.is_empty() {
            let order_parts: Vec<String> = self.order_by.iter()
                .map(|(col, dir)| format!("{} {}", self.format_column(col), dir.as_str()))
                .collect();
            sql.push_str(&format!(" ORDER BY {}", order_parts.join(", ")));
        }
        
        // LIMIT clause
        if let Some(limit) = self.limit_value {
            sql.push_str(&format!(" LIMIT {}", limit));
        }
        
        // OFFSET clause
        if let Some(offset) = self.offset_value {
            sql.push_str(&format!(" OFFSET {}", offset));
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
    
    /// Build WHERE clause SQL (database-specific)
    fn build_where_sql(&self) -> String {
        // Get database type for database-specific SQL generation
        let db_type = crate::database::try_db()
            .map(|db| db.backend())
            .unwrap_or(DatabaseType::Postgres);
        
        self.build_where_sql_for_db(db_type)
    }
    
    /// Build WHERE clause SQL for a specific database type
    fn build_where_sql_for_db(&self, db_type: DatabaseType) -> String {
        let mut conditions = Vec::new();
        
        for cond in &self.conditions {
            let col = db_sql::format_column(db_type, &cond.column);
            
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
                // JSON operations - database specific
                Operator::JsonContains => {
                    if let ConditionValue::Single(val) = &cond.value {
                        let value_str = val.to_string();
                        db_sql::json_contains(db_type, &cond.column, &value_str)
                    } else { continue; }
                }
                Operator::JsonContainedBy => {
                    if let ConditionValue::Single(val) = &cond.value {
                        let value_str = val.to_string();
                        db_sql::json_contained_by(db_type, &cond.column, &value_str)
                    } else { continue; }
                }
                Operator::JsonKeyExists => {
                    if let ConditionValue::Single(serde_json::Value::String(key)) = &cond.value {
                        db_sql::json_key_exists(db_type, &cond.column, key)
                    } else { continue; }
                }
                Operator::JsonKeyNotExists => {
                    if let ConditionValue::Single(serde_json::Value::String(key)) = &cond.value {
                        db_sql::json_key_not_exists(db_type, &cond.column, key)
                    } else { continue; }
                }
                Operator::JsonPathExists => {
                    if let ConditionValue::Single(serde_json::Value::String(path)) = &cond.value {
                        db_sql::json_path_exists(db_type, &cond.column, path)
                    } else { continue; }
                }
                Operator::JsonPathNotExists => {
                    if let ConditionValue::Single(serde_json::Value::String(path)) = &cond.value {
                        db_sql::json_path_not_exists(db_type, &cond.column, path)
                    } else { continue; }
                }
                // Array operations - database specific
                Operator::ArrayContains | Operator::ArrayContainsAll => {
                    if let ConditionValue::List(values) = &cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        db_sql::array_contains(db_type, &cond.column, &array_vals)
                    } else { continue; }
                }
                Operator::ArrayContainedBy => {
                    if let ConditionValue::List(values) = &cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        db_sql::array_contained_by(db_type, &cond.column, &array_vals)
                    } else { continue; }
                }
                Operator::ArrayOverlaps | Operator::ArrayContainsAny => {
                    if let ConditionValue::List(values) = &cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        db_sql::array_overlaps(db_type, &cond.column, &array_vals)
                    } else { continue; }
                }
                Operator::SubqueryIn => {
                    if let ConditionValue::Subquery(subquery_sql) = &cond.value {
                        format!("{} IN ({})", col, subquery_sql)
                    } else { continue; }
                }
                Operator::SubqueryNotIn => {
                    if let ConditionValue::Subquery(subquery_sql) = &cond.value {
                        format!("{} NOT IN ({})", col, subquery_sql)
                    } else { continue; }
                }
                Operator::Raw => {
                    if let ConditionValue::RawExpr(raw_sql) = &cond.value {
                        if cond.column.is_empty() {
                            // Pure raw condition (like EXISTS, raw WHERE)
                            raw_sql.clone()
                        } else {
                            // Column with raw expression
                            format!("{} {}", col, raw_sql)
                        }
                    } else { continue; }
                }
                // = ANY(array) optimization for PostgreSQL
                Operator::EqAny => {
                    if let ConditionValue::List(values) = &cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                serde_json::Value::Null => "NULL".to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        db_sql::eq_any(db_type, &col, &array_vals)
                    } else { continue; }
                }
                // <> ALL(array) optimization for PostgreSQL
                Operator::NeAll => {
                    if let ConditionValue::List(values) = &cond.value {
                        let array_vals: Vec<String> = values.iter()
                            .map(|v| match v {
                                serde_json::Value::String(s) => format!("'{}'", s.replace("'", "''")),
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                serde_json::Value::Null => "NULL".to_string(),
                                _ => v.to_string(),
                            })
                            .collect();
                        db_sql::ne_all(db_type, &col, &array_vals)
                    } else { continue; }
                }
            };
            
            conditions.push(expr);
        }
        
        // Add soft delete filter if applicable
        if M::soft_delete_enabled() {
            let deleted_col = db_sql::quote_ident(db_type, "deleted_at");
            if self.only_trashed {
                conditions.push(format!("{} IS NOT NULL", deleted_col));
            } else if !self.include_trashed {
                conditions.push(format!("{} IS NULL", deleted_col));
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
    
    /// Format an array literal for the current database
    #[allow(dead_code)]
    fn format_array_literal(&self, values: &[serde_json::Value]) -> String {
        let db_type = crate::database::try_db()
            .map(|db| db.backend())
            .unwrap_or(DatabaseType::Postgres);
        
        db_sql::format_array_literal(db_type, values)
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
        
        let conn = crate::database::require_db()?.__internal_connection();
        
        // If we have JOINs or GROUP BY, use raw SQL
        if !self.joins.is_empty() || !self.group_by.is_empty() {
            let results = self.limit(1).get_with_joins().await?;
            return Ok(results.into_iter().next());
        }
        
        // Start with Entity::find()
        let mut select = M::Entity::find();
        
        // Apply WHERE conditions (including soft delete filter if applicable)
        if !self.conditions.is_empty() || !self.or_groups.is_empty() || M::soft_delete_enabled() {
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
        
        let conn = crate::database::require_db()?.__internal_connection();
        
        // Start with Entity::find()
        let mut select = M::Entity::find();
        
        // Apply WHERE conditions (including soft delete filter if applicable)
        if !self.conditions.is_empty() || !self.or_groups.is_empty() || M::soft_delete_enabled() {
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
    /// Returns the number of deleted records.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Delete all inactive users
    /// let deleted = User::query()
    ///     .where_eq("status", "inactive")
    ///     .delete()
    ///     .await?;
    /// println!("Deleted {} records", deleted);
    ///
    /// // Delete with multiple conditions
    /// let deleted = User::query()
    ///     .where_eq("role", "guest")
    ///     .where_lt("last_login", thirty_days_ago)
    ///     .delete()
    ///     .await?;
    ///
    /// // Delete using subquery
    /// let deleted = Comment::query()
    ///     .where_in_subquery("post_id",
    ///         Post::query()
    ///             .select(vec!["id"])
    ///             .where_eq("deleted", true)
    ///     )
    ///     .delete()
    ///     .await?;
    /// ```
    pub async fn delete(self) -> Result<u64> {
        let conn = crate::database::require_db()?.__internal_connection();
        
        // Start with Entity::delete_many()
        let mut delete = M::Entity::delete_many();
        
        // Apply WHERE conditions (including soft delete filter if applicable)
        if !self.conditions.is_empty() || !self.or_groups.is_empty() || M::soft_delete_enabled() {
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
    
    /// Soft delete all matching records (set deleted_at timestamp)
    ///
    /// Only works on models with soft delete enabled.
    /// Returns the number of soft-deleted records.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Soft delete all expired sessions
    /// let deleted = Session::query()
    ///     .where_lt("expires_at", now)
    ///     .soft_delete()
    ///     .await?;
    ///
    /// // Later restore them with:
    /// // Session::query().only_trashed().where_eq("id", session_id).restore().await?;
    /// ```
    pub async fn soft_delete(self) -> Result<u64> {
        if !M::soft_delete_enabled() {
            return Err(Error::invalid_query(
                "soft_delete() can only be used on models with soft delete enabled".to_string()
            ));
        }
        
        // Build a raw UPDATE query to set deleted_at
        let table = M::table_name();
        let where_sql = self.build_where_sql();
        
        let sql = if where_sql.is_empty() {
            format!(
                "UPDATE \"{}\" SET \"deleted_at\" = NOW()",
                table
            )
        } else {
            format!(
                "UPDATE \"{}\" SET \"deleted_at\" = NOW() WHERE {}",
                table, where_sql
            )
        };
        
        // Execute raw SQL
        let conn = crate::database::require_db()?.__internal_connection();
        let stmt = Statement::from_string(
            DbBackend::Postgres,
            sql,
        );
        let result = conn
            .execute_raw(stmt)
            .await
            .map_err(translate_error)?;
        
        Ok(result.rows_affected())
    }
    
    /// Restore soft-deleted records (set deleted_at to NULL)
    ///
    /// Use with `.only_trashed()` to target soft-deleted records.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Restore all soft-deleted posts by a specific user
    /// let restored = Post::query()
    ///     .only_trashed()
    ///     .where_eq("user_id", user_id)
    ///     .restore()
    ///     .await?;
    /// ```
    pub async fn restore(self) -> Result<u64> {
        if !M::soft_delete_enabled() {
            return Err(Error::invalid_query(
                "restore() can only be used on models with soft delete enabled".to_string()
            ));
        }
        
        // Build a raw UPDATE query to clear deleted_at
        let table = M::table_name();
        let where_sql = self.build_where_sql();
        
        let sql = if where_sql.is_empty() {
            format!(
                "UPDATE \"{}\" SET \"deleted_at\" = NULL WHERE \"deleted_at\" IS NOT NULL",
                table
            )
        } else {
            format!(
                "UPDATE \"{}\" SET \"deleted_at\" = NULL WHERE {} AND \"deleted_at\" IS NOT NULL",
                table, where_sql
            )
        };
        
        // Execute raw SQL
        let conn = crate::database::require_db()?.__internal_connection();
        let stmt = Statement::from_string(
            DbBackend::Postgres,
            sql,
        );
        let result = conn
            .execute_raw(stmt)
            .await
            .map_err(translate_error)?;
        
        Ok(result.rows_affected())
    }
    
    /// Force delete records (bypass soft delete)
    ///
    /// Permanently deletes records even if soft delete is enabled.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Permanently delete very old soft-deleted records
    /// let deleted = User::query()
    ///     .only_trashed()
    ///     .where_lt("deleted_at", one_year_ago)
    ///     .force_delete()
    ///     .await?;
    /// ```
    pub async fn force_delete(self) -> Result<u64> {
        let table = M::table_name();
        let where_sql = self.build_where_sql();
        
        let sql = if where_sql.is_empty() {
            format!("DELETE FROM \"{}\"", table)
        } else {
            format!("DELETE FROM \"{}\" WHERE {}", table, where_sql)
        };
        
        // Execute raw SQL
        let conn = crate::database::require_db()?.__internal_connection();
        let stmt = Statement::from_string(
            DbBackend::Postgres,
            sql,
        );
        let result = conn
            .execute_raw(stmt)
            .await
            .map_err(translate_error)?;
        
        Ok(result.rows_affected())
    }
    
    /// Execute query with raw select expressions and return as JSON
    ///
    /// Use this when you have `select_raw()` expressions that don't map
    /// directly to the model structure.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Get aggregated data
    /// let results = Order::query()
    ///     .group_by("user_id")
    ///     .select_raw("user_id, SUM(total) as total_spent, COUNT(*) as order_count")
    ///     .get_json()
    ///     .await?;
    ///
    /// for row in results {
    ///     println!("User {}: ${}", row["user_id"], row["total_spent"]);
    /// }
    /// ```
    pub async fn get_json(self) -> Result<Vec<serde_json::Value>> {
        let sql = self.build_select_sql();
        crate::database::Database::raw_json(&sql).await
    }
}

impl<M: Model> Default for QueryBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::db_sql;
    use super::{
        Order, UnionType, UnionClause, FrameBound, FrameType,
        WindowFunction, WindowFunctionType, CTE,
    };
    use crate::config::DatabaseType;
    
    #[test]
    fn test_quote_char() {
        assert_eq!(db_sql::quote_char(DatabaseType::Postgres), '"');
        assert_eq!(db_sql::quote_char(DatabaseType::MySQL), '`');
        assert_eq!(db_sql::quote_char(DatabaseType::MariaDB), '`');
        assert_eq!(db_sql::quote_char(DatabaseType::SQLite), '"');
    }
    
    #[test]
    fn test_quote_ident() {
        assert_eq!(db_sql::quote_ident(DatabaseType::Postgres, "column"), "\"column\"");
        assert_eq!(db_sql::quote_ident(DatabaseType::MySQL, "column"), "`column`");
        assert_eq!(db_sql::quote_ident(DatabaseType::MariaDB, "column"), "`column`");
        assert_eq!(db_sql::quote_ident(DatabaseType::SQLite, "column"), "\"column\"");
    }
    
    #[test]
    fn test_json_contains_postgres() {
        let sql = db_sql::json_contains(DatabaseType::Postgres, "metadata", r#"{"key": "value"}"#);
        assert!(sql.contains("@>"));
        assert!(sql.contains("\"metadata\""));
    }
    
    #[test]
    fn test_json_contains_mysql() {
        let sql = db_sql::json_contains(DatabaseType::MySQL, "metadata", r#"{"key": "value"}"#);
        assert!(sql.contains("JSON_CONTAINS"));
        assert!(sql.contains("`metadata`"));

        let sql = db_sql::json_contains(DatabaseType::MariaDB, "metadata", r#"{"key": "value"}"#);
        assert!(sql.contains("JSON_CONTAINS"));
        assert!(sql.contains("`metadata`"));
    }
    
    #[test]
    fn test_json_contains_sqlite() {
        let sql = db_sql::json_contains(DatabaseType::SQLite, "metadata", "test_value");
        assert!(sql.contains("json_each"));
        assert!(sql.contains("\"metadata\""));
    }
    
    #[test]
    fn test_json_key_exists_postgres() {
        let sql = db_sql::json_key_exists(DatabaseType::Postgres, "data", "email");
        assert_eq!(sql, "\"data\" ? 'email'");
    }
    
    #[test]
    fn test_json_key_exists_mysql() {
        let sql = db_sql::json_key_exists(DatabaseType::MySQL, "data", "email");
        assert!(sql.contains("JSON_CONTAINS_PATH"));
        assert!(sql.contains("$.email"));

        let sql = db_sql::json_key_exists(DatabaseType::MariaDB, "data", "email");
        assert!(sql.contains("JSON_CONTAINS_PATH"));
        assert!(sql.contains("$.email"));
    }
    
    #[test]
    fn test_json_key_exists_sqlite() {
        let sql = db_sql::json_key_exists(DatabaseType::SQLite, "data", "email");
        assert!(sql.contains("json_extract"));
        assert!(sql.contains("$.email"));
        assert!(sql.contains("IS NOT NULL"));
    }
    
    #[test]
    fn test_json_path_exists_postgres() {
        let sql = db_sql::json_path_exists(DatabaseType::Postgres, "data", "$.user.name");
        assert!(sql.contains("@?"));
    }
    
    #[test]
    fn test_json_path_exists_mysql() {
        let sql = db_sql::json_path_exists(DatabaseType::MySQL, "data", "$.user.name");
        assert!(sql.contains("JSON_CONTAINS_PATH"));

        let sql = db_sql::json_path_exists(DatabaseType::MariaDB, "data", "$.user.name");
        assert!(sql.contains("JSON_CONTAINS_PATH"));
    }
    
    #[test]
    fn test_json_path_exists_sqlite() {
        let sql = db_sql::json_path_exists(DatabaseType::SQLite, "data", "$.user.name");
        assert!(sql.contains("json_extract"));
    }
    
    #[test]
    fn test_array_contains_postgres() {
        let values = vec!["'admin'".to_string(), "'user'".to_string()];
        let sql = db_sql::array_contains(DatabaseType::Postgres, "roles", &values);
        assert!(sql.contains("@>"));
        assert!(sql.contains("ARRAY["));
    }
    
    #[test]
    fn test_array_contains_mysql() {
        let values = vec!["'admin'".to_string(), "'user'".to_string()];
        let sql = db_sql::array_contains(DatabaseType::MySQL, "roles", &values);
        assert!(sql.contains("JSON_CONTAINS"));

        let sql = db_sql::array_contains(DatabaseType::MariaDB, "roles", &values);
        assert!(sql.contains("JSON_CONTAINS"));
    }
    
    #[test]
    fn test_array_contains_sqlite() {
        let values = vec!["'admin'".to_string(), "'user'".to_string()];
        let sql = db_sql::array_contains(DatabaseType::SQLite, "roles", &values);
        assert!(sql.contains("json_each"));
    }
    
    #[test]
    fn test_array_overlaps_postgres() {
        let values = vec!["'a'".to_string(), "'b'".to_string()];
        let sql = db_sql::array_overlaps(DatabaseType::Postgres, "tags", &values);
        assert!(sql.contains("&&"));
        assert!(sql.contains("ARRAY["));
    }
    
    #[test]
    fn test_array_overlaps_mysql() {
        let values = vec!["'a'".to_string(), "'b'".to_string()];
        let sql = db_sql::array_overlaps(DatabaseType::MySQL, "tags", &values);
        // MySQL uses OR conditions for overlaps
        assert!(sql.contains(" OR "));

        let sql = db_sql::array_overlaps(DatabaseType::MariaDB, "tags", &values);
        assert!(sql.contains(" OR "));
    }
    
    #[test]
    fn test_array_overlaps_sqlite() {
        let values = vec!["'a'".to_string(), "'b'".to_string()];
        let sql = db_sql::array_overlaps(DatabaseType::SQLite, "tags", &values);
        // SQLite uses OR conditions for overlaps
        assert!(sql.contains(" OR "));
    }
    
    #[test]
    fn test_format_column_simple() {
        assert_eq!(
            db_sql::format_column(DatabaseType::Postgres, "name"),
            "\"name\""
        );
        assert_eq!(
            db_sql::format_column(DatabaseType::MySQL, "name"),
            "`name`"
        );
        assert_eq!(
            db_sql::format_column(DatabaseType::MariaDB, "name"),
            "`name`"
        );
    }
    
    #[test]
    fn test_format_column_dotted() {
        assert_eq!(
            db_sql::format_column(DatabaseType::Postgres, "users.name"),
            "\"users\".\"name\""
        );
        assert_eq!(
            db_sql::format_column(DatabaseType::MySQL, "users.name"),
            "`users`.`name`"
        );
        assert_eq!(
            db_sql::format_column(DatabaseType::MariaDB, "users.name"),
            "`users`.`name`"
        );
    }
    
    #[test]
    fn test_format_column_expression() {
        // Expressions should be passed through unchanged
        assert_eq!(
            db_sql::format_column(DatabaseType::Postgres, "COUNT(*)"),
            "COUNT(*)"
        );
    }
    
    #[test]
    fn test_cast_to_float() {
        assert_eq!(
            db_sql::cast_to_float(DatabaseType::Postgres, "value"),
            "CAST(value AS FLOAT8)"
        );
        assert_eq!(
            db_sql::cast_to_float(DatabaseType::MySQL, "value"),
            "CAST(value AS DOUBLE)"
        );
        assert_eq!(
            db_sql::cast_to_float(DatabaseType::MariaDB, "value"),
            "CAST(value AS DOUBLE)"
        );
        assert_eq!(
            db_sql::cast_to_float(DatabaseType::SQLite, "value"),
            "CAST(value AS REAL)"
        );
    }
    
    #[test]
    fn test_sql_injection_prevention() {
        // Verify that single quotes are escaped
        let sql = db_sql::json_contains(DatabaseType::Postgres, "data", "O'Brien");
        assert!(sql.contains("O''Brien"));
        
        let sql = db_sql::json_key_exists(DatabaseType::MySQL, "data", "key'; DROP TABLE--");
        assert!(sql.contains("key''; DROP TABLE--"));

        let sql = db_sql::json_key_exists(DatabaseType::MariaDB, "data", "key'; DROP TABLE--");
        assert!(sql.contains("key''; DROP TABLE--"));
    }
    
    // =========================================================================
    // UNION TESTS
    // =========================================================================
    
    #[test]
    fn test_union_type_sql() {
        assert_eq!(UnionType::Union.as_sql(), "UNION");
        assert_eq!(UnionType::UnionAll.as_sql(), "UNION ALL");
    }
    
    #[test]
    fn test_union_clause_creation() {
        let clause = UnionClause {
            union_type: UnionType::Union,
            query_sql: "SELECT * FROM users WHERE active = true".to_string(),
        };
        assert_eq!(clause.union_type, UnionType::Union);
        assert!(clause.query_sql.contains("active = true"));
    }
    
    // =========================================================================
    // WINDOW FUNCTION TESTS
    // =========================================================================
    
    #[test]
    fn test_frame_bound_sql() {
        assert_eq!(FrameBound::UnboundedPreceding.as_sql(), "UNBOUNDED PRECEDING");
        assert_eq!(FrameBound::UnboundedFollowing.as_sql(), "UNBOUNDED FOLLOWING");
        assert_eq!(FrameBound::CurrentRow.as_sql(), "CURRENT ROW");
        assert_eq!(FrameBound::Preceding(5).as_sql(), "5 PRECEDING");
        assert_eq!(FrameBound::Following(3).as_sql(), "3 FOLLOWING");
    }
    
    #[test]
    fn test_frame_type_sql() {
        assert_eq!(FrameType::Rows.as_sql(), "ROWS");
        assert_eq!(FrameType::Range.as_sql(), "RANGE");
        assert_eq!(FrameType::Groups.as_sql(), "GROUPS");
    }
    
    #[test]
    fn test_window_function_type_row_number() {
        let wft = WindowFunctionType::RowNumber;
        assert_eq!(wft.as_sql(), "ROW_NUMBER()");
    }
    
    #[test]
    fn test_window_function_type_rank() {
        let wft = WindowFunctionType::Rank;
        assert_eq!(wft.as_sql(), "RANK()");
    }
    
    #[test]
    fn test_window_function_type_dense_rank() {
        let wft = WindowFunctionType::DenseRank;
        assert_eq!(wft.as_sql(), "DENSE_RANK()");
    }
    
    #[test]
    fn test_window_function_type_ntile() {
        let wft = WindowFunctionType::Ntile(4);
        assert_eq!(wft.as_sql(), "NTILE(4)");
    }
    
    #[test]
    fn test_window_function_type_lag() {
        let wft = WindowFunctionType::Lag("price".to_string(), Some(1), Some("0".to_string()));
        let sql = wft.as_sql();
        assert!(sql.contains("LAG"));
        assert!(sql.contains("\"price\""));
        assert!(sql.contains("1"));
    }
    
    #[test]
    fn test_window_function_type_lead() {
        let wft = WindowFunctionType::Lead("date".to_string(), Some(1), None);
        let sql = wft.as_sql();
        assert!(sql.contains("LEAD"));
        assert!(sql.contains("\"date\""));
    }
    
    #[test]
    fn test_window_function_type_first_value() {
        let wft = WindowFunctionType::FirstValue("amount".to_string());
        assert_eq!(wft.as_sql(), "FIRST_VALUE(\"amount\")");
    }
    
    #[test]
    fn test_window_function_type_last_value() {
        let wft = WindowFunctionType::LastValue("total".to_string());
        assert_eq!(wft.as_sql(), "LAST_VALUE(\"total\")");
    }
    
    #[test]
    fn test_window_function_type_sum() {
        let wft = WindowFunctionType::Sum("amount".to_string());
        assert_eq!(wft.as_sql(), "SUM(\"amount\")");
    }
    
    #[test]
    fn test_window_function_type_count() {
        let wft1 = WindowFunctionType::Count(None);
        assert_eq!(wft1.as_sql(), "COUNT(*)");
        
        let wft2 = WindowFunctionType::Count(Some("id".to_string()));
        assert_eq!(wft2.as_sql(), "COUNT(\"id\")");
    }
    
    #[test]
    fn test_window_function_basic() {
        let wf = WindowFunction::new(WindowFunctionType::RowNumber, "row_num");
        let sql = wf.to_sql();
        assert!(sql.contains("ROW_NUMBER()"));
        assert!(sql.contains("OVER"));
        assert!(sql.contains("AS \"row_num\""));
    }
    
    #[test]
    fn test_window_function_with_partition() {
        let wf = WindowFunction::new(WindowFunctionType::RowNumber, "row_num")
            .partition_by("category");
        let sql = wf.to_sql();
        assert!(sql.contains("PARTITION BY \"category\""));
    }
    
    #[test]
    fn test_window_function_with_order() {
        let wf = WindowFunction::new(WindowFunctionType::Rank, "rank")
            .order_by("score", Order::Desc);
        let sql = wf.to_sql();
        assert!(sql.contains("ORDER BY \"score\" DESC"));
    }
    
    #[test]
    fn test_window_function_with_frame() {
        let wf = WindowFunction::new(WindowFunctionType::Sum("amount".to_string()), "running_total")
            .order_by("date", Order::Asc)
            .frame(FrameType::Rows, FrameBound::UnboundedPreceding, FrameBound::CurrentRow);
        let sql = wf.to_sql();
        assert!(sql.contains("ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW"));
    }
    
    #[test]
    fn test_window_function_full() {
        let wf = WindowFunction::new(WindowFunctionType::Sum("sales".to_string()), "total_sales")
            .partition_by("region")
            .order_by("month", Order::Asc)
            .frame(FrameType::Range, FrameBound::UnboundedPreceding, FrameBound::CurrentRow);
        let sql = wf.to_sql();
        assert!(sql.contains("SUM(\"sales\")"));
        assert!(sql.contains("PARTITION BY \"region\""));
        assert!(sql.contains("ORDER BY \"month\" ASC"));
        assert!(sql.contains("RANGE BETWEEN"));
        assert!(sql.contains("AS \"total_sales\""));
    }
    
    // =========================================================================
    // CTE TESTS
    // =========================================================================
    
    #[test]
    fn test_cte_basic() {
        let cte = CTE::new("active_users", "SELECT * FROM users WHERE active = true".to_string());
        let sql = cte.to_sql();
        assert!(sql.contains("\"active_users\""));
        assert!(sql.contains("AS ("));
        assert!(sql.contains("active = true"));
    }
    
    #[test]
    fn test_cte_with_columns() {
        let cte = CTE::with_columns(
            "user_stats",
            vec!["user_id", "total", "count"],
            "SELECT user_id, SUM(amount), COUNT(*) FROM orders GROUP BY user_id".to_string()
        );
        let sql = cte.to_sql();
        assert!(sql.contains("\"user_stats\""));
        assert!(sql.contains("(\"user_id\", \"total\", \"count\")"));
        assert!(sql.contains("GROUP BY"));
    }
    
    #[test]
    fn test_cte_recursive() {
        let cte = CTE::new(
            "tree",
            "SELECT 1 UNION ALL SELECT 2".to_string()
        ).recursive();
        assert!(cte.recursive);
    }
    
    #[test]
    fn test_cte_name_quoting() {
        let cte = CTE::new("my_cte", "SELECT 1".to_string());
        let sql = cte.to_sql();
        assert!(sql.starts_with("\"my_cte\""));
    }
}
