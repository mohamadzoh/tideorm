use thiserror::Error;

/// Errors that can occur during translation operations.
#[derive(Debug, Clone, Error)]
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

impl From<TranslationError> for crate::Error {
    fn from(err: TranslationError) -> Self {
        crate::Error::query(err.to_string())
    }
}
