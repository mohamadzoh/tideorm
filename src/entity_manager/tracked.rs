#![allow(missing_docs)]

use std::future::Future;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Error, Result};
use crate::model::{Model, ModelMeta};
use crate::relations::require_scalar_relation_key;

use super::{__model_entity_manager_key, EntityManager, TideEntityManagerMeta};

#[derive(Debug, Clone)]
pub struct TrackedHasMany<T: Model> {
    pub foreign_key: &'static str,
    pub local_key: &'static str,
    pub relation_name: &'static str,
    pub owner_table: &'static str,
    pub child_table: &'static str,
    owner_key: Option<String>,
    plain: crate::relations::DirectHasMany<T>,
    cached: Option<Vec<T>>,
    parent_pk: Option<serde_json::Value>,
    entity_manager: Option<Arc<EntityManager>>,
    _marker: PhantomData<T>,
}

impl<T: Model> Default for TrackedHasMany<T> {
    fn default() -> Self {
        Self::new("", "")
    }
}

impl<T: Model> TrackedHasMany<T> {
    fn ensure_configured(&self) -> Result<()> {
        if self.foreign_key.is_empty() || self.local_key.is_empty() {
            return Err(Error::invalid_query(
                "HasMany relation is not configured; call with_relations() before loading"
                    .to_string(),
            ));
        }

        Ok(())
    }

    pub fn new(foreign_key: &'static str, local_key: &'static str) -> Self {
        Self {
            foreign_key,
            local_key,
            relation_name: "",
            owner_table: "",
            child_table: "",
            owner_key: None,
            plain: crate::relations::DirectHasMany::new(foreign_key, local_key),
            cached: None,
            parent_pk: None,
            entity_manager: None,
            _marker: PhantomData,
        }
    }

    pub fn with_metadata(
        mut self,
        relation_name: &'static str,
        owner_table: &'static str,
        child_table: &'static str,
    ) -> Self {
        self.relation_name = relation_name;
        self.owner_table = owner_table;
        self.child_table = child_table;
        self
    }

    pub fn with_owner_key(mut self, owner_key: String) -> Self {
        self.owner_key = Some(owner_key);
        self
    }

    pub fn with_parent_pk(mut self, pk: serde_json::Value) -> Self {
        self.plain = self.plain.with_parent_pk(pk.clone());
        self.parent_pk = Some(pk);
        self
    }

    #[doc(hidden)]
    pub fn set_cached(&mut self, models: Vec<T>) {
        self.plain.set_cached(models.clone());
        self.cached = Some(models);
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        let same_relation = self.foreign_key == previous.foreign_key
            && self.local_key == previous.local_key
            && self.parent_pk == previous.parent_pk
            && self.relation_name == previous.relation_name
            && self.owner_table == previous.owner_table
            && self.child_table == previous.child_table;

        let allow_cached_without_context = previous.parent_pk.is_none();

        if same_relation {
            self.plain.preserve_runtime_state_from(&previous.plain);
            if self.entity_manager.is_none() {
                self.entity_manager = previous.entity_manager.clone();
            }
            if self.owner_key.is_none() {
                self.owner_key = previous.owner_key.clone();
            }
        }

        if ((allow_cached_without_context && previous.cached.is_some()) || same_relation)
            && self.cached.is_none()
        {
            self.cached = previous.cached.clone();
            if let Some(cached) = self.cached.clone() {
                self.plain.set_cached(cached);
            }
        }
    }

    pub fn as_mut(&mut self) -> Option<&mut Vec<T>> {
        self.cached.as_mut()
    }

    pub fn items(&self) -> Option<&Vec<T>> {
        self.cached.as_ref()
    }

    pub fn get_cached(&self) -> Option<&[T]> {
        self.cached.as_deref()
    }

    pub fn is_loaded(&self) -> bool {
        self.cached.is_some()
    }

    pub fn current_keys(&self) -> Result<Vec<String>>
    where
        T: Model + ModelMeta,
    {
        let mut keys = Vec::new();
        for item in self.cached.as_deref().unwrap_or(&[]) {
            if let Some(key) = __model_entity_manager_key(item)? {
                keys.push(key);
            }
        }

        Ok(keys)
    }

