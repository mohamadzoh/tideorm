#![allow(missing_docs)]

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use parking_lot::Mutex;

use crate::error::Result;
use crate::model::Model;

use super::{
    EntityManager, TideEntityManagerMergePersisted, TideEntityManagerMeta, TideEntityManagerSync,
};

pub(super) type IdentityRollbackLog = HashMap<super::IdentityKey, Box<dyn IdentityMapRollback>>;
pub(super) type ManagedCheckpoints = Vec<Box<dyn super::managed::ManagedCheckpoint>>;

pub(super) struct EntityManagerRollbackState {
    managed_entries: Vec<Arc<dyn super::managed::ManagedOps>>,
    managed_identity_map: HashMap<super::IdentityKey, Arc<dyn Any + Send + Sync>>,
    snapshots: HashMap<super::SnapshotKey, HashSet<String>>,
}

pub(super) trait IdentityMapRollback: Send {
    fn restore(self: Box<Self>, entity_manager: &EntityManager);
}

struct IdentityMapRollbackEntry<T> {
    key: super::IdentityKey,
    original: Option<T>,
}

impl<T> IdentityMapRollback for IdentityMapRollbackEntry<T>
where
    T: Send + Sync + 'static,
{
    fn restore(self: Box<Self>, entity_manager: &EntityManager) {
        let mut map = entity_manager.identity_map.write();
        match self.original {
            Some(entity) => {
                map.insert(self.key, Box::new(entity));
            }
            None => {
                map.remove(&self.key);
            }
        }
    }
}

thread_local! {
    static ENTITY_MANAGER_TRANSACTION_SCOPE: Cell<bool> = const { Cell::new(false) };
    static ENTITY_MANAGER_IDENTITY_ROLLBACK: RefCell<Option<Arc<Mutex<IdentityRollbackLog>>>> = const { RefCell::new(None) };
}

pub(super) fn new_identity_rollback_log() -> Arc<Mutex<IdentityRollbackLog>> {
    Arc::new(Mutex::new(HashMap::new()))
}

fn current_identity_rollback_log() -> Option<Arc<Mutex<IdentityRollbackLog>>> {
    ENTITY_MANAGER_IDENTITY_ROLLBACK.with(|log| log.borrow().clone())
}

pub(super) fn in_entity_manager_transaction_scope() -> bool {
    ENTITY_MANAGER_TRANSACTION_SCOPE.with(|active| active.get())
}

pub(super) fn with_entity_manager_transaction_scope<F>(
    rollback_log: Arc<Mutex<IdentityRollbackLog>>,
    future: F,
) -> impl std::future::Future<Output = F::Output>
where
    F: std::future::Future,
{
    struct ScopedEntityManagerTransactionFuture<F> {
        future: Pin<Box<F>>,
        rollback_log: Arc<Mutex<IdentityRollbackLog>>,
    }

    impl<F> std::future::Future for ScopedEntityManagerTransactionFuture<F>
    where
        F: std::future::Future,
    {
        type Output = F::Output;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();
            let previous = ENTITY_MANAGER_TRANSACTION_SCOPE.with(|active| {
                let prev = active.get();
                active.set(true);
                prev
            });
            let previous_log = ENTITY_MANAGER_IDENTITY_ROLLBACK
                .with(|log| log.replace(Some(this.rollback_log.clone())));
            let result = this.future.as_mut().poll(cx);
            ENTITY_MANAGER_IDENTITY_ROLLBACK.with(|log| {
                *log.borrow_mut() = previous_log;
            });
            ENTITY_MANAGER_TRANSACTION_SCOPE.with(|active| active.set(previous));
            result
        }
    }

    ScopedEntityManagerTransactionFuture {
        future: Box::pin(future),
        rollback_log,
    }
}

