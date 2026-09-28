#![allow(missing_docs)]

use std::any::Any;
use std::sync::Arc;

use async_trait::async_trait;

use crate::error::{Error, Result};

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

/// One type-erased `with_one` / `with_many` relation of a [`NestedSaveBuilder`].
#[async_trait]
trait RelationSaveOp: Send {
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
impl<R: Model> RelationSaveOp for OneRelationSaveFn<R> {
    async fn run(self: Box<Self>, parent_pk_value: serde_json::Value) -> Result<SavedRelation> {
        let Self {
            related,
            foreign_key,
        } = *self;
        let saved = apply_foreign_key(related, &foreign_key, &parent_pk_value)?
            .save()
            .await?;
        Ok(SavedRelation::one(saved))
    }
}

#[async_trait]
impl<R: Model> RelationSaveOp for ManyRelationSaveFn<R> {
    async fn run(self: Box<Self>, parent_pk_value: serde_json::Value) -> Result<SavedRelation> {
        let Self {
            related,
            foreign_key,
        } = *self;
        let saved = save_related(related, &foreign_key, &parent_pk_value).await?;
        Ok(SavedRelation::many(saved))
    }
}

/// The models one relation saved, as they were stored: a model's own serde
/// may leave a field out of its JSON, which a round trip through it would
/// lose.
#[derive(Clone)]
enum SavedRelationInner {
    /// A model `R`.
    One(Arc<dyn Any + Send + Sync>),
    /// A `Vec<R>`.
    Many(Arc<dyn Any + Send + Sync>),
}

/// Saved nested relation payload returned by [`NestedSaveBuilder::save`].
#[derive(Clone)]
pub struct SavedRelation(SavedRelationInner);

impl std::fmt::Debug for SavedRelation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let shape = match self.0 {
            SavedRelationInner::One(_) => "one",
            SavedRelationInner::Many(_) => "many",
        };
        formatter
            .debug_struct("SavedRelation")
            .field("shape", &shape)
            .finish_non_exhaustive()
    }
}

impl SavedRelation {
    fn one<R: Model>(saved: R) -> Self {
        Self(SavedRelationInner::One(Arc::new(saved)))
    }

    fn many<R: Model>(saved: Vec<R>) -> Self {
        Self(SavedRelationInner::Many(Arc::new(saved)))
    }

    /// Returns true when this result came from `with_one`.
    pub fn is_one(&self) -> bool {
        matches!(self.0, SavedRelationInner::One(_))
    }

    /// Returns true when this result came from `with_many`.
    pub fn is_many(&self) -> bool {
        matches!(self.0, SavedRelationInner::Many(_))
    }

    /// The model a `with_one` saved, as it was stored.
    pub fn into_one<R: Model>(self) -> Result<R> {
        match self.0 {
            SavedRelationInner::One(saved) => downcast_saved(saved),
            SavedRelationInner::Many(_) => Err(Error::conversion(
                "Expected a single related model but received a relation collection".to_string(),
            )),
        }
    }

    /// The models a `with_many` saved, as they were stored.
    pub fn into_many<R: Model>(self) -> Result<Vec<R>> {
        match self.0 {
            SavedRelationInner::Many(saved) => downcast_saved(saved),
            SavedRelationInner::One(_) => Err(Error::conversion(
                "Expected a related model collection but received a single relation".to_string(),
            )),
        }
    }
}

/// The saved value as `T`, cloned only when a clone of the result shares it.
fn downcast_saved<T: Clone + Send + Sync + 'static>(
    saved: Arc<dyn Any + Send + Sync>,
) -> Result<T> {
    let saved = saved.downcast::<T>().map_err(|_| {
        Error::conversion(format!(
            "the saved relation does not hold {}",
            std::any::type_name::<T>()
        ))
    })?;
    Ok(Arc::try_unwrap(saved).unwrap_or_else(|shared| (*shared).clone()))
}

