use std::any::TypeId;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;

use crate::error::{Error, Result};
use crate::internal::Value;
use crate::orm::sql_builder::ValueKey;
use crate::orm::{EntityTrait, Iterable, ModelTrait};

use super::Model;

/// A baseline's model type and primary key.
type RowKey = (TypeId, Vec<ValueKey>);
/// The pool a baseline's row was read from: the same key in two databases is
/// two rows, and each keeps its own baseline.
type Origin = Option<u64>;
/// Whose baseline an entry is: the pool its row came from, and the task that
/// read or wrote it, `None` for the one every task shares.
type Slot = (Origin, Option<Task>);
/// Each field's value, by field name, shared so a reader copies no values.
type SnapshotValues = Arc<HashMap<&'static str, Value>>;
/// One row's baselines, by slot.
type Slots = HashMap<Slot, SnapshotEntry>;

/// An async task. Two requests a server handles side by side run in two
/// tasks, so each compares its copy of a row with the one it read or wrote,
/// whatever the other writes meanwhile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Task {
    #[cfg(feature = "runtime-tokio")]
    Tokio(tokio::task::Id),
    #[cfg(feature = "runtime-async-std")]
    AsyncStd(async_std::task::TaskId),
}

/// The task running this code; `None` outside one, such as in a `block_on`
/// body (`#[tokio::main]`, `#[tokio::test]`).
fn current_task() -> Option<Task> {
    #[cfg(feature = "runtime-tokio")]
    if let Some(id) = tokio::task::try_id() {
        return Some(Task::Tokio(id));
    }
    #[cfg(feature = "runtime-async-std")]
    if let Some(task) = async_std::task::try_current() {
        return Some(Task::AsyncStd(task.id()));
    }
    None
}

/// Upper bound on how many dirty-tracking baselines are kept in memory.
///
/// The baseline store is process-global, so leaving it unbounded means a
/// read-heavy service paging through a large table grows until it runs out of
/// memory. Once the bound is reached, the oldest remembered baselines are
/// evicted first; a model whose baseline was evicted stops reporting dirty
/// state instead of returning stale data. A row read in a task takes two
/// entries, the task's and the shared one, which share their values.
const DEFAULT_SNAPSHOT_CAPACITY: usize = 20_000;

struct SnapshotEntry {
    sequence: u64,
    /// `None` when the pool's baseline was forgotten while another pool
    /// still held a different one: a model does not say which pool it came
    /// from, so the other baseline must not become the key's.
    values: Option<SnapshotValues>,
}

/// Bounded, insertion-ordered store of dirty-tracking baselines.
///
/// `entries` holds each row's snapshots by model type, key and slot (pool and
/// task), so invalidating a model touches only its own rows, and `order` maps
/// insertion sequence to row and slot, one per snapshot, so eviction is
/// O(log n) instead of an O(n) scan. Every mutation keeps the two in sync, so
/// `order` can never accumulate stale keys.
struct SnapshotStore {
    entries: HashMap<TypeId, HashMap<Vec<ValueKey>, Slots>>,
    order: BTreeMap<u64, (RowKey, Slot)>,
    next_sequence: u64,
    capacity: usize,
}

