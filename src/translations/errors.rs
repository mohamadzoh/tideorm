/// Errors that can occur during translation operations.
#[derive(Debug, Clone)]
pub enum TranslationError {
    /// Invalid or non-translatable field
    InvalidField(String),
    /// Invalid or disallowed language
    InvalidLanguage(String),
    /// Failed to parse translations data
    ParseError(String),
    /// Model doesn't support translations
    NotSupported,
}

impl std::fmt::Display for TranslationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TranslationError::InvalidField(msg) => write!(f, "Invalid field: {}", msg),
            TranslationError::InvalidLanguage(msg) => write!(f, "Invalid language: {}", msg),
            TranslationError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            TranslationError::NotSupported => write!(f, "Model does not support translations"),
        }
    }
}

impl std::error::Error for TranslationError {}

impl From<TranslationError> for crate::Error {
    fn from(err: TranslationError) -> Self {
        crate::Error::query(err.to_string())
    }
}
