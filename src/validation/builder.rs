use std::fmt;

use super::{ValidatableValue, ValidationErrors, ValidationRule};

/// Builder for creating validation rules programmatically
///
/// ```
/// use tideorm::validation::ValidationBuilder;
///
/// let username = ValidationBuilder::new("username")
///     .required()
///     .min_length(3)
///     .alphanumeric();
/// let errors = username.validate(&"a!".to_string()).unwrap_err();
/// assert_eq!(errors.field_errors("username").len(), 2);
///
/// let age = ValidationBuilder::new("age").range(18, 120);
/// assert!(age.validate(&18_i64).is_ok());
/// ```
#[derive(Debug, Clone)]
pub struct ValidationBuilder {
    field: String,
    rules: Vec<ValidationRule>,
}

impl ValidationBuilder {
    /// Start collecting rules for one field.
    #[must_use]
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            rules: vec![],
        }
    }

    /// Append the `Required` rule.
    #[must_use]
    pub fn required(mut self) -> Self {
        self.rules.push(ValidationRule::Required);
        self
    }

    /// Append the `Email` rule.
    #[must_use]
    pub fn email(mut self) -> Self {
        self.rules.push(ValidationRule::Email);
        self
    }

    /// Append the `Url` rule.
    #[must_use]
    pub fn url(mut self) -> Self {
        self.rules.push(ValidationRule::Url);
        self
    }

    /// Append a minimum-length rule.
    #[must_use]
    pub fn min_length(mut self, len: usize) -> Self {
        self.rules.push(ValidationRule::MinLength(len));
        self
    }

    /// Append a maximum-length rule.
    #[must_use]
    pub fn max_length(mut self, len: usize) -> Self {
        self.rules.push(ValidationRule::MaxLength(len));
        self
    }

    /// Append an exact-length rule.
    #[must_use]
    pub fn length(mut self, len: usize) -> Self {
        self.rules.push(ValidationRule::Length(len));
        self
    }

    /// Append a minimum-value rule.
    ///
    /// The bound keeps the exact digits it is written with, as
    /// `#[validate(min = ..)]` does: `min(18)` compares an integer exactly,
    /// and a `Decimal` or `"0.10"` bound compares a `Decimal` without rounding.
    #[must_use]
    pub fn min(mut self, bound: impl fmt::Display) -> Self {
        self.rules.push(ValidationRule::MinExact(bound.to_string()));
        self
    }

    /// Append a maximum-value rule, its bound kept as [`min`](Self::min) keeps it.
    #[must_use]
    pub fn max(mut self, bound: impl fmt::Display) -> Self {
        self.rules.push(ValidationRule::MaxExact(bound.to_string()));
        self
    }

    /// Append an inclusive numeric range rule, its bounds kept as
    /// [`min`](Self::min) keeps them.
    #[must_use]
    pub fn range(mut self, min: impl fmt::Display, max: impl fmt::Display) -> Self {
        self.rules
            .push(ValidationRule::RangeExact(min.to_string(), max.to_string()));
        self
    }

    /// Append a regex-match rule.
    #[must_use]
    pub fn regex(mut self, pattern: impl Into<String>) -> Self {
        self.rules.push(ValidationRule::Regex(pattern.into()));
        self
    }

    /// Append an alphabetic-only rule.
    #[must_use]
    pub fn alpha(mut self) -> Self {
        self.rules.push(ValidationRule::Alpha);
        self
    }

    /// Append an alphanumeric-only rule.
    #[must_use]
    pub fn alphanumeric(mut self) -> Self {
        self.rules.push(ValidationRule::Alphanumeric);
        self
    }

    /// Append a numeric-string rule.
    #[must_use]
    pub fn numeric(mut self) -> Self {
        self.rules.push(ValidationRule::Numeric);
        self
    }

    /// Append a UUID-shape rule.
    #[must_use]
    pub fn uuid(mut self) -> Self {
        self.rules.push(ValidationRule::Uuid);
        self
    }

    /// Append an allowed-values rule.
    #[must_use]
    pub fn in_list(mut self, values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.rules.push(ValidationRule::In(
            values.into_iter().map(Into::into).collect(),
        ));
        self
    }

    /// Append a disallowed-values rule.
    #[must_use]
    pub fn not_in(mut self, values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.rules.push(ValidationRule::NotIn(
            values.into_iter().map(Into::into).collect(),
        ));
        self
    }

    /// Apply every rule to `value`, returning all failures together under
    /// the builder's field.
    pub fn validate<T: ValidatableValue>(&self, value: &T) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        errors.check(&self.field, value, &self.rules);
        errors.to_result()
    }

    /// Finish the builder and return the field plus its rules.
    #[must_use]
    pub fn build(self) -> (String, Vec<ValidationRule>) {
        (self.field, self.rules)
    }
}
