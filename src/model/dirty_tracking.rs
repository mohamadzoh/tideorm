use std::any::TypeId;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;

use crate::error::{Error, Result};
use crate::internal::Value;
use crate::orm::sql_builder::ValueKey;

use super::Model;

/// A baseline's model type and primary key.
type RowKey = (TypeId, Vec<ValueKey>);
/// The pool a baseline's row was read from: the same key in two databases is
/// two rows, and each keeps its own baseline.
type Origin = Option<u64>;
/// Each field's value, by field name, shared so a reader copies no values.
type SnapshotValues = Arc<HashMap<&'static str, Value>>;

/// Upper bound on how many dirty-tracking baselines are kept in memory.
///
/// The baseline store is process-global, so leaving it unbounded means a
/// read-heavy service paging through a large table grows until it runs out of
/// memory. Once the bound is reached, the oldest remembered baselines are
/// evicted first; a model whose baseline was evicted stops reporting dirty
/// state instead of returning stale data.
const DEFAULT_SNAPSHOT_CAPACITY: usize = 10_000;

struct SnapshotEntry {
    sequence: u64,
    /// `None` when the pool's baseline was forgotten while another pool
    /// still held a different one: a model does not say which pool it came
    /// from, so the other baseline must not become the key's.
    values: Option<SnapshotValues>,
}

/// Bounded, insertion-ordered store of dirty-tracking baselines.
///
/// `entries` holds each row's snapshots by the pool they were read from, and
/// `order` maps insertion sequence to row and pool, one per snapshot, so
/// eviction is O(log n) instead of an O(n) scan. Every mutation keeps the two
/// in sync, so `order` can never accumulate stale keys.
struct SnapshotStore {
    entries: HashMap<RowKey, HashMap<Origin, SnapshotEntry>>,
    order: BTreeMap<u64, (RowKey, Origin)>,
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

    /// The baseline of `row`, when it is the same in every pool the row was
    /// read from.
    ///
    /// A model does not record which database it came from, so where two
    /// databases gave the key different rows, or one pool's baseline of it
    /// was forgotten, no baseline is known for it: the scope a model is
    /// inspected in need not be the one it was loaded through, and guessing
    /// would report another row's values.
    fn get(&self, row: &RowKey) -> Option<&SnapshotValues> {
        let mut baselines = self
            .entries
            .get(row)?
            .values()
            .map(|entry| entry.values.as_ref());
        let first = baselines.next()??;
        baselines.all(|other| other == Some(first)).then_some(first)
    }

