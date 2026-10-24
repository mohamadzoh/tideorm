#![allow(missing_docs)]

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::error::Result;
use crate::model::Model;
use crate::relations::{HasMany, HasManyThrough, HasOne};

use super::{EntityManager, TideEntityManagerMergePersisted, TideEntityManagerSync};

pub(super) type IdentityRollbackLog =
    HashMap<(super::IdentityKey, u64), Box<dyn IdentityMapRollback>>;

pub(super) trait IdentityMapRollback: Send {
    fn restore(self: Box<Self>, entity_manager: &EntityManager);
}

struct IdentityMapRollbackEntry<T> {
    key: super::IdentityKey,
    generation: u64,
    original: Option<Arc<T>>,
}

impl<T> IdentityMapRollback for IdentityMapRollbackEntry<T>
where
    T: Send + Sync + 'static,
{
    fn restore(self: Box<Self>, entity_manager: &EntityManager) {
        let mut map = entity_manager.identity_map.write();
        if entity_manager
            .identity_generation
            .load(super::Ordering::Relaxed)
            != self.generation
        {
            return;
        }
        match self.original {
            Some(entity) => {
                map.insert(self.key, entity);
            }
            None => {
                map.remove(&self.key);
            }
        }
    }
}

thread_local! {
    /// The manager whose unit of work is running here, by address; `0` for none.
    static ENTITY_MANAGER_TRANSACTION_SCOPE: Cell<usize> = const { Cell::new(0) };
    static ENTITY_MANAGER_IDENTITY_ROLLBACK: RefCell<Option<Arc<Mutex<IdentityRollbackLog>>>> = const { RefCell::new(None) };
}

pub(super) fn new_identity_rollback_log() -> Arc<Mutex<IdentityRollbackLog>> {
    Arc::new(Mutex::new(HashMap::new()))
}

fn current_identity_rollback_log() -> Option<Arc<Mutex<IdentityRollbackLog>>> {
    ENTITY_MANAGER_IDENTITY_ROLLBACK.with(|log| log.borrow().clone())
}

/// The scope identity of `entity_manager`.
fn scope_owner(entity_manager: &EntityManager) -> usize {
    std::ptr::from_ref(entity_manager) as usize
}

/// Whether `entity_manager`'s own unit of work is running here. Another
/// manager's does not count: joining it would run this manager's statements
/// in that transaction, on that manager's database, without this one's
/// identity-map rollback.
pub(super) fn in_entity_manager_transaction_scope(entity_manager: &EntityManager) -> bool {
    ENTITY_MANAGER_TRANSACTION_SCOPE.with(|owner| owner.get()) == scope_owner(entity_manager)
}

/// Restores the transaction-scope thread-locals when the guard is dropped.
///
/// The restore has to happen in `Drop` rather than in plain statements after
/// `poll`: a panic inside the wrapped future unwinds straight past those
/// statements and would leave `ENTITY_MANAGER_TRANSACTION_SCOPE` stuck at
/// set on that worker thread forever, so every later flush/save polled there
/// would take the "already in scope" path and run with no transaction at all.
struct ResetEntityManagerTransactionScope {
    /// Scope owner observed before the scope was installed.
    scope: usize,
    /// Identity rollback log observed before the scope was installed.
    log: Option<Arc<Mutex<IdentityRollbackLog>>>,
}

impl Drop for ResetEntityManagerTransactionScope {
    fn drop(&mut self) {
        ENTITY_MANAGER_IDENTITY_ROLLBACK.with(|log| {
            *log.borrow_mut() = self.log.take();
        });
        ENTITY_MANAGER_TRANSACTION_SCOPE.with(|active| active.set(self.scope));
    }
}

fn install_entity_manager_transaction_scope(
    owner: usize,
    rollback_log: &Arc<Mutex<IdentityRollbackLog>>,
) -> ResetEntityManagerTransactionScope {
    let scope = ENTITY_MANAGER_TRANSACTION_SCOPE.with(|active| active.replace(owner));
    let log = ENTITY_MANAGER_IDENTITY_ROLLBACK.with(|log| log.replace(Some(rollback_log.clone())));
    ResetEntityManagerTransactionScope { scope, log }
}

pub(super) fn with_entity_manager_transaction_scope<F>(
    entity_manager: &EntityManager,
    rollback_log: Arc<Mutex<IdentityRollbackLog>>,
    future: F,
) -> impl std::future::Future<Output = F::Output> + use<F>
where
    F: std::future::Future,
{
    let owner = scope_owner(entity_manager);
    crate::internal::per_poll(future, move || {
        install_entity_manager_transaction_scope(owner, &rollback_log)
    })
}