impl SnapshotStore {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: BTreeMap::new(),
            next_sequence: 0,
            capacity: capacity.max(1),
        }
    }

    /// The baseline of `row` for `task`: the task's own, if it read or wrote
    /// the row, or else the shared one, and either only when it is the same
    /// in every pool the row was read from.
    ///
    /// A model does not record which database it came from, so where two
    /// databases gave the key different rows, or one pool's baseline of it
    /// was forgotten, no baseline is known for it: the scope a model is
    /// inspected in need not be the one it was loaded through, and guessing
    /// would report another row's values.
    fn get(&self, row: &RowKey, task: Option<Task>) -> Option<&SnapshotValues> {
        let slots = self.slots(row)?;
        let task = task.filter(|task| slots.keys().any(|(_, owner)| *owner == Some(*task)));
        let mut baselines = slots
            .iter()
            .filter(|((_, owner), _)| *owner == task)
            .map(|(_, entry)| entry.values.as_ref());
        let first = baselines.next()??;
        baselines.all(|other| other == Some(first)).then_some(first)
    }

    /// The baselines of `row`, by slot.
    fn slots(&self, (model, key): &RowKey) -> Option<&Slots> {
        self.entries.get(model)?.get(key)
    }

    /// Drop `row`'s emptied slots, and its model's rows once they are empty.
    fn remove_row(&mut self, (model, key): &RowKey) {
        if let Some(rows) = self.entries.get_mut(model) {
            rows.remove(key);
            if rows.is_empty() {
                self.entries.remove(model);
            }
        }
    }

    /// Store one slot's baseline of `row` and return its sequence.
    fn insert(&mut self, row: RowKey, slot: Slot, values: Option<SnapshotValues>) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.put(row, slot, SnapshotEntry { sequence, values });
        sequence
    }

    fn put(&mut self, row: RowKey, slot: Slot, entry: SnapshotEntry) {
        self.order.insert(entry.sequence, (row.clone(), slot));
        let (model, key) = row;
        let slots = self
            .entries
            .entry(model)
            .or_default()
            .entry(key)
            .or_default();
        if let Some(previous) = slots.insert(slot, entry) {
            self.order.remove(&previous.sequence);
        }

        // A row leaves the evicted baseline's task with every pool's baseline
        // of it, and the shared ones with it, so neither another pool's
        // baseline nor the shared one becomes what that task reads.
        while self.order.len() > self.capacity {
            let Some((_, (row, (_, task)))) = self.order.pop_first() else {
                break;
            };
            let Some(slots) = self
                .entries
                .get_mut(&row.0)
                .and_then(|rows| rows.get_mut(&row.1))
            else {
                continue;
            };
            slots.retain(|(_, owner), entry| {
                let keep = owner.is_some() && *owner != task;
                if !keep {
                    self.order.remove(&entry.sequence);
                }
                keep
            });
            if slots.is_empty() {
                self.remove_row(&row);
            }
        }
    }

    /// Remove one slot's baseline of `row`.
    fn take(&mut self, row: &RowKey, slot: Slot) -> Option<SnapshotEntry> {
        let baselines = self.entries.get_mut(&row.0)?.get_mut(&row.1)?;
        let entry = baselines.remove(&slot)?;
        if baselines.is_empty() {
            self.remove_row(row);
        }
        self.order.remove(&entry.sequence);
        Some(entry)
    }

    /// Set one slot's baseline of `row` to `values`, or forget it for `None`.
    /// Returns the entry it replaces, and the sequence of the entry left in
    /// its place, if any, for [`restore`](Self::restore).
    fn replace(
        &mut self,
        row: RowKey,
        slot: Slot,
        values: Option<SnapshotValues>,
    ) -> (Option<SnapshotEntry>, Option<u64>) {
        let previous = self.take(&row, slot);
        let values = match values {
            Some(values) => Some(values),
            // Forgotten while another pool still holds a different baseline
            // of the key for the same task: that baseline must not become
            // the key's.
            None if previous.as_ref().is_some_and(|previous| {
                self.slots(&row).is_some_and(|others| {
                    others
                        .iter()
                        .filter(|((_, owner), _)| *owner == slot.1)
                        .any(|(_, other)| {
                            previous.values.is_none() || other.values != previous.values
                        })
                })
            }) =>
            {
                None
            }
            None => return (previous, None),
        };
        let sequence = self.insert(row, slot, values);
        (previous, Some(sequence))
    }

    /// Undo a [`replace`](Self::replace) that left `standing` behind, by
    /// putting `previous` back, unless the slot's baseline of `row` changed
    /// again since: a transaction that rolls back undoes its own reads and
    /// writes, not a later one's.
    fn restore(
        &mut self,
        row: RowKey,
        slot: Slot,
        standing: Option<u64>,
        previous: Option<SnapshotEntry>,
    ) {
        let current = self
            .slots(&row)
            .and_then(|baselines| baselines.get(&slot))
            .map(|entry| entry.sequence);
        if current != standing {
            return;
        }
        self.take(&row, slot);
        if let Some(previous) = previous {
            self.put(row, slot, previous);
        }
    }

    /// Drop every baseline. Sequences go on from where they were, so an
    /// undo step still pending cannot mistake a new baseline for its own.
    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
}

