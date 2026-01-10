//! File Attachments System
//!
//! This module provides file attachment functionality for TideORM models.
//!
//! ## Overview
//!
//! File attachments allow you to associate files with your models using a JSONB column.
//! Files can be attached as:
//! - **HasOne**: Single file attachment (e.g., `thumbnail`, `avatar`)
//! - **HasMany**: Multiple file attachments (e.g., `images`, `documents`)
//!
//! ## Setup
//!
//! Add a `files` JSONB column to your table and use the `#[tide(has_one_file)]` or
//! `#[tide(has_many_files)]` attributes:
//!
//! ```rust,ignore
//! #[derive(Model, Clone, Debug, Serialize, Deserialize)]
//! #[tide(table = "products")]
//! #[tide(has_one_file = "thumbnail")]
//! #[tide(has_many_files = "images,documents")]
//! pub struct Product {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub name: String,
//!     pub files: Option<Json>,  // JSONB column storing attachments
//! }
//! ```
//!
//! ## Usage
//!
//! ### Attaching Files
//!
//! ```rust,ignore
//! // Attach a single file (hasOne)
//! product.attach("thumbnail", "uploads/thumb.jpg")?;
//!
//! // Attach multiple files (hasMany)
//! product.attach("images", "uploads/img1.jpg")?;
//! product.attach("images", "uploads/img2.jpg")?;
//!
//! // Or attach multiple at once
//! product.attach_many("images", vec!["uploads/img3.jpg", "uploads/img4.jpg"])?;
//!
//! // Save the model to persist changes
//! product.update().await?;
//! ```
//!
//! ### Detaching Files
//!
//! ```rust,ignore
//! // Detach a specific file
//! product.detach("images", Some("uploads/img1.jpg"))?;
//!
//! // Detach all files from a relation
//! product.detach("images", None)?;
//!
//! product.update().await?;
//! ```
//!
//! ### Syncing Files (Replace All)
//!
//! ```rust,ignore
//! // Replace all images with new ones
//! product.sync("images", vec!["uploads/new1.jpg", "uploads/new2.jpg"])?;
//!
//! // Clear all images
//! product.sync("images", vec![])?;
//!
//! product.update().await?;
//! ```
//!
//! ## File Metadata
//!
//! Each attachment stores metadata:
//! - `key`: The file path/key
//! - `filename`: Extracted filename
//! - `created_at`: Timestamp when attached
//! - Additional fields can be added via `attach_with_metadata`

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
        attachment.original_filename = original_filename.map(|s| s.to_string());
        attachment.size = size;
        attachment.mime_type = mime_type.map(|s| s.to_string());
        attachment
    }
    
    /// Add custom metadata
    pub fn add_metadata(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.metadata.insert(key.to_string(), value.into());
        self
    }
    
    /// Convert to JSON value
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::json!({}))
    }
}

/// Trait for models with file attachments
///
/// This trait is automatically implemented for models with file attachment configuration.
/// You can also implement it manually for custom behavior.
pub trait HasAttachments {
    /// Get the list of hasOne file relations
    fn has_one_files() -> Vec<&'static str>;
    
    /// Get the list of hasMany file relations
    fn has_many_files() -> Vec<&'static str>;
    