pub(super) fn record_identity_map_rollback<T>(
    entity_manager: &EntityManager,
    key: &super::IdentityKey,
    map: &HashMap<super::IdentityKey, Arc<dyn std::any::Any + Send + Sync>>,
) where
    T: Clone + Send + Sync + 'static,
{
    if !in_entity_manager_transaction_scope(entity_manager) {
        return;
    }

    let Some(rollback_log) = current_identity_rollback_log() else {
        return;
    };

    let generation = entity_manager
        .identity_generation
        .load(super::Ordering::Relaxed);
    let log_key = (key.clone(), generation);
    let mut rollback_log = rollback_log.lock();
    if rollback_log.contains_key(&log_key) {
        return;
    }

    let original = map
        .get(key)
        .cloned()
        .and_then(|value| value.downcast::<T>().ok());
    let key = key.clone();
    rollback_log.insert(
        log_key,
        Box::new(IdentityMapRollbackEntry {
            key,
            generation,
            original,
        }),
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

pub(super) async fn save_with_entity_manager<T>(
    entity: &T,
    entity_manager: &Arc<EntityManager>,
) -> Result<T>
where
    T: TideEntityManagerMergePersisted + TideEntityManagerSync,
{
    if in_entity_manager_transaction_scope(entity_manager) {
        return save_in_scope(entity, entity_manager).await;
    }

    let entity_manager_for_work = entity_manager.clone();
    let entity = entity.clone();
    in_unit_of_work(entity_manager, move || async move {
        save_in_scope(&entity, &entity_manager_for_work).await
    })
    .await
}

/// Run `work` as a unit of work of `entity_manager`: after any the manager is
/// running already, in one transaction, with the context put back if that
/// does not commit.
pub(super) async fn in_unit_of_work<T, Fut>(
    entity_manager: &Arc<EntityManager>,
    work: impl FnOnce() -> Fut + Send + 'static,
) -> Result<T>
where
    Fut: std::future::Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    let _running = entity_manager.unit_of_work.lock().await;
    let identity_rollback = new_identity_rollback_log();
    let owner = entity_manager.clone();
    let manager = entity_manager.clone();
    entity_manager
        .db
        .transaction(move |_| {
            let restored = identity_rollback.clone();
            Box::pin(with_entity_manager_transaction_scope(
                owner.as_ref(),
                identity_rollback,
                async move {
                    // The identity-map entries the work replaces are put back
                    // unless it commits: when it fails, when its future is
                    // dropped part way, and when a transaction enclosing it
                    // rolls back later, or the manager would hand out rows
                    // that were never committed. Registered in the work's own
                    // transaction, whose commit hands it to a transaction of
                    // the same database around it and drops it otherwise, as
                    // final. Managed entries undo their own flushes the same way.
                    crate::cache::undo_on_rollback(move || {
                        rollback_identity_map(&manager, &restored);
                    });
                    work().await
                },
            ))
        })
        .await
}

/// Save `entity` and sync its loaded relations inside the unit of work the caller
/// already opened.
pub(super) async fn save_in_scope<T>(entity: &T, entity_manager: &Arc<EntityManager>) -> Result<T>
where
    T: TideEntityManagerMergePersisted + TideEntityManagerSync,
{
    let mut aggregate = entity.clone();
    let persisted = with_entity_manager_db(
        entity_manager,
        <T as crate::model::Model>::save(entity.clone()),
    )
    .await?;
    aggregate.tide_merge_persisted(persisted);
    let aggregate = sync_aggregate(aggregate, entity, entity_manager).await?;
    // A managed handle to the same row now loaded older values; one flushed
    // later would write them back over what was just stored.
    if let Some(managed) = entity_manager.get_managed_by_key::<T>(&aggregate.tide_pk_key()) {
        managed.entry.rebase(&aggregate)?;
    }
    Ok(aggregate)
}

pub(crate) async fn sync_entity_manager_relations_only_impl<T>(
    entity: &T,
    entity_manager: &Arc<EntityManager>,
) -> Result<T>
where
    T: TideEntityManagerMergePersisted + TideEntityManagerSync,
{
    sync_aggregate(entity.clone(), entity, entity_manager).await
}

/// `aggregate`, `entity` as written, with its relation wrappers rebuilt from
/// `entity`'s and its loaded relations synced, filed in the identity map.
async fn sync_aggregate<T>(
    mut aggregate: T,
    entity: &T,
    entity_manager: &Arc<EntityManager>,
) -> Result<T>
where
    T: TideEntityManagerMergePersisted + TideEntityManagerSync,
{
    <T as crate::internal::InternalModel>::refresh_runtime_relations_from(&mut aggregate, entity);
    aggregate
        .tide_sync_entity_manager_relations(entity_manager)
        .await?;
    entity_manager.put(&mut aggregate);
    Ok(aggregate)
}

/// Persists one entity held by a loaded relation during a relation sync — or,
/// when the identity map already holds an identical copy, only syncs that
/// entity's own relations — and returns its identity key afterwards.
///
/// The copies are compared column by column, as a managed flush compares an
/// entity with its snapshot: their JSON leaves out a field the model's own
/// serde derive skips, so an edit to one would never be written.
#[doc(hidden)]
pub async fn __sync_related_entity<T>(
    entity: &mut T,
    entity_manager: &Arc<EntityManager>,
) -> Result<Option<String>>
where
    T: TideEntityManagerMergePersisted + TideEntityManagerSync,
    <<T as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
        PartialEq,
{
    let existing_key = super::meta::model_entity_manager_key(entity)?;
    let unchanged = match existing_key.as_deref() {
        Some(key) => match entity_manager.get_by_entity_manager_key::<T>(key) {
            Some(cached) => entity.to_entity_model() == cached.to_entity_model(),
            None => false,
        },
        None => false,
    };

    if unchanged {
        entity
            .tide_sync_entity_manager_relations(entity_manager)
            .await?;
        entity_manager.put(entity);
        return Ok(existing_key);
    }

    let saved = save_in_scope(entity, entity_manager).await?;
    let saved_key = super::meta::model_entity_manager_key(&saved)?;
    *entity = saved;
    Ok(saved_key)
}

/// The keys of `baseline` that `current` no longer holds: the rows a copy of a
/// relation removed since it loaded. None without a baseline, since nothing can
/// have left a set that was never recorded.
fn removed_keys(baseline: Option<&[String]>, current: &[String]) -> Vec<String> {
    let current: HashSet<&String> = current.iter().collect();
    baseline
        .unwrap_or_default()
        .iter()
        .filter(|key| !current.contains(key))
        .cloned()
        .collect()
}

/// The keys of `current` that `baseline` did not hold, each once. Without a
/// baseline every key is new: reporting none would lose a relation set before
/// it ever loaded, and attaching one already attached is a no-op.
fn added_keys(baseline: Option<&[String]>, current: &[String]) -> Vec<String> {
    let baseline: HashSet<&String> = baseline.unwrap_or_default().iter().collect();
    let mut seen = HashSet::new();
    current
        .iter()
        .filter(|key| !baseline.contains(key) && seen.insert(*key))
        .cloned()
        .collect()
}

/// Deletes the entities a copy of an owner's relation held at its baseline but
/// no longer lists in `current_keys`, and drops them from the identity map.
#[doc(hidden)]
pub async fn __delete_detached_entities<T>(
    entity_manager: &Arc<EntityManager>,
    baseline: Option<&[String]>,
    current_keys: &[String],
) -> Result<()>
where
    T: Model + Clone + Send + Sync + 'static,
{
    for key in removed_keys(baseline, current_keys) {
        if let Some(deleted) = entity_manager.get_by_entity_manager_key::<T>(&key) {
            with_entity_manager_db(entity_manager, deleted.delete()).await?;
        }
        entity_manager.remove_by_entity_manager_key::<T>(&key);
    }

    Ok(())
}

pub(crate) async fn with_entity_manager_db<F, T>(
    entity_manager: &Arc<EntityManager>,
    future: F,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    if in_entity_manager_transaction_scope(entity_manager) {
        return future.await;
    }

    crate::database::__in_db_scope(entity_manager.db.as_ref(), future).await
}

/// The owner an aggregate's relation belongs to, for the relation syncs a
/// generated `tide_sync_entity_manager_relations` runs.
#[doc(hidden)]
pub struct __SyncOwner<'a> {
    pub entity_manager: &'a Arc<EntityManager>,
    pub table: &'static str,
}