fn snapshot_store() -> &'static RwLock<SnapshotStore> {
    static STORE: OnceLock<RwLock<SnapshotStore>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(SnapshotStore::new(DEFAULT_SNAPSHOT_CAPACITY)))
}

/// `model`'s row: its type and its key columns' values, integers by value
/// whatever their width. `None` for a model not saved yet.
fn row_key_for_model<M: Model>(model: &M) -> Option<RowKey> {
    if M::primary_key_is_new(&model.primary_key()) {
        return None;
    }
    let key = M::primary_key_columns()
        .into_iter()
        .map(|column| ValueKey::new(ModelTrait::get(model, column)))
        .collect();
    Some((TypeId::of::<M>(), key))
}

/// What `read` takes from `model`'s baseline in the current task, read under
/// the store's lock; `None` when no baseline is known.
fn read_snapshot<M: Model, T>(model: &M, read: impl FnOnce(&SnapshotValues) -> T) -> Option<T> {
    let row = row_key_for_model(model)?;
    let store = snapshot_store().read();
    store.get(&row, current_task()).map(read)
}

/// Each field with the value its column holds. The generated entity's
/// columns follow `field_names()`, so no name is looked up.
fn field_values<M: Model>(model: &M) -> impl Iterator<Item = (&'static str, Value)> + '_ {
    M::field_names()
        .iter()
        .copied()
        .zip(<<M::Entity as EntityTrait>::Column as Iterable>::iter())
        .map(|(field, column)| (field, ModelTrait::get(model, column)))
}

fn capture_snapshot<M: Model>(model: &M) -> SnapshotValues {
    Arc::new(field_values(model).collect())
}

/// Remember each of `models`' current persisted state as its dirty-tracking
/// baseline of the pool `origin` it was read from, all under one lock.
///
/// The baseline store is capacity-bounded; remembering past that point evicts
/// the oldest baselines rather than growing without limit.
pub(crate) fn remember_models_from<M: Model>(models: &[M], origin: Origin) {
    // Captured before the lock is taken, so no model is read under it.
    let baselines: Vec<_> = models
        .iter()
        .filter_map(|model| Some((row_key_for_model(model)?, Some(capture_snapshot(model)))))
        .collect();
    set_baselines(baselines, origin);
}

/// Set the pool `origin`'s baseline of each row, or drop it for `None`, for
/// the current task and for the shared one, which a task that never read the
/// row falls back to.
///
/// A row read or written inside a transaction is only what the database
/// holds once it commits, so if it rolls back instead, the baselines this
/// replaced come back, unless the slot's baseline of the row changed again
/// in the meantime.
fn set_baselines(baselines: Vec<(RowKey, Option<SnapshotValues>)>, origin: Origin) {
    if baselines.is_empty() {
        return;
    }
    let own = current_task().map(|task| (origin, Some(task)));
    let mut store = snapshot_store().write();
    let mut undo = Vec::with_capacity(baselines.len() * 2);
    for (row, values) in baselines {
        for slot in own.into_iter().chain([(origin, None)]) {
            let (previous, standing) = store.replace(row.clone(), slot, values.clone());
            undo.push((row.clone(), slot, standing, previous));
        }
    }
    drop(store);
    crate::cache::undo_on_rollback(move || {
        let mut store = snapshot_store().write();
        for (row, slot, standing, previous) in undo.into_iter().rev() {
            store.restore(row, slot, standing, previous);
        }
    });
}

/// Forget one model's dirty-tracking baseline.
pub fn forget_model<M: Model>(model: &M) {
    forget_model_from(model, crate::database::scope_origin())
}

/// Forget `model`'s baseline as read from the pool `origin`.
pub(crate) fn forget_model_from<M: Model>(model: &M, origin: Origin) {
    if let Some(row) = row_key_for_model(model) {
        set_baselines(vec![(row, None)], origin);
    }
}

/// Forget every dirty-tracking baseline for one model type: the current
/// task's and the shared ones. Another task's stay, as that task's copies
/// were read before this write.
pub fn invalidate_model<M: Model>() {
    let model_type = TypeId::of::<M>();
    let task = current_task();
    let mut store = snapshot_store().write();
    let keys: Vec<_> = store
        .entries
        .get(&model_type)
        .into_iter()
        .flatten()
        .flat_map(|(key, baselines)| {
            baselines
                .keys()
                .filter(|(_, owner)| owner.is_none() || *owner == task)
                .map(move |slot| ((model_type, key.clone()), *slot))
        })
        .collect();
    let mut undo = Vec::with_capacity(keys.len());
    for (row, slot) in keys {
        let previous = store.take(&row, slot);
        let standing = store.insert(row.clone(), slot, None);
        undo.push((row, slot, standing, previous));
    }
    drop(store);
    let mut undo = InvalidatedBaselines(undo);
    crate::cache::undo_on_rollback(move || {
        let mut store = snapshot_store().write();
        for (row, slot, standing, previous) in undo.0.drain(..) {
            store.restore(row, slot, Some(standing), previous);
        }
    });
}

/// A committed invalidation forgets its tombstones. Until then they retain
/// mutation identities so rollback cannot overwrite a newer baseline.
struct InvalidatedBaselines(Vec<(RowKey, Slot, u64, Option<SnapshotEntry>)>);

impl Drop for InvalidatedBaselines {
    fn drop(&mut self) {
        if self.0.is_empty() {
            return;
        }
        let mut store = snapshot_store().write();
        for (row, slot, standing, _) in self.0.drain(..) {
            let current = store.slots(&row).and_then(|baselines| baselines.get(&slot));
            if current.is_some_and(|entry| entry.sequence == standing) {
                store.take(&row, slot);
            }
        }
    }
}

/// Clear every remembered dirty-tracking baseline.
pub fn clear_all() {
    snapshot_store().write().clear();
}

/// Fields whose current value differs from the remembered baseline.
///
/// `None` means there is no baseline to compare against — a new or
/// hand-built model, a model rebuilt from JSON, one whose baseline was
/// evicted from the bounded store, or one whose key two databases gave
/// different rows. `Some(fields)` means a baseline was
/// found, so an empty vector really does mean "nothing changed".
pub(crate) fn changed_fields<M: Model>(model: &M) -> Option<Vec<&'static str>> {
    // A shared handle, so the model's fields are not read under the lock.
    let snapshot = read_snapshot(model, Arc::clone)?;
    Some(
        field_values(model)
            .filter(|(field, value)| Some(value) != snapshot.get(field))
            .map(|(field, _)| field)
            .collect(),
    )
}