    /// Store one pool's baseline of `row` and return its sequence.
    fn insert(&mut self, row: RowKey, origin: Origin, values: Option<SnapshotValues>) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.put(row, origin, SnapshotEntry { sequence, values });
        sequence
    }

    fn put(&mut self, row: RowKey, origin: Origin, entry: SnapshotEntry) {
        self.order.insert(entry.sequence, (row.clone(), origin));
        if let Some(previous) = self.entries.entry(row).or_default().insert(origin, entry) {
            self.order.remove(&previous.sequence);
        }

        // A row leaves with every pool's baseline of it, so no pool's
        // baseline outlives another's and becomes the key's.
        while self.order.len() > self.capacity {
            let Some((_, (row, _))) = self.order.pop_first() else {
                break;
            };
            for entry in self
                .entries
                .remove(&row)
                .into_iter()
                .flat_map(HashMap::into_values)
            {
                self.order.remove(&entry.sequence);
            }
        }
    }

    /// Remove one pool's baseline of `row`.
    fn take(&mut self, row: &RowKey, origin: Origin) -> Option<SnapshotEntry> {
        let baselines = self.entries.get_mut(row)?;
        let entry = baselines.remove(&origin)?;
        if baselines.is_empty() {
            self.entries.remove(row);
        }
        self.order.remove(&entry.sequence);
        Some(entry)
    }

    /// Set one pool's baseline of `row` to `values`, or forget it for `None`.
    /// Returns the entry it replaces, and the sequence of the entry left in
    /// its place, if any, for [`restore`](Self::restore).
    fn replace(
        &mut self,
        row: RowKey,
        origin: Origin,
        values: Option<SnapshotValues>,
    ) -> (Option<SnapshotEntry>, Option<u64>) {
        let previous = self.take(&row, origin);
        let values = match values {
            Some(values) => Some(values),
            // Forgotten while another pool still holds a different baseline
            // of the key: that baseline must not become the key's.
            None if previous.as_ref().is_some_and(|previous| {
                self.entries.get(&row).is_some_and(|others| {
                    others
                        .values()
                        .any(|other| previous.values.is_none() || other.values != previous.values)
                })
            }) =>
            {
                None
            }
            None => return (previous, None),
        };
        let sequence = self.insert(row, origin, values);
        (previous, Some(sequence))
    }

    /// Undo a [`replace`](Self::replace) that left `standing` behind, by
    /// putting `previous` back, unless the pool's baseline of `row` changed
    /// again since: a transaction that rolls back undoes its own reads and
    /// writes, not a later one's.
    fn restore(
        &mut self,
        row: RowKey,
        origin: Origin,
        standing: Option<u64>,
        previous: Option<SnapshotEntry>,
    ) {
        let current = self
            .entries
            .get(&row)
            .and_then(|baselines| baselines.get(&origin))
            .map(|entry| entry.sequence);
        if current != standing {
            return;
        }
        self.take(&row, origin);
        if let Some(previous) = previous {
            self.put(row, origin, previous);
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
        .map(|column| ValueKey::new(crate::orm::ModelTrait::get(model, column)))
        .collect();
    Some((TypeId::of::<M>(), key))
}

/// What `read` takes from `model`'s baseline, read under the store's lock;
/// `None` when no baseline is known.
fn read_snapshot<M: Model, T>(model: &M, read: impl FnOnce(&SnapshotValues) -> T) -> Option<T> {
    let row = row_key_for_model(model)?;
    let store = snapshot_store().read();
    store.get(&row).map(read)
}

fn capture_snapshot<M: Model>(model: &M) -> SnapshotValues {
    Arc::new(
        M::field_names()
            .iter()
            .filter_map(|field| Some((*field, model.field_value(field)?)))
            .collect(),
    )
}

/// Remember `model`'s current persisted state as the dirty-tracking baseline
/// of the pool `origin` it was read from.
///
/// The baseline store is capacity-bounded; remembering past that point evicts
/// the oldest baselines rather than growing without limit.
pub(crate) fn remember_model_from<M: Model>(model: &M, origin: Origin) {
    if let Some(row) = row_key_for_model(model) {
        set_baseline(row, origin, Some(capture_snapshot(model)));
    }
}

/// Set the pool `origin`'s baseline of `row`, or drop it for `None`.
///
/// A row read or written inside a transaction is only what the database
/// holds once it commits, so if it rolls back instead, the baseline this
/// replaced comes back, unless the pool's baseline of the row changed again
/// in the meantime.
fn set_baseline(row: RowKey, origin: Origin, values: Option<SnapshotValues>) {
    let (previous, standing) = snapshot_store()
        .write()
        .replace(row.clone(), origin, values);
    crate::cache::undo_on_rollback(move || {
        snapshot_store()
            .write()
            .restore(row, origin, standing, previous);
    });
}

/// Forget one model's dirty-tracking baseline.
pub fn forget_model<M: Model>(model: &M) {
    forget_model_from(model, crate::database::scope_origin())
}

/// Forget `model`'s baseline as read from the pool `origin`.
pub(crate) fn forget_model_from<M: Model>(model: &M, origin: Origin) {
    if let Some(row) = row_key_for_model(model) {
        set_baseline(row, origin, None);
    }
}

/// Forget every dirty-tracking baseline for one model type.
pub fn invalidate_model<M: Model>() {
    let model_type = TypeId::of::<M>();
    let mut store = snapshot_store().write();
    let keys: Vec<_> = store
        .entries
        .iter()
        .filter(|((type_id, _), _)| *type_id == model_type)
        .flat_map(|(row, baselines)| baselines.keys().map(move |origin| (row.clone(), *origin)))
        .collect();
    let mut undo = Vec::with_capacity(keys.len());
    for (row, origin) in keys {
        let previous = store.take(&row, origin);
        let standing = store.insert(row.clone(), origin, None);
        undo.push((row, origin, standing, previous));
    }
    drop(store);
    let mut undo = InvalidatedBaselines(undo);
    crate::cache::undo_on_rollback(move || {
        let mut store = snapshot_store().write();
        for (row, origin, standing, previous) in undo.0.drain(..) {
            store.restore(row, origin, Some(standing), previous);
        }
    });
}

/// A committed invalidation forgets its tombstones. Until then they retain
/// mutation identities so rollback cannot overwrite a newer baseline.
struct InvalidatedBaselines(Vec<(RowKey, Origin, u64, Option<SnapshotEntry>)>);

impl Drop for InvalidatedBaselines {
    fn drop(&mut self) {
        if self.0.is_empty() {
            return;
        }
        let mut store = snapshot_store().write();
        for (row, origin, standing, _) in self.0.drain(..) {
            let current = store
                .entries
                .get(&row)
                .and_then(|baselines| baselines.get(&origin));
            if current.is_some_and(|entry| entry.sequence == standing) {
                store.take(&row, origin);
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
        M::field_names()
            .iter()
            .filter(|field| model.field_value(field).as_ref() != snapshot.get(**field))
            .copied()
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
