//! Strongly-Typed Columns
//!
//! This module provides compile-time type safety for column operations.
//! Instead of using string column names that can have runtime errors,
//! typed columns catch type mismatches at compile time.
//!
//! ## Example
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//!
//! // Define typed columns for a model
//! mod user_columns {
//!     use tideorm::columns::*;
//!     
//!     pub struct UserColumns {
//!         pub id: Column<i64>,
//!         pub name: Column<String>,
//!         pub email: Column<String>,
//!         pub age: Column<Option<i32>>,
//!         pub active: Column<bool>,
//!     }
//!     
//!     pub const COLUMNS: UserColumns = UserColumns {
//!         id: Column::new("id"),
//!         name: Column::new("name"),
//!         email: Column::new("email"),
//!         age: Column::new("age"),
//!         active: Column::new("active"),
//!     };
//! }
//!
//! use user_columns::COLUMNS as USER;
//!
//! // Type-safe queries
//! User::query()
//!     .where_col(USER.name.eq("Alice"))           // OK: String == &str
//!     .where_col(USER.age.gt(18))                 // OK: Option<i32> > i32
//!     .where_col(USER.active.eq(true))            // OK: bool == bool
//!     // .where_col(USER.name.eq(123))            // COMPILE ERROR: String != i32
//!     // .where_col(USER.age.like("%test%"))      // COMPILE ERROR: i32 doesn't support LIKE
//!     .get()
//!     .await?;
//! ```

use std::marker::PhantomData;

// =============================================================================
// TYPED COLUMN
// =============================================================================

/// Trait for types that can be used as column names in queries.
/// 
/// This allows both string literals and typed `Column<T>` to be used
/// interchangeably in query methods like `where_eq`.
pub trait IntoColumnName {
    /// Get the column name as a string
    fn column_name(&self) -> &str;
}

impl IntoColumnName for &str {
    fn column_name(&self) -> &str {
        self
    }
}

impl IntoColumnName for String {
    fn column_name(&self) -> &str {
        self.as_str()
    }
}

impl IntoColumnName for &String {
    fn column_name(&self) -> &str {
        self.as_str()
    }
}

impl<T> IntoColumnName for Column<T> {
    fn column_name(&self) -> &str {
        self.name
    }
}

/// A strongly-typed column reference
///
/// This provides compile-time type safety for column operations.
/// The type parameter `T` represents the Rust type of the column.
#[derive(Debug, Clone, Copy)]
pub struct Column<T> {
    name: &'static str,
    _phantom: PhantomData<T>,
}

impl<T> Column<T> {
    /// Create a new typed column reference
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            _phantom: PhantomData,
        }
    }
    
    /// Get the column name
    pub const fn name(&self) -> &'static str {
        self.name
    }
}

// =============================================================================
// COLUMN CONDITIONS
// =============================================================================

/// A type-safe column condition for WHERE clauses
#[derive(Debug, Clone)]
pub struct ColumnCondition {
    /// The column name
    pub column: String,
    /// The operator
    pub operator: ColumnOperator,
    /// The value (as JSON for flexibility)
    pub value: serde_json::Value,
}

/// Operators for column conditions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnOperator {
    /// Equal to (=)
    Eq,
    /// Not equal to (<>)
    NotEq,
    /// Greater than (>)
    Gt,
    /// Greater than or equal (>=)
    Gte,
    /// Less than (<)
    Lt,
    /// Less than or equal (<=)
    Lte,
    /// LIKE pattern match
    Like,
    /// NOT LIKE pattern match
    NotLike,
    /// IN list
    In,
    /// NOT IN list
    NotIn,
    /// IS NULL
    IsNull,
    /// IS NOT NULL
    IsNotNull,
    /// BETWEEN range
    Between,
}

impl ColumnOperator {
    /// Convert to SQL operator string
    pub fn to_sql(&self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::NotEq => "<>",
            Self::Gt => ">",
            Self::Gte => ">=",
            Self::Lt => "<",
            Self::Lte => "<=",
            Self::Like => "LIKE",
            Self::NotLike => "NOT LIKE",
            Self::In => "IN",
            Self::NotIn => "NOT IN",
            Self::IsNull => "IS NULL",
            Self::IsNotNull => "IS NOT NULL",
            Self::Between => "BETWEEN",
        }
    }
}

// =============================================================================
// COLUMN EXPRESSION TRAITS
// =============================================================================

/// Trait for types that can be compared for equality
pub trait ColumnEq<T> {
    /// Create an equals condition
    fn eq(self, value: T) -> ColumnCondition;
    /// Create a not equals condition
    fn ne(self, value: T) -> ColumnCondition;
}

/// Trait for types that support ordering comparisons
pub trait ColumnOrd<T>: ColumnEq<T> {
    /// Create a greater than condition
    fn gt(self, value: T) -> ColumnCondition;
    /// Create a greater than or equal condition
    fn gte(self, value: T) -> ColumnCondition;
    /// Create a less than condition
    fn lt(self, value: T) -> ColumnCondition;
    /// Create a less than or equal condition
    fn lte(self, value: T) -> ColumnCondition;
    /// Create a between condition
    fn between(self, low: T, high: T) -> ColumnCondition;
}

