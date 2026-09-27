use thiserror::Error;

/// Errors that can occur during attachment operations
#[derive(Debug, Clone, Error)]
pub enum AttachmentError {
    /// Invalid or unknown relation name
    #[error("Invalid relation: {0}")]
    InvalidRelation(String),
}

impl From<AttachmentError> for crate::Error {
    fn from(err: AttachmentError) -> Self {
        crate::Error::query(err.to_string())
    }
}
