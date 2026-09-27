#![allow(missing_docs)]

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::Result;
use crate::model::Model;
use crate::relations::{DirectHasMany, SnapshotOwner, register_loaded};

use super::{EntityManager, TideEntityManagerMeta, model_entity_manager_key};

/// What `HasMany` names under the `entity-manager` feature: the plain
/// one-to-many relation, which it derefs to for every read, plus the owner
/// identity the entity manager needs to snapshot and sync it on save.
#[derive(Debug, Clone)]
pub struct TrackedHasMany<T: Model> {
    pub relation_name: &'static str,
    pub owner_table: &'static str,
    owner_key: Option<String>,
    plain: DirectHasMany<T>,
}

impl<T: Model> Default for TrackedHasMany<T> {
    fn default() -> Self {
        Self {
            relation_name: "",
            owner_table: "",
            owner_key: None,
            plain: DirectHasMany::default(),
        }
    }
}

impl<T: Model> TrackedHasMany<T> {
    pub fn new(foreign_key: &'static str, local_key: &'static str) -> Self {
        Self {
            plain: DirectHasMany::new(foreign_key, local_key),
            ..Self::default()
        }
    }

    pub fn with_metadata(mut self, relation_name: &'static str, owner_table: &'static str) -> Self {
        self.relation_name = relation_name;
        self.owner_table = owner_table;
        self
    }

    pub fn with_owner_key(mut self, owner_key: String) -> Self {
        self.owner_key = Some(owner_key);
        self
    }

    pub fn with_parent_pk(mut self, pk: serde_json::Value) -> Self {
        self.plain = self.plain.with_parent_pk(pk);
        self
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        let same_relation = self.relation_name == previous.relation_name
            && self.owner_table == previous.owner_table
            && self.plain.same_relation(&previous.plain);

        self.plain.preserve_runtime_state_from(&previous.plain);
        if same_relation && self.owner_key.is_none() {
            self.owner_key = previous.owner_key.clone();
        }
    }

    pub fn as_mut(&mut self) -> Option<&mut Vec<T>> {
        self.plain.cached.as_mut()
    }

    pub fn is_loaded(&self) -> bool {
        self.plain.cached.is_some()
    }

    /// Entity-manager keys of the cached rows, skipping any not inserted yet.
    pub fn current_keys(&self) -> Result<Vec<String>> {
        self.plain
            .cached
            .iter()
            .flatten()
            .filter_map(|item| model_entity_manager_key(item).transpose())
            .collect()
    }

    pub async fn load_in_entity_manager(
        &mut self,
        entity_manager: &Arc<EntityManager>,
    ) -> Result<&Vec<T>>
    where
        T: TideEntityManagerMeta,
    {
        let owner = SnapshotOwner::new(self.owner_table, &self.owner_key, self.relation_name)?;
        self.plain.attach_query_database(entity_manager.database());

        let models = match self.plain.cached.take() {
            Some(models) => models,
            None => {
                self.plain
                    .query("HasMany::load_in_entity_manager")?
                    .get()
                    .await?
            }
        };
        let cached = self.plain.cached.insert(models);
        register_loaded(entity_manager, cached.iter_mut(), Some(owner)).await?;
        Ok(cached)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/tracked_entity_manager_relation_tests.rs"]
mod tests;

pub trait TrackedHasManyEntityManagerExt<T: Model> {
    fn load<'a>(
        &'a mut self,
        entity_manager: &'a Arc<EntityManager>,
    ) -> impl std::future::Future<Output = Result<&'a Vec<T>>> + Send;
}

impl<T> TrackedHasManyEntityManagerExt<T> for TrackedHasMany<T>
where
    T: Model + TideEntityManagerMeta,
{
    async fn load<'a>(&'a mut self, entity_manager: &'a Arc<EntityManager>) -> Result<&'a Vec<T>> {
        self.load_in_entity_manager(entity_manager).await
    }
}

impl<T> super::EntityManagerLoad for TrackedHasMany<T>
where
    T: Model + TideEntityManagerMeta,
{
    type Output<'a>
        = &'a Vec<T>
    where
        Self: 'a;

    async fn load_with_entity_manager<'a>(
        &'a mut self,
        entity_manager: &'a Arc<EntityManager>,
    ) -> Result<Self::Output<'a>> {
        self.load_in_entity_manager(entity_manager).await
    }
}

impl<T: Model> Deref for TrackedHasMany<T> {
    type Target = DirectHasMany<T>;

    fn deref(&self) -> &Self::Target {
        &self.plain
    }
}

impl<T: Model> DerefMut for TrackedHasMany<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.plain
    }
}

impl<T: Model> Serialize for TrackedHasMany<T> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.plain.serialize(serializer)
    }
}

impl<'de, T: Model> Deserialize<'de> for TrackedHasMany<T> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self {
            plain: DirectHasMany::deserialize(deserializer)?,
            ..Self::default()
        })
    }
}
