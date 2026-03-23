use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::marker::PhantomData;

use crate::error::{Error, Result};
use crate::model::Model;
use crate::query::QueryBuilder;

use super::require_scalar_relation_key;

#[derive(Debug, Clone)]
pub struct HasOne<E: Model> {
    pub foreign_key: &'static str,
    pub local_key: &'static str,
    cached: Option<Box<E>>,
    parent_pk: Option<serde_json::Value>,
    _marker: PhantomData<E>,
}

impl<E: Model> HasOne<E> {
    fn ensure_configured(&self) -> Result<()> {
        if self.foreign_key.is_empty() || self.local_key.is_empty() {
            Err(Error::query(String::from(
                "HasOne relation is not configured; use HasOne::new(...) or a macro-generated relation field",
            )))
        } else {
            Ok(())
        }
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
    pub fn set_cached(&mut self, model: Option<E>) {
        self.cached = model.map(Box::new);
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        if self.foreign_key == previous.foreign_key
            && self.local_key == previous.local_key
            && self.parent_pk == previous.parent_pk
        {
            self.cached = previous.cached.clone();
        }
    }

    pub async fn load(&self) -> Result<Option<E>> {
        if let Some(cached) = &self.cached {
            return Ok(Some((**cached).clone()));
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

        let query = E::query().where_eq(self.foreign_key, pk.clone());
        constraint_fn(query).first().await
    }

    pub async fn exists(&self) -> Result<bool> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasOne::exists")?;

        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .exists()
            .await
    }

    pub fn get_cached(&self) -> Option<&E> {
        self.cached.as_deref()
    }
}

impl<E: Model> Default for HasOne<E> {
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

impl<E: Model + Serialize> Serialize for HasOne<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for HasOne<E> {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::default())
    }
}

#[derive(Debug, Clone)]
pub struct HasMany<E: Model> {
    pub foreign_key: &'static str,
    pub local_key: &'static str,
    cached: Option<Vec<E>>,
    parent_pk: Option<serde_json::Value>,
    _marker: PhantomData<E>,
}

impl<E: Model> HasMany<E> {
    fn ensure_configured(&self) -> Result<()> {
        if self.foreign_key.is_empty() || self.local_key.is_empty() {
            Err(Error::query(String::from(
                "HasMany relation is not configured; use HasMany::new(...) or a macro-generated relation field",
            )))
        } else {
            Ok(())
        }
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
        if self.foreign_key == previous.foreign_key
            && self.local_key == previous.local_key
            && self.parent_pk == previous.parent_pk
        {
            self.cached = previous.cached.clone();
        }
    }

    pub async fn load(&self) -> Result<Vec<E>> {
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
        self.cached.as_deref()
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
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::default())
    }
}

#[derive(Debug, Clone)]
pub struct BelongsTo<E: Model> {
    pub foreign_key: &'static str,
    pub owner_key: &'static str,
    cached: Option<Box<E>>,
    fk_value: Option<serde_json::Value>,
    _marker: PhantomData<E>,
}

impl<E: Model> BelongsTo<E> {
    fn ensure_configured(&self) -> Result<()> {
        if self.foreign_key.is_empty() || self.owner_key.is_empty() {
            Err(Error::query(String::from(
                "BelongsTo relation is not configured; use BelongsTo::new(...) or a macro-generated relation field",
            )))
        } else {
            Ok(())
        }
    }

    pub fn new(foreign_key: &'static str, owner_key: &'static str) -> Self {
        Self {
            foreign_key,
            owner_key,
            cached: None,
            fk_value: None,
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
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        if self.foreign_key == previous.foreign_key
            && self.owner_key == previous.owner_key
            && self.fk_value == previous.fk_value
        {
            self.cached = previous.cached.clone();
        }
    }

    pub async fn load(&self) -> Result<Option<E>> {
        if let Some(cached) = &self.cached {
            return Ok(Some((**cached).clone()));
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

        let query = E::query().where_eq(self.owner_key, fk.clone());
        constraint_fn(query).first().await
    }

    pub async fn exists(&self) -> Result<bool> {
        self.ensure_configured()?;

        let fk = self
            .fk_value
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Foreign key value not set for relation")))?;
        let fk = require_scalar_relation_key(fk, "BelongsTo::exists")?;

        E::query()
            .where_eq(self.owner_key, fk.clone())
            .exists()
            .await
    }

    pub fn get_cached(&self) -> Option<&E> {
        self.cached.as_deref()
    }
}

impl<E: Model> Default for BelongsTo<E> {
    fn default() -> Self {
        Self {
            foreign_key: "",
            owner_key: "",
            cached: None,
            fk_value: None,
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
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::default())
    }
}
