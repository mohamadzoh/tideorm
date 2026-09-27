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
    values: SnapshotValues,
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
    /// databases gave the key different rows, no baseline is known for it:
    /// the scope a model is inspected in need not be the one it was loaded
    /// through, and guessing would report another row's values.
    fn get(&self, row: &RowKey) -> Option<&SnapshotValues> {
        let mut baselines = self.entries.get(row)?.values().map(|entry| &entry.values);
        let first = baselines.next()?;
        baselines.all(|other| other == first).then_some(first)
    }

    fn insert(&mut self, row: RowKey, origin: Origin, values: SnapshotValues) {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);

        let entry = SnapshotEntry { sequence, values };
        let baselines = self.entries.entry(row.clone()).or_default();
        if let Some(previous) = baselines.insert(origin, entry) {
            self.order.remove(&previous.sequence);
        }
        self.order.insert(sequence, (row, origin));

        while self.order.len() > self.capacity {
            let Some((_, (row, origin))) = self.order.pop_first() else {
                break;
            };
            self.take(&row, origin);
        }
    }

    /// Remove one pool's baseline of `row` from `entries` alone.
    fn take(&mut self, row: &RowKey, origin: Origin) -> Option<SnapshotEntry> {
        let baselines = self.entries.get_mut(row)?;
        let entry = baselines.remove(&origin);
        if baselines.is_empty() {
            self.entries.remove(row);
        }
        entry
    }

    fn remove(&mut self, row: &RowKey, origin: Origin) {
        if let Some(entry) = self.take(row, origin) {
            self.order.remove(&entry.sequence);
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

    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.next_sequence = 0;
    }
}

thread_local! {
    /// The pool a load in progress reads from, when it is not the scope's.
    static LOADING_ORIGIN: Cell<Option<Option<u64>>> = const { Cell::new(None) };
}

/// Run `load` with the rows it converts remembered as read from `origin`,
/// for a load through a handle other than the scope's own connection.
pub(crate) fn loading_from<T>(origin: Option<u64>, load: impl FnOnce() -> T) -> T {
    let previous = LOADING_ORIGIN.with(|slot| slot.replace(Some(origin)));
    let result = load();
    LOADING_ORIGIN.with(|slot| slot.set(previous));
    result
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
    snapshot_store().write().insert(row, origin(), snapshot);
    Ok(())
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

    snapshot_store().write().remove(&row, origin());
    Ok(())
}

/// Forget one dirty-tracking baseline by primary key.
pub fn forget_primary_key<M: Model>(primary_key: &M::PrimaryKey) -> Result<()> {
    let Some(row) = row_key_for_primary_key::<M>(primary_key)? else {
        return Ok(());
    };

    snapshot_store().write().remove(&row, origin());
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
        return Err(Error::invalid_query(format!(
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
