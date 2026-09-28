//! Model validation.
//!
//! `#[validate(..)]` field attributes compile to [`ValidationRule`]s. The
//! derived [`Validate::validate`] checks every rule on every field and returns
//! all failures together in one [`ValidationErrors`], before `create`,
//! `update` or `save` reaches the database. [`Validator::validate_rule`]
//! applies a single rule by hand, and [`ValidationBuilder`] assembles the rule
//! list for one field.

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

mod builder;
mod values;

pub use builder::ValidationBuilder;
pub use values::ValidatableValue;
use values::compiled_validation_regex;

/// Collection of validation errors organized by field name
///
/// Fields come out in the order their first error was added, which for a
/// derived model is the order the struct declares them, so [`first`](Self::first),
/// [`messages`](Self::messages) and the error `create()` returns are the same
/// on every run.
#[derive(Debug, Clone, Default)]
pub struct ValidationErrors {
    errors: HashMap<String, Vec<String>>,
    order: Vec<String>,
}

impl ValidationErrors {
    /// Start an empty validation-error collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one error message to a field.
    pub fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        let field = field.into();
        if !self.errors.contains_key(&field) {
            self.order.push(field.clone());
        }
        self.errors.entry(field).or_default().push(message.into());
    }

    /// Apply each of `rules` to `field`'s `value`, recording every failure; a
    /// generated `Validate` impl checks each field with one call.
    #[doc(hidden)]
    pub fn __check<T: ValidatableValue>(
        &mut self,
        field: &str,
        value: &T,
        rules: &[ValidationRule],
    ) {
        for rule in rules {
            if let Some(message) = Validator::validate_rule(value, rule, field) {
                self.add(field, message);
            }
        }
    }

    /// True when no field errors have been collected.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Number of fields that currently have at least one error.
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    /// Borrow the full error list for one field.
    pub fn get(&self, field: &str) -> Option<&Vec<String>> {
        self.errors.get(field)
    }

    /// Clone the error messages for one field.
    pub fn field_errors(&self, field: &str) -> Vec<String> {
        self.errors.get(field).cloned().unwrap_or_default()
    }

    /// Borrow the whole field-to-errors map.
    pub fn all(&self) -> &HashMap<String, Vec<String>> {
        &self.errors
    }

    /// Iterate through all field-error pairs, in the order the fields failed.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Vec<String>)> {
        self.order
            .iter()
            .filter_map(|field| self.errors.get_key_value(field))
    }

    /// Return the first field/message pair, if any.
    pub fn first(&self) -> Option<(&String, &String)> {
        self.iter()
            .next()
            .and_then(|(field, messages)| messages.first().map(|msg| (field, msg)))
    }

    /// Flatten all field errors into display strings.
    pub fn messages(&self) -> Vec<String> {
        self.iter()
            .flat_map(|(field, messages)| {
                messages
                    .iter()
                    .map(move |msg| format!("{}: {}", field, msg))
            })
            .collect()
    }

    /// Append all errors from another collection.
    pub fn merge(&mut self, mut other: ValidationErrors) {
        for field in other.order {
            for message in other.errors.remove(&field).unwrap_or_default() {
                self.add(field.clone(), message);
            }
        }
    }

    /// Return `Ok(())` when empty, otherwise return the collection as `Err`.
    pub fn to_result(self) -> Result<(), Self> {
        if self.is_empty() { Ok(()) } else { Err(self) }
    }

    /// Flatten all errors into `(field, message)` pairs.
    pub fn errors(&self) -> Vec<(String, String)> {
        self.iter()
            .flat_map(|(field, messages)| {
                messages.iter().map(move |msg| (field.clone(), msg.clone()))
            })
            .collect()
    }
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let messages: Vec<String> = self.messages();
        write!(f, "{}", messages.join("; "))
    }
}

impl std::error::Error for ValidationErrors {}

