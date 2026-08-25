#![allow(missing_docs)]

use async_trait::async_trait;
use std::collections::HashMap;
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

/// Coarse flush ordering rank: inserts, then updates, then deletes.
///
/// A flush runs every pending operation inside a single transaction, so a
/// mis-ordered foreign key aborts the whole flush. Grouping the work by
/// operation kind is the first half of the ordering; [`plan_flush_order`]
/// supplies the second half, which orders tables against each other inside a
/// kind. Sorting must stay stable so entries that tie on both keep their
/// registration order.
fn flush_order_rank(state: EntityState) -> u8 {
    match state {
        EntityState::New => 0,
        EntityState::Managed => 1,
        EntityState::Removed => 2,
        EntityState::Detached => 3,
    }
}

/// Table-level dependency order for one pass of a flush.
///
/// Grouping by operation kind alone still writes `posts` before `users` when
/// the child was persisted first, which the database rejects. This ranks the
/// tables taking part in the pass so inserts can run parents-first and deletes
/// children-first.
pub(super) struct FlushOrder {
    ranks: HashMap<&'static str, usize>,
    unordered: Vec<&'static str>,
}

impl FlushOrder {
    fn rank(&self, table: &str) -> usize {
        self.ranks.get(table).copied().unwrap_or(0)
    }

    /// Tables whose declared foreign keys form a cycle, so no total order over
    /// them exists. They keep registration order and are named in the error if
    /// the database then rejects a write.
    pub(super) fn unordered_tables(&self) -> &[&'static str] {
        &self.unordered
    }
}

