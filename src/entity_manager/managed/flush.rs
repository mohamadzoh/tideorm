//! Flushing one entry: writing or deleting its row, and putting the entry
//! back if the flush's transaction does not commit.

use std::sync::{Arc, Weak};

use crate::entity_manager::save::{save_in_scope, sync_entity_manager_relations_only_impl};
use crate::entity_manager::{
    EntityManager, ManagedModel, model_identity_key, with_entity_manager_db,
};

use super::{EntityState, FieldChanges, ManagedEntry, merge_fields};

/// What one flush of an entry changed, each part as `(before, after)`, to
/// put back if the flush's transaction does not commit.
struct FlushUndo<T> {
    fields: Option<FieldChanges<T>>,
    snapshot: (Option<T>, Option<T>),
    persisted_key: (Option<String>, Option<String>),
    identity_key: (Option<String>, Option<String>),
    state: (EntityState, EntityState),
}

impl<T> ManagedEntry<T>
where
    T: ManagedModel,
{
    /// Delete the row of an entry marked removed, if one was stored, and
    /// detach the entry.
    pub(super) async fn flush_removal(
        self: &Arc<Self>,
        entity_manager: &Arc<EntityManager>,
    ) -> crate::error::Result<()> {
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
            // A detached merge may have no loaded snapshot. In that
            // case `entity` is the live value, whose key can have been
            // edited since it was attached. Never delete another row.
            let delete_key = model_identity_key(&entity);
            if delete_key != key {
                return Err(crate::error::Error::query(format!(
                    "the primary key of a managed {} row changed from {key} to \
                     {delete_key}; refusing to delete a different row",
                    <T as crate::model::ModelMeta>::table_name()
                )));
            }
            with_entity_manager_db(entity_manager, <T as crate::model::Model>::delete(entity))
                .await?;
            entity_manager.remove_by_entity_manager_key::<T>(&key);
        }

        let snapshot = self.snapshot.write().take();
        let persisted_key = self.persisted_key.write().take();
        let identity_key = self.identity_key.read().clone();
        let state = self.state();
        self.evict(entity_manager);
        let undo = FlushUndo {
            fields: None,
            snapshot: (snapshot, None),
            persisted_key: (persisted_key, None),
            identity_key: (identity_key, None),
            state: (state, EntityState::Detached),
        };
        self.undo_flush_on_rollback(entity_manager, undo);
        Ok(())
    }

    /// Insert or update the entry's row and sync its relations, then move
    /// the entry onto the row as stored.
    pub(super) async fn flush_write(
        self: &Arc<Self>,
        entity_manager: &Arc<EntityManager>,
    ) -> crate::error::Result<()> {
        let current = self.current.read().clone();
        // A saved row is identified by its primary key, so writing
        // this entity under a changed key would write another row.
        if let Some(persisted) = self.persisted_key.read().as_deref() {
            let key = model_identity_key(&current);
            if key != persisted {
                return Err(crate::error::Error::query(format!(
                    "the primary key of a managed {} row changed from {persisted} to \
                     {key}; a managed entity keeps its key, so detach it and persist \
                     a new one",
                    <T as crate::model::ModelMeta>::table_name()
                )));
            }
        }
        let columns_changed = match self.snapshot.read().as_ref() {
            Some(snapshot) => !snapshot.same_columns(&current),
            None => true,
        };

        // The map entry may predate the insert: `persist` files a
        // client-assigned key immediately. Evict under whatever it was
        // actually filed as, not under the persisted key it may not have.
        let previous_key = self.identity_key.read().as_ref().cloned();
        let saved = if columns_changed {
            save_in_scope(&current, entity_manager).await?
        } else {
            sync_entity_manager_relations_only_impl(&current, entity_manager).await?
        };
        let next_key = Some(model_identity_key(&saved));

        // Edits made while the row was being written stay, over the
        // row as stored, and leave the entry dirty for the next flush.
        let fields = {
            let mut live = self.current.write();
            // A relation edited meanwhile cannot be merged with the rows
            // the flush wrote for it. Failing rolls the flush back,
            // which leaves the edited value for the next flush.
            if !live.tide_relations_match(&current) {
                return Err(crate::error::Error::query(format!(
                    "a relation of a managed {} row was edited while the flush wrote \
                     it; the flush was rolled back and the edit kept, so flush again",
                    <T as crate::model::ModelMeta>::table_name()
                )));
            }
            let mut stored = saved.clone();
            let fields = merge_fields(&mut stored, &*live, &current, &saved)?;
            *live = stored;
            FieldChanges {
                fields,
                before: current,
                after: saved.clone(),
            }
        };
        let snapshot = self.snapshot.write().replace(saved.clone());
        let persisted_key = std::mem::replace(&mut *self.persisted_key.write(), next_key.clone());

        // A remove or detach may have arrived during the save. Keep
        // that request, while recording the saved row so a pending
        // removal can delete even an insert that just received its key.
        // Serialize re-filing with evict, without holding this lock
        // across database work or user edits to the current value.
        let mut state = self.state.write();
        let state_before = *state;
        let identity_before = self.identity_key.read().clone();
        if *state != EntityState::Detached {
            if let Some(ref previous_key) = previous_key
                && Some(previous_key.as_str()) != next_key.as_deref()
            {
                entity_manager.remove_managed_entry::<T>(previous_key);
            }
            if let Some(key) = next_key.as_deref() {
                entity_manager.put_managed_entry::<T>(key, self.clone());
            }
            *self.identity_key.write() = next_key.clone();
            if *state != EntityState::Removed {
                *state = EntityState::Managed;
            }
        }
        let undo = FlushUndo {
            fields: Some(fields),
            snapshot: (snapshot, Some(saved)),
            persisted_key: (persisted_key, next_key),
            identity_key: (identity_before, self.identity_key.read().clone()),
            state: (state_before, *state),
        };
        drop(state);
        self.undo_flush_on_rollback(entity_manager, undo);
        Ok(())
    }

    /// Put back what a flush changed if its transaction does not commit:
    /// when the flush fails, is cancelled, or a transaction enclosing it
    /// rolls back.
    fn undo_flush_on_rollback(
        self: &Arc<Self>,
        entity_manager: &Arc<EntityManager>,
        undo: FlushUndo<T>,
    ) {
        let entry = Arc::clone(self);
        let entity_manager = Arc::downgrade(entity_manager);
        crate::cache::undo_on_rollback(move || entry.undo_flush(&entity_manager, undo));
    }

    /// Put back each part of `undo` that still holds what the flush left, so
    /// what was done to the entry since, an edit, a removal or a detach,
    /// stays. An entry the flush detached is managed again.
    fn undo_flush(self: &Arc<Self>, entity_manager: &Weak<EntityManager>, undo: FlushUndo<T>) {
        let Some(entity_manager) = entity_manager.upgrade() else {
            return;
        };
        let mut state = self.state.write();
        if let Some(fields) = undo.fields {
            let mut current = self.current.write();
            // The entry took the stored row's relation wrappers, with the
            // rows the flush wrote and the keys a save diffs against; unless
            // edited since, they go back to what the flush read.
            if current.tide_relations_match(&fields.after) {
                current.tide_relations_from(&fields.before);
            }
            fields.undo(&mut *current);
        }
        {
            let mut snapshot = self.snapshot.write();
            let unchanged = match (snapshot.as_ref(), undo.snapshot.1.as_ref()) {
                (Some(now), Some(written)) => now.same_columns(written),
                (now, written) => now.is_none() && written.is_none(),
            };
            if unchanged {
                *snapshot = undo.snapshot.0;
            }
        }
        {
            let mut persisted_key = self.persisted_key.write();
            if *persisted_key == undo.persisted_key.1 {
                *persisted_key = undo.persisted_key.0;
            }
        }
        {
            let (before, after) = undo.identity_key;
            let mut identity_key = self.identity_key.write();
            if *identity_key == after && before != after {
                if let Some(key) = after.as_deref() {
                    entity_manager.remove_managed_entry::<T>(key);
                }
                if let Some(key) = before.as_deref() {
                    entity_manager.put_managed_entry::<T>(key, self.clone());
                }
                *identity_key = before;
            }
        }
        let (before, after) = undo.state;
        let reregister = *state == after && before != after && after == EntityState::Detached;
        if *state == after {
            *state = before;
        }
        // A flush that completes drops its detached entries from the list; the
        // list's lock is taken before an entry's when it does.
        drop(state);
        if reregister {
            entity_manager.reregister_managed_entry(self.clone());
        }
    }
}