impl From<ValidationErrors> for crate::error::Error {
    fn from(errors: ValidationErrors) -> Self {
        if let Some((field, message)) = errors.first() {
            crate::error::Error::Validation {
                field: field.clone(),
                message: message.clone(),
            }
        } else {
            crate::error::Error::Validation {
                field: "unknown".to_string(),
                message: "Validation failed".to_string(),
            }
        }
    }
}

/// A single validation rule
#[derive(Debug, Clone)]
pub enum ValidationRule {
    /// Field must not be empty
    Required,
    /// Must be a valid email address
    Email,
    /// Must be a valid URL
    Url,
    /// Minimum string length
    MinLength(usize),
    /// Maximum string length
    MaxLength(usize),
    /// Exact string length
    Length(usize),
    /// Minimum numeric value
    Min(f64),
    /// Maximum numeric value
    Max(f64),
    /// Value must be within a range (inclusive)
    Range(f64, f64),
    /// Must match a regular expression pattern
    Regex(String),
    /// Must contain only letters (a-zA-Z)
    Alpha,
    /// Must contain only letters and numbers
    Alphanumeric,
    /// Must be numeric
    Numeric,
    /// Must be a valid UUID
    Uuid,
    /// Must be in a list of allowed values
    In(Vec<String>),
    /// Must not be in a list of disallowed values
    NotIn(Vec<String>),
}

impl ValidationRule {
    /// Render the default error message for this rule and field.
    pub fn message(&self, field: &str) -> String {
        match self {
            ValidationRule::Required => format!("The {} field is required", field),
            ValidationRule::Email => format!("The {} must be a valid email address", field),
            ValidationRule::Url => format!("The {} must be a valid URL", field),
            ValidationRule::MinLength(len) => {
                format!("The {} must be at least {} characters", field, len)
            }
            ValidationRule::MaxLength(len) => {
                format!("The {} must not exceed {} characters", field, len)
            }
            ValidationRule::Length(len) => {
                format!("The {} must be exactly {} characters", field, len)
            }
            ValidationRule::Min(val) => format!("The {} must be at least {}", field, val),
            ValidationRule::Max(val) => format!("The {} must not exceed {}", field, val),
            ValidationRule::Range(min, max) => {
                format!("The {} must be between {} and {}", field, min, max)
            }
            ValidationRule::Regex(pattern) => {
                format!("The {} format is invalid (must match: {})", field, pattern)
            }
            ValidationRule::Alpha => format!("The {} must only contain letters", field),
            ValidationRule::Alphanumeric => {
                format!("The {} must only contain letters and numbers", field)
            }
            ValidationRule::Numeric => format!("The {} must be a number", field),
            ValidationRule::Uuid => format!("The {} must be a valid UUID", field),
            ValidationRule::In(values) => {
                format!("The {} must be one of: {}", field, values.join(", "))
            }
            ValidationRule::NotIn(values) => {
                format!("The {} must not be one of: {}", field, values.join(", "))
            }
        }
    }

