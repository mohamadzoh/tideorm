use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::marker::PhantomData;

use crate::error::{Error, Result};
use crate::model::Model;
use crate::query::QueryBuilder;

use super::helpers::build_self_ref_tree_sql;
use super::require_scalar_relation_key;

/// SelfRef relation type - represents a self-referencing relationship
#[derive(Debug, Clone)]
pub struct SelfRef<E: Model> {
    pub foreign_key: &'static str,
    pub local_key: &'static str,
    cached: Option<Box<E>>,
    fk_value: Option<serde_json::Value>,
    _marker: PhantomData<E>,
}

impl<E: Model> SelfRef<E> {
    pub fn new(foreign_key: &'static str, local_key: &'static str) -> Self {
        Self {
            foreign_key,
            local_key,
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
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        if (previous.fk_value.is_none() && previous.cached.is_some())
            || (self.foreign_key == previous.foreign_key
                && self.local_key == previous.local_key
                && self.fk_value == previous.fk_value)
        {
            self.cached = previous.cached.clone();
        }
    }

    pub async fn load(&self) -> Result<Option<E>> {
        if let Some(cached) = &self.cached {
            return Ok(Some((**cached).clone()));
        }

        let fk = match &self.fk_value {
            Some(v) if !v.is_null() => require_scalar_relation_key(v, "SelfRef::load")?,
            _ => return Ok(None),
        };

        E::query()
            .where_eq(self.local_key, fk.clone())
            .first()
            .await
    }

    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Option<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        let fk = match &self.fk_value {
            Some(v) if !v.is_null() => require_scalar_relation_key(v, "SelfRef::load_with")?,
            _ => return Ok(None),
        };

        let query = E::query().where_eq(self.local_key, fk.clone());
        constraint_fn(query).first().await
    }

    pub async fn exists(&self) -> Result<bool> {
        let fk = match &self.fk_value {
            Some(v) if !v.is_null() => require_scalar_relation_key(v, "SelfRef::exists")?,
            _ => return Ok(false),
        };

        E::query()
            .where_eq(self.local_key, fk.clone())
            .exists()
            .await
    }

    pub fn get_cached(&self) -> Option<&E> {
        self.cached.as_deref()
    }
}

impl<E: Model> Default for SelfRef<E> {
    fn default() -> Self {
        Self {
            foreign_key: "parent_id",
            local_key: "id",
            cached: None,
            fk_value: None,
            _marker: PhantomData,
        }
    }
}

impl<E: Model + Serialize> Serialize for SelfRef<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for SelfRef<E> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let cached = Option::<E>::deserialize(deserializer)?;
        Ok(Self {
            cached: cached.map(Box::new),
            ..Self::default()
        })
    }
}

/// SelfRefMany relation type - represents the inverse of a self-referencing relationship.
#[derive(Debug, Clone)]
pub struct SelfRefMany<E: Model> {
    pub foreign_key: &'static str,
    pub local_key: &'static str,
    cached: Option<Vec<E>>,
    parent_pk: Option<serde_json::Value>,
    _marker: PhantomData<E>,
}

impl<E: Model> SelfRefMany<E> {
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
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        if (previous.parent_pk.is_none() && previous.cached.is_some())
            || (self.foreign_key == previous.foreign_key
                && self.local_key == previous.local_key
                && self.parent_pk == previous.parent_pk)
        {
            self.cached = previous.cached.clone();
        }
    }

    pub async fn load(&self) -> Result<Vec<E>> {
        if let Some(cached) = &self.cached {
            return Ok(cached.clone());
        }

        let pk = self.parent_pk.as_ref().ok_or_else(|| {
            Error::query(String::from(
                "Parent primary key not set for self-reference",
            ))
        })?;
        let pk = require_scalar_relation_key(pk, "SelfRefMany::load")?;

        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .get()
            .await
    }

    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        let pk = self.parent_pk.as_ref().ok_or_else(|| {
            Error::query(String::from(
                "Parent primary key not set for self-reference",
            ))
        })?;
        let pk = require_scalar_relation_key(pk, "SelfRefMany::load_with")?;

        let query = E::query().where_eq(self.foreign_key, pk.clone());
        constraint_fn(query).get().await
    }

    pub async fn count(&self) -> Result<u64> {
        let pk = self.parent_pk.as_ref().ok_or_else(|| {
            Error::query(String::from(
                "Parent primary key not set for self-reference",
            ))
        })?;
        let pk = require_scalar_relation_key(pk, "SelfRefMany::count")?;

        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .count()
            .await
    }

    pub async fn exists(&self) -> Result<bool> {
        let pk = self.parent_pk.as_ref().ok_or_else(|| {
            Error::query(String::from(
                "Parent primary key not set for self-reference",
            ))
        })?;
        let pk = require_scalar_relation_key(pk, "SelfRefMany::exists")?;

        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .exists()
            .await
    }

    pub fn get_cached(&self) -> Option<&[E]> {
        self.cached.as_deref()
    }

    pub async fn load_tree(&self, max_depth: usize) -> Result<Vec<E>> {
        if max_depth == 0 {
            return Ok(Vec::new());
        }

        let pk = self.parent_pk.as_ref().ok_or_else(|| {
            Error::query(String::from(
                "Parent primary key not set for self-reference",
            ))
        })?;
        let pk = require_scalar_relation_key(pk, "SelfRefMany::load_tree")?;

        let db = crate::database::__current_db()?;
        let (sql, params) = build_self_ref_tree_sql::<E>(
            self.foreign_key,
            self.local_key,
            pk,
            max_depth,
            db.backend(),
        )?;

        db.__raw_with_params::<E>(&sql, params).await
    }
}

impl<E: Model> Default for SelfRefMany<E> {
    fn default() -> Self {
        Self {
            foreign_key: "parent_id",
            local_key: "id",
            cached: None,
            parent_pk: None,
            _marker: PhantomData,
        }
    }
}

impl<E: Model + Serialize> Serialize for SelfRefMany<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for SelfRefMany<E> {
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
