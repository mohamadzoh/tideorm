#![allow(missing_docs)]

use crate::error::Result;

use super::{Model, ModelMeta};

/// Builder for creating new model instances.
pub struct CreateBuilder<M: ModelMeta> {
    _marker: std::marker::PhantomData<M>,
    values: std::collections::HashMap<String, serde_json::Value>,
}

impl<M: ModelMeta> CreateBuilder<M> {
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
            values: std::collections::HashMap::new(),
        }
    }

    pub fn set(mut self, field: &str, value: impl Into<serde_json::Value>) -> Self {
        self.values.insert(field.to_string(), value.into());
        self
    }
}

impl<M: ModelMeta> Default for CreateBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for updating model instances.
pub struct UpdateBuilder<M: Model> {
    model: M,
    changes: std::collections::HashMap<String, serde_json::Value>,
}

impl<M: Model> UpdateBuilder<M> {
    pub fn new(model: M) -> Self {
        Self {
            model,
            changes: std::collections::HashMap::new(),
        }
    }

    pub fn set(mut self, field: &str, value: impl Into<serde_json::Value>) -> Self {
        self.changes.insert(field.to_string(), value.into());
        self
    }

    pub async fn save(self) -> Result<M> {
        self.model.update().await
    }
}

/// Builder for on-conflict (upsert) operations.
#[doc(hidden)]
pub struct OnConflictBuilder<M: Model> {
    pub _marker: std::marker::PhantomData<M>,
    pub conflict_columns: Vec<String>,
    pub update_columns: Option<Vec<String>>,
    pub exclude_columns: Option<Vec<String>>,
}

impl<M: Model> OnConflictBuilder<M> {
    pub fn new(conflict_columns: Vec<String>) -> Self {
        Self {
            _marker: std::marker::PhantomData,
            conflict_columns,
            update_columns: None,
            exclude_columns: None,
        }
    }

    pub fn update_columns(mut self, columns: Vec<&str>) -> Self {
        self.update_columns = Some(columns.into_iter().map(|s| s.to_string()).collect());
        self
    }

    pub fn update_all_except(mut self, columns: Vec<&str>) -> Self {
        self.exclude_columns = Some(columns.into_iter().map(|s| s.to_string()).collect());
        self
    }

    pub async fn insert(self, model: M) -> Result<M>
    where
        M: Sized,
    {
        M::__insert_with_conflict(model, self).await
    }
}