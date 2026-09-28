use std::any::TypeId;
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use parking_lot::RwLock;

use crate::error::{Error, Result};

use super::Model;

/// A baseline's model type and primary key.
type RowKey = (TypeId, String);
/// The pool a baseline's row was read from: the same key in two databases is
/// two rows, and each keeps its own baseline.
type Origin = Option<u64>;
type SnapshotValues = HashMap<String, serde_json::Value>;

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

    fn remove_model_type(&mut self, model_type: TypeId) {
        let mut evicted = Vec::new();
        self.entries.retain(|(type_id, _), baselines| {
            if *type_id == model_type {
                evicted.extend(baselines.values().map(|entry| entry.sequence));
                false
            } else {
                true
            }
        });

        for sequence in evicted {
            self.order.remove(&sequence);
        }
    }

    /// Drop every baseline. Sequences go on from where they were, so an
    /// undo step still pending cannot mistake a new baseline for its own.
    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
}

thread_local! {
    /// The pool a load in progress reads from, when it is not the scope's.
    static LOADING_ORIGIN: Cell<Option<Option<u64>>> = const { Cell::new(None) };
}

/// Run `load` with the rows it converts remembered as read from `origin`,
/// for a load through a handle other than the scope's own connection.
pub(crate) fn loading_from<T>(origin: Option<u64>, load: impl FnOnce() -> T) -> T {
    /// Puts the previous origin back when the load ends, even by a panic,
    /// which would otherwise leave later loads on the thread under `origin`.
    struct Restore(Option<Option<u64>>);

    impl Drop for Restore {
        fn drop(&mut self) {
            LOADING_ORIGIN.with(|slot| slot.set(self.0));
        }
    }

    let _restore = Restore(LOADING_ORIGIN.with(|slot| slot.replace(Some(origin))));
    load()
}

/// The pool the baselines being remembered or forgotten belong to.
fn origin() -> Origin {
    LOADING_ORIGIN
        .with(Cell::get)
        .unwrap_or_else(crate::database::__scope_origin)
}

fn snapshot_store() -> &'static RwLock<SnapshotStore> {
    static STORE: OnceLock<RwLock<SnapshotStore>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(SnapshotStore::new(DEFAULT_SNAPSHOT_CAPACITY)))
}

fn row_key_for_primary_key<M: Model>(primary_key: &M::PrimaryKey) -> Result<Option<RowKey>> {
    if M::primary_key_is_new(primary_key) {
        return Ok(None);
    }

    let key = serde_json::to_string(primary_key).map_err(Error::from)?;
    Ok(Some((TypeId::of::<M>(), key)))
}

fn row_key_for_model<M: Model>(model: &M) -> Result<Option<RowKey>> {
    row_key_for_primary_key::<M>(&model.primary_key())
}

fn snapshot_values_for_model<M: Model>(model: &M) -> Result<Option<SnapshotValues>> {
    let Some(row) = row_key_for_model(model)? else {
        return Ok(None);
    };

    let store = snapshot_store().read();
    Ok(store.get(&row).cloned())
}

fn capture_snapshot<M: Model>(model: &M) -> Result<SnapshotValues> {
    let mut snapshot = HashMap::with_capacity(M::field_names().len());

    for field in M::field_names() {
        if let Some(value) = model.field_json_value(field)? {
            snapshot.insert((*field).to_string(), value);
        }
    }

    Ok(snapshot)
}

/// Remember one model's current persisted state as the dirty-tracking baseline.
///
/// The baseline store is capacity-bounded; remembering past that point evicts
/// the oldest baselines rather than growing without limit.
pub fn remember_model<M: Model>(model: &M) -> Result<()> {
    let Some(row) = row_key_for_model(model)? else {
        return Ok(());
    };

    let snapshot = capture_snapshot(model)?;
    set_baseline(row, Some(snapshot));
    Ok(())
}

/// Set the current pool's baseline of `row`, or drop it for `None`.
///
/// A row read or written inside a transaction is only what the database
/// holds once it commits, so if it rolls back instead, the baseline this
/// replaced comes back, unless the pool's baseline of the row changed again
/// in the meantime.
fn set_baseline(row: RowKey, values: Option<SnapshotValues>) {
    let origin = origin();
    let (previous, standing) = snapshot_store()
        .write()
        .replace(row.clone(), origin, values);
    crate::cache::undo_on_rollback(move || {
        snapshot_store()
            .write()
            .restore(row, origin, standing, previous);
    });
}

/// Remember a collection of models as dirty-tracking baselines.
///
/// A collection larger than the store's capacity keeps only its trailing
/// models; the earlier ones are evicted as the later ones are remembered.
pub fn remember_collection<M: Model>(models: &[M]) -> Result<()> {
    for model in models {
        remember_model(model)?;
    }

    Ok(())
}

/// Forget one model's dirty-tracking baseline.
pub fn forget_model<M: Model>(model: &M) -> Result<()> {
    let Some(row) = row_key_for_model(model)? else {
        return Ok(());
    };

    set_baseline(row, None);
    Ok(())
}

/// Forget every dirty-tracking baseline for one model type.
pub fn invalidate_model<M: Model>() {
    let model_type = TypeId::of::<M>();
    snapshot_store().write().remove_model_type(model_type);
}

/// Clear every remembered dirty-tracking baseline.
pub fn clear_all() {
    snapshot_store().write().clear();
}

/// Fields whose current value differs from the remembered baseline.
///
/// `Ok(None)` means there is no baseline to compare against — a new or
/// hand-built model, a model rebuilt from JSON, one whose baseline was
/// evicted from the bounded store, or one whose key two databases gave
/// different rows. `Ok(Some(fields))` means a baseline was
/// found, so an empty vector really does mean "nothing changed".
pub(crate) fn changed_fields<M: Model>(model: &M) -> Result<Option<Vec<&'static str>>> {
    let Some(snapshot) = snapshot_values_for_model(model)? else {
        return Ok(None);
    };

    let mut changed = Vec::new();
    for field in M::field_names() {
        let previous = snapshot.get(*field).cloned();
        if model.field_json_value(field)? != previous {
            changed.push(*field);
        }
    }

    Ok(Some(changed))
}

/// One field's remembered value.
///
/// The outer `Option` reports baseline presence exactly like [`changed_fields`]
/// does: `Ok(None)` means no baseline exists. The inner `Option` is the
/// remembered value itself, so `Ok(Some(None))` means the baseline held no
/// value for this field.
pub(crate) fn original_value<M: Model>(
    model: &M,
    field: &str,
) -> Result<Option<Option<serde_json::Value>>> {
    let Some(field_name) = M::canonical_field_name(field) else {
        return Err(Error::query(format!(
            "unknown field or column '{}' for model '{}'",
            field,
            M::table_name()
        )));
    };

    let Some(snapshot) = snapshot_values_for_model(model)? else {
        return Ok(None);
    };

    Ok(Some(snapshot.get(field_name).cloned()))
}

#[cfg(test)]
#[path = "../../tests/unit/model_dirty_tracking_tests.rs"]
mod tests;