    /// Validate one value against this rule.
    pub fn validate<T: ValidatableValue>(&self, value: &T) -> Result<(), String> {
        match Validator::validate_rule(value, self, "field") {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// Validator for applying validation rules
pub struct Validator;

impl Validator {
    /// Apply one validation rule and return the generated message on failure.
    pub fn validate_rule<T: ValidatableValue>(
        value: &T,
        rule: &ValidationRule,
        field: &str,
    ) -> Option<String> {
        // A text rule checks a text value and passes any other.
        let text_fails = |fails: &dyn Fn(&str) -> bool| value.as_str_value().is_some_and(fails);
        let fails = match rule {
            ValidationRule::Required => value.is_empty_value(),
            ValidationRule::Email => text_fails(&|s| !Self::is_valid_email(s)),
            ValidationRule::Url => text_fails(&|s| !Self::is_valid_url(s)),
            ValidationRule::MinLength(min) => text_fails(&|s| s.chars().count() < *min),
            ValidationRule::MaxLength(max) => text_fails(&|s| s.chars().count() > *max),
            ValidationRule::Length(len) => text_fails(&|s| s.chars().count() != *len),
            ValidationRule::Min(min) => below_bound(value, *min) == Some(true),
            ValidationRule::Max(max) => above_bound(value, *max) == Some(true),
            ValidationRule::Range(min, max) => {
                below_bound(value, *min) == Some(true) || above_bound(value, *max) == Some(true)
            }
            ValidationRule::Regex(pattern) => {
                // A pattern that does not compile fails for every value: letting
                // it pass would accept everything the rule exists to reject.
                let Some(re) = compiled_validation_regex(pattern) else {
                    return Some(format!(
                        "The {} validation pattern `{}` is not a valid regular expression",
                        field, pattern
                    ));
                };
                text_fails(&|s| !re.is_match(s))
            }
            ValidationRule::Alpha => text_fails(&|s| !s.chars().all(char::is_alphabetic)),
            ValidationRule::Alphanumeric => text_fails(&|s| !s.chars().all(char::is_alphanumeric)),
            // `parse` also reads "NaN", "inf" and an overflowing "1e999".
            ValidationRule::Numeric => text_fails(&|s| !s.parse::<f64>().is_ok_and(f64::is_finite)),
            ValidationRule::Uuid => text_fails(&|s| uuid::Uuid::parse_str(s).is_err()),
            ValidationRule::In(values) => text_fails(&|s| !values.iter().any(|v| v == s)),
            ValidationRule::NotIn(values) => text_fails(&|s| values.iter().any(|v| v == s)),
        };
        fails.then(|| rule.message(field))
    }

    /// Minimal email-shape check used by the built-in email rule.
    pub fn is_valid_email(s: &str) -> bool {
        static EMAIL_REGEX: OnceLock<regex::Regex> = OnceLock::new();
        EMAIL_REGEX
            .get_or_init(|| {
                regex::Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$")
                    .expect("the built-in email pattern is a valid regex")
            })
            .is_match(s)
    }

    /// URL check used by the built-in URL rule.
    pub fn is_valid_url(s: &str) -> bool {
        match url::Url::parse(s) {
            Ok(url) => matches!(url.scheme(), "http" | "https") && url.has_host(),
            Err(_) => false,
        }
    }
}

/// Trait for models that can be validated
///
/// The model derive implements this for every model from its `#[validate(..)]`
/// field attributes. Implement it by hand for types that are not TideORM models.
pub trait Validate {
    /// Check every rule and return all failures together.
    fn validate(&self) -> Result<(), ValidationErrors>;

    /// Validate and return `self` for builder-style call chains.
    fn validated(self) -> Result<Self, ValidationErrors>
    where
        Self: Sized,
    {
        self.validate()?;
        Ok(self)
    }
}

/// Whether a numeric `value` lies below `bound`; `None` for a value that is
/// not a number. An integer is compared exactly: past 2^53 it has no exact
/// `f64`, and rounding it would let a value one past the bound through. NaN
/// lies outside every bound.
fn below_bound<T: ValidatableValue + ?Sized>(value: &T, bound: f64) -> Option<bool> {
    if let Some(integer) = value.as_i128_value() {
        return Some(if bound > i128::MAX as f64 {
            true
        } else if bound <= i128::MIN as f64 {
            false
        } else {
            integer < bound.ceil() as i128
        });
    }
    value
        .as_f64_value()
        .map(|n| n.is_nan() || n < value.numeric_bound(bound))
}

/// Whether a numeric `value` lies above `bound`, as [`below_bound`] compares.
fn above_bound<T: ValidatableValue + ?Sized>(value: &T, bound: f64) -> Option<bool> {
    if let Some(integer) = value.as_i128_value() {
        return Some(if bound >= i128::MAX as f64 {
            false
        } else if bound < i128::MIN as f64 {
            true
        } else {
            integer > bound.floor() as i128
        });
    }
    value
        .as_f64_value()
        .map(|n| n.is_nan() || n > value.numeric_bound(bound))
}

#[cfg(test)]
#[path = "../tests/unit/validation_tests.rs"]
mod tests;
