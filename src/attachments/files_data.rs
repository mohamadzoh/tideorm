use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::FileAttachment;

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
        Self {
            data: HashMap::new(),
        }
    }

    /// Create from JSON value
    pub fn from_json(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Object(map) => {
                let data: HashMap<String, serde_json::Value> =
                    map.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
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
        self.data
            .insert(relation.to_string(), serde_json::Value::Null);
    }

    /// Get a single file (hasOne)
    pub fn get_one(&self, relation: &str) -> Option<FileAttachment> {
        self.data
            .get(relation)
            .filter(|value| !value.is_null())
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    }

    /// Add to file array (hasMany)
    pub fn add_many(&mut self, relation: &str, attachment: FileAttachment) {
        let mut array = self.get_many_raw(relation);
        array.push(attachment.to_json());
        self.data
            .insert(relation.to_string(), serde_json::Value::Array(array));
    }

    /// Remove from file array (hasMany)
    pub fn remove_from_many(&mut self, relation: &str, file_key: &str) {
        let array: Vec<serde_json::Value> = self
            .get_many_raw(relation)
            .into_iter()
            .filter(|item| item.get("key").and_then(|key| key.as_str()) != Some(file_key))
            .collect();
        self.data
            .insert(relation.to_string(), serde_json::Value::Array(array));
    }

    /// Clear all files from array (hasMany)
    pub fn clear_many(&mut self, relation: &str) {
        self.data
            .insert(relation.to_string(), serde_json::Value::Array(vec![]));
    }

    /// Get all files from array (hasMany)
    pub fn get_many(&self, relation: &str) -> Vec<FileAttachment> {
        self.get_many_raw(relation)
            .into_iter()
            .filter_map(|value| serde_json::from_value(value).ok())
            .collect()
    }

    fn get_many_raw(&self, relation: &str) -> Vec<serde_json::Value> {
        self.data
            .get(relation)
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default()
    }

    /// Check if relation has files
    pub fn has_files(&self, relation: &str) -> bool {
        match self.data.get(relation) {
            Some(serde_json::Value::Null) => false,
            Some(serde_json::Value::Array(array)) => !array.is_empty(),
            Some(serde_json::Value::Object(_)) => true,
            _ => false,
        }
    }

    /// Count files in relation
    pub fn count_files(&self, relation: &str) -> usize {
        match self.data.get(relation) {
            Some(serde_json::Value::Null) => 0,
            Some(serde_json::Value::Array(array)) => array.len(),
            Some(serde_json::Value::Object(_)) => 1,
            _ => 0,
        }
    }
}