/// Record `parent -> child` if it is a real, expressible constraint.
///
/// Edges to tables outside this pass cannot change the order, and dropping
/// them stops an absent table from inventing a cycle the flush would never
/// hit. A self-edge is dropped too: a self-referencing foreign key
/// (`parent_id` on the same table) is a legal schema, not a table-level
/// ordering constraint, and keeping it would report every such table as an
/// unbreakable cycle.
fn add_dependency_edge(
    edges: &mut Vec<(&'static str, &'static str)>,
    tables: &[&'static str],
    parent: &'static str,
    child: &'static str,
) {
    if parent == child || !tables.contains(&parent) || !tables.contains(&child) {
        return;
    }

    if !edges.contains(&(parent, child)) {
        edges.push((parent, child));
    }
}

/// Topologically sort the tables taking part in this flush pass.
///
/// Edges come from both ends of every declared relation — a child's
/// `belongs_to` and a parent's `has_one`/`has_many` — so one declaration on
/// either side is enough. Kahn's algorithm runs over the tables in first-seen
/// order, which keeps the result deterministic; whatever it cannot drain is a
/// cycle and is appended in registration order rather than looping forever.
pub(super) fn plan_flush_order(entries: &[Arc<dyn ManagedOps>]) -> FlushOrder {
    let mut tables: Vec<&'static str> = Vec::new();
    for entry in entries {
        let table = entry.table_name();
        if !tables.contains(&table) {
            tables.push(table);
        }
    }

    let mut edges: Vec<(&'static str, &'static str)> = Vec::new();
    for entry in entries {
        let table = entry.table_name();
        for parent in entry.parent_tables() {
            add_dependency_edge(&mut edges, &tables, parent, table);
        }
        for child in entry.child_tables() {
            add_dependency_edge(&mut edges, &tables, table, child);
        }
    }

    let mut ordered: Vec<&'static str> = Vec::with_capacity(tables.len());
    let mut remaining = tables;
    while !remaining.is_empty() {
        let next = remaining.iter().position(|table| {
            !edges
                .iter()
                .any(|(parent, child)| child == table && remaining.contains(parent))
        });
        let Some(next) = next else {
            break;
        };

        ordered.push(remaining.remove(next));
    }

    let unordered = remaining.clone();
    ordered.extend(remaining);

    let ranks = ordered
        .into_iter()
        .enumerate()
        .map(|(rank, table)| (table, rank))
        .collect();

    FlushOrder { ranks, unordered }
}

/// Stable sort key placing an entry inside its operation kind.
///
/// Inserts ascend the table order so a parent row exists before the child that
/// references it; deletes descend it so children go before the parent they
/// reference. Updates and detached entries add no constraint of their own and
/// tie, which the stable sort resolves as registration order.
pub(super) fn flush_sort_key(entry: &dyn ManagedOps, order: &FlushOrder) -> (u8, usize) {
    let state = entry.current_state();
    let rank = order.rank(entry.table_name());
    let within_kind = match state {
        EntityState::New => rank,
        EntityState::Removed => order.ranks.len().saturating_sub(rank),
        EntityState::Managed | EntityState::Detached => 0,
    };

    (flush_order_rank(state), within_kind)
}

/// Say why a flush was ordered the way it was when the database rejects it.
///
/// The ordering only covers foreign keys a model actually declares, so a flush
/// can still abort on SQLSTATE 23503 — previously with nothing but the driver's
/// message, which never mentions that an entity manager chose the write order.
/// Name the table being written and either the cycle that could not be broken
/// or the fact that no relation declared the missing dependency. The driver
/// failure stays attached as the source, so the SQLSTATE, constraint name and
/// error chain survive.
pub(super) fn explain_flush_ordering_failure(
    error: crate::error::Error,
    table: &'static str,
    unordered: &[&'static str],
) -> crate::error::Error {
    if !error.is_foreign_key_violation() {
        return error;
    }

    let detail = if unordered.is_empty() {
        String::from(
            "every table in this flush was ordered parent-before-child, so the row it \
             references belongs to a table this model declares no relation to; declare the \
             relation (`belongs_to` or `has_many`/`has_one`) or persist the parent first",
        )
    } else {
        format!(
            "these tables declare foreign keys in a cycle and could not be ordered: {}; \
             flush one side of the cycle before the other",
            unordered.join(", ")
        )
    };

    let rendered = error.to_string();
    let failure = error.into_db_failure().map(|failure| *failure);

    crate::error::Error::query(format!(
        "entity manager flush failed writing `{table}`: {rendered} — {detail}"
    ))
    .with_db_failure(failure)
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

#[async_trait]
pub(crate) trait ManagedOps: Send + Sync {
    fn current_state(&self) -> EntityState;
    fn detach_from_context(&self, entity_manager: &EntityManager);
    fn checkpoint(self: Arc<Self>) -> Box<dyn ManagedCheckpoint>;

    /// Table this entry writes to, used to order the flush.
    ///
    /// The three relation-order hooks default to "no declared relations", so a
    /// hand-written entry keeps flushing in registration order within its
    /// operation kind instead of having to describe a schema it does not have.
    fn table_name(&self) -> &'static str {
        ""
    }

    /// Tables that must hold a row before this entry can be inserted.
    fn parent_tables(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// Tables holding rows that reference this entry.
    fn child_tables(&self) -> Vec<&'static str> {
        Vec::new()
    }

    async fn flush(
        self: Arc<Self>,
        entity_manager: &Arc<EntityManager>,
    ) -> crate::error::Result<()>;
}

pub(crate) trait ManagedCheckpoint: Send {
    fn rollback(self: Box<Self>, entity_manager: &EntityManager);
}

pub(crate) struct ManagedEntry<T> {
    current: RwLock<T>,
    snapshot: RwLock<Option<T>>,
    state: RwLock<EntityState>,
    persisted_key: RwLock<Option<String>>,
    /// The key this entry is filed under in the manager's identity map.
    ///
    /// Deliberately separate from [`Self::persisted_key`], which answers a
    /// different question: whether a row for this entity exists in the database.
    /// The two coincide for a loaded entity but not for one handed to `persist`
    /// with a client-assigned primary key — that is trackable immediately, so it
    /// goes into the identity map, while nothing has been inserted yet. Keying
    /// removal off `persisted_key` there left an entry no path could clear, so
    /// `detach` did not detach and a later `find_managed` returned a row that had
    /// never been written.
    identity_key: RwLock<Option<String>>,
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
            // A caller that already knows the persisted key is filing the entry
            // under it; `persist` overrides this for the client-assigned case.
            identity_key: RwLock::new(persisted_key.clone()),
            persisted_key: RwLock::new(persisted_key),
        }
    }

    /// Record the identity-map key this entry was filed under.
    pub(crate) fn set_identity_key(&self, key: Option<String>) {
        *self.identity_key.write() = key;
    }

    pub(crate) fn identity_key(&self) -> Option<String> {
        self.identity_key.read().clone()
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

    fn mark_detached(&self) {
        *self.state.write() = EntityState::Detached;
    }

    pub(crate) fn mark_detached_public(&self) {
        self.mark_detached();
    }
}

#[async_trait]
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

    fn table_name(&self) -> &'static str {
        <T as TideEntityManagerMeta>::tide_table_name()
    }

    fn parent_tables(&self) -> Vec<&'static str> {
        <T as TideEntityManagerMeta>::tide_parent_tables()
    }

    fn child_tables(&self) -> Vec<&'static str> {
        <T as TideEntityManagerMeta>::tide_child_tables()
    }

    fn detach_from_context(&self, entity_manager: &EntityManager) {
        // `identity_key`, not `persisted_key`: an entity given to `persist` with a
        // client-assigned primary key is in the map without having been inserted.
        if let Some(key) = self.identity_key.read().as_ref() {
            entity_manager.remove_managed_entry::<T>(key);
        }
        *self.identity_key.write() = None;

        self.mark_detached();
    }

    fn checkpoint(self: Arc<Self>) -> Box<dyn ManagedCheckpoint> {
        Box::new(ManagedEntryCheckpoint {
            entry: self.clone(),
            current: self.current.read().clone(),
            snapshot: self.snapshot.read().clone(),
            state: self.state(),
            persisted_key: self.persisted_key.read().clone(),
            identity_key: self.identity_key.read().clone(),
        })
    }

    async fn flush(
        self: Arc<Self>,
        entity_manager: &Arc<EntityManager>,
    ) -> crate::error::Result<()> {
        match self.state() {
            EntityState::Detached => Ok(()),
            EntityState::Removed => {
                // Only a row that exists gets a DELETE, so that stays gated on
                // `persisted_key`. Evicting from the identity map is a separate
                // question and uses `identity_key`, which is also set for an
                // entity `persist`ed under a client-assigned key and never
                // inserted — without this it survived the remove and a later
                // `find_managed` handed back a row that was never written.
                let persisted = self.persisted_key.read().as_ref().cloned();
                if let Some(key) = persisted {
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
                }

                if let Some(key) = self.identity_key.read().as_ref() {
                    entity_manager.remove_managed_entry::<T>(key);
                }

                *self.snapshot.write() = None;
                *self.persisted_key.write() = None;
                *self.identity_key.write() = None;
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

                // The map entry may predate the insert: `persist` files a
                // client-assigned key immediately. Evict under whatever it was
                // actually filed as, not under the persisted key it may not have.
                let previous_key = self.identity_key.read().as_ref().cloned();
                let saved = if columns_changed {
                    save_with_entity_manager_impl(&current, entity_manager).await?
                } else {
                    sync_entity_manager_relations_only_impl(&current, entity_manager).await?
                };
                let next_key = Some(saved.tide_pk_key());

                if let Some(ref previous_key) = previous_key
                    && Some(previous_key.as_str()) != next_key.as_deref()
                {
                    entity_manager.remove_managed_entry::<T>(previous_key);
                }

                if let Some(key) = next_key.as_deref() {
                    entity_manager.put_managed_entry::<T>(key, self.clone());
                }

                *self.current.write() = saved.clone();
                *self.snapshot.write() = Some(saved.clone());
                *self.identity_key.write() = next_key.clone();
                *self.persisted_key.write() = next_key;
                *self.state.write() = EntityState::Managed;
                entity_manager.put(saved);
                Ok(())
            }
        }
    }
}

struct ManagedEntryCheckpoint<T> {
    entry: Arc<ManagedEntry<T>>,
    current: T,
    snapshot: Option<T>,
    state: EntityState,
    persisted_key: Option<String>,
    identity_key: Option<String>,
}

impl<T> ManagedCheckpoint for ManagedEntryCheckpoint<T>
where
    T: Send + Sync + 'static,
{
    fn rollback(self: Box<Self>, entity_manager: &EntityManager) {
        // Both evict and re-file go through `identity_key`, which is what the map
        // is actually keyed by; `persisted_key` is restored as plain state.
        if let Some(current_key) = self.entry.identity_key.read().as_deref() {
            entity_manager.remove_managed_entry::<T>(current_key);
        }

        if let Some(previous_key) = self.identity_key.as_deref() {
            entity_manager.put_managed_entry::<T>(previous_key, self.entry.clone());
        }

        *self.entry.current.write() = self.current;
        *self.entry.snapshot.write() = self.snapshot;
        *self.entry.state.write() = self.state;
        *self.entry.persisted_key.write() = self.persisted_key;
        *self.entry.identity_key.write() = self.identity_key;
    }
}

#[cfg(test)]
#[path = "../../tests/unit/entity_manager_flush_order_tests.rs"]
mod tests;