    /// Get all file relation names
    fn all_file_relations() -> Vec<&'static str> {
        let mut relations = Self::has_one_files();
        relations.extend(Self::has_many_files());
        relations
    }
    
    /// Check if a relation is hasOne
    fn is_has_one_relation(relation: &str) -> bool {
        Self::has_one_files().contains(&relation)
    }
    
    /// Check if a relation is hasMany
    fn is_has_many_relation(relation: &str) -> bool {
        Self::has_many_files().contains(&relation)
    }
    
    /// Get the current files data from the model
    fn get_files_data(&self) -> Result<FilesData, AttachmentError>;
    
    /// Set the files data on the model
    fn set_files_data(&mut self, data: FilesData) -> Result<(), AttachmentError>;
    
    // =========================================================================
    // ATTACH METHODS
    // =========================================================================
    
    /// Attach a file to a relation
    ///
    /// For hasOne relations, this replaces any existing attachment.
    /// For hasMany relations, this adds to the existing attachments.
    ///
    /// # Example
    /// ```rust,ignore
    /// // Attach a thumbnail (hasOne)
    /// product.attach("thumbnail", "uploads/thumb.jpg")?;
    ///
    /// // Attach an image (hasMany)
    /// product.attach("images", "uploads/img1.jpg")?;
    /// ```
    fn attach(&mut self, relation: &str, file_key: &str) -> Result<(), AttachmentError> {
        self.attach_with_metadata(relation, FileAttachment::new(file_key))
    }
    
    /// Attach a file with custom metadata
    ///
    /// # Example
    /// ```rust,ignore
    /// let attachment = FileAttachment::with_metadata(
    ///     "uploads/document.pdf",
    ///     Some("My Document.pdf"),
    ///     Some(1024 * 1024), // 1MB
    ///     Some("application/pdf"),
    /// );
    /// product.attach_with_metadata("documents", attachment)?;
    /// ```
    fn attach_with_metadata(&mut self, relation: &str, attachment: FileAttachment) -> Result<(), AttachmentError> {
        self.validate_relation(relation)?;
        
        let mut files = self.get_files_data()?;
        
        if Self::is_has_one_relation(relation) {
            files.set_one(relation, attachment);
        } else {
            files.add_many(relation, attachment);
        }
        
        self.set_files_data(files)
    }
    
    /// Attach multiple files at once (hasMany only)
    ///
    /// # Example
    /// ```rust,ignore
    /// product.attach_many("images", vec!["img1.jpg", "img2.jpg", "img3.jpg"])?;
    /// ```
    fn attach_many(&mut self, relation: &str, file_keys: Vec<&str>) -> Result<(), AttachmentError> {
        if !Self::is_has_many_relation(relation) {
            return Err(AttachmentError::InvalidRelation(
                format!("'{}' is not a hasMany relation, use attach() instead", relation)
            ));
        }
        
        let mut files = self.get_files_data()?;
        
        for key in file_keys {
            files.add_many(relation, FileAttachment::new(key));
        }
        
        self.set_files_data(files)
    }
    
    // =========================================================================
    // DETACH METHODS
    // =========================================================================
    
    /// Detach a file from a relation
    ///
    /// For hasOne, pass `None` as `file_key` to remove the attachment.
    /// For hasMany, pass `Some(key)` to remove a specific file, or `None` to remove all.
    ///
    /// # Example
    /// ```rust,ignore
    /// // Remove thumbnail (hasOne)
    /// product.detach("thumbnail", None)?;
    ///
    /// // Remove specific image (hasMany)
    /// product.detach("images", Some("uploads/img1.jpg"))?;
    ///
    /// // Remove all images (hasMany)
    /// product.detach("images", None)?;
    /// ```
    fn detach(&mut self, relation: &str, file_key: Option<&str>) -> Result<(), AttachmentError> {
        self.validate_relation(relation)?;
        
        let mut files = self.get_files_data()?;
        
        if Self::is_has_one_relation(relation) {
            files.remove_one(relation);
        } else if let Some(key) = file_key {
            files.remove_from_many(relation, key);
        } else {
            files.clear_many(relation);
        }
        
        self.set_files_data(files)
    }
    
    /// Detach multiple files at once (hasMany only)
    ///
    /// # Example
    /// ```rust,ignore
    /// product.detach_many("images", vec!["img1.jpg", "img2.jpg"])?;
    /// ```
    fn detach_many(&mut self, relation: &str, file_keys: Vec<&str>) -> Result<(), AttachmentError> {
        if !Self::is_has_many_relation(relation) {
            return Err(AttachmentError::InvalidRelation(
                format!("'{}' is not a hasMany relation", relation)
            ));
        }
        
        let mut files = self.get_files_data()?;
        
        for key in file_keys {
            files.remove_from_many(relation, key);
        }
        
        self.set_files_data(files)
    }
    
    // =========================================================================
    // SYNC METHODS
    // =========================================================================
    
    /// Sync files for a relation (replace all existing with new ones)
    ///
    /// This is useful when you have a form with file inputs and want to
    /// replace the entire collection.
    ///
    /// # Example
    /// ```rust,ignore
    /// // Replace all images with new ones
    /// product.sync("images", vec!["new1.jpg", "new2.jpg"])?;
    ///
    /// // Clear all images
    /// product.sync("images", vec![])?;
    ///
    /// // Replace thumbnail
    /// product.sync("thumbnail", vec!["new-thumb.jpg"])?;
    /// ```
    fn sync(&mut self, relation: &str, file_keys: Vec<&str>) -> Result<(), AttachmentError> {
        self.validate_relation(relation)?;
        
        let mut files = self.get_files_data()?;
        
        if Self::is_has_one_relation(relation) {
            if file_keys.is_empty() {
                files.remove_one(relation);
            } else {
                files.set_one(relation, FileAttachment::new(file_keys[0]));
            }
        } else {
            // Clear and add new ones
            files.clear_many(relation);
            for key in file_keys {
                files.add_many(relation, FileAttachment::new(key));
            }
        }
        
        self.set_files_data(files)
    }
    
    /// Sync files with custom metadata
    ///
    /// # Example
    /// ```rust,ignore
    /// let attachments = vec![
    ///     FileAttachment::with_metadata("img1.jpg", Some("Photo 1"), Some(1024), Some("image/jpeg")),
    ///     FileAttachment::with_metadata("img2.jpg", Some("Photo 2"), Some(2048), Some("image/jpeg")),
    /// ];
    /// product.sync_with_metadata("images", attachments)?;
    /// ```
    fn sync_with_metadata(&mut self, relation: &str, attachments: Vec<FileAttachment>) -> Result<(), AttachmentError> {
        self.validate_relation(relation)?;
        
        let mut files = self.get_files_data()?;
        
        if Self::is_has_one_relation(relation) {
            if attachments.is_empty() {
                files.remove_one(relation);
            } else {
                files.set_one(relation, attachments.into_iter().next().unwrap());
            }
        } else {
            files.clear_many(relation);
            for attachment in attachments {
                files.add_many(relation, attachment);
            }
        }
        
        self.set_files_data(files)
    }
    
    // =========================================================================
    // GETTER METHODS
    // =========================================================================
    
    /// Get a single file attachment (hasOne)
    ///
    /// # Example
    /// ```rust,ignore
    /// if let Some(thumb) = product.get_file("thumbnail")? {
    ///     println!("Thumbnail: {}", thumb.key);
    /// }
    /// ```
    fn get_file(&self, relation: &str) -> Result<Option<FileAttachment>, AttachmentError> {
        let files = self.get_files_data()?;
        Ok(files.get_one(relation))
    }
    
    /// Get multiple file attachments (hasMany)
    ///
    /// # Example
    /// ```rust,ignore
    /// let images = product.get_files("images")?;
    /// for img in images {
    ///     println!("Image: {}", img.key);
    /// }
    /// ```
    fn get_files(&self, relation: &str) -> Result<Vec<FileAttachment>, AttachmentError> {
        let files = self.get_files_data()?;
        Ok(files.get_many(relation))
    }
    
    /// Check if a relation has any files
    fn has_files(&self, relation: &str) -> Result<bool, AttachmentError> {
        let files = self.get_files_data()?;
        Ok(files.has_files(relation))
    }
    
    /// Count files in a relation
    fn count_files(&self, relation: &str) -> Result<usize, AttachmentError> {
        let files = self.get_files_data()?;
        Ok(files.count_files(relation))
    }
    
    // =========================================================================
    // HELPER METHODS
    // =========================================================================
    
    /// Validate that a relation exists
    fn validate_relation(&self, relation: &str) -> Result<(), AttachmentError> {
        if !Self::all_file_relations().contains(&relation) {
            return Err(AttachmentError::InvalidRelation(
                format!("Unknown file relation: '{}'. Available: {:?}", relation, Self::all_file_relations())
            ));
        }
        Ok(())
    }
}

