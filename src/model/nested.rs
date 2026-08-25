#![allow(missing_docs)]

use async_trait::async_trait;

use crate::error::{Error, Result};
use crate::internal::{EntityTrait, InternalModel, IntoActiveModel};

use super::Model;

// Nested saves must never dispatch lifecycle callbacks themselves.
//
// `Callbacks` is optional, so callback dispatch is resolved by autoref
// specialization (see `crate::callbacks`), and that only works at concrete call
// sites. Inside the generic helpers below the compiler type-checks once with
// `R` opaque, `R: Callbacks` is unprovable, and the no-op fallback is selected
// for every instantiation — including models that do implement `Callbacks`.
//
// Every write below therefore goes through `Model::create`, `Model::update`,
// `Model::save`, or `Model::delete`, which the derive emits as concrete impls
// where the specialization resolves against the real model type.

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

/// Resolve a caller-supplied foreign-key name to the related model's Rust field
/// name, which is the key its serde impl uses.
///
/// Every other TideORM API accepts either the DB column name or the Rust field
/// name, so nested saves accept both too. An unresolvable name is a hard error:
/// writing it into the serialized model used to land in serde's `__ignore`
/// bucket, leaving the children with whatever foreign key they already carried
/// (usually `0`) and reporting success.
fn resolve_foreign_key_field<R: Model>(foreign_key: &str) -> Result<&'static str> {
    R::field_names()
        .iter()
        .copied()
        .zip(R::column_names().iter().copied())
        .find_map(|(field_name, column_name)| {
            (field_name == foreign_key || column_name == foreign_key).then_some(field_name)
        })
        .ok_or_else(|| {
            Error::invalid_query(format!(
                "Unknown foreign key '{}' for {}; expected one of: {}",
                foreign_key,
                R::table_name(),
                R::field_names().join(", ")
            ))
        })
}

fn apply_foreign_key<R: Model>(
    related: R,
    foreign_key: &str,
    parent_pk_value: &serde_json::Value,
) -> Result<R> {
    let foreign_key = resolve_foreign_key_field::<R>(foreign_key)?;

    let mut related_json = serde_json::to_value(&related)
        .map_err(|e| Error::conversion(format!("Failed to serialize related model: {}", e)))?;

    match related_json {
        // The parent key is written through verbatim. Coercing a numeric-looking
        // string key ("00420") into an integer silently rewrote the child's
        // foreign key, and the resulting deserialize failure surfaced only after
        // the parent row had already been written.
        serde_json::Value::Object(ref mut map) => {
            map.insert(foreign_key.to_string(), parent_pk_value.clone());
        }
        _ => {
            return Err(Error::conversion(format!(
                "Related model for {} did not serialize to a JSON object",
                R::table_name()
            )));
        }
    }

    serde_json::from_value(related_json)
        .map_err(|e| Error::conversion(format!("Failed to deserialize related model: {}", e)))
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

async fn save_related_models_as_json<R>(
    related: Vec<R>,
    foreign_key: String,
    parent_pk_value: serde_json::Value,
) -> Result<SavedRelation>
where
    R: Model,
    <<R as InternalModel>::Entity as EntityTrait>::Model: IntoActiveModel<R::ActiveModel>,
{
    let mut saved_json = Vec::with_capacity(related.len());
    for item in related {
        let item = apply_foreign_key(item, &foreign_key, &parent_pk_value)?;
        let saved = R::create(item).await?;
        saved_json.push(
            serde_json::to_value(&saved).map_err(|e| {
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
        // Resolved up front so a bad foreign-key name fails before anything is
        // written at all.
        let foreign_key = resolve_foreign_key_field::<R>(foreign_key)?;

        // Parent and child are one unit of work: without a transaction a failure
        // on the child leaves the parent committed and orphaned.
        // `Database::transaction` defers to an ambient transaction, so this nests
        // as a SAVEPOINT when the caller already opened one.
        super::crud::transaction(move |_| {
            Box::pin(async move {
                let parent = self.save().await?;

                let pk_value =
                    require_scalar_primary_key::<Self>(&parent.primary_key(), "save_with_one")?;

                let related = apply_foreign_key(related, foreign_key, &pk_value)?;

                let related = related.save().await?;

                Ok((parent, related))
            })
        })
        .await
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
        if related.is_empty() {
            let parent = self.save().await?;
            return Ok((parent, Vec::new()));
        }

        let foreign_key = resolve_foreign_key_field::<R>(foreign_key)?;

        super::crud::transaction(move |_| {
            Box::pin(async move {
                let parent = self.save().await?;

                let pk_value =
                    require_scalar_primary_key::<Self>(&parent.primary_key(), "save_with_many")?;

                let mut saved_related = Vec::with_capacity(related.len());
                for item in related {
                    let item = apply_foreign_key(item, foreign_key, &pk_value)?;
                    saved_related.push(R::create(item).await?);
                }

                Ok((parent, saved_related))
            })
        })
        .await
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
        let Self {
            parent,
            one_relations,
            many_relations,
        } = self;

        // The parent and every nested relation are one unit of work; see
        // `NestedSave::save_with_one` for why this nests instead of opening a
        // second top-level transaction.
        super::crud::transaction(move |_| {
            Box::pin(async move {
                let parent = parent.save().await?;

                let pk_value =
                    require_scalar_primary_key::<M>(&parent.primary_key(), "nested save builder")?;

                let mut saved_relations = Vec::new();

                for save_relation in one_relations {
                    saved_relations.push(save_relation.run(pk_value.clone()).await?);
                }

                for save_relations in many_relations {
                    saved_relations.push(save_relations.run(pk_value.clone()).await?);
                }

                Ok((parent, saved_relations))
            })
        })
        .await
    }
}

#[cfg(all(test, feature = "sqlite"))]
#[path = "../../tests/unit/model_nested_tests.rs"]
mod tests;
