use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::marker::PhantomData;
#[cfg(feature = "entity-manager")]
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::model::Model;
use crate::query::QueryBuilder;

use super::helpers::{cached_ref, ensure_relation_configured, preserve_cached_value};
use super::require_scalar_relation_key;

fn has_active_database() -> bool {
    crate::database::__current_db().is_ok()
}

#[derive(Debug, Clone)]
pub struct HasOne<E: Model> {
    pub foreign_key: &'static str,
    pub local_key: &'static str,
    #[cfg(feature = "entity-manager")]
    pub relation_name: &'static str,
    #[cfg(feature = "entity-manager")]
    pub owner_table: &'static str,
    #[cfg(feature = "entity-manager")]
    pub child_table: &'static str,
    cached: Option<Box<E>>,
    loaded: bool,
    parent_pk: Option<serde_json::Value>,
    #[cfg(feature = "entity-manager")]
    owner_key: Option<String>,
    #[cfg(feature = "entity-manager")]
    entity_manager: Option<Arc<crate::entity_manager::EntityManager>>,
    _marker: PhantomData<E>,
}

impl<E: Model> HasOne<E> {
    fn ensure_configured(&self) -> Result<()> {
        ensure_relation_configured("HasOne", &[self.foreign_key, self.local_key])
    }

    pub fn new(foreign_key: &'static str, local_key: &'static str) -> Self {
        Self {
            foreign_key,
            local_key,
            #[cfg(feature = "entity-manager")]
            relation_name: "",
            #[cfg(feature = "entity-manager")]
            owner_table: "",
            #[cfg(feature = "entity-manager")]
            child_table: "",
            cached: None,
            loaded: false,
            parent_pk: None,
            #[cfg(feature = "entity-manager")]
            owner_key: None,
            #[cfg(feature = "entity-manager")]
            entity_manager: None,
            _marker: PhantomData,
        }
    }

    pub fn with_parent_pk(mut self, pk: serde_json::Value) -> Self {
        self.parent_pk = Some(pk);
        self
    }

    #[cfg(feature = "entity-manager")]
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

    #[cfg(feature = "entity-manager")]
    pub fn with_owner_key(mut self, owner_key: String) -> Self {
        self.owner_key = Some(owner_key);
        self
    }

    #[doc(hidden)]
    pub fn set_cached(&mut self, model: Option<E>) {
        self.cached = model.map(Box::new);
        self.loaded = true;
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        let same_relation = self.foreign_key == previous.foreign_key
            && self.local_key == previous.local_key
            && self.parent_pk == previous.parent_pk
            && {
                #[cfg(feature = "entity-manager")]
                {
                    self.relation_name == previous.relation_name
                        && self.owner_table == previous.owner_table
                        && self.child_table == previous.child_table
                }
                #[cfg(not(feature = "entity-manager"))]
                {
                    true
                }
            };

        preserve_cached_value(
            &mut self.cached,
            &previous.cached,
            previous.parent_pk.is_none(),
            same_relation,
        );

        if previous.parent_pk.is_none() && !self.loaded {
            self.loaded = previous.loaded;
        }

        #[cfg(feature = "entity-manager")]
        if same_relation {
            if !self.loaded {
                self.loaded = previous.loaded;
            }
            if self.owner_key.is_none() {
                self.owner_key = previous.owner_key.clone();
            }
            if self.entity_manager.is_none() {
                self.entity_manager = previous.entity_manager.clone();
            }
        }

        #[cfg(not(feature = "entity-manager"))]
        if same_relation && !self.loaded {
            self.loaded = previous.loaded;
        }
    }

