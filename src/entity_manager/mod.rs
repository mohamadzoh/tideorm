#![allow(missing_docs)]

mod managed;
mod meta;
mod save;
mod state;
mod tracked;

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
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
type SharedManagedCheckpoints = Arc<parking_lot::Mutex<save::ManagedCheckpoints>>;
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
    ) -> impl std::future::Future<Output = crate::error::Result<Self::Output<'a>>> + Send;
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

        // Auto-increment models have no usable key until the insert flushes; a
        // client-assigned primary key can be tracked in the managed identity map
        // right away so a later `find_managed`/`merge` reuses this handle instead
        // of opening a second one for the same row.
        let key = meta::model_entity_manager_key(&entity).unwrap_or(None);

        // `persisted_key` stays `None`: it means "this row exists in the database",
        // and nothing has been inserted yet. The flush stamps it once the save
        // succeeds. Setting it here made `put` + `remove` + `flush` issue a real
        // DELETE — and fire before/after_delete — for a row that was never written.
        // The identity map is keyed separately below, so tracking still works.
        let entry = Arc::new(managed::ManagedEntry::new(
            entity,
            None,
            EntityState::New,
            None,
        ));
        self.register_managed_entry(entry.clone());
        if let Some(key) = key.as_deref() {
            self.put_managed_entry::<T>(key, entry.clone());
            // Record what the entry was filed under. `persisted_key` stays `None`
            // — nothing is inserted yet — so without this the removal paths, which
            // all key off the map entry, would have nothing to evict.
            entry.set_identity_key(Some(key.to_string()));
        }
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
            entity,
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
        // `identity_key`, not `persisted_key`: an entity `persist`ed with a
        // client-assigned primary key is in the identity map before any insert,
        // so keying the eviction off the persisted key left it behind and the
        // detach silently did nothing.
        if let Some(key) = managed.entry.identity_key() {
            self.remove_managed_entry::<T>(&key);
        }
        managed.entry.set_identity_key(None);

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
                Box::pin(async move {
                    save::with_entity_manager_transaction_scope(
                        transaction_identity_rollback,
                        entity_manager
                            .flush_in_scope_with_checkpoints(Some(&transaction_checkpoints)),
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
        checkpoints: Option<&SharedManagedCheckpoints>,
    ) -> crate::error::Result<()> {
        let mut processed = 0;
        let mut passes = 0;
        let mut checkpointed = HashSet::<usize>::new();
        loop {
            let (mut entries, entries_len) = {
                let all_entries = self.managed_entries.read();
                let len = all_entries.len();
                let entries: Vec<_> = all_entries.iter().skip(processed).cloned().collect();
                (entries, len)
            };
            if processed >= entries_len {
                break;
            }

            if passes >= MAX_FLUSH_PASSES {
                return Err(crate::error::Error::invalid_query(format!(
                    "entity manager flush exceeded {MAX_FLUSH_PASSES} passes while new managed entries kept being registered; check relation sync for cycles"
                )));
            }
            passes += 1;

            // Run inserts first and deletes last, and inside each of those order
            // the tables against each other so a parent row exists before the
            // child that references it (and is deleted after it). The sort is
            // stable, so entries that tie on both still flush in registration
            // order.
            let order = managed::plan_flush_order(&entries);
            entries.sort_by_key(|entry| managed::flush_sort_key(entry.as_ref(), &order));

            for entry in entries {
                if let Some(checkpoints) = checkpoints {
                    let entry_ptr = Arc::as_ptr(&entry).cast::<()>() as usize;
                    if checkpointed.insert(entry_ptr) {
                        checkpoints.lock().push(entry.clone().checkpoint());
                    }
                }

                let table = entry.table_name();
                if let Err(error) = entry.flush(self).await {
                    return Err(managed::explain_flush_ordering_failure(
                        error,
                        table,
                        order.unordered_tables(),
                    ));
                }
            }

            processed = entries_len;
        }

        let mut managed_entries = self.managed_entries.write();
        managed_entries.retain(|entry| entry.current_state() != EntityState::Detached);
        Ok(())
    }

    pub fn clear(&self) {
        let entries: Vec<_> = self.managed_entries.read().iter().cloned().collect();
        for entry in &entries {
            entry.detach_from_context(self);
        }

        self.identity_map.write().clear();
        self.managed_identity_map.write().clear();
        self.managed_entries.write().clear();
        self.snapshots.write().clear();
    }

    pub async fn load<'a, R>(
        self: &'a Arc<Self>,
        relation: &'a mut R,
    ) -> crate::error::Result<R::Output<'a>>
    where
        R: EntityManagerLoad + 'a,
    {
        relation.load_with_entity_manager(self).await
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

        // An unsaved entity has no identity to share. `tide_pk_key` renders its
        // default primary key as an ordinary string, so every new instance of a
        // model keys to the same thing — file two of them and the second gets
        // handed the first one back, which silently drops it and inserts the
        // first twice. Hand it straight back instead; the flush files it under a
        // real key once the insert assigns one.
        if entity.tide_pk_is_new() {
            return entity;
        }

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
}

#[cfg(test)]
#[path = "../../tests/unit/entity_manager_mod_tests.rs"]
mod tests;