/// Container for all file attachments on a model
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FilesData {
    /// Map of relation name to file data
    #[serde(flatten)]
    data: HashMap<String, serde_json::Value>,
}

impl FilesData {
    /// Create empty files data
    pub fn new() -> Self {
        Self { data: HashMap::new() }
    }
    
    /// Create from JSON value
    pub fn from_json(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Object(map) => {
                let data: HashMap<String, serde_json::Value> = map
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                Self { data }
            }
            _ => Self::new(),
        }
    }
    
    /// Convert to JSON value
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(&self.data).unwrap_or(serde_json::json!({}))
    }
    
    /// Set a single file (hasOne)
    pub fn set_one(&mut self, relation: &str, attachment: FileAttachment) {
        self.data.insert(relation.to_string(), attachment.to_json());
    }
    
    /// Remove a single file (hasOne)
    pub fn remove_one(&mut self, relation: &str) {
        self.data.insert(relation.to_string(), serde_json::Value::Null);
    }
    
    /// Get a single file (hasOne)
    pub fn get_one(&self, relation: &str) -> Option<FileAttachment> {
        self.data.get(relation)
            .filter(|v| !v.is_null())
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
    
    /// Add to file array (hasMany)
    pub fn add_many(&mut self, relation: &str, attachment: FileAttachment) {
        let mut array = self.get_many_raw(relation);
        array.push(attachment.to_json());
        self.data.insert(relation.to_string(), serde_json::Value::Array(array));
    }
    
    /// Remove from file array (hasMany)
    pub fn remove_from_many(&mut self, relation: &str, file_key: &str) {
        let array: Vec<serde_json::Value> = self.get_many_raw(relation)
            .into_iter()
            .filter(|item| {
                item.get("key").and_then(|k| k.as_str()) != Some(file_key)
            })
            .collect();
        self.data.insert(relation.to_string(), serde_json::Value::Array(array));
    }
    
    /// Clear all files from array (hasMany)
    pub fn clear_many(&mut self, relation: &str) {
        self.data.insert(relation.to_string(), serde_json::Value::Array(vec![]));
    }
    
    /// Get all files from array (hasMany)
    pub fn get_many(&self, relation: &str) -> Vec<FileAttachment> {
        self.get_many_raw(relation)
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect()
    }
    
    fn get_many_raw(&self, relation: &str) -> Vec<serde_json::Value> {
        self.data.get(relation)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    }
    
    /// Check if relation has files
    pub fn has_files(&self, relation: &str) -> bool {
        match self.data.get(relation) {
            Some(serde_json::Value::Null) => false,
            Some(serde_json::Value::Array(arr)) => !arr.is_empty(),
            Some(serde_json::Value::Object(_)) => true,
            _ => false,
        }
    }
    
    /// Count files in relation
    pub fn count_files(&self, relation: &str) -> usize {
        match self.data.get(relation) {
            Some(serde_json::Value::Null) => 0,
            Some(serde_json::Value::Array(arr)) => arr.len(),
            Some(serde_json::Value::Object(_)) => 1,
            _ => 0,
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_file_attachment_creation() {
        let attachment = FileAttachment::new("uploads/2024/01/image.jpg");
        assert_eq!(attachment.key, "uploads/2024/01/image.jpg");
        assert_eq!(attachment.filename, "image.jpg");
    }
    
    #[test]
    fn test_file_attachment_with_metadata() {
        let attachment = FileAttachment::with_metadata(
            "uploads/doc.pdf",
            Some("My Document.pdf"),
            Some(1024),
            Some("application/pdf"),
        );
        assert_eq!(attachment.original_filename, Some("My Document.pdf".to_string()));
        assert_eq!(attachment.size, Some(1024));
        assert_eq!(attachment.mime_type, Some("application/pdf".to_string()));
    }
    
    #[test]
    fn test_files_data_has_one() {
        let mut files = FilesData::new();
        files.set_one("thumbnail", FileAttachment::new("thumb.jpg"));
        
        assert!(files.has_files("thumbnail"));
        assert_eq!(files.count_files("thumbnail"), 1);
        
        let thumb = files.get_one("thumbnail").unwrap();
        assert_eq!(thumb.key, "thumb.jpg");
        
        files.remove_one("thumbnail");
        assert!(!files.has_files("thumbnail"));
    }
    
    #[test]
    fn test_files_data_has_many() {
        let mut files = FilesData::new();
        files.add_many("images", FileAttachment::new("img1.jpg"));
        files.add_many("images", FileAttachment::new("img2.jpg"));
        
        assert!(files.has_files("images"));
        assert_eq!(files.count_files("images"), 2);
        
        let images = files.get_many("images");
        assert_eq!(images.len(), 2);
        
        files.remove_from_many("images", "img1.jpg");
        assert_eq!(files.count_files("images"), 1);
        
        files.clear_many("images");
        assert!(!files.has_files("images"));
    }
}
