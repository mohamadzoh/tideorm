use super::{
    Column, ColumnCondition, ColumnEq, ColumnIn, ColumnLike, ColumnNullable, ColumnOperator,
    ColumnOrd, escape_like_literal,
};

// =============================================================================
// IMPLEMENTATIONS FOR COMMON TYPES
// =============================================================================

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
            operator: ColumnOperator::LikeEscaped,
            value: serde_json::json!(format!("%{}%", escape_like_literal(substr))),
        }
    }

    fn starts_with(self, prefix: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::LikeEscaped,
            value: serde_json::json!(format!("{}%", escape_like_literal(prefix))),
        }
    }

    fn ends_with(self, suffix: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::LikeEscaped,
            value: serde_json::json!(format!("%{}", escape_like_literal(suffix))),
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
            operator: ColumnOperator::LikeEscaped,
            value: serde_json::json!(format!("%{}%", escape_like_literal(substr))),
        }
    }

    fn starts_with(self, prefix: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::LikeEscaped,
            value: serde_json::json!(format!("{}%", escape_like_literal(prefix))),
        }
    }

    fn ends_with(self, suffix: &str) -> ColumnCondition {
        ColumnCondition {
            column: self.name.to_string(),
            operator: ColumnOperator::LikeEscaped,
            value: serde_json::json!(format!("%{}", escape_like_literal(suffix))),
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
