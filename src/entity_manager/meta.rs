#![allow(missing_docs)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::model::{Model, ModelMeta};

use super::EntityManager;

pub trait TideEntityManagerMeta {
    fn tide_table_name() -> &'static str
    where
        Self: Sized;

    fn tide_pk_key(&self) -> String;
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
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
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
