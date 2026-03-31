#![allow(missing_docs)]

use std::sync::Arc;

use crate::error::Result;
use crate::model::Model;

use super::{
    EntityManager, TideEntityManagerMergePersisted, TideEntityManagerMeta,
    TideEntityManagerSync,
};

pub async fn save_with_entity_manager<T>(entity: &T, entity_manager: &Arc<EntityManager>) -> Result<T>
where
    T: TideEntityManagerMeta
        + TideEntityManagerMergePersisted
        + TideEntityManagerSync
        + Model
        + Clone
        + Send
        + Sync
        + 'static,
{
    save_with_entity_manager_impl(entity, entity_manager).await
}

pub(crate) async fn save_with_entity_manager_impl<T>(
    entity: &T,
    entity_manager: &Arc<EntityManager>,
) -> Result<T>
where
    T: TideEntityManagerMeta
        + TideEntityManagerMergePersisted
        + TideEntityManagerSync
        + Model
        + Clone
        + Send
        + Sync
        + 'static,
{
    let persisted = __with_entity_manager_db(
        entity_manager,
        <T as crate::model::Model>::save(entity.clone()),
    )
    .await?;
    let mut aggregate = entity.clone();
    let previous = aggregate.clone();
    aggregate.tide_merge_persisted(persisted);
    <T as crate::internal::InternalModel>::refresh_runtime_relations_from(&mut aggregate, &previous);
    aggregate
        .tide_sync_entity_manager_relations(entity_manager)
        .await?;
    entity_manager.put(aggregate.clone());
    Ok(aggregate)
}

#[doc(hidden)]
pub async fn __with_entity_manager_db<F, T>(
    entity_manager: &Arc<EntityManager>,
    future: F,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    crate::database::__in_db_scope(entity_manager.db.as_ref(), future).await
}