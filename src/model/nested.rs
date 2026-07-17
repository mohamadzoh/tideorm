#![allow(missing_docs)]

use async_trait::async_trait;

use crate::callbacks::{
    AfterCreateDispatch, AfterUpdateDispatch, BeforeCreateDispatch, BeforeUpdateDispatch,
};
use crate::error::{Error, Result};
use crate::internal::{EntityTrait, InternalModel, IntoActiveModel, OnConflict, translate_error};

use super::Model;

#[async_trait]
trait OneRelationSaveOp: Send {
    async fn run(self: Box<Self>, parent_pk_value: serde_json::Value) -> Result<SavedRelation>;
}

#[async_trait]
trait ManyRelationSaveOp: Send {
    async fn run(self: Box<Self>, parent_pk_value: serde_json::Value) -> Result<SavedRelation>;
}

struct OneRelationSaveFn<R> {
    related: R,
    foreign_key: String,
}

struct ManyRelationSaveFn<R> {
    related: Vec<R>,
    foreign_key: String,
}

#[async_trait]
impl<R: Model + Send> OneRelationSaveOp for OneRelationSaveFn<R> {
    async fn run(self: Box<Self>, parent_pk_value: serde_json::Value) -> Result<SavedRelation> {
        save_related_model_as_json(self.related, self.foreign_key, parent_pk_value).await
    }
}

#[async_trait]
impl<R: Model + Send> ManyRelationSaveOp for ManyRelationSaveFn<R>
where
    <<R as InternalModel>::Entity as EntityTrait>::Model: IntoActiveModel<R::ActiveModel>,
{
    async fn run(self: Box<Self>, parent_pk_value: serde_json::Value) -> Result<SavedRelation> {
        save_related_models_as_json(self.related, self.foreign_key, parent_pk_value).await
    }
}

#[derive(Debug, Clone, PartialEq)]
enum SavedRelationInner {
    One(serde_json::Value),
    Many(Vec<serde_json::Value>),
}

/// Saved nested relation payload returned by [`NestedSaveBuilder::save`].
#[derive(Debug, Clone, PartialEq)]
pub struct SavedRelation(SavedRelationInner);

impl SavedRelation {
    fn one(value: serde_json::Value) -> Self {
        Self(SavedRelationInner::One(value))
    }

    fn many(values: Vec<serde_json::Value>) -> Self {
        Self(SavedRelationInner::Many(values))
    }

    /// Returns true when this result came from `with_one`.
    pub fn is_one(&self) -> bool {
        matches!(self.0, SavedRelationInner::One(_))
    }

    /// Returns true when this result came from `with_many`.
    pub fn is_many(&self) -> bool {
        matches!(self.0, SavedRelationInner::Many(_))
    }

    /// Convert a single related-model result into its concrete model type.
    pub fn into_one<R: Model>(self) -> Result<R> {
        match self.0 {
            SavedRelationInner::One(value) => serde_json::from_value(value).map_err(|e| {
                Error::conversion(format!("Failed to deserialize related model: {}", e))
            }),
            SavedRelationInner::Many(_) => Err(Error::conversion(
                "Expected a single related model but received a relation collection".to_string(),
            )),
        }
    }

