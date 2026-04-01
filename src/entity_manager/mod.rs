#![allow(missing_docs)]

mod managed;
mod meta;
mod save;
mod tracked;

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use parking_lot::RwLock;

pub use managed::{EntityState, Managed};
pub use meta::{
    TideEntityManagerFieldWriter, TideEntityManagerMergePersisted, TideEntityManagerMeta,
    TideEntityManagerSync,
};
pub use save::save_with_entity_manager;
pub use tracked::{TrackedHasMany, TrackedHasManyEntityManagerExt};

#[doc(hidden)]
pub use meta::{
    model_entity_manager_key as __model_entity_manager_key,
    pk_to_entity_manager_key as __pk_to_entity_manager_key,
};
#[doc(hidden)]
pub use save::{__save_with_entity_manager_in_scope, __with_entity_manager_db};

type IdentityKey = (TypeId, String);
type SnapshotKey = (&'static str, TypeId, String, &'static str);
const MAX_FLUSH_PASSES: usize = 16;

impl std::fmt::Debug for EntityManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntityManager")
            .field("identity_map_len", &self.identity_map.read().len())
            .field(
                "managed_identity_map_len",
                &self.managed_identity_map.read().len(),
            )
            .field("managed_entries_len", &self.managed_entries.read().len())
            .field("snapshots_len", &self.snapshots.read().len())
            .finish()
    }
}

#[doc(hidden)]
pub trait EntityManagerLoad {
    type Output<'a>
    where
        Self: 'a;

    fn load_with_entity_manager<'a>(
        &'a mut self,
        entity_manager: &'a Arc<EntityManager>,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<Self::Output<'a>>> + Send + 'a>>;
}

#[doc(hidden)]
pub struct EntityManager {
    identity_map: RwLock<HashMap<IdentityKey, Box<dyn Any + Send + Sync>>>,
    managed_identity_map: RwLock<HashMap<IdentityKey, Arc<dyn Any + Send + Sync>>>,
    managed_entries: RwLock<Vec<Arc<dyn managed::ManagedOps>>>,
    snapshots: RwLock<HashMap<SnapshotKey, HashSet<String>>>,
    pub(crate) db: Arc<crate::database::Database>,
}

impl EntityManager {
    pub fn new(db: Arc<crate::database::Database>) -> Arc<Self> {
        Arc::new(Self {
            identity_map: RwLock::new(HashMap::new()),
            managed_identity_map: RwLock::new(HashMap::new()),
            managed_entries: RwLock::new(Vec::new()),
            snapshots: RwLock::new(HashMap::new()),
            db,
        })
    }

    pub async fn find_managed<T>(
        self: &Arc<Self>,
        pk: <T as crate::model::ModelMeta>::PrimaryKey,
    ) -> crate::error::Result<Option<Managed<T>>>
    where
        T: crate::model::Model
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
        let key = meta::pk_to_entity_manager_key(&pk)?;
        if let Some(existing) = self.get_managed_by_key::<T>(&key) {
            return Ok(Some(existing));
        }

