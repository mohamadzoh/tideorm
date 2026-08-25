use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// File attachment metadata
///
/// # Trust boundary
///
/// TideORM stores the `key` as opaque text and never opens, reads, or writes the
/// file it names. Everything that resolves a key into something real — an object
/// store lookup, a filesystem path, a redirect target — happens in caller-supplied
/// code: a storage backend, or the [`FileUrlGenerator`](crate::config::FileUrlGenerator)
/// behind [`FileAttachment::url`].
///
/// **Validating keys is therefore the caller's job.** A key that arrives from an
/// upload handler is untrusted input: `attach("avatar", "../../etc/passwd")` is
/// stored verbatim, and `url()` joins it onto the configured base URL verbatim.
/// Screen keys with [`FileAttachment::is_safe_key`] at the boundary where they
/// enter the system, or generate keys server-side and never accept them from the
/// client at all.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAttachment {
    /// The file key/path (e.g., "uploads/2024/01/image.jpg")
    ///
    /// Stored verbatim and never validated on load; see the type-level
    /// trust-boundary note.
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
    ///
    /// The key is accepted as-is. Check it with [`FileAttachment::is_safe_key`]
    /// first when it came from outside the application.
    pub fn new(key: &str) -> Self {
        let filename = key
            .split(['/', '\\'])
            .next_back()
            .unwrap_or(key)
            .to_string();
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

    /// Report whether a key is a plain relative storage key.
    ///
    /// Call this where an untrusted key enters the application, before handing it
    /// to [`FileAttachment::new`] or `HasAttachments::attach`. TideORM does not
    /// call it for you: existing applications legitimately store keys this check
    /// rejects, so enforcing it inside `attach()` would be a breaking change.
    ///
    /// A key is rejected when it
    /// - is empty or only whitespace,
    /// - contains a NUL byte,
    /// - is absolute (`/x` or `\x`),
    /// - contains `//`, which covers `scheme://host` and protocol-relative URLs,
    /// - contains `:`, which covers Windows drive prefixes such as `C:\x` and
    ///   `C:x`, alternate data streams, and remaining URL schemes,
    /// - or carries a `..` path segment.
    ///
    /// Both `/` and `\` count as separators, because a backend resolving the key
    /// on Windows treats them alike.
    ///
    /// Keys that pass are only *shaped* like safe keys. The storage backend
    /// remains responsible for confining them to the intended prefix and for
    /// authorizing the access.
    pub fn is_safe_key(key: &str) -> bool {
        if key.trim().is_empty() || key.contains('\0') || key.contains(':') || key.contains("//") {
            return false;
        }

        if key.starts_with('/') || key.starts_with('\\') {
            return false;
        }

        !key.split(['/', '\\']).any(|segment| segment == "..")
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
    ///
    /// The default generator concatenates the configured base URL and the stored
    /// key with no escaping and no traversal check, so a key that was never
    /// screened by [`FileAttachment::is_safe_key`] can point the URL outside the
    /// intended prefix or at another host entirely.
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
