use super::{
    Column, ColumnCondition, ColumnEq, ColumnIn, ColumnLike, ColumnNullable, ColumnOrd,
    escape_like_literal,
};
use crate::query::Operator;
use serde_json::json;

impl<T> Column<T> {
    fn cond(self, operator: Operator, value: serde_json::Value) -> ColumnCondition {
        ColumnCondition {
            column: super::column_reference(&self, None),
            operator,
            value,
        }
    }
}

// Each macro implements one trait for every listed column type, so a nullable
// column (`Column<Option<T>>`) shares the comparisons of its plain twin.

macro_rules! impl_eq {
    ($value:ty => $($column:ty),+) => {$(
        impl ColumnEq<$value> for Column<$column> {
            fn eq(self, value: $value) -> ColumnCondition {
                self.cond(Operator::Eq, json!(value))
            }

            fn ne(self, value: $value) -> ColumnCondition {
                self.cond(Operator::NotEq, json!(value))
            }
        }
    )+};
}

macro_rules! impl_ord {
    ($value:ty => $($column:ty),+) => {$(
        impl ColumnOrd<$value> for Column<$column> {
            fn gt(self, value: $value) -> ColumnCondition {
                self.cond(Operator::Gt, json!(value))
            }

            fn gte(self, value: $value) -> ColumnCondition {
                self.cond(Operator::Gte, json!(value))
            }

            fn lt(self, value: $value) -> ColumnCondition {
                self.cond(Operator::Lt, json!(value))
            }

            fn lte(self, value: $value) -> ColumnCondition {
                self.cond(Operator::Lte, json!(value))
            }

            fn between(self, low: $value, high: $value) -> ColumnCondition {
                self.cond(Operator::Between, json!([low, high]))
            }
        }
    )+};
}

macro_rules! impl_in {
    (@methods $value:ty) => {
        fn is_in(self, values: impl IntoIterator<Item = $value>) -> ColumnCondition {
            self.cond(
            Operator::In,
            serde_json::Value::Array(values.into_iter().map(crate::query::filter_value).collect()),
        )
        }

        fn not_in(self, values: impl IntoIterator<Item = $value>) -> ColumnCondition {
            self.cond(
            Operator::NotIn,
            serde_json::Value::Array(values.into_iter().map(crate::query::filter_value).collect()),
        )
        }
    };
    (<$lifetime:lifetime> $value:ty => $($column:ty),+) => {$(
        impl<$lifetime> ColumnIn<$value> for Column<$column> {
            impl_in!(@methods $value);
        }
    )+};
}

macro_rules! impl_like {
    ($($column:ty),+) => {$(
        impl ColumnLike for Column<$column> {
            fn like(self, pattern: &str) -> ColumnCondition {
                self.cond(Operator::Like, json!(pattern))
            }

            fn not_like(self, pattern: &str) -> ColumnCondition {
                self.cond(Operator::NotLike, json!(pattern))
            }

            fn contains(self, substr: &str) -> ColumnCondition {
                self.cond(
                    Operator::LikeEscaped,
                    json!(format!("%{}%", escape_like_literal(substr))),
                )
            }

            fn starts_with(self, prefix: &str) -> ColumnCondition {
                self.cond(
                    Operator::LikeEscaped,
                    json!(format!("{}%", escape_like_literal(prefix))),
                )
            }

            fn ends_with(self, suffix: &str) -> ColumnCondition {
                self.cond(
                    Operator::LikeEscaped,
                    json!(format!("%{}", escape_like_literal(suffix))),
                )
            }
        }
    )+};
}

macro_rules! impl_nullable {
    ($($column:ty),+) => {$(
        impl ColumnNullable for Column<$column> {
            fn is_null(self) -> ColumnCondition {
                self.cond(Operator::IsNull, serde_json::Value::Null)
            }

            fn is_not_null(self) -> ColumnCondition {
                self.cond(Operator::IsNotNull, serde_json::Value::Null)
            }
        }
    )+};
}

macro_rules! impl_ordered {
    ($($t:ty),*) => {$(
        impl_ord!($t => $t, Option<$t>);
        impl_nullable!(Option<$t>);
    )*};
}

impl_ordered!(i8, i16, i32, i64, u8, u16, u32, u64, f32, f64);
impl_ordered!(
    uuid::Uuid,
    chrono::DateTime<chrono::Utc>,
    chrono::NaiveDateTime,
    chrono::NaiveDate,
    chrono::NaiveTime,
    rust_decimal::Decimal
);

impl_eq!(&str => String, Option<String>);
impl_like!(String, Option<String>);
impl_in!(<'a> &'a str => String, Option<String>);
impl_nullable!(Option<String>);

impl_nullable!(Option<bool>);
// Equality and IN lists work for a column of any type that serializes — an
// enum, a newtype, a JSON value — and a nullable column (`Column<Option<T>>`)
// compares with a plain `T` too.
impl<T: serde::Serialize> ColumnEq<T> for Column<T> {
    fn eq(self, value: T) -> ColumnCondition {
        self.cond(Operator::Eq, crate::query::filter_value(value))
    }

    fn ne(self, value: T) -> ColumnCondition {
        self.cond(Operator::NotEq, crate::query::filter_value(value))
    }
}

impl<T: serde::Serialize> ColumnEq<T> for Column<Option<T>> {
    fn eq(self, value: T) -> ColumnCondition {
        self.cond(Operator::Eq, crate::query::filter_value(value))
    }

    fn ne(self, value: T) -> ColumnCondition {
        self.cond(Operator::NotEq, crate::query::filter_value(value))
    }
}

impl<T: serde::Serialize> ColumnIn<T> for Column<T> {
    impl_in!(@methods T);
}

impl<T: serde::Serialize> ColumnIn<T> for Column<Option<T>> {
    impl_in!(@methods T);
}
