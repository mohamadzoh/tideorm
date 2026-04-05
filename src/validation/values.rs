use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

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
}

macro_rules! impl_validatable_for_int {
    ($($t:ty),*) => {
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
            }
        )*
    };
}

impl_validatable_for_int!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64
);