        let result = self.find::<T>(pk).await?;
        match result {
            Some(model) => Ok(Some(self.attach_persisted_managed(model))),
            None => Ok(None),
        }
    }

    pub async fn find<T>(
        self: &Arc<Self>,
        pk: <T as crate::model::ModelMeta>::PrimaryKey,
    ) -> crate::error::Result<Option<T>>
    where
        T: crate::model::Model + TideEntityManagerMeta + Clone + Send + Sync + 'static,
    {
        if let Some(cached) = self.get::<T>(&pk)? {
            return Ok(Some(cached));
        }

        let result = <T as crate::model::Model>::find_with(pk, self.database()).await?;
        match result {
            Some(model) => Ok(Some(self.register(model).await)),
            None => Ok(None),
        }
    }

    pub fn persist<T>(self: &Arc<Self>, entity: T) -> Managed<T>
    where
        T: crate::model::Model
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
        let mut entity = entity;
        entity.tide_attach_entity_manager_database(self.database());

        if let Some(existing) = self.get_managed_by_model(&entity).unwrap_or(None) {
            existing.entry.replace(entity);
            return existing;
        }

        let entry = Arc::new(managed::ManagedEntry::new(
            entity,
            None,
            EntityState::New,
            None,
        ));
        self.register_managed_entry(entry.clone());
        Managed::from_entry(entry)
    }

    pub fn merge<T>(self: &Arc<Self>, entity: T) -> crate::error::Result<Managed<T>>
    where
        T: crate::model::Model
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
        let mut entity = entity;
        entity.tide_attach_entity_manager_database(self.database());

        let Some(key) = meta::model_entity_manager_key(&entity)? else {
            return Ok(self.persist(entity));
        };

        if let Some(existing) = self.get_managed_by_key::<T>(&key) {
            existing.entry.overwrite_merged(entity);
            return Ok(existing);
        }

        let snapshot = self.get_by_entity_manager_key::<T>(&key);

        let entry = Arc::new(managed::ManagedEntry::new(
            entity.clone(),
            snapshot,
            EntityState::Managed,
            Some(key.clone()),
        ));
        self.register_managed_entry(entry.clone());
        self.put_managed_entry::<T>(&key, entry.clone());
        Ok(Managed::from_entry(entry))
    }

    pub fn remove<T>(&self, managed: &Managed<T>)
    where
        T: Send + Sync + 'static,
    {
        managed.entry.mark_removed();
    }

    pub fn detach<T>(&self, managed: &Managed<T>)
    where
        T: Send + Sync + 'static,
    {
        if let Some(key) = managed.entry.persisted_key() {
            self.remove_managed_entry::<T>(&key);
        }

        managed.entry.mark_detached_public();
        self.remove_managed_ops_entry(managed);
    }

    pub async fn flush(self: &Arc<Self>) -> crate::error::Result<()> {
        if save::in_entity_manager_transaction_scope() {
            return self.flush_in_scope().await;
        }

        let rollback_state = save::capture_entity_manager_rollback_state(self.as_ref());
        let checkpoints = Arc::new(parking_lot::Mutex::new(Vec::<
            Box<dyn managed::ManagedCheckpoint>,
        >::new()));
        let identity_rollback = save::new_identity_rollback_log();
        let entity_manager = self.clone();
        let transaction_checkpoints = checkpoints.clone();
        let transaction_identity_rollback = identity_rollback.clone();
        let result = self
            .db
            .transaction(move |_| {
                let entity_manager = entity_manager.clone();
                let checkpoints = transaction_checkpoints.clone();
                let identity_rollback = transaction_identity_rollback.clone();
                Box::pin(async move {
                    save::with_entity_manager_transaction_scope(
                        identity_rollback,
                        entity_manager.flush_in_scope_with_checkpoints(Some(&checkpoints)),
                    )
                    .await
                })
            })
            .await;

        if let Err(error) = result {
            let checkpoints = std::mem::take(&mut *checkpoints.lock());
            save::rollback_entity_manager_state(
                self.as_ref(),
                checkpoints,
                rollback_state,
                &identity_rollback,
            );
            return Err(error);
        }

        Ok(())
    }

    async fn flush_in_scope(self: &Arc<Self>) -> crate::error::Result<()> {
        self.flush_in_scope_with_checkpoints(None).await
    }

    async fn flush_in_scope_with_checkpoints(
        self: &Arc<Self>,
        checkpoints: Option<&Arc<parking_lot::Mutex<Vec<Box<dyn managed::ManagedCheckpoint>>>>>,
    ) -> crate::error::Result<()> {
        let mut processed = 0;
        let mut passes = 0;
        let mut checkpointed = HashSet::<usize>::new();
        loop {
            let entries = self.managed_entries.read().clone();
            if processed >= entries.len() {
                break;
            }

            if passes >= MAX_FLUSH_PASSES {
                return Err(crate::error::Error::invalid_query(format!(
                    "entity manager flush exceeded {MAX_FLUSH_PASSES} passes while new managed entries kept being registered; check relation sync for cycles"
                )));
            }
            passes += 1;

            for entry in entries.iter().skip(processed).cloned() {
                if let Some(checkpoints) = checkpoints {
                    let entry_ptr = Arc::as_ptr(&entry).cast::<()>() as usize;
                    if checkpointed.insert(entry_ptr) {
                        checkpoints.lock().push(entry.clone().checkpoint());
                    }
                }

                entry.flush(self).await?;
            }

            processed = entries.len();
        }

        let mut managed_entries = self.managed_entries.write();
        managed_entries.retain(|entry| entry.current_state() != EntityState::Detached);
        Ok(())
    }

    pub fn clear(&self) {
        let entries = self.managed_entries.read().clone();
        for entry in &entries {
            entry.detach_from_context(self);
        }

        self.identity_map.write().clear();
        self.managed_identity_map.write().clear();
        self.managed_entries.write().clear();
        self.snapshots.write().clear();
    }

    pub fn load<'a, R>(
        self: &'a Arc<Self>,
        relation: &'a mut R,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<R::Output<'a>>> + Send + 'a>>
    where
        R: EntityManagerLoad + 'a,
    {
        relation.load_with_entity_manager(self)
    }

    pub async fn save<T>(self: &Arc<Self>, entity: &T) -> crate::error::Result<T>
    where
        T: TideEntityManagerMeta
            + TideEntityManagerMergePersisted
            + TideEntityManagerSync
            + crate::model::Model
            + Clone
            + Send
            + Sync
            + 'static,
    {
        save::save_with_entity_manager(entity, self).await
    }

    #[doc(hidden)]
    pub fn database(&self) -> &crate::database::Database {
        self.db.as_ref()
    }

    fn attach_persisted_managed<T>(self: &Arc<Self>, entity: T) -> Managed<T>
    where
        T: crate::model::Model
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
        let mut entity = entity;
        entity.tide_attach_entity_manager_database(self.database());

        let key = entity.tide_pk_key();
        if let Some(existing) = self.get_managed_by_key::<T>(&key) {
            existing.entry.overwrite_clean(entity.clone(), Some(key));
            self.put(entity);
            return existing;
        }

        let entry = Arc::new(managed::ManagedEntry::new(
            entity.clone(),
            Some(entity.clone()),
            EntityState::Managed,
            Some(key.clone()),
        ));
        self.register_managed_entry(entry.clone());
        self.put_managed_entry::<T>(&key, entry.clone());
        self.put(entity);
        Managed::from_entry(entry)
    }

    pub async fn register<T>(&self, entity: T) -> T
    where
        T: TideEntityManagerMeta + Clone + Send + Sync + 'static,
    {
        let mut entity = entity;
        entity.tide_attach_entity_manager_database(self.database());

        let key = (TypeId::of::<T>(), entity.tide_pk_key());

        if let Some(existing) = self.get_by_key::<T>(&key) {
            return existing;
        }

        let mut map = self.identity_map.write();
        if let Some(existing) = map.get(&key).and_then(|value| value.downcast_ref::<T>()) {
            return existing.clone();
        }

        map.insert(key, Box::new(entity.clone()));
        entity
    }

    pub async fn snapshot<T: 'static>(
        &self,
        owner_table: &'static str,
        owner_key: &str,
        relation: &'static str,
        ids: &[String],
    ) {
        let key = (
            owner_table,
            TypeId::of::<T>(),
            owner_key.to_string(),
            relation,
        );
        let mut snapshots = self.snapshots.write();
        snapshots.insert(key, ids.iter().cloned().collect());
    }

    pub async fn deletions<T: 'static>(
        &self,
        owner_table: &'static str,
        owner_key: &str,
        relation: &'static str,
        current_ids: &[String],
    ) -> Vec<String> {
        let key = (
            owner_table,
            TypeId::of::<T>(),
            owner_key.to_string(),
            relation,
        );
        let current: HashSet<_> = current_ids.iter().cloned().collect();
        let snapshots = self.snapshots.read();
        let Some(previous) = snapshots.get(&key) else {
            return Vec::new();
        };

        previous
            .iter()
            .filter(|id| !current.contains(id.as_str()))
            .cloned()
            .collect()
    }

    #[doc(hidden)]
    pub async fn additions<T: 'static>(
        &self,
        owner_table: &'static str,
        owner_key: &str,
        relation: &'static str,
        current_ids: &[String],
    ) -> Vec<String> {
        let key = (
            owner_table,
            TypeId::of::<T>(),
            owner_key.to_string(),
            relation,
        );
        let current: HashSet<_> = current_ids.iter().cloned().collect();
        let snapshots = self.snapshots.read();
        let Some(previous) = snapshots.get(&key) else {
            return current.into_iter().collect();
        };

        current
            .into_iter()
            .filter(|id| !previous.contains(id.as_str()))
            .collect()
    }

    #[doc(hidden)]
    pub fn get<T>(
        &self,
        pk: &<T as crate::model::ModelMeta>::PrimaryKey,
    ) -> crate::error::Result<Option<T>>
    where
        T: crate::model::ModelMeta + Clone + Send + Sync + 'static,
    {
        let key = (TypeId::of::<T>(), meta::pk_to_entity_manager_key(pk)?);
        Ok(self.get_by_key(&key))
    }

    #[doc(hidden)]
    pub fn get_by_entity_manager_key<T>(&self, key: &str) -> Option<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.get_by_key(&(TypeId::of::<T>(), key.to_string()))
    }

    #[doc(hidden)]
    pub fn find_by_field<T>(
        &self,
        field: &str,
        value: &serde_json::Value,
    ) -> crate::error::Result<Option<T>>
    where
        T: crate::internal::InternalModel + Clone + Send + Sync + 'static,
    {
        let map = self.identity_map.read();
        for entry in map.values() {
            let Some(model) = entry.downcast_ref::<T>() else {
                continue;
            };

            if <T as crate::internal::InternalModel>::field_json_value(model, field)?.as_ref()
                == Some(value)
            {
                return Ok(Some(model.clone()));
            }
        }

        Ok(None)
    }

    #[doc(hidden)]
    pub fn put<T>(&self, entity: T)
    where
        T: TideEntityManagerMeta + Clone + Send + Sync + 'static,
    {
        let mut entity = entity;
        entity.tide_attach_entity_manager_database(self.database());

        let key = (TypeId::of::<T>(), entity.tide_pk_key());
        save::record_identity_map_rollback::<T>(self, &key);
        let mut map = self.identity_map.write();
        map.insert(key, Box::new(entity));
    }

    #[doc(hidden)]
    pub fn remove_by_entity_manager_key<T>(&self, key: &str)
    where
        T: Clone + Send + Sync + 'static,
    {
        let key = (TypeId::of::<T>(), key.to_string());
        save::record_identity_map_rollback::<T>(self, &key);
        let mut map = self.identity_map.write();
        map.remove(&key);
    }

    pub(crate) fn get_managed_by_key<T>(&self, key: &str) -> Option<Managed<T>>
    where
        T: Send + Sync + 'static,
    {
        let map = self.managed_identity_map.read();
        let entry = map.get(&(TypeId::of::<T>(), key.to_string()))?.clone();
        let entry = entry.downcast::<managed::ManagedEntry<T>>().ok()?;
        Some(Managed::from_entry(entry))
    }

    pub(crate) fn get_managed_by_model<T>(
        &self,
        entity: &T,
    ) -> crate::error::Result<Option<Managed<T>>>
    where
        T: crate::model::Model + crate::model::ModelMeta + Send + Sync + 'static,
    {
        let Some(key) = meta::model_entity_manager_key(entity)? else {
            return Ok(None);
        };

        Ok(self.get_managed_by_key::<T>(&key))
    }

    pub(crate) fn put_managed_entry<T>(&self, key: &str, entry: Arc<managed::ManagedEntry<T>>)
    where
        T: Send + Sync + 'static,
    {
        let erased: Arc<dyn Any + Send + Sync> = entry;
        let mut map = self.managed_identity_map.write();
        map.insert((TypeId::of::<T>(), key.to_string()), erased);
    }

    pub(crate) fn remove_managed_entry<T>(&self, key: &str)
    where
        T: Send + Sync + 'static,
    {
        let mut map = self.managed_identity_map.write();
        map.remove(&(TypeId::of::<T>(), key.to_string()));
    }

    fn remove_managed_ops_entry<T>(&self, managed: &Managed<T>) {
        let target = Arc::as_ptr(&managed.entry).cast::<()>();
        self.managed_entries
            .write()
            .retain(|entry| Arc::as_ptr(entry).cast::<()>() != target);
    }

    fn register_managed_entry<T>(&self, entry: Arc<managed::ManagedEntry<T>>)
    where
        T: crate::model::Model
            + TideEntityManagerMeta
            + TideEntityManagerMergePersisted
            + TideEntityManagerSync
            + serde::Serialize
            + Send
            + Sync
            + 'static,
        <<T as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
            PartialEq,
    {
        let ops: Arc<dyn managed::ManagedOps> = entry;
        self.managed_entries.write().push(ops);
    }

    fn get_by_key<T>(&self, key: &IdentityKey) -> Option<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        let map = self.identity_map.read();
        map.get(key)
            .and_then(|value| value.downcast_ref::<T>())
            .cloned()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/entity_manager_mod_tests.rs"]
mod tests;