    pub async fn load(&self) -> Result<Option<E>> {
        #[cfg(feature = "entity-manager")]
        if self.loaded && self.entity_manager.is_some() {
            return Ok(self.cached.as_deref().cloned());
        }

        let can_query = {
            #[cfg(feature = "entity-manager")]
            {
                has_active_database() || self.entity_manager.is_some()
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                has_active_database()
            }
        };

        if can_query && self.ensure_configured().is_ok() {
            if let Some(pk) = self.parent_pk.as_ref() {
                let pk = require_scalar_relation_key(pk, "HasOne::load")?;

                let query = {
                    #[cfg(feature = "entity-manager")]
                    {
                        if let Some(entity_manager) = &self.entity_manager {
                            E::query_with(entity_manager.database())
                        } else {
                            E::query()
                        }
                    }
                    #[cfg(not(feature = "entity-manager"))]
                    {
                        E::query()
                    }
                };

                return query.where_eq(self.foreign_key, pk.clone()).first().await;
            }
        }

        if self.loaded {
            return Ok(self.cached.as_deref().cloned());
        }

        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasOne::load")?;

        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .first()
            .await
    }

    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Option<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasOne::load_with")?;

        let query = {
            #[cfg(feature = "entity-manager")]
            {
                if let Some(entity_manager) = &self.entity_manager {
                    E::query_with(entity_manager.database())
                } else {
                    E::query()
                }
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                E::query()
            }
        }
        .where_eq(self.foreign_key, pk.clone());
        constraint_fn(query).first().await
    }

    pub async fn exists(&self) -> Result<bool> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasOne::exists")?;

        let query = {
            #[cfg(feature = "entity-manager")]
            {
                if let Some(entity_manager) = &self.entity_manager {
                    E::query_with(entity_manager.database())
                } else {
                    E::query()
                }
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                E::query()
            }
        };

        query.where_eq(self.foreign_key, pk.clone()).exists().await
    }

    pub fn as_mut(&mut self) -> Option<&mut E> {
        self.cached.as_deref_mut()
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn clear(&mut self) {
        self.cached = None;
        self.loaded = true;
    }

    pub fn get_cached(&self) -> Option<&E> {
        cached_ref(&self.cached)
    }

    #[cfg(feature = "entity-manager")]
    pub fn current_key(&self) -> Result<Option<String>>
    where
        E: crate::model::ModelMeta,
    {
        match self.cached.as_deref() {
            Some(item) => crate::entity_manager::__model_entity_manager_key(item),
            None => Ok(None),
        }
    }

    #[cfg(feature = "entity-manager")]
    pub async fn load_in_entity_manager(
        &mut self,
        entity_manager: &Arc<crate::entity_manager::EntityManager>,
    ) -> Result<Option<&E>>
    where
        E: crate::internal::InternalModel
            + crate::entity_manager::TideEntityManagerMeta
            + Clone
            + Send
            + Sync
            + 'static,
    {
        if self.loaded {
            if let Some(cached) = self.cached.as_deref() {
                entity_manager.put(cached.clone());
            }

            self.entity_manager = Some(entity_manager.clone());
            let owner_key = self.owner_key.as_deref().ok_or_else(|| {
                Error::query(format!(
                    "entity manager owner key not set for relation '{}'",
                    self.relation_name
                ))
            })?;
            let ids = self.current_key()?.into_iter().collect::<Vec<_>>();
            entity_manager
                .snapshot::<E>(self.owner_table, owner_key, self.relation_name, &ids)
                .await;
            return Ok(self.cached.as_deref());
        }

        self.ensure_configured()?;

        let pk_value = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk_value = require_scalar_relation_key(pk_value, "HasOne::load_in_entity_manager")?;
        let owner_key = self.owner_key.as_deref().ok_or_else(|| {
            Error::query(format!(
                "entity manager owner key not set for relation '{}'",
                self.relation_name
            ))
        })?;

        let loaded =
            if let Some(cached) = entity_manager.find_by_field::<E>(self.foreign_key, pk_value)? {
                Some(cached)
            } else {
                E::query_with(entity_manager.database())
                    .where_eq(self.foreign_key, pk_value.clone())
                    .first()
                    .await?
            };

        self.cached = match loaded {
            Some(entity) => Some(Box::new(entity_manager.register(entity).await)),
            None => None,
        };
        self.loaded = true;
        self.entity_manager = Some(entity_manager.clone());

        let ids = self.current_key()?.into_iter().collect::<Vec<_>>();
        entity_manager
            .snapshot::<E>(self.owner_table, owner_key, self.relation_name, &ids)
            .await;

        Ok(self.cached.as_deref())
    }
}