/// Trait for string-like types that support LIKE
pub trait ColumnLike {
    /// Create a LIKE pattern condition
    fn like(self, pattern: &str) -> ColumnCondition;
    /// Create a NOT LIKE pattern condition
    fn not_like(self, pattern: &str) -> ColumnCondition;
    /// Create a LIKE '%value%' condition
    fn contains(self, substr: &str) -> ColumnCondition;
    /// Create a LIKE 'value%' condition
    fn starts_with(self, prefix: &str) -> ColumnCondition;
    /// Create a LIKE '%value' condition
    fn ends_with(self, suffix: &str) -> ColumnCondition;
}

/// Trait for nullable columns
#[allow(clippy::wrong_self_convention)]
pub trait ColumnNullable {
    /// Create an IS NULL condition
    fn is_null(self) -> ColumnCondition;
    /// Create an IS NOT NULL condition
    fn is_not_null(self) -> ColumnCondition;
}

/// Trait for types that support IN clauses
#[allow(clippy::wrong_self_convention)]
pub trait ColumnIn<T> {
    /// Create an IN list condition
    fn is_in(self, values: Vec<T>) -> ColumnCondition;
    /// Create a NOT IN list condition
    fn not_in(self, values: Vec<T>) -> ColumnCondition;
}

// =============================================================================
// IMPLEMENTATIONS FOR COMMON TYPES
// =============================================================================

// Helper macro to implement traits for numeric types
macro_rules! impl_column_numeric {
    ($($t:ty),*) => {
        $(
            impl ColumnEq<$t> for Column<$t> {
                fn eq(self, value: $t) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::Eq,
                        value: serde_json::json!(value),
                    }
                }
                
                fn ne(self, value: $t) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::NotEq,
                        value: serde_json::json!(value),
                    }
                }
            }
            
            impl ColumnOrd<$t> for Column<$t> {
                fn gt(self, value: $t) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::Gt,
                        value: serde_json::json!(value),
                    }
                }
                
                fn gte(self, value: $t) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::Gte,
                        value: serde_json::json!(value),
                    }
                }
                
                fn lt(self, value: $t) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::Lt,
                        value: serde_json::json!(value),
                    }
                }
                
                fn lte(self, value: $t) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::Lte,
                        value: serde_json::json!(value),
                    }
                }
                
                fn between(self, low: $t, high: $t) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::Between,
                        value: serde_json::json!([low, high]),
                    }
                }
            }
            
            impl ColumnIn<$t> for Column<$t> {
                fn is_in(self, values: Vec<$t>) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::In,
                        value: serde_json::json!(values),
                    }
                }
                
                fn not_in(self, values: Vec<$t>) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::NotIn,
                        value: serde_json::json!(values),
                    }
                }
            }
            
            // Optional versions
            impl ColumnEq<$t> for Column<Option<$t>> {
                fn eq(self, value: $t) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::Eq,
                        value: serde_json::json!(value),
                    }
                }
                
                fn ne(self, value: $t) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::NotEq,
                        value: serde_json::json!(value),
                    }
                }
            }
            
            impl ColumnOrd<$t> for Column<Option<$t>> {
                fn gt(self, value: $t) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::Gt,
                        value: serde_json::json!(value),
                    }
                }
                
                fn gte(self, value: $t) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::Gte,
                        value: serde_json::json!(value),
                    }
                }
                
                fn lt(self, value: $t) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::Lt,
                        value: serde_json::json!(value),
                    }
                }
                
                fn lte(self, value: $t) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::Lte,
                        value: serde_json::json!(value),
                    }
                }
                
                fn between(self, low: $t, high: $t) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::Between,
                        value: serde_json::json!([low, high]),
                    }
                }
            }
            
            impl ColumnNullable for Column<Option<$t>> {
                fn is_null(self) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::IsNull,
                        value: serde_json::Value::Null,
                    }
                }
                
                fn is_not_null(self) -> ColumnCondition {
                    ColumnCondition {
                        column: self.name.to_string(),
                        operator: ColumnOperator::IsNotNull,
                        value: serde_json::Value::Null,
                    }
                }
            }
        )*
    };
}

impl_column_numeric!(i8, i16, i32, i64, u8, u16, u32, u64, f32, f64);

// String implementations
impl ColumnEq<&str> for Column<String> {
    fn eq(self, value: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::Eq,
            value: serde_json::json!(value),
        }
    }
    
    fn ne(self, value: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::NotEq,
            value: serde_json::json!(value),
        }
    }
}

impl ColumnEq<String> for Column<String> {
    fn eq(self, value: String) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::Eq,
            value: serde_json::json!(value),
        }
    }
    
    fn ne(self, value: String) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::NotEq,
            value: serde_json::json!(value),
        }
    }
}

