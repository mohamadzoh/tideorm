use async_trait::async_trait;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::orm::sql_builder::ValueKey;

use super::{EntityManager, ManagedModel};

mod flush;
mod flush_order;

use flush_order::RowLink;
pub(super) use flush_order::{
    explain_flush_ordering_failure, flush_sort_key, order_self_references, plan_flush_order,
};

/// Where a [`Managed`] entity stands with its [`EntityManager`], which decides
/// what the next [`flush`](EntityManager::flush) writes for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityState {
    /// Handed to [`persist`](EntityManager::persist) and not stored yet: the
    /// flush inserts it.
    New,
    /// Stored and tracked: the flush updates it when it changed.
    Managed,
    /// Handed to [`remove`](EntityManager::remove): the flush deletes it.
    Removed,
    /// No longer tracked, after [`detach`](EntityManager::detach) or
    /// [`clear`](EntityManager::clear): the flush leaves it alone.
    Detached,
}

/// A handle on an entity an [`EntityManager`] tracks.
///
/// Clones share one entity: an edit through any of them is what the next
/// [`flush`](EntityManager::flush) writes. Read it with [`read`](Self::read)
/// or [`get`](Self::get) and change it with [`edit`](Self::edit) or
/// [`replace`](Self::replace).
#[derive(Clone)]
pub struct Managed<T> {
    pub(crate) entry: Arc<ManagedEntry<T>>,
}

impl<T: std::fmt::Debug> std::fmt::Debug for Managed<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.read(|entity| {
            f.debug_struct("Managed")
                .field("state", &self.state())
                .field("entity", entity)
                .finish()
        })
    }
}

impl<T> Managed<T> {
    pub(crate) fn from_entry(entry: Arc<ManagedEntry<T>>) -> Self {
        Self { entry }
    }

    /// Where the entity stands with its manager.
    pub fn state(&self) -> EntityState {
        self.entry.state()
    }

    /// A copy of the entity as it is now.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.entry.get()
    }

    /// Run `read` on the entity as it is now, without copying it.
    ///
    /// The entity is locked for reading while `read` runs: an
    /// [`edit`](Self::edit) of it from inside waits forever.
    pub fn read<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        self.entry.read(read)
    }

    /// Change the entity in place; the next flush writes what changed.
    ///
    /// The entity is locked while `edit` runs: reading it from inside
    /// through this or another handle waits forever.
    pub fn edit<R>(&self, edit: impl FnOnce(&mut T) -> R) -> R {
        self.entry.edit(edit)
    }

    /// Put `entity` in place of the tracked one; the next flush writes what
    /// changed.
    pub fn replace(&self, entity: T) {
        self.entry.replace(entity);
    }
}

#[async_trait]
pub(crate) trait ManagedOps: Send + Sync {
    fn current_state(&self) -> EntityState;
    fn detach_from_context(&self, entity_manager: &EntityManager);

    /// Table this entry writes to, used to order the flush.
    fn table_name(&self) -> &'static str;

    /// Tables that must hold a row before this entry can be inserted.
    fn parent_tables(&self) -> Vec<&'static str>;

    /// Tables holding rows that reference this entry.
    fn child_tables(&self) -> Vec<&'static str>;

    /// The links a self-referencing foreign key makes between this entry's
    /// row and other rows of its table.
    fn row_links(&self) -> Vec<RowLink> {
        Vec::new()
    }

    async fn flush(
        self: Arc<Self>,
        entity_manager: &Arc<EntityManager>,
    ) -> crate::error::Result<()>;
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

    pub(crate) fn state(&self) -> EntityState {
        *self.state.read()
    }

    pub(crate) fn get(&self) -> T
    where
        T: Clone,
    {
        self.current.read().clone()
    }

    pub(crate) fn read<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        read(&self.current.read())
    }

    pub(crate) fn edit<R>(&self, edit: impl FnOnce(&mut T) -> R) -> R {
        let mut current = self.current.write();
        edit(&mut current)
    }

    pub(crate) fn replace(&self, entity: T) {
        *self.current.write() = entity;
    }

    /// Hand the entry `entity` to write, taking back a pending removal. A row
    /// already stored is updated; one not inserted yet (a `persist` that has
    /// not flushed) stays an insert, which flushes in the insert phase, before
    /// the children that point at it.
    pub(crate) fn overwrite_merged(&self, entity: T) {
        let state = if self.persisted_key.read().is_some() {
            EntityState::Managed
        } else {
            EntityState::New
        };
        *self.current.write() = entity;
        *self.state.write() = state;
    }

    /// Move the entry onto `saved`, the row as another path of this context
    /// just stored it: a field the holder has not changed since loading takes
    /// the stored value, one it changed keeps its edit, and the snapshot
    /// becomes the stored row. A later flush then writes the edits over the
    /// stored row instead of putting back the values the entry loaded. An
    /// entry never loaded (a pending insert) has nothing to rebase.
    ///
    /// Inside a transaction the entry goes back to what it held if that
    /// transaction does not commit, keeping the edits made since.
    pub(crate) fn rebase(self: &Arc<Self>, saved: &T) -> crate::error::Result<()>
    where
        T: crate::model::Model + Clone + Send + Sync + 'static,
    {
        let mut snapshot = self.snapshot.write();
        let Some(loaded) = snapshot.as_ref() else {
            return Ok(());
        };
        let mut current = self.current.write();
        let previous = current.clone();
        let fields = merge_fields(&mut *current, &previous, loaded, saved)?;
        let loaded = snapshot
            .replace(saved.clone())
            .expect("the snapshot was read above");

        let changes = FieldChanges {
            fields,
            before: previous,
            after: saved.clone(),
        };
        let entry = Arc::clone(self);
        crate::cache::undo_on_rollback(move || {
            changes.undo(&mut *entry.current.write());
            *entry.snapshot.write() = Some(loaded);
        });
        Ok(())
    }

    pub(crate) fn mark_removed(&self) {
        *self.state.write() = EntityState::Removed;
    }
}