impl<E: Model> Default for HasOne<E> {
    fn default() -> Self {
        Self {
            foreign_key: "",
            local_key: "",
            #[cfg(feature = "entity-manager")]
            relation_name: "",
            #[cfg(feature = "entity-manager")]
            owner_table: "",
            #[cfg(feature = "entity-manager")]
            child_table: "",
            cached: None,
            loaded: false,
            parent_pk: None,
            #[cfg(feature = "entity-manager")]
            owner_key: None,
            #[cfg(feature = "entity-manager")]
            entity_manager: None,
            _marker: PhantomData,
        }
    }
}

impl<E: Model + Serialize> Serialize for HasOne<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for HasOne<E> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let cached = Option::<E>::deserialize(deserializer)?;
        let loaded = cached.is_some();
        Ok(Self {
            cached: cached.map(Box::new),
            loaded,
            ..Self::default()
        })
    }
}

#[cfg(feature = "entity-manager")]
impl<E> crate::entity_manager::EntityManagerLoad for HasOne<E>
where
    E: crate::internal::InternalModel
        + crate::entity_manager::TideEntityManagerMeta
        + Model
        + Clone
        + Send
        + Sync
        + 'static,
{
    type Output<'a>
        = Option<&'a E>
    where
        Self: 'a;

    fn load_with_entity_manager<'a>(
        &'a mut self,
        entity_manager: &'a Arc<crate::entity_manager::EntityManager>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Output<'a>>> + Send + 'a>>
    {
        Box::pin(async move { self.load_in_entity_manager(entity_manager).await })
    }
}

#[cfg_attr(feature = "entity-manager", allow(dead_code))]
#[derive(Debug, Clone)]
pub struct HasMany<E: Model> {
    pub foreign_key: &'static str,
    pub local_key: &'static str,
    cached: Option<Vec<E>>,
    parent_pk: Option<serde_json::Value>,
    _marker: PhantomData<E>,
}

#[cfg_attr(feature = "entity-manager", allow(dead_code))]
impl<E: Model> HasMany<E> {
    fn ensure_configured(&self) -> Result<()> {
        ensure_relation_configured("HasMany", &[self.foreign_key, self.local_key])
    }

    pub fn new(foreign_key: &'static str, local_key: &'static str) -> Self {
        Self {
            foreign_key,
            local_key,
            cached: None,
            parent_pk: None,
            _marker: PhantomData,
        }
    }

    pub fn with_parent_pk(mut self, pk: serde_json::Value) -> Self {
        self.parent_pk = Some(pk);
        self
    }

    #[doc(hidden)]
    pub fn set_cached(&mut self, models: Vec<E>) {
        self.cached = Some(models);
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        preserve_cached_value(
            &mut self.cached,
            &previous.cached,
            previous.parent_pk.is_none(),
            self.foreign_key == previous.foreign_key
                && self.local_key == previous.local_key
                && self.parent_pk == previous.parent_pk,
        );
    }

    pub async fn load(&self) -> Result<Vec<E>> {
        if has_active_database() && self.ensure_configured().is_ok() {
            if let Some(pk) = self.parent_pk.as_ref() {
                let pk = require_scalar_relation_key(pk, "HasMany::load")?;

                return E::query()
                    .where_eq(self.foreign_key, pk.clone())
                    .get()
                    .await;
            }
        }

        if let Some(cached) = &self.cached {
            return Ok(cached.clone());
        }

        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasMany::load")?;

        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .get()
            .await
    }

    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasMany::load_with")?;

        let query = E::query().where_eq(self.foreign_key, pk.clone());
        constraint_fn(query).get().await
    }

    pub async fn count(&self) -> Result<u64> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasMany::count")?;

        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .count()
            .await
    }

    pub async fn exists(&self) -> Result<bool> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasMany::exists")?;

        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .exists()
            .await
    }

    pub fn get_cached(&self) -> Option<&[E]> {
        cached_ref(&self.cached)
    }
}