pub(super) fn record_identity_map_rollback<T>(
    entity_manager: &EntityManager,
    key: &super::IdentityKey,
) where
    T: Clone + Send + Sync + 'static,
{
    if !in_entity_manager_transaction_scope() {
        return;
    }

    let Some(rollback_log) = current_identity_rollback_log() else {
        return;
    };

    let mut rollback_log = rollback_log.lock();
    if rollback_log.contains_key(key) {
        return;
    }

    let original = entity_manager
        .identity_map
        .read()
        .get(key)
        .and_then(|value| value.downcast_ref::<T>())
        .cloned();
    let key = key.clone();
    rollback_log.insert(
        key.clone(),
        Box::new(IdentityMapRollbackEntry { key, original }),
    );
}

pub(super) fn rollback_identity_map(
    entity_manager: &EntityManager,
    rollback_log: &Arc<Mutex<IdentityRollbackLog>>,
) {
    let rollback_entries = {
        let mut rollback_log = rollback_log.lock();
        std::mem::take(&mut *rollback_log)
    };

    for entry in rollback_entries.into_values() {
        entry.restore(entity_manager);
    }
}

pub(super) fn capture_entity_manager_rollback_state(
    entity_manager: &EntityManager,
) -> EntityManagerRollbackState {
    EntityManagerRollbackState {
        managed_entries: entity_manager.managed_entries.read().clone(),
        managed_identity_map: entity_manager.managed_identity_map.read().clone(),
        snapshots: entity_manager.snapshots.read().clone(),
    }
}

pub(super) fn capture_managed_checkpoints(entity_manager: &EntityManager) -> ManagedCheckpoints {
    entity_manager
        .managed_entries
        .read()
        .iter()
        .cloned()
        .map(|entry| entry.checkpoint())
        .collect()
}

pub(super) fn rollback_entity_manager_state(
    entity_manager: &EntityManager,
    checkpoints: ManagedCheckpoints,
    rollback_state: EntityManagerRollbackState,
    identity_rollback: &Arc<Mutex<IdentityRollbackLog>>,
) {
    for checkpoint in checkpoints {
        checkpoint.rollback(entity_manager);
    }

    *entity_manager.managed_entries.write() = rollback_state.managed_entries;
    *entity_manager.managed_identity_map.write() = rollback_state.managed_identity_map;
    rollback_identity_map(entity_manager, identity_rollback);
    *entity_manager.snapshots.write() = rollback_state.snapshots;
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

    let rollback_state = capture_entity_manager_rollback_state(entity_manager.as_ref());
    let checkpoints = capture_managed_checkpoints(entity_manager.as_ref());
    let identity_rollback = new_identity_rollback_log();
    let db = entity_manager.db.clone();
    let entity_manager_for_txn = entity_manager.clone();
    let identity_rollback_for_txn = identity_rollback.clone();
    let entity = entity.clone();
    let result = db
        .transaction(move |_| {
            Box::pin(async move {
                with_entity_manager_transaction_scope(
                    identity_rollback_for_txn,
                    save_with_entity_manager_impl(&entity, &entity_manager_for_txn),
                )
                .await
            })
        })
        .await;

    match result {
        Ok(saved) => Ok(saved),
        Err(error) => {
            rollback_entity_manager_state(
                entity_manager.as_ref(),
                checkpoints,
                rollback_state,
                &identity_rollback,
            );
            Err(error)
        }
    }
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
    let mut aggregate = entity.clone();
    let persisted = __with_entity_manager_db(
        entity_manager,
        <T as crate::model::Model>::save(entity.clone()),
    )
    .await?;
    aggregate.tide_merge_persisted(persisted);
    <T as crate::internal::InternalModel>::refresh_runtime_relations_from(
        &mut aggregate,
        entity,
    );
    aggregate
        .tide_sync_entity_manager_relations(entity_manager)
        .await?;
    entity_manager.put(aggregate.clone());
    Ok(aggregate)
}

pub(crate) async fn sync_entity_manager_relations_only_impl<T>(
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
    let mut aggregate = entity.clone();
    <T as crate::internal::InternalModel>::refresh_runtime_relations_from(
        &mut aggregate,
        entity,
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
