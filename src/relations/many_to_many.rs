use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::marker::PhantomData;

use crate::error::{Error, Result};
use crate::model::Model;
use crate::query::QueryBuilder;

use super::helpers::{
    cached_ref, ensure_relation_configured, preserve_cached_value, push_param, quote_ident,
};
use super::require_scalar_relation_key;

#[derive(Debug, Clone)]
pub struct HasManyThrough<Related: Model, Pivot: Model> {
    pub foreign_key: &'static str,
    pub related_key: &'static str,
    pub local_key: &'static str,
    pub related_local_key: &'static str,
    pub pivot_table: &'static str,
    cached: Option<Vec<Related>>,
    parent_pk: Option<serde_json::Value>,
    _marker: PhantomData<(Related, Pivot)>,
}

impl<Related: Model, Pivot: Model> HasManyThrough<Related, Pivot> {
    fn ensure_configured(&self) -> Result<()> {
        ensure_relation_configured(
            "HasManyThrough",
            &[
                self.foreign_key,
                self.related_key,
                self.local_key,
                self.related_local_key,
                self.pivot_table,
            ],
        )
    }

    pub fn new(
        foreign_key: &'static str,
        related_key: &'static str,
        local_key: &'static str,
        related_local_key: &'static str,
        pivot_table: &'static str,
    ) -> Self {
        Self {
            foreign_key,
            related_key,
            local_key,
            related_local_key,
            pivot_table,
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
    pub fn set_cached(&mut self, models: Vec<Related>) {
        self.cached = Some(models);
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        preserve_cached_value(
            &mut self.cached,
            &previous.cached,
            previous.parent_pk.is_none(),
            self.foreign_key == previous.foreign_key
                && self.related_key == previous.related_key
                && self.local_key == previous.local_key
                && self.related_local_key == previous.related_local_key
                && self.pivot_table == previous.pivot_table
                && self.parent_pk == previous.parent_pk,
        );
    }

    pub async fn load(&self) -> Result<Vec<Related>> {
        if let Some(cached) = &self.cached {
            return Ok(cached.clone());
        }

        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasManyThrough::load")?;
        let pivot_related_column = format!("{}.{}", self.pivot_table, self.related_key);
        let related_local_column = format!("{}.{}", Related::table_name(), self.related_local_key);

        Related::query()
            .inner_join(
                self.pivot_table,
                &pivot_related_column,
                &related_local_column,
            )
            .where_eq(
                format!("{}.{}", self.pivot_table, self.foreign_key),
                pk.clone(),
            )
            .get()
            .await
    }

    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<Related>>
    where
        F: FnOnce(QueryBuilder<Related>) -> QueryBuilder<Related> + Send,
    {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasManyThrough::load_with")?;
        let pivot_related_column = format!("{}.{}", self.pivot_table, self.related_key);
        let related_local_column = format!("{}.{}", Related::table_name(), self.related_local_key);

        let query = Related::query()
            .inner_join(
                self.pivot_table,
                &pivot_related_column,
                &related_local_column,
            )
            .where_eq(
                format!("{}.{}", self.pivot_table, self.foreign_key),
                pk.clone(),
            );

        constraint_fn(query).get().await
    }

    pub async fn count(&self) -> Result<u64> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasManyThrough::count")?;

        Pivot::query()
            .where_eq(
                format!("{}.{}", self.pivot_table, self.foreign_key),
                pk.clone(),
            )
            .count()
            .await
    }

    pub async fn attach(&self, related_id: impl Into<serde_json::Value>) -> Result<()> {
        self.ensure_configured()?;

        let db = crate::database::require_db()?;
        let db_type = db.backend();
        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasManyThrough::attach")?;

        let mut params = Vec::new();
        let parent_placeholder = push_param(
            db_type,
            &mut params,
            crate::internal::Value::from(pk.clone()),
        );
        let related_placeholder = push_param(
            db_type,
            &mut params,
            crate::internal::Value::from(related_id.into()),
        );
        let sql = format!(
            "INSERT INTO {} ({}, {}) VALUES ({}, {})",
            quote_ident(db_type, self.pivot_table),
            quote_ident(db_type, self.foreign_key),
            quote_ident(db_type, self.related_key),
            parent_placeholder,
            related_placeholder
        );

        db.__execute_with_params(&sql, params).await?;
        Ok(())
    }

    pub async fn detach(&self, related_id: impl Into<serde_json::Value>) -> Result<u64> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasManyThrough::detach")?;

        Pivot::query()
            .where_eq(self.foreign_key, pk.clone())
            .where_eq(self.related_key, related_id.into())
            .delete()
            .await
    }

    pub async fn sync(&self, related_ids: Vec<serde_json::Value>) -> Result<()> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasManyThrough::sync")?;

        Pivot::query()
            .where_eq(self.foreign_key, pk.clone())
            .delete()
            .await?;

        for id in related_ids {
            self.attach(id).await?;
        }

        Ok(())
    }

    pub fn get_cached(&self) -> Option<&[Related]> {
        cached_ref(&self.cached)
    }
}

impl<Related: Model, Pivot: Model> Default for HasManyThrough<Related, Pivot> {
    fn default() -> Self {
        Self {
            foreign_key: "",
            related_key: "",
            local_key: "",
            related_local_key: "",
            pivot_table: "",
            cached: None,
            parent_pk: None,
            _marker: PhantomData,
        }
    }
}

impl<Related: Model + Serialize, Pivot: Model> Serialize for HasManyThrough<Related, Pivot> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, Related: Model, Pivot: Model> Deserialize<'de> for HasManyThrough<Related, Pivot> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let cached = Option::<Vec<Related>>::deserialize(deserializer)?;
        Ok(Self {
            cached,
            ..Self::default()
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithPivot<M, P> {
    #[serde(flatten)]
    pub model: M,
    pub pivot: P,
}

impl<M, P> WithPivot<M, P> {
    pub fn new(model: M, pivot: P) -> Self {
        Self { model, pivot }
    }

    pub fn into_model(self) -> M {
        self.model
    }

    pub fn pivot(&self) -> &P {
        &self.pivot
    }

    pub fn into_parts(self) -> (M, P) {
        (self.model, self.pivot)
    }
}

impl<M, P> std::ops::Deref for WithPivot<M, P> {
    type Target = M;

    fn deref(&self) -> &Self::Target {
        &self.model
    }
}
