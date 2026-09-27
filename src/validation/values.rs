use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Compile a validation pattern once per process; `None` when it is not a
/// valid regex.
pub(super) fn compiled_validation_regex(pattern: &str) -> Option<regex::Regex> {
    static REGEX_CACHE: OnceLock<Mutex<HashMap<String, Option<regex::Regex>>>> = OnceLock::new();
    let cache = REGEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache
        .entry(pattern.to_string())
        .or_insert_with(|| regex::Regex::new(pattern).ok())
        .clone()
}

/// Value adapter used by the generic validator helpers.
pub trait ValidatableValue {
    /// Return whether the value counts as empty for `Required`.
    fn is_empty_value(&self) -> bool;

    /// Return a string view for string-oriented rules.
    fn as_str_value(&self) -> Option<&str>;

    /// Return a numeric representation for numeric rules.
    fn as_f64_value(&self) -> Option<f64>;

    /// Return the value as an exact integer, for an integer value: numeric
    /// rules compare it exactly, where an integer past 2^53 has no exact `f64`.
    fn as_i128_value(&self) -> Option<i128> {
        None
    }

    /// `bound` at the precision this value holds a number in, which the
    /// numeric rules compare it with: an `f32` holds `0.7` as `0.699999988`,
    /// so a `min = 0.7` must be rounded the same way or `0.7` fails it.
    fn numeric_bound(&self, bound: f64) -> f64 {
        bound
    }
}

impl ValidatableValue for String {
    fn is_empty_value(&self) -> bool {
        self.trim().is_empty()
    }

    fn as_str_value(&self) -> Option<&str> {
        Some(self.as_str())
    }

    fn as_f64_value(&self) -> Option<f64> {
        self.parse().ok()
    }

    fn as_i128_value(&self) -> Option<i128> {
        self.parse().ok()
    }
}

impl ValidatableValue for &str {
    fn is_empty_value(&self) -> bool {
        self.trim().is_empty()
    }

    fn as_str_value(&self) -> Option<&str> {
        Some(self)
    }

    fn as_f64_value(&self) -> Option<f64> {
        self.parse().ok()
    }

    fn as_i128_value(&self) -> Option<i128> {
        self.parse().ok()
    }
}

impl<T: ValidatableValue> ValidatableValue for Option<T> {
    fn is_empty_value(&self) -> bool {
        match self {
            Some(value) => value.is_empty_value(),
            None => true,
        }
    }

    fn as_str_value(&self) -> Option<&str> {
        self.as_ref().and_then(|value| value.as_str_value())
    }

    fn as_f64_value(&self) -> Option<f64> {
        self.as_ref().and_then(|value| value.as_f64_value())
    }

    fn as_i128_value(&self) -> Option<i128> {
        self.as_ref().and_then(|value| value.as_i128_value())
    }

    fn numeric_bound(&self, bound: f64) -> f64 {
        self.as_ref()
            .map_or(bound, |value| value.numeric_bound(bound))
    }
}

macro_rules! impl_validatable_for_number {
    ($($t:ty),* ; $($int:ty),*) => {
        $(
            impl ValidatableValue for $t {
                fn is_empty_value(&self) -> bool {
                    false
                }

                fn as_str_value(&self) -> Option<&str> {
                    None
                }

                fn as_f64_value(&self) -> Option<f64> {
                    Some(*self as f64)
                }

                fn numeric_bound(&self, bound: f64) -> f64 {
                    bound as $t as f64
                }
            }
        )*
        $(
            impl ValidatableValue for $int {
                fn is_empty_value(&self) -> bool {
                    false
                }

                fn as_str_value(&self) -> Option<&str> {
                    None
                }

                fn as_f64_value(&self) -> Option<f64> {
                    Some(*self as f64)
                }

                fn as_i128_value(&self) -> Option<i128> {
                    i128::try_from(*self).ok()
                }
            }
        )*
    };
}

impl_validatable_for_number!(
    f32, f64;
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

impl ValidatableValue for rust_decimal::Decimal {
    fn is_empty_value(&self) -> bool {
        false
    }

    fn as_str_value(&self) -> Option<&str> {
        None
    }

    fn as_f64_value(&self) -> Option<f64> {
        rust_decimal::prelude::ToPrimitive::to_f64(self)
    }

    fn as_i128_value(&self) -> Option<i128> {
        if self.fract().is_zero() {
            rust_decimal::prelude::ToPrimitive::to_i128(self)
        } else {
            None
        }
    }
}

impl ValidatableValue for serde_json::Value {
    /// `null`, a blank string and an empty array or object are empty.
    fn is_empty_value(&self) -> bool {
        match self {
            serde_json::Value::Null => true,
            serde_json::Value::String(text) => text.trim().is_empty(),
            serde_json::Value::Array(items) => items.is_empty(),
            serde_json::Value::Object(members) => members.is_empty(),
            serde_json::Value::Bool(_) | serde_json::Value::Number(_) => false,
        }
    }

    fn as_str_value(&self) -> Option<&str> {
        self.as_str()
    }

    fn as_f64_value(&self) -> Option<f64> {
        self.as_f64()
    }

    fn as_i128_value(&self) -> Option<i128> {
        self.as_i64()
            .map(i128::from)
            .or_else(|| self.as_u64().map(i128::from))
    }
}

impl<T> ValidatableValue for Vec<T> {
    fn is_empty_value(&self) -> bool {
        self.is_empty()
    }

    fn as_str_value(&self) -> Option<&str> {
        None
    }

    fn as_f64_value(&self) -> Option<f64> {
        None
    }
}

/// Types only `required` applies to, and which are never empty themselves:
/// an `Option` of one is empty when it is `None`.
macro_rules! impl_validatable_presence_only {
    ($($t:ty),* $(,)?) => {
        $(
            impl ValidatableValue for $t {
                fn is_empty_value(&self) -> bool {
                    false
                }

                fn as_str_value(&self) -> Option<&str> {
                    None
                }

                fn as_f64_value(&self) -> Option<f64> {
                    None
                }
            }
        )*
    };
}

impl_validatable_presence_only!(
    bool,
    char,
    uuid::Uuid,
    chrono::NaiveDate,
    chrono::NaiveDateTime,
    chrono::NaiveTime,
);

impl<Tz: chrono::TimeZone> ValidatableValue for chrono::DateTime<Tz> {
    fn is_empty_value(&self) -> bool {
        false
    }

    fn as_str_value(&self) -> Option<&str> {
        None
    }

    fn as_f64_value(&self) -> Option<f64> {
        None
    }
}
