/// Errors that can occur during attachment operations
#[derive(Debug, Clone)]
pub enum AttachmentError {
    /// Invalid or unknown relation name
    InvalidRelation(String),
    /// Failed to parse files data
    ParseError(String),
    /// Model doesn't support file attachments
    NotSupported,
}

impl std::fmt::Display for AttachmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttachmentError::InvalidRelation(msg) => write!(f, "Invalid relation: {}", msg),
            AttachmentError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            AttachmentError::NotSupported => write!(f, "Model does not support file attachments"),
        }
    }
}

impl std::error::Error for AttachmentError {}

impl From<AttachmentError> for crate::Error {
    fn from(err: AttachmentError) -> Self {
        crate::Error::query(err.to_string())
    }
}