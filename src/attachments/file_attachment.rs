use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
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
///
/// It serializes as one JSON object: the fields below and, beside them, each
/// [`metadata`](Self::metadata) entry. An entry named like a field that is set
/// (`key`, `size`, ..) is left out, since the field owns that name; one named
/// like an unset optional field is kept, and reads back as metadata when it is
/// not that field's type, so `add_metadata("size", "12KB")` round-trips.
#[derive(Debug, Clone)]
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
    pub original_filename: Option<String>,

    /// File size in bytes
    pub size: Option<u64>,

    /// MIME type
    pub mime_type: Option<String>,

    /// Custom metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Serialize for FileAttachment {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("key", &self.key)?;
        map.serialize_entry("filename", &self.filename)?;
        map.serialize_entry("created_at", &self.created_at)?;
        let optional = [
            (
                "original_filename",
                self.original_filename
                    .as_deref()
                    .map(serde_json::Value::from),
            ),
            ("size", self.size.map(serde_json::Value::from)),
            (
                "mime_type",
                self.mime_type.as_deref().map(serde_json::Value::from),
            ),
        ];
        for (name, value) in &optional {
            if let Some(value) = value {
                map.serialize_entry(name, value)?;
            }
        }
        for (name, value) in &self.metadata {
            let owned_by_field = matches!(name.as_str(), "key" | "filename" | "created_at")
                || optional
                    .iter()
                    .any(|(field, value)| field == name && value.is_some());
            if !owned_by_field {
                map.serialize_entry(name, value)?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for FileAttachment {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut members = serde_json::Map::<String, serde_json::Value>::deserialize(deserializer)?;
        // A member of the field's type is the field; anything else stays
        // metadata. A `null` is an unset optional field.
        let mut text = |name: &str| match members.get(name) {
            Some(serde_json::Value::String(_)) => members
                .remove(name)
                .and_then(|value| value.as_str().map(str::to_string)),
            Some(serde_json::Value::Null) => {
                members.remove(name);
                None
            }
            _ => None,
        };
        let required = |value: Option<String>, name: &'static str| {
            value.ok_or_else(|| serde::de::Error::missing_field(name))
        };
        let key = required(text("key"), "key")?;
        let filename = required(text("filename"), "filename")?;
        let created_at = required(text("created_at"), "created_at")?;
        let original_filename = text("original_filename");
        let mime_type = text("mime_type");
        let size = match members.get("size") {
            Some(value) if value.is_u64() || value.is_null() => {
                members.remove("size").and_then(|value| value.as_u64())
            }
            _ => None,
        };

        Ok(Self {
            key,
            filename,
            created_at,
            original_filename,
            size,
            mime_type,
            metadata: members.into_iter().collect(),
        })
    }
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
        crate::config::Config::get_file_url_generator()(field_name, self)
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
