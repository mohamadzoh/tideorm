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
                let data: HashMap<String, serde_json::Value> = map
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
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
        self.data
            .insert(relation.to_string(), serde_json::Value::Null);
    }

    /// Get a single file (hasOne), returning None for malformed legacy data.
    /// Use [`Self::try_get_one`] to distinguish malformed data from an absent file.
    pub fn get_one(&self, relation: &str) -> Option<FileAttachment> {
        self.data
            .get(relation)
            .filter(|value| !value.is_null())
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    }

    /// Add to file array (hasMany)
    pub fn add_many(&mut self, relation: &str, attachment: FileAttachment) {
        self.many_mut(relation).push(attachment.to_json());
    }

    fn many_mut(&mut self, relation: &str) -> &mut Vec<serde_json::Value> {
        let value = self
            .data
            .entry(relation.to_string())
            .or_insert_with(|| serde_json::json!([]));
        if !value.is_array() {
            *value = serde_json::json!([]);
        }
        value.as_array_mut().expect("array initialized above")
    }

    /// Remove one key from a hasMany relation.
    pub fn remove_from_many(&mut self, relation: &str, file_key: &str) {
        self.remove_many(relation, &[file_key]);
    }

    /// Remove all matching keys in a single pass.
    pub fn remove_many(&mut self, relation: &str, file_keys: &[&str]) {
        let keys: std::collections::HashSet<&str> = file_keys.iter().copied().collect();
        self.many_mut(relation).retain(|item| {
            !item
                .get("key")
                .and_then(|key| key.as_str())
                .is_some_and(|key| keys.contains(key))
        });
    }

    /// Decode a hasOne relation, reporting malformed stored data.
    pub fn try_get_one(&self, relation: &str) -> Result<Option<FileAttachment>, serde_json::Error> {
        self.data
            .get(relation)
            .filter(|value| !value.is_null())
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()
    }

    /// Decode every attachment, reporting malformed entries instead of dropping them.
    pub fn try_get_many(&self, relation: &str) -> Result<Vec<FileAttachment>, serde_json::Error> {
        match self.data.get(relation) {
            None | Some(serde_json::Value::Null) => Ok(Vec::new()),
            Some(value) => serde_json::from_value(value.clone()),
        }
    }

    /// Clear all files from array (hasMany)
    pub fn clear_many(&mut self, relation: &str) {
        self.data
            .insert(relation.to_string(), serde_json::Value::Array(vec![]));
    }

    /// Get valid files from an array, skipping malformed legacy entries.
    /// Use [`Self::try_get_many`] to report malformed data.
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
        self.count_files(relation) > 0
    }

    /// Count raw entries, including malformed attachment data.
    /// Trait-level attachment counts decode first and report errors.
    pub fn count_files(&self, relation: &str) -> usize {
        match self.data.get(relation) {
            Some(serde_json::Value::Null) => 0,
            Some(serde_json::Value::Array(array)) => array.len(),
            Some(serde_json::Value::Object(_)) => 1,
            _ => 0,
        }
    }
}