impl<T: Send + Sync + 'static> ManagedEntry<T> {
    /// Take the entry out of the managed map, under the key it was filed as,
    /// and detach it. `identity_key`, not `persisted_key`: an entity
    /// `persist`ed with a client-assigned key is filed before any insert.
    pub(crate) fn evict(&self, entity_manager: &EntityManager) {
        // Flush completion uses the same lock when re-filing a saved entry.
        // Detaching must remove that entry and change its state atomically
        // with respect to that completion, or the save can reattach it.
        let mut state = self.state.write();
        if let Some(key) = self.identity_key.write().take() {
            entity_manager.remove_managed_entry::<T>(&key);
        }
        *state = EntityState::Detached;
    }
}

#[async_trait]
impl<T> ManagedOps for ManagedEntry<T>
where
    T: ManagedModel,
{
    fn current_state(&self) -> EntityState {
        self.state()
    }

    fn table_name(&self) -> &'static str {
        <T as crate::model::ModelMeta>::table_name()
    }

    fn parent_tables(&self) -> Vec<&'static str> {
        <T as crate::model::ModelMeta>::__parent_tables()
    }

    fn child_tables(&self) -> Vec<&'static str> {
        <T as crate::model::ModelMeta>::__child_tables()
    }

    fn row_links(&self) -> Vec<RowLink> {
        let table = <T as crate::model::ModelMeta>::table_name();
        let current = self.current.read();
        // A key the database has not numbered yet is no row's to reference.
        let unnumbered = <T as crate::model::Model>::is_new(&current);
        // Both sides read as keys of the referenced column, as the database
        // compares them.
        let key = |name: &str, kind: Option<&crate::orm::ColumnType>| {
            <T as crate::internal::InternalModel>::field_value(&current, name)
                .filter(|value| !crate::query::is_sql_null(value))
                .map(|value| ValueKey::new(crate::internal::column_value(&value, kind)))
        };
        <T as crate::model::ModelMeta>::__row_references()
            .into_iter()
            .filter(|(related, _, _)| *related == table)
            .map(|(_, foreign_key, referenced)| {
                let is_key = <T as crate::model::ModelMeta>::canonical_column_name(referenced)
                    .is_some_and(|column| {
                        <T as crate::model::ModelMeta>::primary_key_names().contains(&column)
                    });
                let kind = crate::internal::column_type_of::<T>(referenced);
                RowLink {
                    foreign_key,
                    referenced,
                    provides: if unnumbered && is_key {
                        None
                    } else {
                        key(referenced, kind.as_ref())
                    },
                    requires: key(foreign_key, kind.as_ref()),
                }
            })
            .collect()
    }

    fn detach_from_context(&self, entity_manager: &EntityManager) {
        self.evict(entity_manager);
    }

    async fn flush(
        self: Arc<Self>,
        entity_manager: &Arc<EntityManager>,
    ) -> crate::error::Result<()> {
        match self.state() {
            EntityState::Detached => Ok(()),
            EntityState::Removed => self.flush_removal(entity_manager).await,
            EntityState::New | EntityState::Managed => self.flush_write(entity_manager).await,
        }
    }
}

/// The fields a merge gave the stored value, which differed from what the
/// entry held: `before` holds those values, `after` the stored ones.
struct FieldChanges<T> {
    fields: Vec<&'static str>,
    before: T,
    after: T,
}

impl<T: crate::model::Model> FieldChanges<T> {
    /// Put back each field still holding the stored value: a field edited
    /// since keeps the edit.
    fn undo(self, target: &mut T) {
        for field in self.fields {
            if target.field_value(field) == self.after.field_value(field) {
                // An undo has no caller to report to; the field keeps its value.
                let _ = target.copy_field_from(&self.before, field);
            }
        }
    }
}

/// Give `target` each field of `holder` merged with `saved`, the row as
/// stored: a field `holder` still holds as in `base` takes the stored value,
/// and one changed since `base` keeps the change. Relation wrappers are
/// `target`'s own. Fields are compared and copied as values
/// ([`field_value`](crate::internal::InternalModel::field_value)), so a NaN
/// float neither hides a change nor fails a copy. Returns the fields that
/// took a different stored value.
fn merge_fields<T>(
    target: &mut T,
    holder: &T,
    base: &T,
    saved: &T,
) -> crate::error::Result<Vec<&'static str>>
where
    T: crate::model::Model,
{
    let mut changed = Vec::new();
    for &field in <T as crate::model::ModelMeta>::field_names() {
        let held = holder.field_value(field);
        let source = if held == base.field_value(field) {
            if held != saved.field_value(field) {
                changed.push(field);
            }
            saved
        } else {
            holder
        };
        target.copy_field_from(source, field)?;
    }
    Ok(changed)
}

#[cfg(test)]
#[path = "../../tests/unit/entity_manager_flush_order_tests.rs"]
mod tests;
