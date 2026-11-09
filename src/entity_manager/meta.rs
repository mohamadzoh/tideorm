#![allow(missing_docs)]

use std::sync::Arc;

use crate::error::{Error, Result};
use crate::model::Model;

use super::EntityManager;

#[doc(hidden)]
pub trait TideEntityManagerSync: Model {
    fn tide_sync_entity_manager_relations<'a>(
        &'a mut self,
        entity_manager: &'a Arc<EntityManager>,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
}

#[doc(hidden)]
pub trait TideEntityManagerMergePersisted {
    fn tide_merge_persisted(&mut self, persisted: Self);

    /// Take every relation wrapper of `source`, its cache, load and the keys
    /// a save diffs it against included.
    fn tide_relations_from(&mut self, _source: &Self) {}

    /// Whether every relation of `other` caches what this entity's does.
    fn tide_relations_match(&self, _other: &Self) -> bool {
        true
    }
}

/// A model an [`EntityManager`] can manage: every model the derive generates
/// while the `entity-manager` feature is on. Generic code names this one
/// bound to hand models to a manager.
///
/// ```
/// use std::sync::Arc;
/// use tideorm::prelude::*;
///
/// fn track<M: ManagedModel>(manager: &Arc<EntityManager>, model: M) -> Managed<M> {
///     manager.persist(model)
/// }
/// ```
pub trait ManagedModel: TideEntityManagerMergePersisted + TideEntityManagerSync {}

impl<T: TideEntityManagerMergePersisted + TideEntityManagerSync> ManagedModel for T {}

#[doc(hidden)]
pub fn pk_to_entity_manager_key<T>(value: &T) -> Result<String>
where
    T: serde::Serialize,
{
    serde_json::to_string(value).map_err(Error::from)
}

/// `model`'s identity key as the identity maps file it and every relation
/// wrapper's owner key writes it, where nothing can fail: a key that does not
/// serialize falls back to the model's own rendering of it, which maps equal
/// keys alike and holds " = ", so it never collides with a serialized key.
pub(crate) fn model_identity_key<M: Model>(model: &M) -> String {
    let primary_key = model.primary_key();
    pk_to_entity_manager_key(&primary_key).unwrap_or_else(|_| M::primary_key_display(&primary_key))
}

#[doc(hidden)]
pub fn model_entity_manager_key<T>(model: &T) -> Result<Option<String>>
where
    T: Model,
{
    let primary_key = model.primary_key();
    if T::primary_key_is_new(&primary_key) {
        return Ok(None);
    }

    pk_to_entity_manager_key(&primary_key).map(Some)
}
