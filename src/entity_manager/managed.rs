#![allow(missing_docs)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::model::Model;

use super::{
    EntityManager, TideEntityManagerMergePersisted, TideEntityManagerMeta, TideEntityManagerSync,
    save::{save_with_entity_manager_impl, sync_entity_manager_relations_only_impl},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityState {
    New,
    Managed,
    Removed,
    Detached,
}

#[derive(Clone)]
pub struct Managed<T> {
    pub(crate) entry: Arc<ManagedEntry<T>>,
}

impl<T> Managed<T> {
    pub(crate) fn from_entry(entry: Arc<ManagedEntry<T>>) -> Self {
        Self { entry }
    }

    pub fn state(&self) -> EntityState {
        self.entry.state()
    }

    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.entry.get()
    }

    pub fn edit<R>(&self, edit: impl FnOnce(&mut T) -> R) -> R {
        self.entry.edit(edit)
    }

    pub fn replace(&self, entity: T) {
        self.entry.replace(entity);
    }
}

pub(crate) trait ManagedOps: Send + Sync {
    fn current_state(&self) -> EntityState;
    fn detach_from_context(&self, entity_manager: &EntityManager);
    fn checkpoint(self: Arc<Self>) -> Box<dyn ManagedCheckpoint>;
    fn flush<'a>(
        self: Arc<Self>,
        entity_manager: &'a Arc<EntityManager>,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<()>> + Send + 'a>>;
}

pub(crate) trait ManagedCheckpoint: Send {
    fn rollback(self: Box<Self>, entity_manager: &EntityManager);
}

pub(crate) struct ManagedEntry<T> {
    current: RwLock<T>,
    snapshot: RwLock<Option<T>>,
    state: RwLock<EntityState>,
    persisted_key: RwLock<Option<String>>,
}

impl<T> ManagedEntry<T> {
    pub(crate) fn new(
        entity: T,
        snapshot: Option<T>,
        state: EntityState,
        persisted_key: Option<String>,
    ) -> Self {
        Self {
            current: RwLock::new(entity),
            snapshot: RwLock::new(snapshot),
            state: RwLock::new(state),
            persisted_key: RwLock::new(persisted_key),
        }
    }

    pub(crate) fn state(&self) -> EntityState {
        *self.state.read()
    }

    pub(crate) fn get(&self) -> T
    where
        T: Clone,
    {
        self.current.read().clone()
    }

    pub(crate) fn edit<R>(&self, edit: impl FnOnce(&mut T) -> R) -> R {
        let mut current = self.current.write();
        edit(&mut current)
    }

    pub(crate) fn replace(&self, entity: T) {
        *self.current.write() = entity;
    }

    pub(crate) fn overwrite_clean(&self, entity: T, persisted_key: Option<String>)
    where
        T: Clone,
    {
        *self.current.write() = entity.clone();
        *self.snapshot.write() = Some(entity);
        *self.persisted_key.write() = persisted_key;
        *self.state.write() = EntityState::Managed;
    }

    pub(crate) fn overwrite_merged(&self, entity: T) {
        *self.current.write() = entity;
        *self.state.write() = EntityState::Managed;
    }

    pub(crate) fn mark_removed(&self) {
        *self.state.write() = EntityState::Removed;
    }

    pub(crate) fn persisted_key(&self) -> Option<String> {
        self.persisted_key.read().clone()
    }

    fn mark_detached(&self) {
        *self.state.write() = EntityState::Detached;
    }

    pub(crate) fn mark_detached_public(&self) {
        self.mark_detached();
    }
}

impl<T> ManagedOps for ManagedEntry<T>
where
    T: Model
        + TideEntityManagerMeta
        + TideEntityManagerMergePersisted
        + TideEntityManagerSync
        + serde::Serialize
        + Clone
        + Send
        + Sync
        + 'static,
    <<T as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
        PartialEq,
{
    fn current_state(&self) -> EntityState {
        self.state()
    }

    fn detach_from_context(&self, entity_manager: &EntityManager) {
        if let Some(key) = self.persisted_key.read().clone() {
            entity_manager.remove_managed_entry::<T>(&key);
        }

        self.mark_detached();
    }

    fn checkpoint(self: Arc<Self>) -> Box<dyn ManagedCheckpoint> {
        Box::new(ManagedEntryCheckpoint {
            entry: self.clone(),
            current: self.current.read().clone(),
            snapshot: self.snapshot.read().clone(),
            state: self.state(),
            persisted_key: self.persisted_key.read().clone(),
        })
    }

    fn flush<'a>(
        self: Arc<Self>,
        entity_manager: &'a Arc<EntityManager>,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            match self.state() {
                EntityState::Detached => Ok(()),
                EntityState::Removed => {
                    let key = self.persisted_key.read().clone();
                    if let Some(key) = key {
                        let entity = self
                            .snapshot
                            .read()
                            .clone()
                            .unwrap_or_else(|| self.current.read().clone());
                        super::__with_entity_manager_db(
                            entity_manager,
                            <T as crate::model::Model>::delete(entity),
                        )
                        .await?;
                        entity_manager.remove_by_entity_manager_key::<T>(&key);
                        entity_manager.remove_managed_entry::<T>(&key);
                    }

                    *self.snapshot.write() = None;
                    *self.persisted_key.write() = None;
                    self.mark_detached();
                    Ok(())
                }
                EntityState::New | EntityState::Managed => {
                    let current = self.current.read().clone();
                    let snapshot = self.snapshot.read().clone();
                    let columns_changed = match snapshot.as_ref() {
                        Some(snapshot) => snapshot.to_entity_model() != current.to_entity_model(),
                        None => true,
                    };

                    let previous_key = self.persisted_key.read().clone();
                    let saved = if columns_changed {
                        save_with_entity_manager_impl(&current, entity_manager).await?
                    } else {
                        sync_entity_manager_relations_only_impl(&current, entity_manager).await?
                    };
                    let next_key = Some(saved.tide_pk_key());

                    if let Some(previous_key) = previous_key.as_deref() {
                        if Some(previous_key) != next_key.as_deref() {
                            entity_manager.remove_managed_entry::<T>(previous_key);
                        }
                    }

                    if let Some(key) = next_key.as_deref() {
                        entity_manager.put_managed_entry::<T>(key, self.clone());
                    }

                    *self.current.write() = saved.clone();
                    *self.snapshot.write() = Some(saved.clone());
                    *self.persisted_key.write() = next_key;
                    *self.state.write() = EntityState::Managed;
                    entity_manager.put(saved);
                    Ok(())
                }
            }
        })
    }
}

struct ManagedEntryCheckpoint<T> {
    entry: Arc<ManagedEntry<T>>,
    current: T,
    snapshot: Option<T>,
    state: EntityState,
    persisted_key: Option<String>,
}

impl<T> ManagedCheckpoint for ManagedEntryCheckpoint<T>
where
    T: Send + Sync + 'static,
{
    fn rollback(self: Box<Self>, entity_manager: &EntityManager) {
        if let Some(current_key) = self.entry.persisted_key.read().clone() {
            entity_manager.remove_managed_entry::<T>(&current_key);
        }

        if let Some(previous_key) = self.persisted_key.as_deref() {
            entity_manager.put_managed_entry::<T>(previous_key, self.entry.clone());
        }

        *self.entry.current.write() = self.current;
        *self.entry.snapshot.write() = self.snapshot;
        *self.entry.state.write() = self.state;
        *self.entry.persisted_key.write() = self.persisted_key;
    }
}