fn require_scalar_primary_key<M: Model>(
    primary_key: &M::PrimaryKey,
    context: &str,
) -> Result<serde_json::Value> {
    let value = serde_json::to_value(primary_key)
        .map_err(|e| Error::conversion(format!("Failed to serialize primary key: {}", e)))?;
    if value.is_array() || value.is_object() {
        return Err(Error::query(format!(
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
/// written into the serialized model it would land in serde's ignored-field
/// bucket, leaving the child's foreign key unchanged while the save succeeds.
fn resolve_foreign_key_field<R: Model>(foreign_key: &str) -> Result<&'static str> {
    R::canonical_field_name(foreign_key).ok_or_else(|| {
        Error::query(format!(
            "Unknown foreign key '{}' for {}; expected one of: {}",
            foreign_key,
            R::table_name(),
            R::field_names().join(", ")
        ))
    })
}

fn apply_foreign_key<R: Model>(
    mut related: R,
    foreign_key: &str,
    parent_pk_value: &serde_json::Value,
) -> Result<R> {
    let foreign_key = resolve_foreign_key_field::<R>(foreign_key)?;
    // Set in place rather than through the model's serde, whose derive may
    // write the field under a renamed key the parent's value would miss.
    if related.set_field_json(foreign_key, parent_pk_value.clone())? {
        Ok(related)
    } else {
        Err(Error::conversion(format!(
            "{} has no field '{}' to hold the parent's key",
            R::table_name(),
            foreign_key
        )))
    }
}

/// Point every child at the parent and save it: a new child is inserted and
/// one already stored is updated, as `save_with_one` does its child.
async fn save_related<R: Model>(
    related: Vec<R>,
    foreign_key: &str,
    parent_pk_value: &serde_json::Value,
) -> Result<Vec<R>> {
    let mut saved = Vec::with_capacity(related.len());
    for item in related {
        saved.push(
            apply_foreign_key(item, foreign_key, parent_pk_value)?
                .save()
                .await?,
        );
    }

    Ok(saved)
}

/// Save `parent`, then each of `children` pointed at its key, in one
/// transaction. The foreign-key name is resolved first, so a bad one fails
/// before anything is written.
async fn save_parent_then<M: Model, R: Model>(
    parent: M,
    children: Vec<R>,
    foreign_key: &str,
    context: &'static str,
) -> Result<(M, Vec<R>)> {
    let foreign_key = resolve_foreign_key_field::<R>(foreign_key)?;

    super::crud::transaction(move |_| {
        Box::pin(async move {
            let parent = parent.save().await?;
            let pk_value = require_scalar_primary_key::<M>(&parent.primary_key(), context)?;
            let children = save_related(children, foreign_key, &pk_value).await?;

            Ok((parent, children))
        })
    })
    .await
}

/// Extension trait for cascade save operations.
///
/// Each method writes the parent and its children as one unit of work: they
/// run in a single transaction — a SAVEPOINT when the caller already opened
/// one — so a failing child also rolls back the parent's write.
#[async_trait]
pub trait NestedSave: Model {
    async fn save_with_one<R: Model>(self, related: R, foreign_key: &str) -> Result<(Self, R)> {
        let (parent, mut related) =
            save_parent_then(self, vec![related], foreign_key, "save_with_one").await?;
        let related = related.pop().expect("save_related saves every child");
        Ok((parent, related))
    }

    async fn save_with_many<R: Model>(
        self,
        related: Vec<R>,
        foreign_key: &str,
    ) -> Result<(Self, Vec<R>)> {
        save_parent_then(self, related, foreign_key, "save_with_many").await
    }

    async fn update_with_one<R: Model>(self, related: R) -> Result<(Self, R)> {
        super::crud::transaction(move |_| {
            Box::pin(async move {
                let parent = self.update().await?;
                let related = related.update().await?;

                Ok((parent, related))
            })
        })
        .await
    }

    async fn update_with_many<R: Model>(self, related: Vec<R>) -> Result<(Self, Vec<R>)> {
        super::crud::transaction(move |_| {
            Box::pin(async move {
                let parent = self.update().await?;
                let mut updated = Vec::with_capacity(related.len());
                for item in related {
                    updated.push(item.update().await?);
                }

                Ok((parent, updated))
            })
        })
        .await
    }

    /// Delete the children, then the parent, and return the total row count.
    async fn delete_with_many<R: Model>(self, related: Vec<R>) -> Result<u64> {
        super::crud::transaction(move |_| {
            Box::pin(async move {
                let mut deleted = 0;
                for item in related {
                    deleted += item.delete().await?;
                }

                Ok(deleted + self.delete().await?)
            })
        })
        .await
    }
}

impl<M: Model> NestedSave for M {}

/// Builder for nested/cascade saves.
///
/// `with_one` relations are saved, and returned, before `with_many` ones
/// whatever order they were added in.
pub struct NestedSaveBuilder<M: Model> {
    parent: M,
    one_relations: Vec<Box<dyn RelationSaveOp>>,
    many_relations: Vec<Box<dyn RelationSaveOp>>,
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
        self.one_relations.push(Box::new(OneRelationSaveFn {
            related,
            foreign_key: foreign_key.to_string(),
        }));
        self
    }

    pub fn with_many<R: Model>(mut self, related: Vec<R>, foreign_key: &str) -> Self {
        self.many_relations.push(Box::new(ManyRelationSaveFn {
            related,
            foreign_key: foreign_key.to_string(),
        }));
        self
    }

    /// Save the parent and every relation in one transaction, as
    /// [`NestedSave`] does.
    pub async fn save(self) -> Result<(M, Vec<SavedRelation>)> {
        let Self {
            parent,
            one_relations,
            many_relations,
        } = self;

        super::crud::transaction(move |_| {
            Box::pin(async move {
                let parent = parent.save().await?;
                let pk_value =
                    require_scalar_primary_key::<M>(&parent.primary_key(), "nested save builder")?;

                let mut saved_relations =
                    Vec::with_capacity(one_relations.len() + many_relations.len());
                for relation in one_relations.into_iter().chain(many_relations) {
                    saved_relations.push(relation.run(pk_value.clone()).await?);
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
