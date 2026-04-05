use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::marker::PhantomData;
#[cfg(feature = "entity-manager")]
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::model::Model;
use crate::query::QueryBuilder;

#[cfg(feature = "entity-manager")]
mod entity_manager_support;

use super::helpers::{cached_ref, ensure_relation_configured, preserve_cached_value, quote_ident};
use super::require_scalar_relation_key;

#[derive(Debug, Clone)]
pub struct HasManyThrough<Related: Model, Pivot: Model> {
    pub foreign_key: &'static str,
    pub related_key: &'static str,
    pub local_key: &'static str,
    pub related_local_key: &'static str,
    pub pivot_table: &'static str,
    #[cfg(feature = "entity-manager")]
    pub relation_name: &'static str,
    #[cfg(feature = "entity-manager")]
    pub owner_table: &'static str,
    #[cfg(feature = "entity-manager")]
    pub related_table: &'static str,
    cached: Option<Vec<Related>>,
    parent_pk: Option<serde_json::Value>,
    #[cfg(feature = "entity-manager")]
    owner_key: Option<String>,
    #[cfg(feature = "entity-manager")]
    entity_manager: Option<Arc<crate::entity_manager::EntityManager>>,
    #[cfg(feature = "entity-manager")]
    query_db: Option<crate::database::Database>,
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
            #[cfg(feature = "entity-manager")]
            relation_name: "",
            #[cfg(feature = "entity-manager")]
            owner_table: "",
            #[cfg(feature = "entity-manager")]
            related_table: "",
            cached: None,
            parent_pk: None,
            #[cfg(feature = "entity-manager")]
            owner_key: None,
            #[cfg(feature = "entity-manager")]
            entity_manager: None,
            #[cfg(feature = "entity-manager")]
            query_db: None,
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
        let same_relation = self.foreign_key == previous.foreign_key
            && self.related_key == previous.related_key
            && self.local_key == previous.local_key
            && self.related_local_key == previous.related_local_key
            && self.pivot_table == previous.pivot_table
            && self.parent_pk == previous.parent_pk
            && {
                #[cfg(feature = "entity-manager")]
                {
                    self.relation_name == previous.relation_name
                        && self.owner_table == previous.owner_table
                        && self.related_table == previous.related_table
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

        #[cfg(feature = "entity-manager")]
        if same_relation {
            if self.owner_key.is_none() {
                self.owner_key = previous.owner_key.clone();
            }
            if self.entity_manager.is_none() {
                self.entity_manager = previous.entity_manager.clone();
            }
            if self.query_db.is_none() {
                self.query_db = previous.query_db.clone();
            }
        }
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

        let query = {
            #[cfg(feature = "entity-manager")]
            {
                let db = self.scoped_database()?;
                Related::query_with(&db)
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                Related::query()
            }
        };

        query
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

        let query = {
            #[cfg(feature = "entity-manager")]
            {
                let db = self.scoped_database()?;
                Related::query_with(&db)
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                Related::query()
            }
        }
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

        let query = {
            #[cfg(feature = "entity-manager")]
            {
                let db = self.scoped_database()?;
                Pivot::query_with(&db)
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                Pivot::query()
            }
        };

        query
            .where_eq(
                format!("{}.{}", self.pivot_table, self.foreign_key),
                pk.clone(),
            )
            .count()
            .await
    }

    pub async fn attach(&self, related_id: impl Into<serde_json::Value>) -> Result<()> {
        self.ensure_configured()?;

        let db = {
            #[cfg(feature = "entity-manager")]
            {
                self.scoped_database()?
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                crate::database::require_db()?
            }
        };
        let db_type = db.backend();
        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasManyThrough::attach")?;
        let related_id = related_id.into();
        let related_id = require_scalar_relation_key(&related_id, "HasManyThrough::attach")?;

        let mut params = Vec::new();
        let parent_placeholder = crate::internal::push_param(
            db_type,
            &mut params,
            crate::internal::json_to_db_value(pk),
        );
        let related_placeholder = crate::internal::push_param(
            db_type,
            &mut params,
            crate::internal::json_to_db_value(related_id),
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

        let query = {
            #[cfg(feature = "entity-manager")]
            {
                let db = self.scoped_database()?;
                Pivot::query_with(&db)
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                Pivot::query()
            }
        };

        query
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

        let query = {
            #[cfg(feature = "entity-manager")]
            {
                let db = self.scoped_database()?;
                Pivot::query_with(&db)
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                Pivot::query()
            }
        };

        query
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

    pub fn as_mut(&mut self) -> Option<&mut Vec<Related>> {
        self.cached.as_mut()
    }

    pub fn items(&self) -> Option<&Vec<Related>> {
        self.cached.as_ref()
    }

    pub fn is_loaded(&self) -> bool {
        self.cached.is_some()
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
            #[cfg(feature = "entity-manager")]
            relation_name: "",
            #[cfg(feature = "entity-manager")]
            owner_table: "",
            #[cfg(feature = "entity-manager")]
            related_table: "",
            cached: None,
            parent_pk: None,
            #[cfg(feature = "entity-manager")]
            owner_key: None,
            #[cfg(feature = "entity-manager")]
            entity_manager: None,
            #[cfg(feature = "entity-manager")]
            query_db: None,
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

#[cfg(all(test, feature = "entity-manager"))]
#[path = "../../tests/unit/many_to_many_entity_manager_tests.rs"]
mod entity_manager_tests;

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