impl ColumnLike for Column<String> {
    fn like(self, pattern: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::Like,
            value: serde_json::json!(pattern),
        }
    }
    
    fn not_like(self, pattern: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::NotLike,
            value: serde_json::json!(pattern),
        }
    }
    
    fn contains(self, substr: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::Like,
            value: serde_json::json!(format!("%{}%", substr)),
        }
    }
    
    fn starts_with(self, prefix: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::Like,
            value: serde_json::json!(format!("{}%", prefix)),
        }
    }
    
    fn ends_with(self, suffix: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::Like,
            value: serde_json::json!(format!("%{}", suffix)),
        }
    }
}

impl ColumnIn<&str> for Column<String> {
    fn is_in(self, values: Vec<&str>) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::In,
            value: serde_json::json!(values),
        }
    }
    
    fn not_in(self, values: Vec<&str>) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::NotIn,
            value: serde_json::json!(values),
        }
    }
}

impl ColumnIn<String> for Column<String> {
    fn is_in(self, values: Vec<String>) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::In,
            value: serde_json::json!(values),
        }
    }
    
    fn not_in(self, values: Vec<String>) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::NotIn,
            value: serde_json::json!(values),
        }
    }
}

// Optional String
impl ColumnEq<&str> for Column<Option<String>> {
    fn eq(self, value: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::Eq,
            value: serde_json::json!(value),
        }
    }
    
    fn ne(self, value: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::NotEq,
            value: serde_json::json!(value),
        }
    }
}

impl ColumnLike for Column<Option<String>> {
    fn like(self, pattern: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::Like,
            value: serde_json::json!(pattern),
        }
    }
    
    fn not_like(self, pattern: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::NotLike,
            value: serde_json::json!(pattern),
        }
    }
    
    fn contains(self, substr: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::Like,
            value: serde_json::json!(format!("%{}%", substr)),
        }
    }
    
    fn starts_with(self, prefix: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::Like,
            value: serde_json::json!(format!("{}%", prefix)),
        }
    }
    
    fn ends_with(self, suffix: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::Like,
            value: serde_json::json!(format!("%{}", suffix)),
        }
    }
}

impl ColumnNullable for Column<Option<String>> {
    fn is_null(self) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::IsNull,
            value: serde_json::Value::Null,
        }
    }
    
    fn is_not_null(self) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::IsNotNull,
            value: serde_json::Value::Null,
        }
    }
}

// Bool implementations
impl ColumnEq<bool> for Column<bool> {
    fn eq(self, value: bool) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::Eq,
            value: serde_json::json!(value),
        }
    }
    
    fn ne(self, value: bool) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::NotEq,
            value: serde_json::json!(value),
        }
    }
}

impl ColumnEq<bool> for Column<Option<bool>> {
    fn eq(self, value: bool) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::Eq,
            value: serde_json::json!(value),
        }
    }
    
    fn ne(self, value: bool) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::NotEq,
            value: serde_json::json!(value),
        }
    }
}

impl ColumnNullable for Column<Option<bool>> {
    fn is_null(self) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::IsNull,
            value: serde_json::Value::Null,
        }
    }
    
    fn is_not_null(self) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::IsNotNull,
            value: serde_json::Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_typed_column_creation() {
        let col: Column<i64> = Column::new("id");
        assert_eq!(col.name(), "id");
    }
    
    #[test]
    fn test_typed_column_eq() {
        let col: Column<i64> = Column::new("id");
        let cond = col.eq(42);
        assert_eq!(cond.column, "id");
        assert_eq!(cond.operator, ColumnOperator::Eq);
        assert_eq!(cond.value, serde_json::json!(42));
    }
    
    #[test]
    fn test_typed_column_string_like() {
        let col: Column<String> = Column::new("name");
        let cond = col.contains("test");
        assert_eq!(cond.column, "name");
        assert_eq!(cond.operator, ColumnOperator::Like);
        assert_eq!(cond.value, serde_json::json!("%test%"));
    }
    
    #[test]
    fn test_typed_column_nullable() {
        let col: Column<Option<i32>> = Column::new("age");
        let cond = col.is_null();
        assert_eq!(cond.column, "age");
        assert_eq!(cond.operator, ColumnOperator::IsNull);
    }
    
    #[test]
    fn test_typed_column_between() {
        let col: Column<i32> = Column::new("score");
        let cond = col.between(10, 100);
        assert_eq!(cond.column, "score");
        assert_eq!(cond.operator, ColumnOperator::Between);
        assert_eq!(cond.value, serde_json::json!([10, 100]));
    }
    
    #[test]
    fn test_typed_column_in() {
        let col: Column<String> = Column::new("status");
        let cond = col.is_in(vec!["active", "pending"]);
        assert_eq!(cond.column, "status");
        assert_eq!(cond.operator, ColumnOperator::In);
    }
}
