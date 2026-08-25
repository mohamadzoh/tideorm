#![allow(missing_docs)]

use std::sync::Arc;

use crate::error::{Error, Result};
use crate::model::{Model, ModelMeta};

use super::EntityManager;

pub trait TideEntityManagerMeta {
    fn tide_table_name() -> &'static str
    where
        Self: Sized;

    fn tide_pk_key(&self) -> String;

    /// Whether this entity's primary key is still the type's default, i.e. no
    /// row has been inserted for it yet.
    ///
    /// [`Self::tide_pk_key`] is infallible and has no notion of "unsaved", so it
    /// renders a default key as an ordinary string — `"0"` for an `i64`. Every
    /// unsaved instance of a model therefore produces the *same* identity key,
    /// and filing two of them in the identity map makes the second collide with
    /// the first. Callers that key the map must skip an entity for which this
    /// returns `true`: it has no identity to share yet.
    ///
    /// Defaults to `false` for hand-written implementations, which preserves the
    /// previous behaviour for anyone not deriving `Model`.
    fn tide_pk_is_new(&self) -> bool {
        false
    }

    /// Tables a row of this model points at with a foreign key, so they have to
    /// hold a row before this one can be inserted.
    ///
    /// Derived from the model's declared `belongs_to` relations. The flush
    /// planner turns these into dependency edges and orders inserts
    /// parent-first, deletes child-first. Only *declared* relations are
    /// visible here: a bare foreign-key column with no relation attribute
    /// contributes no edge, and a polymorphic (`MorphTo`) target has no
    /// statically known table, so neither constrains the flush order.
    ///
    /// Defaults to empty for hand-written implementations, which then flush in
    /// registration order within their operation kind.
    fn tide_parent_tables() -> Vec<&'static str>
    where
        Self: Sized,
    {
        Vec::new()
    }

    /// Tables whose rows point back at this model with a foreign key, so they
    /// can only be inserted once this one holds a row.
    ///
    /// Derived from the model's declared `has_one` and `has_many` relations;
    /// see [`TideEntityManagerMeta::tide_parent_tables`] for what is and is not
    /// visible. `has_many_through` contributes nothing because the pivot row is
    /// written by relation sync, after both sides are saved.
    fn tide_child_tables() -> Vec<&'static str>
    where
        Self: Sized,
    {
        Vec::new()
    }

    fn tide_attach_entity_manager_database(&mut self, _database: &crate::database::Database) {}
}

#[doc(hidden)]
pub trait TideEntityManagerFieldWriter {
    fn tide_set_field_value(&mut self, field: &str, value: serde_json::Value) -> Result<()>;
}

#[doc(hidden)]
pub trait TideEntityManagerSync:
    TideEntityManagerMeta + Model + Clone + Send + Sync + 'static
{
    fn tide_sync_entity_manager_relations<'a>(
        &'a mut self,
        entity_manager: &'a Arc<EntityManager>,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
}

#[doc(hidden)]
pub trait TideEntityManagerMergePersisted {
    fn tide_merge_persisted(&mut self, persisted: Self);
}

#[doc(hidden)]
pub fn pk_to_entity_manager_key<T>(value: &T) -> Result<String>
where
    T: serde::Serialize,
{
    serde_json::to_string(value).map_err(Error::from)
}

#[doc(hidden)]
pub fn model_entity_manager_key<T>(model: &T) -> Result<Option<String>>
where
    T: Model + ModelMeta,
{
    let primary_key = model.primary_key();
    if T::primary_key_is_new(&primary_key) {
        return Ok(None);
    }

    pk_to_entity_manager_key(&primary_key).map(Some)
}
