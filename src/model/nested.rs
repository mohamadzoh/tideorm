#![allow(missing_docs)]

use async_trait::async_trait;
use sea_orm::sea_query::OnConflict;

use crate::callbacks::{AfterCreateDispatch, AfterDeleteDispatch, AfterUpdateDispatch, BeforeCreateDispatch, BeforeDeleteDispatch, BeforeUpdateDispatch};
use crate::error::{Error, Result};
use crate::internal::{EntityTrait, InternalModel, IntoActiveModel, translate_error};

use super::Model;

fn parent_primary_key_json<M: Model>(parent: &M) -> serde_json::Value {
    let pk = parent.primary_key();
    serde_json::Value::String(format!("{}", pk))
}

fn primary_key_field_name<M: Model>() -> &'static str {
    M::column_names()
        .iter()
        .position(|column| *column == M::primary_key_name())
        .and_then(|index| M::field_names().get(index).copied())
        .unwrap_or(M::primary_key_name())
}

fn apply_foreign_key<R: Model>(
    related: R,
    foreign_key: &str,
    parent_pk_value: &serde_json::Value,
) -> Result<R> {
    let mut related_json = serde_json::to_value(&related)
        .map_err(|e| Error::conversion(format!("Failed to serialize related model: {}", e)))?;

    if let serde_json::Value::Object(ref mut map) = related_json {
        let pk_str = parent_pk_value.as_str().unwrap_or_default();
        if let Ok(pk_i64) = pk_str.parse::<i64>() {
            map.insert(foreign_key.to_string(), serde_json::json!(pk_i64));
        } else {
            map.insert(foreign_key.to_string(), parent_pk_value.clone());
        }
    }

    serde_json::from_value(related_json)
        .map_err(|e| Error::conversion(format!("Failed to deserialize related model: {}", e)))
}

fn related_primary_key_value<R: Model>(related: &R) -> Result<serde_json::Value> {
    let related_json = serde_json::to_value(related)
        .map_err(|e| Error::conversion(format!("Failed to serialize related model: {}", e)))?;
    let field_name = primary_key_field_name::<R>();

    match related_json {
        serde_json::Value::Object(map) => map
            .get(field_name)
            .cloned()
            .or_else(|| map.get(R::primary_key_name()).cloned())
            .ok_or_else(|| {
                Error::conversion(format!(
                    "Failed to read primary key '{}' from related model JSON",
                    field_name
                ))
            }),
        _ => Err(Error::conversion(
            "Failed to serialize related model into an object for primary key extraction",
        )),
    }
}

fn primary_key_identity(value: &serde_json::Value) -> String {
    value.to_string()
}

fn reorder_models_by_primary_key<R: Model>(
    models: Vec<R>,
    ordered_primary_keys: &[String],
) -> Result<Vec<R>> {
    let mut models_by_pk = std::collections::HashMap::with_capacity(models.len());
    for model in models {
        let key = primary_key_identity(&related_primary_key_value(&model)?);
        models_by_pk.insert(key, model);
    }

    ordered_primary_keys
        .iter()
        .map(|key| {
            models_by_pk.remove(key).ok_or_else(|| {
                Error::query(format!(
                    "Bulk nested operation completed but could not reload related model with primary key {}",
                    key
                ))
            })
        })
        .collect()
}

async fn bulk_upsert_models<R>(related: Vec<R>) -> Result<Vec<R>>
where
    R: Model,
    <<R as InternalModel>::Entity as EntityTrait>::Model: IntoActiveModel<R::ActiveModel>,
{
    if related.is_empty() {
        return Ok(Vec::new());
    }

    let db = crate::database::__current_db()?;
    let conn = db.__internal_connection();
    let pk_column = R::primary_key_column().ok_or_else(|| {
        Error::invalid_query(format!(
            "bulk nested update requires a primary key column for {}",
            R::table_name()
        ))
    })?;
    let pk_values: Vec<serde_json::Value> = related
        .iter()
        .map(related_primary_key_value)
        .collect::<Result<_>>()?;
    let pk_order: Vec<String> = pk_values.iter().map(primary_key_identity).collect();
    let update_columns: Vec<_> = R::column_names()
        .iter()
        .copied()
        .filter(|column| *column != R::primary_key_name())
        .filter_map(R::column_from_str)
        .collect();
    let active_models: Vec<_> = related
        .into_iter()
        .map(|model| model.to_sea_model().into_active_model())
        .collect();

    let on_conflict = if update_columns.is_empty() {
        OnConflict::column(pk_column).do_nothing().to_owned()
    } else {
        OnConflict::column(pk_column)
            .update_columns(update_columns)
            .to_owned()
    };

    let result = R::Entity::insert_many(active_models)
        .on_conflict(on_conflict)
        .exec(&conn);
    crate::profiling::__profile_future(result)
        .await
        .map_err(translate_error)
        .map_err(|err| err.with_context(crate::error::ErrorContext::new().table(R::table_name()).query("nested bulk upsert")))?;

    let fetched = R::query()
        .where_in(R::primary_key_name(), pk_values)
        .get()
        .await?;

    reorder_models_by_primary_key(fetched, &pk_order)
}

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
        <<R as InternalModel>::Entity as EntityTrait>::Model: IntoActiveModel<R::ActiveModel>,
    {
        let parent = self.save().await?;

        if related.is_empty() {
            return Ok((parent, Vec::new()));
        }

        let pk_value = parent_primary_key_json(&parent);

        let mut prepared_related = Vec::with_capacity(related.len());
        for item in related {
            let mut item = apply_foreign_key(item, foreign_key, &pk_value)?;
            (&mut item).run_before_create()?;
            prepared_related.push(item);
        }

        let saved_related = R::insert_all(prepared_related).await?;
        for item in &saved_related {
            item.run_after_create()?;
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
        <<R as InternalModel>::Entity as EntityTrait>::Model: IntoActiveModel<R::ActiveModel>,
    {
        let parent = self.update().await?;

        if related.is_empty() {
            return Ok((parent, Vec::new()));
        }

        if related.iter().any(super::crud::is_new) {
            let mut updated = Vec::with_capacity(related.len());
            for item in related {
                updated.push(item.update().await?);
            }
            return Ok((parent, updated));
        }

        let mut prepared_related = Vec::with_capacity(related.len());
        for mut item in related {
            (&mut item).run_before_update()?;
            prepared_related.push(item);
        }

        let updated = bulk_upsert_models(prepared_related).await?;
        for item in &updated {
            item.run_after_update()?;
        }

        Ok((parent, updated))
    }

    async fn delete_with_many<R: Model>(self, related: Vec<R>) -> Result<u64>
    where
        Self: Sized,
    {
        let related_deleted = if related.is_empty() {
            0
        } else {
            let mut pk_values = Vec::with_capacity(related.len());
            for item in &related {
                item.run_before_delete()?;
                pk_values.push(related_primary_key_value(item)?);
            }

            let deleted = R::query()
                .where_in(R::primary_key_name(), pk_values)
                .delete()
                .await?;

            for item in &related {
                item.run_after_delete()?;
            }

            deleted
        };

        Ok(related_deleted + self.delete().await?)
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

#[cfg(all(test, feature = "sqlite"))]
#[path = "../testing/model_nested_tests.rs"]
mod tests;