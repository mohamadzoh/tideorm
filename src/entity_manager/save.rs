#![allow(missing_docs)]

#[cfg(not(feature = "runtime-tokio"))]
use std::cell::RefCell;
use std::sync::Arc;
#[cfg(not(feature = "runtime-tokio"))]
use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::error::Result;
use crate::model::Model;

use super::{
    EntityManager, TideEntityManagerMergePersisted, TideEntityManagerMeta, TideEntityManagerSync,
};

#[cfg(feature = "runtime-tokio")]
tokio::task_local! {
    static ENTITY_MANAGER_TRANSACTION_SCOPE: bool;
}

#[cfg(not(feature = "runtime-tokio"))]
thread_local! {
    static ENTITY_MANAGER_TRANSACTION_SCOPE: RefCell<bool> = const { RefCell::new(false) };
}

#[cfg(feature = "runtime-tokio")]
pub(super) fn in_entity_manager_transaction_scope() -> bool {
    ENTITY_MANAGER_TRANSACTION_SCOPE
        .try_with(|active| *active)
        .unwrap_or(false)
}

#[cfg(not(feature = "runtime-tokio"))]
pub(super) fn in_entity_manager_transaction_scope() -> bool {
    ENTITY_MANAGER_TRANSACTION_SCOPE.with(|active| *active.borrow())
}

#[cfg(feature = "runtime-tokio")]
pub(super) async fn with_entity_manager_transaction_scope<F>(future: F) -> F::Output
where
    F: std::future::Future,
{
    ENTITY_MANAGER_TRANSACTION_SCOPE.scope(true, future).await
}

#[cfg(not(feature = "runtime-tokio"))]
pub(super) fn with_entity_manager_transaction_scope<F>(
    future: F,
) -> impl std::future::Future<Output = F::Output>
where
    F: std::future::Future,
{
    struct ScopedEntityManagerTransactionFuture<F> {
        future: Pin<Box<F>>,
    }

    impl<F> std::future::Future for ScopedEntityManagerTransactionFuture<F>
    where
        F: std::future::Future,
    {
        type Output = F::Output;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();
            let previous = ENTITY_MANAGER_TRANSACTION_SCOPE.with(|active| active.replace(true));
            let result = this.future.as_mut().poll(cx);
            ENTITY_MANAGER_TRANSACTION_SCOPE.with(|active| {
                *active.borrow_mut() = previous;
            });
            result
        }
    }

    ScopedEntityManagerTransactionFuture {
        future: Box::pin(future),
    }
}

pub async fn save_with_entity_manager<T>(
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
    if in_entity_manager_transaction_scope() {
        return save_with_entity_manager_impl(entity, entity_manager).await;
    }

    let snapshots = entity_manager.snapshots.read().clone();
    let db = entity_manager.db.clone();
    let entity_manager = entity_manager.clone();
    let transaction_entity_manager = entity_manager.clone();
    let entity = entity.clone();
    let result = db
        .transaction(move |_| {
            let entity_manager = transaction_entity_manager.clone();
            Box::pin(async move {
                with_entity_manager_transaction_scope(save_with_entity_manager_impl(
                    &entity,
                    &entity_manager,
                ))
                .await
            })
        })
        .await;

    if result.is_err() {
        entity_manager.identity_map.write().clear();
        *entity_manager.snapshots.write() = snapshots;
    }

    result
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
    <T as crate::internal::InternalModel>::refresh_runtime_relations_from(
        &mut aggregate,
        &previous,
    );
    aggregate
        .tide_sync_entity_manager_relations(entity_manager)
        .await?;
    entity_manager.put(aggregate.clone());
    Ok(aggregate)
}

#[doc(hidden)]
pub async fn __save_with_entity_manager_in_scope<T>(
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
    save_with_entity_manager_impl(entity, entity_manager).await
}

#[doc(hidden)]
pub async fn __with_entity_manager_db<F, T>(
    entity_manager: &Arc<EntityManager>,
    future: F,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    if in_entity_manager_transaction_scope() {
        return future.await;
    }

    crate::database::__in_db_scope(entity_manager.db.as_ref(), future).await
}
