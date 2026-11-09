use thiserror::Error;

/// Errors that can occur during translation operations.
///
/// Marked `#[non_exhaustive]`: match with a wildcard arm.
#[derive(Debug, Clone, Error)]
#[non_exhaustive]
pub enum TranslationError {
    /// Invalid or non-translatable field
    #[error("Invalid field: {0}")]
    InvalidField(String),
    /// Invalid or disallowed language
    #[error("Invalid language: {0}")]
    InvalidLanguage(String),
    /// Failed to parse translations data
    #[error("Parse error: {0}")]
    ParseError(String),
}

/// A field or language the model does not allow is a
/// [`Validation`](crate::Error::Validation) error naming `field` or
/// `language`, since the keys usually come from a request; data that does not
/// parse is a [`Conversion`](crate::Error::Conversion) error.
impl From<TranslationError> for crate::Error {
    fn from(err: TranslationError) -> Self {
        match err {
            TranslationError::InvalidField(message) => crate::Error::validation("field", message),
            TranslationError::InvalidLanguage(message) => {
                crate::Error::validation("language", message)
            }
            TranslationError::ParseError(_) => crate::Error::conversion(err.to_string()),
        }
    }
}