/// One field's remembered value.
///
/// The outer `Option` reports baseline presence exactly like [`changed_fields`]
/// does: `Ok(None)` means no baseline exists. The inner `Option` is the
/// remembered value itself, `None` for SQL NULL.
pub(crate) fn original_value<M: Model>(model: &M, field: &str) -> Result<Option<Option<Value>>> {
    let field_name = known_field::<M>(field)?;
    Ok(read_snapshot(model, |snapshot| {
        snapshot
            .get(field_name)
            .filter(|value| !crate::query::is_sql_null(value))
            .cloned()
    }))
}

/// Whether one field differs from the baseline: `Ok(None)` when no baseline
/// exists, as for [`changed_fields`].
pub(crate) fn is_field_dirty<M: Model>(model: &M, field: &str) -> Result<Option<bool>> {
    let field_name = known_field::<M>(field)?;
    // A shared handle, as in `changed_fields`: the baseline value is not copied.
    let Some(snapshot) = read_snapshot(model, Arc::clone) else {
        return Ok(None);
    };
    Ok(Some(
        model.field_value(field_name).as_ref() != snapshot.get(field_name),
    ))
}

/// The field `field` names, by field or column name.
fn known_field<M: Model>(field: &str) -> Result<&'static str> {
    M::canonical_field_name(field).ok_or_else(|| {
        Error::query(format!(
            "unknown field or column '{}' for model '{}'",
            field,
            M::table_name()
        ))
    })
}

#[cfg(test)]
#[path = "../../tests/unit/model_dirty_tracking_tests.rs"]
mod tests;
