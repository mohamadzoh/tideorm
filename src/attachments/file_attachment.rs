use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// File attachment metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAttachment {
    /// The file key/path (e.g., "uploads/2024/01/image.jpg")
    pub key: String,

    /// The filename (extracted from key)
    pub filename: String,

    /// When the file was attached
    pub created_at: String,

    /// Original filename (if different from key)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_filename: Option<String>,

    /// File size in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,

    /// MIME type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    /// Custom metadata
    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl FileAttachment {
    /// Create a new file attachment from a key
    pub fn new(key: &str) -> Self {
        let filename = key.split('/').next_back().unwrap_or(key).to_string();
        Self {
            key: key.to_string(),
            filename,
            created_at: chrono::Utc::now().to_rfc3339(),
            original_filename: None,
            size: None,
            mime_type: None,
            metadata: HashMap::new(),
        }
    }

    /// Create with additional metadata
    pub fn with_metadata(
        key: &str,
        original_filename: Option<&str>,
        size: Option<u64>,
        mime_type: Option<&str>,
    ) -> Self {
        let mut attachment = Self::new(key);
        attachment.original_filename = original_filename.map(|value| value.to_string());
        attachment.size = size;
        attachment.mime_type = mime_type.map(|value| value.to_string());
        attachment
    }

    /// Add custom metadata
    pub fn add_metadata(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.metadata.insert(key.to_string(), value.into());
        self
    }

    /// Generate a public URL using the global file URL generator.
    ///
    /// Use `url_with_generator()` when one call site needs different URL rules
    /// without changing the process-wide configuration.
    #[inline]
    pub fn url(&self, field_name: &str) -> String {
        crate::config::Config::generate_file_url(field_name, self)
    }

    /// Generate a public URL using a one-off generator function.
    #[inline]
    pub fn url_with_generator(
        &self,
        field_name: &str,
        generator: crate::config::FileUrlGenerator,
    ) -> String {
        generator(field_name, self)
    }

    /// Convert to JSON value
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::json!({}))
    }
}