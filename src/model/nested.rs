#![allow(missing_docs)]

use async_trait::async_trait;

use crate::error::{Error, Result};

use super::Model;

/// Extension trait for cascade save operations.
#[async_trait]
pub trait NestedSave: Model {
    async fn save_with_one<R: Model>(self, related: R, foreign_key: &str) -> Result<(Self, R)>
    where
        Self: Sized,
    {
        let parent = self.save().await?;

        let pk_value = {
            let pk = parent.primary_key();
            serde_json::Value::String(format!("{}", pk))
        };

        let mut related_json = serde_json::to_value(&related)
            .map_err(|e| Error::conversion(format!("Failed to serialize related model: {}", e)))?;

        if let serde_json::Value::Object(ref mut map) = related_json {
            let pk_str = pk_value.as_str().unwrap_or_default();
            if let Ok(pk_i64) = pk_str.parse::<i64>() {
                map.insert(foreign_key.to_string(), serde_json::json!(pk_i64));
            } else {
                map.insert(foreign_key.to_string(), pk_value);
            }
        }

        let related: R = serde_json::from_value(related_json)
            .map_err(|e| Error::conversion(format!("Failed to deserialize related model: {}", e)))?;

        let related = related.save().await?;

        Ok((parent, related))
    }

    async fn save_with_many<R: Model>(
        self,
        related: Vec<R>,
        foreign_key: &str,
    ) -> Result<(Self, Vec<R>)>
    where
        Self: Sized,
    {
        let parent = self.save().await?;

        let pk_value = {
            let pk = parent.primary_key();
            serde_json::Value::String(format!("{}", pk))
        };

        let mut saved_related = Vec::with_capacity(related.len());
        for item in related {
            let mut item_json = serde_json::to_value(&item).map_err(|e| {
                Error::conversion(format!("Failed to serialize related model: {}", e))
            })?;

            if let serde_json::Value::Object(ref mut map) = item_json {
                let pk_str = pk_value.as_str().unwrap_or_default();
                if let Ok(pk_i64) = pk_str.parse::<i64>() {
                    map.insert(foreign_key.to_string(), serde_json::json!(pk_i64));
                } else {
                    map.insert(foreign_key.to_string(), pk_value.clone());
                }
            }

            let item: R = serde_json::from_value(item_json).map_err(|e| {
                Error::conversion(format!("Failed to deserialize related model: {}", e))
            })?;

            let saved = item.save().await?;
            saved_related.push(saved);
        }

        Ok((parent, saved_related))
    }

    async fn update_with_one<R: Model>(self, related: R) -> Result<(Self, R)>
    where
        Self: Sized,
    {
        let parent = self.update().await?;
        let related = related.update().await?;
        Ok((parent, related))
    }

    async fn update_with_many<R: Model>(self, related: Vec<R>) -> Result<(Self, Vec<R>)>
    where
        Self: Sized,
    {
        let parent = self.update().await?;
        let mut updated = Vec::with_capacity(related.len());
        for item in related {
            updated.push(item.update().await?);
        }
        Ok((parent, updated))
    }

    async fn delete_with_many<R: Model>(self, related: Vec<R>) -> Result<u64>
    where
        Self: Sized,
    {
        let mut total = 0u64;

        for item in related {
            total += item.delete().await?;
        }

        total += self.delete().await?;

        Ok(total)
    }
}

impl<M: Model> NestedSave for M {}

/// Builder for nested/cascade saves.
pub struct NestedSaveBuilder<M: Model> {
    parent: M,
    one_relations: Vec<(serde_json::Value, String)>,
    many_relations: Vec<(Vec<serde_json::Value>, String)>,
}

impl<M: Model> NestedSaveBuilder<M> {
    pub fn new(parent: M) -> Self {
        Self {
            parent,
            one_relations: Vec::new(),
            many_relations: Vec::new(),
        }
    }

    pub fn with_one<R: Model>(mut self, related: R, foreign_key: &str) -> Self {
        if let Ok(json) = serde_json::to_value(&related) {
            self.one_relations.push((json, foreign_key.to_string()));
        }
        self
    }

    pub fn with_many<R: Model>(mut self, related: Vec<R>, foreign_key: &str) -> Self {
        let json_values: Vec<serde_json::Value> = related
            .into_iter()
            .filter_map(|r| serde_json::to_value(&r).ok())
            .collect();
        self.many_relations
            .push((json_values, foreign_key.to_string()));
        self
    }

    pub async fn save(self) -> Result<(M, Vec<serde_json::Value>)> {
        let parent = self.parent.save().await?;

        let pk_value = {
            let pk = parent.primary_key();
            serde_json::Value::String(format!("{}", pk))
        };

        let mut saved_json = Vec::new();

        for (mut json, fk) in self.one_relations {
            if let serde_json::Value::Object(ref mut map) = json {
                let pk_str = pk_value.as_str().unwrap_or_default();
                if let Ok(pk_i64) = pk_str.parse::<i64>() {
                    map.insert(fk, serde_json::json!(pk_i64));
                } else {
                    map.insert(fk, pk_value.clone());
                }
            }
            saved_json.push(json);
        }

        for (items, fk) in self.many_relations {
            for mut json in items {
                if let serde_json::Value::Object(ref mut map) = json {
                    let pk_str = pk_value.as_str().unwrap_or_default();
                    if let Ok(pk_i64) = pk_str.parse::<i64>() {
                        map.insert(fk.clone(), serde_json::json!(pk_i64));
                    } else {
                        map.insert(fk.clone(), pk_value.clone());
                    }
                }
                saved_json.push(json);
            }
        }

        Ok((parent, saved_json))
    }
}