//! Model Validation System
//!
//! This module validates model data before writes.
//!
//! Validation can come from field attributes, custom `Validate` logic, or both.
//! If `save()` or `update()` returns a validation error, inspect the collected
//! field messages here before looking at database-side constraints.
//!
//! Typical flow:
//! - field attributes catch simple shape problems like missing values, invalid email, or range failures
//! - `custom_validations()` handles business rules that need model-aware logic
//! - `validate_all()` is the better entry point when the caller needs every field error instead of the first one
//!
//! If validation unexpectedly passes, check whether the value type implements `ValidatableValue`
//! the way you expect and whether the rule actually runs in `Validator` versus model-level custom logic.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, OnceLock};

/// Collection of validation errors organized by field name
#[derive(Debug, Clone, Default)]
pub struct ValidationErrors {
    errors: HashMap<String, Vec<String>>,
}

impl ValidationErrors {
    /// Start an empty validation-error collection.
    pub fn new() -> Self {
        Self {
            errors: HashMap::new(),
        }
    }

    /// Append one error message to a field.
    pub fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.errors
            .entry(field.into())
            .or_default()
            .push(message.into());
    }

    /// True when no field errors have been collected.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// True when at least one field has an error.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
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

    /// Iterate through all field-error pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Vec<String>)> {
        self.errors.iter()
    }

    /// Return the first field/message pair, if any.
    pub fn first(&self) -> Option<(&String, &String)> {
        self.errors
            .iter()
            .next()
            .and_then(|(field, messages)| messages.first().map(|msg| (field, msg)))
    }

    /// Flatten all field errors into display strings.
    pub fn messages(&self) -> Vec<String> {
        self.errors
            .iter()
            .flat_map(|(field, messages)| {
                messages
                    .iter()
                    .map(move |msg| format!("{}: {}", field, msg))
            })
            .collect()
    }

    /// Append all errors from another collection.
    pub fn merge(&mut self, other: ValidationErrors) {
        for (field, messages) in other.errors {
            for message in messages {
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
        self.errors
            .iter()
            .flat_map(|(field, messages)| {
                messages.iter().map(move |msg| (field.clone(), msg.clone()))
            })
            .collect()
    }

    /// Convert the first collected message into the crate error type.
    pub fn into_error(self) -> Option<crate::error::Error> {
        self.first()
            .map(|(field, message)| crate::error::Error::Validation {
                field: field.clone(),
                message: message.clone(),
            })
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
    /// Must match another field (for confirmations)
    Confirmed(String),
    /// Custom validation marker with message.
    ///
    /// This is not evaluated directly by `Validator::validate_rule`; macro-generated
    /// model validation defers custom checks to `Validate::custom_validations()`.
    Custom(String),
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
            ValidationRule::Confirmed(other) => {
                format!("The {} confirmation does not match {}", field, other)
            }
            ValidationRule::Custom(msg) => msg.clone(),
        }
    }

    /// Validate one value against this rule.
    ///
    /// `ValidationRule::Custom` is a no-op here because custom checks run
    /// through `Validate::custom_validations()`.
    pub fn validate<T: ValidatableValue>(&self, value: &T) -> Result<(), String> {
        match Validator::validate_rule(value, self, "field") {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

fn compiled_validation_regex(pattern: &str) -> Option<regex::Regex> {
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
            Some(v) => v.is_empty_value(),
            None => true,
        }
    }

    fn as_str_value(&self) -> Option<&str> {
        self.as_ref().and_then(|v| v.as_str_value())
    }

    fn as_f64_value(&self) -> Option<f64> {
        self.as_ref().and_then(|v| v.as_f64_value())
    }
}

macro_rules! impl_validatable_for_int {
    ($($t:ty),*) => {
        $(
            impl ValidatableValue for $t {
                fn is_empty_value(&self) -> bool {
                    false  // Numbers are never "empty"
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

/// Validator for applying validation rules
pub struct Validator;

impl Validator {
    /// Apply one validation rule and return the generated message on failure.
    pub fn validate_rule<T: ValidatableValue>(
        value: &T,
        rule: &ValidationRule,
        field: &str,
    ) -> Option<String> {
        match rule {
            ValidationRule::Required => {
                if value.is_empty_value() {
                    return Some(rule.message(field));
                }
            }
            ValidationRule::Email => {
                if let Some(s) = value.as_str_value() {
                    if !Self::is_valid_email(s) {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::Url => {
                if let Some(s) = value.as_str_value() {
                    if !Self::is_valid_url(s) {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::MinLength(min) => {
                if let Some(s) = value.as_str_value() {
                    if s.chars().count() < *min {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::MaxLength(max) => {
                if let Some(s) = value.as_str_value() {
                    if s.chars().count() > *max {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::Length(len) => {
                if let Some(s) = value.as_str_value() {
                    if s.chars().count() != *len {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::Min(min) => {
                if let Some(n) = value.as_f64_value() {
                    if n < *min {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::Max(max) => {
                if let Some(n) = value.as_f64_value() {
                    if n > *max {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::Range(min, max) => {
                if let Some(n) = value.as_f64_value() {
                    if n < *min || n > *max {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::Regex(pattern) => {
                if let Some(s) = value.as_str_value() {
                    if let Some(re) = compiled_validation_regex(pattern) {
                        if !re.is_match(s) {
                            return Some(rule.message(field));
                        }
                    }
                }
            }
            ValidationRule::Alpha => {
                if let Some(s) = value.as_str_value() {
                    if !s.chars().all(|c| c.is_alphabetic()) {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::Alphanumeric => {
                if let Some(s) = value.as_str_value() {
                    if !s.chars().all(|c| c.is_alphanumeric()) {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::Numeric => {
                if let Some(s) = value.as_str_value() {
                    if s.parse::<f64>().is_err() {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::Uuid => {
                if let Some(s) = value.as_str_value() {
                    if uuid::Uuid::parse_str(s).is_err() {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::In(values) => {
                if let Some(s) = value.as_str_value() {
                    if !values.iter().any(|v| v == s) {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::NotIn(values) => {
                if let Some(s) = value.as_str_value() {
                    if values.iter().any(|v| v == s) {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::Confirmed(_) => {
                // This is handled at the model level, not here
            }
            ValidationRule::Custom(_) => {
                // Custom rules are handled through Validate::custom_validations.
            }
        }
        None
    }

    /// Minimal email-shape check used by the built-in email rule.
    pub fn is_valid_email(s: &str) -> bool {
        static EMAIL_REGEX: OnceLock<regex::Regex> = OnceLock::new();
        let email_regex = EMAIL_REGEX.get_or_init(|| {
            regex::Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$")
                .expect("email validation regex should be valid")
        });
        email_regex.is_match(s)
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
/// This trait is automatically implemented by TideORM's model macros
/// when validation attributes are present. You can also implement it manually
/// for custom validation logic.
pub trait Validate {
    /// Return the static field-to-rules mapping for this model.
    fn validation_rules() -> Vec<(&'static str, Vec<ValidationRule>)> {
        vec![]
    }

    /// Validate and stop at the first reported error.
    fn validate(&self) -> Result<(), ValidationErrors>;

    /// Validate and collect all reported field errors.
    fn validate_all(&self) -> Result<(), ValidationErrors> {
        self.validate()
    }

    /// Hook for model-specific business-rule validation.
    fn custom_validations(&self) -> Result<(), ValidationErrors> {
        Ok(())
    }

    /// Validate and return `self` for builder-style call chains.
    fn validated(self) -> Result<Self, ValidationErrors>
    where
        Self: Sized,
    {
        self.validate()?;
        Ok(self)
    }
}

/// Builder for creating validation rules programmatically
pub struct ValidationBuilder {
    field: String,
    rules: Vec<ValidationRule>,
}

impl ValidationBuilder {
    /// Start collecting rules for one field.
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            rules: vec![],
        }
    }

    /// Append the `Required` rule.
    pub fn required(mut self) -> Self {
        self.rules.push(ValidationRule::Required);
        self
    }

    /// Append the `Email` rule.
    pub fn email(mut self) -> Self {
        self.rules.push(ValidationRule::Email);
        self
    }

    /// Append the `Url` rule.
    pub fn url(mut self) -> Self {
        self.rules.push(ValidationRule::Url);
        self
    }

    /// Append a minimum-length rule.
    pub fn min_length(mut self, len: usize) -> Self {
        self.rules.push(ValidationRule::MinLength(len));
        self
    }

    /// Append a maximum-length rule.
    pub fn max_length(mut self, len: usize) -> Self {
        self.rules.push(ValidationRule::MaxLength(len));
        self
    }

    /// Append an exact-length rule.
    pub fn length(mut self, len: usize) -> Self {
        self.rules.push(ValidationRule::Length(len));
        self
    }

    /// Append a minimum-value rule.
    pub fn min(mut self, val: f64) -> Self {
        self.rules.push(ValidationRule::Min(val));
        self
    }

    /// Append a maximum-value rule.
    pub fn max(mut self, val: f64) -> Self {
        self.rules.push(ValidationRule::Max(val));
        self
    }

    /// Append an inclusive numeric range rule.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.rules.push(ValidationRule::Range(min, max));
        self
    }

    /// Append a regex-match rule.
    pub fn regex(mut self, pattern: impl Into<String>) -> Self {
        self.rules.push(ValidationRule::Regex(pattern.into()));
        self
    }

    /// Append an alphabetic-only rule.
    pub fn alpha(mut self) -> Self {
        self.rules.push(ValidationRule::Alpha);
        self
    }

    /// Append an alphanumeric-only rule.
    pub fn alphanumeric(mut self) -> Self {
        self.rules.push(ValidationRule::Alphanumeric);
        self
    }

    /// Append a numeric-string rule.
    pub fn numeric(mut self) -> Self {
        self.rules.push(ValidationRule::Numeric);
        self
    }

    /// Append a UUID-shape rule.
    pub fn uuid(mut self) -> Self {
        self.rules.push(ValidationRule::Uuid);
        self
    }

    /// Append an allowed-values rule.
    pub fn in_list(mut self, values: Vec<impl Into<String>>) -> Self {
        self.rules.push(ValidationRule::In(
            values.into_iter().map(|v| v.into()).collect(),
        ));
        self
    }

    /// Append a disallowed-values rule.
    pub fn not_in(mut self, values: Vec<impl Into<String>>) -> Self {
        self.rules.push(ValidationRule::NotIn(
            values.into_iter().map(|v| v.into()).collect(),
        ));
        self
    }

    /// Add a custom validation marker.
    ///
    /// The message is surfaced when your model's `custom_validations()` adds an
    /// error for the field; it is not evaluated directly by `ValidationBuilder`.
    pub fn custom(mut self, message: impl Into<String>) -> Self {
        self.rules.push(ValidationRule::Custom(message.into()));
        self
    }

    /// Finish the builder and return the field plus its rules.
    pub fn build(self) -> (String, Vec<ValidationRule>) {
        (self.field, self.rules)
    }
}

#[cfg(test)]
#[path = "../tests/unit/validation_tests.rs"]
mod tests;