impl<E: Model> Default for HasMany<E> {
    fn default() -> Self {
        Self {
            foreign_key: "",
            local_key: "",
            cached: None,
            parent_pk: None,
            _marker: PhantomData,
        }
    }
}

impl<E: Model + Serialize> Serialize for HasMany<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for HasMany<E> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let cached = Option::<Vec<E>>::deserialize(deserializer)?;
        Ok(Self {
            cached,
            ..Self::default()
        })
    }
}

#[derive(Debug, Clone)]
pub struct BelongsTo<E: Model> {
    pub foreign_key: &'static str,
    pub owner_key: &'static str,
    cached: Option<Box<E>>,
    loaded: bool,
    fk_value: Option<serde_json::Value>,
    #[cfg(feature = "entity-manager")]
    entity_manager: Option<Arc<crate::entity_manager::EntityManager>>,
    _marker: PhantomData<E>,
}

impl<E: Model> BelongsTo<E> {
    fn ensure_configured(&self) -> Result<()> {
        ensure_relation_configured("BelongsTo", &[self.foreign_key, self.owner_key])
    }

    pub fn new(foreign_key: &'static str, owner_key: &'static str) -> Self {
        Self {
            foreign_key,
            owner_key,
            cached: None,
            loaded: false,
            fk_value: None,
            #[cfg(feature = "entity-manager")]
            entity_manager: None,
            _marker: PhantomData,
        }
    }

    pub fn with_fk_value(mut self, fk: serde_json::Value) -> Self {
        self.fk_value = Some(fk);
        self
    }

    #[doc(hidden)]
    pub fn set_cached(&mut self, model: Option<E>) {
        self.cached = model.map(Box::new);
        self.loaded = true;
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        let same_relation = self.foreign_key == previous.foreign_key
            && self.owner_key == previous.owner_key
            && self.fk_value == previous.fk_value;

        preserve_cached_value(
            &mut self.cached,
            &previous.cached,
            previous.fk_value.is_none(),
            same_relation,
        );

        if previous.fk_value.is_none() && !self.loaded {
            self.loaded = previous.loaded;
        }

        #[cfg(feature = "entity-manager")]
        if same_relation && self.entity_manager.is_none() {
            self.entity_manager = previous.entity_manager.clone();
        }

        if same_relation && !self.loaded {
            self.loaded = previous.loaded;
        }
    }

    pub async fn load(&self) -> Result<Option<E>> {
        #[cfg(feature = "entity-manager")]
        if self.loaded && self.entity_manager.is_some() {
            return Ok(self.cached.as_deref().cloned());
        }

        let can_query = {
            #[cfg(feature = "entity-manager")]
            {
                has_active_database() || self.entity_manager.is_some()
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                has_active_database()
            }
        };

        if can_query && self.ensure_configured().is_ok() {
            if let Some(fk) = self.fk_value.as_ref() {
                let fk = require_scalar_relation_key(fk, "BelongsTo::load")?;

                let query = {
                    #[cfg(feature = "entity-manager")]
                    {
                        if let Some(entity_manager) = &self.entity_manager {
                            E::query_with(entity_manager.database())
                        } else {
                            E::query()
                        }
                    }
                    #[cfg(not(feature = "entity-manager"))]
                    {
                        E::query()
                    }
                };

                return query.where_eq(self.owner_key, fk.clone()).first().await;
            }
        }

        if self.loaded {
            return Ok(self.cached.as_deref().cloned());
        }

        self.ensure_configured()?;

        let fk = self
            .fk_value
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Foreign key value not set for relation")))?;
        let fk = require_scalar_relation_key(fk, "BelongsTo::load")?;

        E::query()
            .where_eq(self.owner_key, fk.clone())
            .first()
            .await
    }

    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Option<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        self.ensure_configured()?;

        let fk = self
            .fk_value
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Foreign key value not set for relation")))?;
        let fk = require_scalar_relation_key(fk, "BelongsTo::load_with")?;

