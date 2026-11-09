#![allow(missing_docs)]

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::error::Result;
use crate::model::Model;
use crate::relations::{HasMany, HasManyThrough, HasOne};

use super::{EntityManager, ManagedModel, model_identity_key};

thread_local! {
    /// The manager whose unit of work is running here, by address; `0` for none.
    static ENTITY_MANAGER_TRANSACTION_SCOPE: Cell<usize> = const { Cell::new(0) };
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

/// Restores the transaction-scope thread-local when the guard is dropped.
///
/// The restore has to happen in `Drop` rather than in plain statements after
/// `poll`: a panic inside the wrapped future unwinds straight past those
/// statements and would leave `ENTITY_MANAGER_TRANSACTION_SCOPE` stuck at
/// set on that worker thread forever, so every later flush/save polled there
/// would take the "already in scope" path and run with no transaction at all.
struct ResetEntityManagerTransactionScope(usize);

impl Drop for ResetEntityManagerTransactionScope {
    fn drop(&mut self) {
        ENTITY_MANAGER_TRANSACTION_SCOPE.with(|active| active.set(self.0));
    }
}

pub(super) fn with_entity_manager_transaction_scope<F>(
    entity_manager: &EntityManager,
    future: F,
) -> impl std::future::Future<Output = F::Output> + use<F>
where
    F: std::future::Future,
{
    let owner = scope_owner(entity_manager);
    crate::internal::per_poll(future, move || {
        ResetEntityManagerTransactionScope(
            ENTITY_MANAGER_TRANSACTION_SCOPE.with(|active| active.replace(owner)),
        )
    })
}

/// Before the unit of work replaces or removes the identity-map entry `key`
/// of `map`, register putting it back unless the work commits: when it
/// fails, when its future is dropped part way, and when a transaction
/// enclosing it rolls back later, or the manager would hand out rows that
/// were never committed. A [`clear`](EntityManager::clear) since then wins.
pub(super) fn record_identity_map_rollback(
    entity_manager: &EntityManager,
    key: &super::IdentityKey,
    map: &HashMap<super::IdentityKey, Arc<dyn std::any::Any + Send + Sync>>,
) {
    if !in_entity_manager_transaction_scope(entity_manager) {
        return;
    }
    let identity_map = entity_manager.identity_map.clone();
    let identity_generation = entity_manager.identity_generation.clone();
    let generation = identity_generation.load(super::Ordering::Relaxed);
    let key = key.clone();
    let original = map.get(&key).cloned();
    entity_manager.on_rollback(move || {
        let mut map = identity_map.write();
        if identity_generation.load(super::Ordering::Relaxed) != generation {
            return;
        }
        match original {
            Some(entity) => map.insert(key, entity),
            None => map.remove(&key),
        };
    });
}

pub(super) async fn save_with_entity_manager<T>(
    entity: &T,
    entity_manager: &Arc<EntityManager>,
) -> Result<T>
where
    T: ManagedModel,
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
    entity_manager
        .db
        .atomically(with_entity_manager_transaction_scope(
            entity_manager,
            async move { work().await },
        ))
        .await
}

/// Save `entity` and sync its loaded relations inside the unit of work the caller
/// already opened.
pub(super) async fn save_in_scope<T>(entity: &T, entity_manager: &Arc<EntityManager>) -> Result<T>
where
    T: ManagedModel,
{
    let mut aggregate = entity.clone();
    let persisted = with_entity_manager_db(
        entity_manager,
        <T as crate::model::Model>::save(entity.clone()),
    )
    .await?;
    aggregate.tide_merge_persisted(persisted);
    let aggregate = sync_aggregate(aggregate, entity_manager).await?;
    // A managed handle to the same row now loaded older values; one flushed
    // later would write them back over what was just stored.
    if let Some(managed) = entity_manager.get_managed_by_key::<T>(&model_identity_key(&aggregate)) {
        managed.entry.rebase(&aggregate)?;
    }
    Ok(aggregate)
}

pub(crate) async fn sync_entity_manager_relations_only_impl<T>(
    entity: &T,
    entity_manager: &Arc<EntityManager>,
) -> Result<T>
where
    T: ManagedModel,
{
    sync_aggregate(entity.clone(), entity_manager).await
}

/// `aggregate`, an entity as written, with its relation wrappers rebuilt from
/// its columns (a key an insert gave it among them), keeping what they
/// loaded, and its loaded relations synced, filed in the identity map.
async fn sync_aggregate<T>(aggregate: T, entity_manager: &Arc<EntityManager>) -> Result<T>
where
    T: ManagedModel,
{
    let mut aggregate = <T as crate::internal::InternalModel>::__rebuild_relations(aggregate);
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
pub(crate) async fn sync_related_entity<T>(
    entity: &mut T,
    entity_manager: &Arc<EntityManager>,
) -> Result<Option<String>>
where
    T: ManagedModel,
{
    let existing_key = super::meta::model_entity_manager_key(entity)?;
    let unchanged = match existing_key.as_deref() {
        Some(key) => match entity_manager.get_by_entity_manager_key::<T>(key) {
            Some(cached) => entity.same_columns(&cached),
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
pub(crate) async fn delete_detached_entities<T>(
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

    entity_manager.db.scope(future).await
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
    owner_value: crate::internal::Value,
) -> Result<()>
where
    R: ManagedModel,
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
    delete_detached_entities::<R>(owner.entity_manager, baseline.as_deref(), &current_keys).await?;

    let mut updated_keys = Vec::with_capacity(children.len());
    for child in children {
        if !child.set_field_value(foreign_key, owner_value.clone())? {
            return Err(crate::Error::query(format!(
                "{} has no field '{}' to hold its owner's key",
                R::table_name(),
                foreign_key
            )));
        }
        updated_keys.extend(sync_related_entity(child, owner.entity_manager).await?);
    }
    state.loaded_keys = Some(updated_keys);
    Ok(())
}

/// Sync a `has_many` relation of an aggregate, if it is loaded.
#[doc(hidden)]
pub async fn __sync_has_many<R>(
    owner: &__SyncOwner<'_>,
    relation: &mut HasMany<R>,
    owner_value: crate::internal::Value,
) -> Result<()>
where
    R: ManagedModel,
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
    owner_value: crate::internal::Value,
) -> Result<()>
where
    R: ManagedModel,
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
    R: ManagedModel,
    P: Model,
{
    if !relation.is_loaded() {
        return Ok(());
    }
    let name = relation.relation_name;
    let related_key = relation.related_local_key;
    let related_value = |model: &R| {
        model.field_value(related_key).ok_or_else(|| {
            crate::Error::query(format!(
                "{} relation '{}' could not read related key '{}' from saved model",
                owner.table, name, related_key
            ))
        })
    };

    let mut updated_keys = Vec::new();
    let mut related_values = HashMap::<String, crate::internal::Value>::new();
    if let Some(items) = relation.as_mut() {
        updated_keys.reserve(items.len());
        for item in items.iter_mut() {
            let key = sync_related_entity(item, owner.entity_manager)
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
            && let Some(value) = deleted.field_value(related_key)
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
