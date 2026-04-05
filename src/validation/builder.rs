use super::ValidationRule;

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
