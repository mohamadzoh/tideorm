//! Model Validation System
//!
//! This module provides a validation system for TideORM models.
//!
//! ## Built-in Validation Rules
//!
//! - `required` - Field must not be empty
//! - `email` - Must be a valid email address
//! - `url` - Must be a valid URL
//! - `min_length` - Minimum string length
//! - `max_length` - Maximum string length
//! - `min` - Minimum numeric value
//! - `max` - Maximum numeric value
//! - `range` - Value must be within a range
//! - `regex` - Must match a regular expression
//! - `alpha` - Must contain only letters
//! - `alphanumeric` - Must contain only letters and numbers
//! - `numeric` - Must be a number
//! - `uuid` - Must be a valid UUID
//! - `custom` - Custom validation function
//!
//! ## Usage
//!
//! ```ignore
//! use tideorm::prelude::*;
//! use tideorm::validation::{Validate, ValidationRule};
//!
//! #[derive(Model)]
//! #[tide(table = "users")]
//! pub struct User {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     
//!     #[validate(email)]
//!     pub email: String,
//!     
//!     #[validate(min_length = 2, max_length = 100)]
//!     pub name: String,
//!     
//!     #[validate(min = 0, max = 150)]
//!     pub age: i32,
//! }
//!
//! // Validation is automatic on save/update, or call manually:
//! let user = User { ... };
//! user.validate()?;  // Returns Result<(), ValidationErrors>
//!
//! // Or get all errors:
//! match user.validate_all() {
//!     Ok(()) => println!("Valid!"),
//!     Err(errors) => {
//!         for (field, messages) in errors.iter() {
//!             println!("{}: {:?}", field, messages);
//!         }
//!     }
//! }
//! ```
//!
//! ## Custom Validation
//!
//! ```ignore
//! impl Validate for User {
//!     fn custom_validations(&self) -> Result<(), ValidationErrors> {
//!         let mut errors = ValidationErrors::new();
//!         
//!         // Custom business logic
//!         if self.email.ends_with("@blocked.com") {
//!             errors.add("email", "This email domain is not allowed");
//!         }
//!         
//!         if errors.is_empty() {
//!             Ok(())
//!         } else {
//!             Err(errors)
//!         }
//!     }
//! }
//! ```

use std::collections::HashMap;
use std::fmt;

/// Collection of validation errors organized by field name
#[derive(Debug, Clone, Default)]
pub struct ValidationErrors {
    errors: HashMap<String, Vec<String>>,
}

impl ValidationErrors {
    /// Create a new empty ValidationErrors
    pub fn new() -> Self {
        Self {
            errors: HashMap::new(),
        }
    }