    pub async fn load_in_entity_manager(
        &mut self,
        entity_manager: &Arc<EntityManager>,
    ) -> Result<&Vec<T>>
    where
        T: TideEntityManagerMeta + Clone + Send + Sync + 'static,
    {
        if self.cached.is_some() {
            if let Some(items) = self.cached.as_ref() {
                for entity in items.iter().cloned() {
                    entity_manager.put(entity);
                }
            }

            self.entity_manager = Some(entity_manager.clone());
            let owner_key = self.owner_key.as_deref().ok_or_else(|| {
                Error::query(format!(
                    "entity manager owner key not set for relation '{}'",
                    self.relation_name
                ))
            })?;
            let ids = self.current_keys()?;
            entity_manager
                .snapshot::<T>(self.owner_table, owner_key, self.relation_name, &ids)
                .await;
            return Ok(self.cached.as_ref().expect("relation cache should exist"));
        }

        self.ensure_configured()?;

        let pk_value = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk_value = require_scalar_relation_key(pk_value, "HasMany::load")?;
        let owner_key = self.owner_key.as_deref().ok_or_else(|| {
            Error::query(format!(
                "entity manager owner key not set for relation '{}'",
                self.relation_name
            ))
        })?;

        let loaded = T::query_with(entity_manager.db.as_ref())
            .where_eq(self.foreign_key, pk_value.clone())
            .get()
            .await?;

        let mut registered = Vec::with_capacity(loaded.len());
        for entity in loaded {
            registered.push(entity_manager.register(entity).await);
        }

        self.plain.set_cached(registered.clone());
        self.cached = Some(registered);
        self.entity_manager = Some(entity_manager.clone());

        let ids = self.current_keys()?;
        entity_manager
            .snapshot::<T>(self.owner_table, owner_key, self.relation_name, &ids)
            .await;

        Ok(self
            .cached
            .as_ref()
            .expect("relation cache should exist after load"))
    }

    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<T>>
    where
        F: FnOnce(crate::query::QueryBuilder<T>) -> crate::query::QueryBuilder<T> + Send,
    {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasMany::load_with")?;

        let query = if let Some(entity_manager) = &self.entity_manager {
            T::query_with(entity_manager.db.as_ref())
        } else {
            T::query()
        };

        constraint_fn(query.where_eq(self.foreign_key, pk.clone()))
            .get()
            .await
    }

    pub async fn count(&self) -> Result<u64> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasMany::count")?;

        let query = if let Some(entity_manager) = &self.entity_manager {
            T::query_with(entity_manager.db.as_ref())
        } else {
            T::query()
        };

        query.where_eq(self.foreign_key, pk.clone()).count().await
    }

    pub async fn exists(&self) -> Result<bool> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasMany::exists")?;

        let query = if let Some(entity_manager) = &self.entity_manager {
            T::query_with(entity_manager.db.as_ref())
        } else {
            T::query()
        };

        query.where_eq(self.foreign_key, pk.clone()).exists().await
    }
}

pub trait TrackedHasManyEntityManagerExt<T: Model> {
    fn load<'a>(
        &'a mut self,
        entity_manager: &'a Arc<EntityManager>,
    ) -> Pin<Box<dyn Future<Output = Result<&'a Vec<T>>> + Send + 'a>>;
}

impl<T> TrackedHasManyEntityManagerExt<T> for TrackedHasMany<T>
where
    T: Model + TideEntityManagerMeta + Clone + Send + Sync + 'static,
{
    fn load<'a>(
        &'a mut self,
        entity_manager: &'a Arc<EntityManager>,
    ) -> Pin<Box<dyn Future<Output = Result<&'a Vec<T>>> + Send + 'a>> {
        Box::pin(async move { self.load_in_entity_manager(entity_manager).await })
    }
}

impl<T> super::EntityManagerLoad for TrackedHasMany<T>
where
    T: Model + TideEntityManagerMeta + Clone + Send + Sync + 'static,
{
    type Output<'a>
        = &'a Vec<T>
    where
        Self: 'a;

    fn load_with_entity_manager<'a>(
        &'a mut self,
        entity_manager: &'a Arc<EntityManager>,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Output<'a>>> + Send + 'a>> {
        Box::pin(async move { self.load_in_entity_manager(entity_manager).await })
    }
}

impl<T: Model> Deref for TrackedHasMany<T> {
    type Target = crate::relations::DirectHasMany<T>;

    fn deref(&self) -> &Self::Target {
        &self.plain
    }
}

impl<T: Model> DerefMut for TrackedHasMany<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.plain
    }
}

impl<T: Model + Serialize> Serialize for TrackedHasMany<T> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, T: Model> Deserialize<'de> for TrackedHasMany<T> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let cached = Option::<Vec<T>>::deserialize(deserializer)?;
        let mut relation = Self {
            cached,
            ..Self::default()
        };

        if let Some(cached) = relation.cached.clone() {
            relation.plain.set_cached(cached);
        }

        Ok(relation)
    }
}
