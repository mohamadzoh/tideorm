use thiserror::Error;

/// Errors that can occur during attachment operations
///
/// Marked `#[non_exhaustive]`: match with a wildcard arm.
#[derive(Debug, Clone, Error)]
#[non_exhaustive]
pub enum AttachmentError {
    /// Stored attachment data could not be decoded.
    #[error("Invalid attachment data: {0}")]
    InvalidData(String),
    /// Invalid or unknown relation name
    #[error("Invalid relation: {0}")]
    InvalidRelation(String),
}

/// Undecodable stored data is a [`Conversion`](crate::Error::Conversion)
/// error, an unknown relation a [`Validation`](crate::Error::Validation) one
/// naming `relation`.
impl From<AttachmentError> for crate::Error {
    fn from(err: AttachmentError) -> Self {
        match err {
            AttachmentError::InvalidData(_) => crate::Error::conversion(err.to_string()),
            AttachmentError::InvalidRelation(message) => {
                crate::Error::validation("relation", message)
            }
        }
    }
}