/// Sync the children a `has_many` or `has_one` relation holds, if it is
/// loaded: delete the ones it held when it loaded or last synced but no
/// longer holds (first, as a unique foreign key refuses the new row beside the
/// old), point every child at the owner and save it, and record the keys it
/// holds now as its next baseline.
async fn sync_owned_children<R, C>(
    owner: &__SyncOwner<'_>,
    state: &mut crate::relations::RelationState<C>,
    children: impl FnOnce(&mut C) -> Vec<&mut R>,
    foreign_key: &str,
    owner_value: serde_json::Value,
) -> Result<()>
where
    R: Model + TideEntityManagerMergePersisted + TideEntityManagerSync,
    <<R as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
        PartialEq,
{
    if !state.is_loaded() {
        return Ok(());
    }
    let baseline = state.loaded_keys.clone();
    let children = state.cached_mut().map(children).unwrap_or_default();
    let current_keys = children
        .iter()
        .filter_map(|child| super::meta::model_entity_manager_key(&**child).transpose())
        .collect::<Result<Vec<String>>>()?;
    __delete_detached_entities::<R>(owner.entity_manager, baseline.as_deref(), &current_keys)
        .await?;

    let mut updated_keys = Vec::with_capacity(children.len());
    for child in children {
        if !child.set_field_json(foreign_key, owner_value.clone())? {
            return Err(crate::Error::query(format!(
                "{} has no field '{}' to hold its owner's key",
                R::table_name(),
                foreign_key
            )));
        }
        updated_keys.extend(__sync_related_entity(child, owner.entity_manager).await?);
    }
    state.loaded_keys = Some(updated_keys);
    Ok(())
}