    /// Add an error message for a field
    pub fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.errors
            .entry(field.into())
            .or_default()
            .push(message.into());
    }

    /// Check if there are any errors
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }
    
    /// Check if there are any errors (alias for !is_empty())
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Get the number of fields with errors
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    /// Get errors for a specific field
    pub fn get(&self, field: &str) -> Option<&Vec<String>> {
        self.errors.get(field)
    }
    
    /// Get errors for a specific field (alias for get())
    pub fn field_errors(&self, field: &str) -> Vec<String> {
        self.errors.get(field).cloned().unwrap_or_default()
    }

    /// Get all errors
    pub fn all(&self) -> &HashMap<String, Vec<String>> {
        &self.errors
    }

    /// Iterate over all errors
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Vec<String>)> {
        self.errors.iter()
    }

    /// Get the first error message (useful for simple error display)
    pub fn first(&self) -> Option<(&String, &String)> {
        self.errors.iter().next().and_then(|(field, messages)| {
            messages.first().map(|msg| (field, msg))
        })
    }

    /// Get all error messages as a flat list
    pub fn messages(&self) -> Vec<String> {
        self.errors
            .iter()
            .flat_map(|(field, messages)| {
                messages.iter().map(move |msg| format!("{}: {}", field, msg))
            })
            .collect()
    }

    /// Merge another ValidationErrors into this one
    pub fn merge(&mut self, other: ValidationErrors) {
        for (field, messages) in other.errors {
            for message in messages {
                self.add(field.clone(), message);
            }
        }
    }

    /// Convert to a Result, returning Ok if empty
    pub fn to_result(self) -> Result<(), Self> {
        if self.is_empty() {
            Ok(())
        } else {
            Err(self)
        }
    }

    /// Get all errors as (field, message) pairs for backwards compatibility
    /// 
    /// Returns a flat list of all errors
    pub fn errors(&self) -> Vec<(String, String)> {
        self.errors
            .iter()
            .flat_map(|(field, messages)| {
                messages.iter().map(move |msg| (field.clone(), msg.clone()))
            })
            .collect()
    }

    /// Convert to a single Error (takes the first error) for backwards compatibility
    pub fn into_error(self) -> Option<crate::error::Error> {
        self.first().map(|(field, message)| {
            crate::error::Error::Validation {
                field: field.clone(),
                message: message.clone(),
            }
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
    /// Custom validation with message
    Custom(String),
}

impl ValidationRule {
    /// Get the error message for this rule
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
    
    /// Validate a value against this rule
    /// 
    /// Returns Ok(()) if the value passes validation, or Err with an error message
    pub fn validate<T: ValidatableValue>(&self, value: &T) -> Result<(), String> {
        match Validator::validate_rule(value, self, "field") {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// Trait for validatable values
pub trait ValidatableValue {
    /// Check if the value is empty (for Required validation)
    fn is_empty_value(&self) -> bool;
    
    /// Get the string representation for string validations
    fn as_str_value(&self) -> Option<&str>;
    
    /// Get the numeric value for numeric validations
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

impl_validatable_for_int!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64);

/// Validator for applying validation rules
pub struct Validator;

impl Validator {
    /// Validate a single value against a rule
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
                    if s.len() < *min {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::MaxLength(max) => {
                if let Some(s) = value.as_str_value() {
                    if s.len() > *max {
                        return Some(rule.message(field));
                    }
                }
            }
            ValidationRule::Length(len) => {
                if let Some(s) = value.as_str_value() {
                    if s.len() != *len {
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
                    if let Ok(re) = regex::Regex::new(pattern) {
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
            ValidationRule::Custom(msg) => {
                // Custom rules are handled at the model level
                return Some(msg.clone());
            }
        }
        None
    }

    /// Check if a string is a valid email address
    pub fn is_valid_email(s: &str) -> bool {
        // Simple email validation regex
        let email_regex = regex::Regex::new(
            r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$"
        ).unwrap();
        email_regex.is_match(s)
    }

    /// Check if a string is a valid URL
    pub fn is_valid_url(s: &str) -> bool {
        s.starts_with("http://") || s.starts_with("https://")
    }
}

/// Trait for models that can be validated
///
/// This trait is automatically implemented by the `#[derive(Model)]` macro
/// when validation attributes are present. You can also implement it manually
/// for custom validation logic.
pub trait Validate {
    /// Get the validation rules for this model
    fn validation_rules() -> Vec<(&'static str, Vec<ValidationRule>)> {
        vec![]
    }

    /// Validate all rules and return the first error
    fn validate(&self) -> Result<(), ValidationErrors>;

    /// Validate all rules and collect all errors
    fn validate_all(&self) -> Result<(), ValidationErrors> {
        self.validate()
    }

    /// Custom validations that can be overridden
    fn custom_validations(&self) -> Result<(), ValidationErrors> {
        Ok(())
    }

    /// Validate and return self, useful for chaining
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
    /// Create a new validation builder for a field
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            rules: vec![],
        }
    }

    /// Add required rule
    pub fn required(mut self) -> Self {
        self.rules.push(ValidationRule::Required);
        self
    }

    /// Add email rule
    pub fn email(mut self) -> Self {
        self.rules.push(ValidationRule::Email);
        self
    }

    /// Add URL rule
    pub fn url(mut self) -> Self {
        self.rules.push(ValidationRule::Url);
        self
    }

    /// Add minimum length rule
    pub fn min_length(mut self, len: usize) -> Self {
        self.rules.push(ValidationRule::MinLength(len));
        self
    }

    /// Add maximum length rule
    pub fn max_length(mut self, len: usize) -> Self {
        self.rules.push(ValidationRule::MaxLength(len));
        self
    }

    /// Add exact length rule
    pub fn length(mut self, len: usize) -> Self {
        self.rules.push(ValidationRule::Length(len));
        self
    }

    /// Add minimum value rule
    pub fn min(mut self, val: f64) -> Self {
        self.rules.push(ValidationRule::Min(val));
        self
    }

    /// Add maximum value rule
    pub fn max(mut self, val: f64) -> Self {
        self.rules.push(ValidationRule::Max(val));
        self
    }

    /// Add range rule
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.rules.push(ValidationRule::Range(min, max));
        self
    }

    /// Add regex rule
    pub fn regex(mut self, pattern: impl Into<String>) -> Self {
        self.rules.push(ValidationRule::Regex(pattern.into()));
        self
    }

    /// Add alpha rule
    pub fn alpha(mut self) -> Self {
        self.rules.push(ValidationRule::Alpha);
        self
    }

    /// Add alphanumeric rule
    pub fn alphanumeric(mut self) -> Self {
        self.rules.push(ValidationRule::Alphanumeric);
        self
    }

    /// Add numeric rule
    pub fn numeric(mut self) -> Self {
        self.rules.push(ValidationRule::Numeric);
        self
    }

    /// Add UUID rule
    pub fn uuid(mut self) -> Self {
        self.rules.push(ValidationRule::Uuid);
        self
    }

    /// Add "in" rule (must be one of the values)
    pub fn in_list(mut self, values: Vec<impl Into<String>>) -> Self {
        self.rules.push(ValidationRule::In(
            values.into_iter().map(|v| v.into()).collect(),
        ));
        self
    }

    /// Add "not in" rule (must not be one of the values)
    pub fn not_in(mut self, values: Vec<impl Into<String>>) -> Self {
        self.rules.push(ValidationRule::NotIn(
            values.into_iter().map(|v| v.into()).collect(),
        ));
        self
    }

    /// Add custom message rule
    pub fn custom(mut self, message: impl Into<String>) -> Self {
        self.rules.push(ValidationRule::Custom(message.into()));
        self
    }

    /// Build the field name and rules tuple
    pub fn build(self) -> (String, Vec<ValidationRule>) {
        (self.field, self.rules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_validation() {
        assert!(Validator::is_valid_email("test@example.com"));
        assert!(Validator::is_valid_email("user.name+tag@domain.co.uk"));
        assert!(!Validator::is_valid_email("invalid"));
        assert!(!Validator::is_valid_email("@example.com"));
        assert!(!Validator::is_valid_email("test@"));
    }

    #[test]
    fn test_url_validation() {
        assert!(Validator::is_valid_url("http://example.com"));
        assert!(Validator::is_valid_url("https://example.com/path?query=1"));
        assert!(!Validator::is_valid_url("example.com"));
        assert!(!Validator::is_valid_url("ftp://example.com"));
    }

    #[test]
    fn test_min_length() {
        let rule = ValidationRule::MinLength(3);
        assert!(Validator::validate_rule(&"ab".to_string(), &rule, "name").is_some());
        assert!(Validator::validate_rule(&"abc".to_string(), &rule, "name").is_none());
        assert!(Validator::validate_rule(&"abcd".to_string(), &rule, "name").is_none());
    }

    #[test]
    fn test_max_length() {
        let rule = ValidationRule::MaxLength(5);
        assert!(Validator::validate_rule(&"abc".to_string(), &rule, "name").is_none());
        assert!(Validator::validate_rule(&"abcde".to_string(), &rule, "name").is_none());
        assert!(Validator::validate_rule(&"abcdef".to_string(), &rule, "name").is_some());
    }

    #[test]
    fn test_range() {
        let rule = ValidationRule::Range(1.0, 10.0);
        assert!(Validator::validate_rule(&0, &rule, "age").is_some());
        assert!(Validator::validate_rule(&1, &rule, "age").is_none());
        assert!(Validator::validate_rule(&5, &rule, "age").is_none());
        assert!(Validator::validate_rule(&10, &rule, "age").is_none());
        assert!(Validator::validate_rule(&11, &rule, "age").is_some());
    }

    #[test]
    fn test_validation_errors() {
        let mut errors = ValidationErrors::new();
        assert!(errors.is_empty());

        errors.add("email", "Invalid email");
        errors.add("email", "Email already taken");
        errors.add("name", "Name is required");

        assert!(!errors.is_empty());
        assert_eq!(errors.len(), 2);
        assert_eq!(errors.get("email").unwrap().len(), 2);
        assert_eq!(errors.get("name").unwrap().len(), 1);

        let messages = errors.messages();
        assert_eq!(messages.len(), 3);
    }

    #[test]
    fn test_validation_builder() {
        let (field, rules) = ValidationBuilder::new("email")
            .required()
            .email()
            .max_length(255)
            .build();

        assert_eq!(field, "email");
        assert_eq!(rules.len(), 3);
    }
}