        let query = {
            #[cfg(feature = "entity-manager")]
            {
                if let Some(entity_manager) = &self.entity_manager {
                    E::query_with(entity_manager.database())
                } else {
                    E::query()
                }
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                E::query()
            }
        }
        .where_eq(self.owner_key, fk.clone());
        constraint_fn(query).first().await
    }

    pub async fn exists(&self) -> Result<bool> {
        self.ensure_configured()?;

        let fk = self
            .fk_value
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Foreign key value not set for relation")))?;
        let fk = require_scalar_relation_key(fk, "BelongsTo::exists")?;

        let query = {
            #[cfg(feature = "entity-manager")]
            {
                if let Some(entity_manager) = &self.entity_manager {
                    E::query_with(entity_manager.database())
                } else {
                    E::query()
                }
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                E::query()
            }
        };

        query.where_eq(self.owner_key, fk.clone()).exists().await
    }

    pub fn as_mut(&mut self) -> Option<&mut E> {
        self.cached.as_deref_mut()
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn get_cached(&self) -> Option<&E> {
        cached_ref(&self.cached)
    }

    #[cfg(feature = "entity-manager")]
    pub async fn load_in_entity_manager(
        &mut self,
        entity_manager: &Arc<crate::entity_manager::EntityManager>,
    ) -> Result<Option<&E>>
    where
        E: crate::internal::InternalModel
            + crate::entity_manager::TideEntityManagerMeta
            + Clone
            + Send
            + Sync
            + 'static,
    {
        if self.loaded {
            if let Some(cached) = self.cached.as_deref() {
                entity_manager.put(cached.clone());
            }

            self.entity_manager = Some(entity_manager.clone());
            return Ok(self.cached.as_deref());
        }

        self.ensure_configured()?;

        let fk = self
            .fk_value
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Foreign key value not set for relation")))?;
        let fk = require_scalar_relation_key(fk, "BelongsTo::load_in_entity_manager")?;

        let loaded = if let Some(cached) = entity_manager.find_by_field::<E>(self.owner_key, fk)? {
            Some(cached)
        } else {
            E::query_with(entity_manager.database())
                .where_eq(self.owner_key, fk.clone())
                .first()
                .await?
        };

        self.cached = match loaded {
            Some(entity) => Some(Box::new(entity_manager.register(entity).await)),
            None => None,
        };
        self.loaded = true;
        self.entity_manager = Some(entity_manager.clone());

        Ok(self.cached.as_deref())
    }
}

impl<E: Model> Default for BelongsTo<E> {
    fn default() -> Self {
        Self {
            foreign_key: "",
            owner_key: "",
            cached: None,
            loaded: false,
            fk_value: None,
            #[cfg(feature = "entity-manager")]
            entity_manager: None,
            _marker: PhantomData,
        }
    }
}

impl<E: Model + Serialize> Serialize for BelongsTo<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for BelongsTo<E> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let cached = Option::<E>::deserialize(deserializer)?;
        let loaded = cached.is_some();
        Ok(Self {
            cached: cached.map(Box::new),
            loaded,
            ..Self::default()
        })
    }
}

#[cfg(all(test, feature = "entity-manager"))]
#[path = "../../tests/unit/direct_entity_manager_relation_tests.rs"]
mod entity_manager_tests;

#[cfg(feature = "entity-manager")]
impl<E> crate::entity_manager::EntityManagerLoad for BelongsTo<E>
where
    E: crate::internal::InternalModel
        + crate::entity_manager::TideEntityManagerMeta
        + Model
        + Clone
        + Send
        + Sync
        + 'static,
{
    type Output<'a>
        = Option<&'a E>
    where
        Self: 'a;

    fn load_with_entity_manager<'a>(
        &'a mut self,
        entity_manager: &'a Arc<crate::entity_manager::EntityManager>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Output<'a>>> + Send + 'a>>
    {
        Box::pin(async move { self.load_in_entity_manager(entity_manager).await })
    }
}