/// Sync a `has_many` relation of an aggregate, if it is loaded.
#[doc(hidden)]
pub async fn __sync_has_many<R>(
    owner: &__SyncOwner<'_>,
    relation: &mut HasMany<R>,
    owner_value: serde_json::Value,
) -> Result<()>
where
    R: Model + TideEntityManagerMergePersisted + TideEntityManagerSync,
    <<R as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
        PartialEq,
{
    let foreign_key = relation.foreign_key;
    sync_owned_children(
        owner,
        &mut relation.state,
        |items| items.iter_mut().collect(),
        foreign_key,
        owner_value,
    )
    .await
}

/// Sync a `has_one` relation of an aggregate, if it is loaded.
#[doc(hidden)]
pub async fn __sync_has_one<R>(
    owner: &__SyncOwner<'_>,
    relation: &mut HasOne<R>,
    owner_value: serde_json::Value,
) -> Result<()>
where
    R: Model + TideEntityManagerMergePersisted + TideEntityManagerSync,
    <<R as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
        PartialEq,
{
    let foreign_key = relation.foreign_key;
    sync_owned_children(
        owner,
        &mut relation.state,
        |child| vec![&mut **child],
        foreign_key,
        owner_value,
    )
    .await
}

/// Sync a loaded `has_many_through` relation of an aggregate: save every
/// related model, then detach the ones it no longer holds and attach the new
/// ones through the pivot.
#[doc(hidden)]
pub async fn __sync_has_many_through<R, P>(
    owner: &__SyncOwner<'_>,
    relation: &mut HasManyThrough<R, P>,
) -> Result<()>
where
    R: Model + TideEntityManagerMergePersisted + TideEntityManagerSync,
    <<R as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
        PartialEq,
    P: Model,
{
    if !relation.is_loaded() {
        return Ok(());
    }
    let name = relation.relation_name;
    let related_key = relation.related_local_key;
    let related_value = |model: &R| {
        model.field_json_value(related_key)?.ok_or_else(|| {
            crate::Error::query(format!(
                "{} relation '{}' could not read related key '{}' from saved model",
                owner.table, name, related_key
            ))
        })
    };

    let mut updated_keys = Vec::new();
    let mut related_values = HashMap::<String, serde_json::Value>::new();
    if let Some(items) = relation.as_mut() {
        updated_keys.reserve(items.len());
        for item in items.iter_mut() {
            let key = __sync_related_entity(item, owner.entity_manager)
                .await?
                .ok_or_else(|| {
                    crate::Error::query(format!(
                        "{} relation '{}' requires persisted related keys after save",
                        owner.table, name
                    ))
                })?;
            related_values.insert(key.clone(), related_value(item)?);
            updated_keys.push(key);
        }
    }

    let entity_manager = owner.entity_manager;
    let baseline = relation.loaded_keys();
    for key in removed_keys(baseline, &updated_keys) {
        if let Some(deleted) = entity_manager.get_by_entity_manager_key::<R>(&key)
            && let Some(value) = deleted.field_json_value(related_key)?
        {
            relation.detach(value).await?;
        }
    }
    for key in added_keys(baseline, &updated_keys) {
        if let Some(value) = related_values.get(&key) {
            relation.attach(value.clone()).await?;
        }
    }
    relation.set_loaded_keys(updated_keys);
    Ok(())
}