    /// Convert a collection result into concrete model values.
    pub fn into_many<R: Model>(self) -> Result<Vec<R>> {
        match self.0 {
            SavedRelationInner::Many(values) => values
                .into_iter()
                .map(|value| {
                    serde_json::from_value(value).map_err(|e| {
                        Error::conversion(format!("Failed to deserialize related model: {}", e))
                    })
                })
                .collect(),
            SavedRelationInner::One(_) => Err(Error::conversion(
                "Expected a related model collection but received a single relation".to_string(),
            )),
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn test_one(value: serde_json::Value) -> Self {
        Self::one(value)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn test_many(values: Vec<serde_json::Value>) -> Self {
        Self::many(values)
    }
}

fn serialize_primary_key<M: Model>(primary_key: &M::PrimaryKey) -> Result<serde_json::Value> {
    serde_json::to_value(primary_key)
        .map_err(|e| Error::conversion(format!("Failed to serialize primary key: {}", e)))
}

fn require_scalar_primary_key<M: Model>(
    primary_key: &M::PrimaryKey,
    context: &str,
) -> Result<serde_json::Value> {
    let value = serialize_primary_key::<M>(primary_key)?;
    if value.is_array() || value.is_object() {
        return Err(Error::invalid_query(format!(
            "{} does not support composite primary keys for {}",
            context,
            M::table_name()
        )));
    }

    Ok(value)
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

fn primary_key_identity<R: Model>(value: &R::PrimaryKey) -> String {
    R::primary_key_display(value)
}

fn reorder_models_by_primary_key<R: Model>(
    models: Vec<R>,
    ordered_primary_keys: &[R::PrimaryKey],
) -> Result<Vec<R>> {
    let mut models_by_pk = std::collections::HashMap::with_capacity(models.len());
    for model in models {
        let key = primary_key_identity::<R>(&model.primary_key());
        models_by_pk.insert(key, model);
    }

    ordered_primary_keys
        .iter()
        .map(|key| {
            let identity = primary_key_identity::<R>(key);
            models_by_pk.remove(&identity).ok_or_else(|| {
                Error::query(format!(
                    "Bulk nested operation completed but could not reload related model with primary key {}",
                    identity
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

    let pk_columns = R::primary_key_columns();
    if pk_columns.is_empty() {
        return Err(Error::invalid_query(format!(
            "bulk nested update requires primary key columns for {}",
            R::table_name()
        )));
    }

    let primary_keys: Vec<R::PrimaryKey> = related.iter().map(Model::primary_key).collect();
    let update_columns: Vec<_> = R::column_names()
        .iter()
        .copied()
        .filter(|column| !R::primary_key_names().contains(column))
        .filter_map(R::column_from_str)
        .collect();
    let active_models: Vec<_> = related
        .into_iter()
        .map(|model| {
            <R as InternalModel>::try_to_entity_model(&model)
                .map(|entity_model| entity_model.into_active_model())
        })
        .collect::<Result<Vec<_>>>()?;

    let on_conflict = if update_columns.is_empty() {
        OnConflict::columns(pk_columns.iter().cloned())
            .do_nothing()
            .to_owned()
    } else {
        OnConflict::columns(pk_columns.iter().cloned())
            .update_columns(update_columns)
            .to_owned()
    };

    match crate::database::__current_connection()? {
        crate::database::ConnectionRef::Database(conn) => {
            crate::profiling::__profile_future(
                R::Entity::insert_many(active_models)
                    .on_conflict(on_conflict)
                    .exec(conn.connection()),
            )
            .await
        }
        crate::database::ConnectionRef::Transaction(tx) => {
            crate::profiling::__profile_future(
                R::Entity::insert_many(active_models)
                    .on_conflict(on_conflict)
                    .exec(tx.as_ref()),
            )
            .await
        }
    }
    .map_err(translate_error)
    .map_err(|err| {
        err.with_context(
            crate::error::ErrorContext::new()
                .table(R::table_name())
                .query("nested bulk upsert"),
        )
    })?;

    let mut reloaded = Vec::with_capacity(primary_keys.len());
    for primary_key in &primary_keys {
        reloaded.push(R::find_or_fail(primary_key.clone()).await?);
    }

    reorder_models_by_primary_key(reloaded, &primary_keys)
}

async fn save_related_model_as_json<R>(
    related: R,
    foreign_key: String,
    parent_pk_value: serde_json::Value,
) -> Result<SavedRelation>
where
    R: Model,
{
    let related = apply_foreign_key(related, &foreign_key, &parent_pk_value)?;
    let related = related.save().await?;
    serde_json::to_value(&related)
        .map(SavedRelation::one)
        .map_err(|e| Error::conversion(format!("Failed to serialize related model: {}", e)))
}

#[allow(clippy::unnecessary_mut_passed)]
async fn save_related_models_as_json<R>(
    related: Vec<R>,
    foreign_key: String,
    parent_pk_value: serde_json::Value,
) -> Result<SavedRelation>
where
    R: Model,
    <<R as InternalModel>::Entity as EntityTrait>::Model: IntoActiveModel<R::ActiveModel>,
{
    if related.is_empty() {
        return Ok(SavedRelation::many(Vec::new()));
    }

    let mut prepared_related = Vec::with_capacity(related.len());
    for item in related {
        let mut item = apply_foreign_key(item, &foreign_key, &parent_pk_value)?;
        (&mut item).run_before_create()?;
        prepared_related.push(item);
    }

    let saved_related = R::insert_all(prepared_related).await?;
    let mut saved_json = Vec::with_capacity(saved_related.len());
    for item in saved_related {
        (&item).run_after_create()?;
        saved_json.push(
            serde_json::to_value(&item).map_err(|e| {
                Error::conversion(format!("Failed to serialize related model: {}", e))
            })?,
        );
    }

    Ok(SavedRelation::many(saved_json))
}

/// Extension trait for cascade save operations.
#[async_trait]
pub trait NestedSave: Model {
    async fn save_with_one<R: Model>(self, related: R, foreign_key: &str) -> Result<(Self, R)>
    where
        Self: Sized,
    {
        let parent = self.save().await?;

        let pk_value = require_scalar_primary_key::<Self>(&parent.primary_key(), "save_with_one")?;

        let related = apply_foreign_key(related, foreign_key, &pk_value)?;

        let related = related.save().await?;

        Ok((parent, related))
    }

    #[allow(clippy::unnecessary_mut_passed)]
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

        let pk_value = require_scalar_primary_key::<Self>(&parent.primary_key(), "save_with_many")?;

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

    #[allow(clippy::unnecessary_mut_passed)]
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
            let mut deleted = 0;
            for item in related {
                deleted += item.delete().await?;
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
    one_relations: Vec<Box<dyn OneRelationSaveOp>>,
    many_relations: Vec<Box<dyn ManyRelationSaveOp>>,
}

impl<M: Model> NestedSaveBuilder<M> {
    pub fn new(parent: M) -> Self {
        Self {
            parent,
            one_relations: Vec::new(),
            many_relations: Vec::new(),
        }
    }

    pub fn with_one<R: Model + 'static>(mut self, related: R, foreign_key: &str) -> Self {
        self.one_relations.push(Box::new(OneRelationSaveFn {
            related,
            foreign_key: foreign_key.to_string(),
        }));
        self
    }

    pub fn with_many<R: Model + 'static>(mut self, related: Vec<R>, foreign_key: &str) -> Self
    where
        <<R as InternalModel>::Entity as EntityTrait>::Model: IntoActiveModel<R::ActiveModel>,
    {
        self.many_relations.push(Box::new(ManyRelationSaveFn {
            related,
            foreign_key: foreign_key.to_string(),
        }));
        self
    }

    pub async fn save(self) -> Result<(M, Vec<SavedRelation>)> {
        let parent = self.parent.save().await?;

        let pk_value =
            require_scalar_primary_key::<M>(&parent.primary_key(), "nested save builder")?;

        let mut saved_relations = Vec::new();

        for save_relation in self.one_relations {
            saved_relations.push(save_relation.run(pk_value.clone()).await?);
        }

        for save_relations in self.many_relations {
            saved_relations.push(save_relations.run(pk_value.clone()).await?);
        }

        Ok((parent, saved_relations))
    }
}

#[cfg(all(test, feature = "sqlite"))]
#[path = "../../tests/unit/model_nested_tests.rs"]
mod tests;
