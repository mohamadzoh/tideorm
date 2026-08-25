use std::any::TypeId;
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use parking_lot::RwLock;

use crate::error::{Error, Result};

use super::Model;

type IdentityKey = (TypeId, String);
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
/// `entries` holds the snapshots, `order` maps insertion sequence to key so
/// eviction is O(log n) instead of an O(n) scan. Every mutation keeps the two
/// in sync, so `order` can never accumulate stale keys.
struct SnapshotStore {
    entries: HashMap<IdentityKey, SnapshotEntry>,
    order: BTreeMap<u64, IdentityKey>,
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

    fn get(&self, key: &IdentityKey) -> Option<&SnapshotValues> {
        self.entries.get(key).map(|entry| &entry.values)
    }

    fn insert(&mut self, key: IdentityKey, values: SnapshotValues) {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);

        let entry = SnapshotEntry { sequence, values };
        if let Some(previous) = self.entries.insert(key.clone(), entry) {
            self.order.remove(&previous.sequence);
        }
        self.order.insert(sequence, key);

        while self.entries.len() > self.capacity {
            let Some((_, evicted)) = self.order.pop_first() else {
                break;
            };
            self.entries.remove(&evicted);
        }
    }

    fn remove(&mut self, key: &IdentityKey) {
        if let Some(entry) = self.entries.remove(key) {
            self.order.remove(&entry.sequence);
        }
    }

    fn remove_model_type(&mut self, model_type: TypeId) {
        let mut evicted = Vec::new();
        self.entries.retain(|(type_id, _), entry| {
            if *type_id == model_type {
                evicted.push(entry.sequence);
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

fn snapshot_store() -> &'static RwLock<SnapshotStore> {
    static STORE: OnceLock<RwLock<SnapshotStore>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(SnapshotStore::new(DEFAULT_SNAPSHOT_CAPACITY)))
}

fn snapshot_key_for_primary_key<M: Model>(
    primary_key: &M::PrimaryKey,
) -> Result<Option<IdentityKey>> {
    if M::primary_key_is_new(primary_key) {
        return Ok(None);
    }

    let key = serde_json::to_string(primary_key).map_err(Error::from)?;
    Ok(Some((TypeId::of::<M>(), key)))
}

fn snapshot_key_for_model<M: Model>(model: &M) -> Result<Option<IdentityKey>> {
    snapshot_key_for_primary_key::<M>(&model.primary_key())
}

fn snapshot_values_for_model<M: Model>(model: &M) -> Result<Option<SnapshotValues>> {
    let Some(key) = snapshot_key_for_model(model)? else {
        return Ok(None);
    };

    let store = snapshot_store().read();
    Ok(store.get(&key).cloned())
}

fn resolve_field_name<M: Model>(field: &str) -> Option<&'static str> {
    if let Some(field_name) = M::field_names().iter().copied().find(|name| *name == field) {
        return Some(field_name);
    }

    M::field_names()
        .iter()
        .copied()
        .zip(M::column_names().iter().copied())
        .find_map(|(field_name, column_name)| (column_name == field).then_some(field_name))
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
    let Some(key) = snapshot_key_for_model(model)? else {
        return Ok(());
    };

    let snapshot = capture_snapshot(model)?;
    snapshot_store().write().insert(key, snapshot);
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
    let Some(key) = snapshot_key_for_model(model)? else {
        return Ok(());
    };

    snapshot_store().write().remove(&key);
    Ok(())
}

/// Forget one dirty-tracking baseline by primary key.
pub fn forget_primary_key<M: Model>(primary_key: &M::PrimaryKey) -> Result<()> {
    let Some(key) = snapshot_key_for_primary_key::<M>(primary_key)? else {
        return Ok(());
    };

    snapshot_store().write().remove(&key);
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
/// hand-built model, a model rebuilt from JSON, or one whose baseline was
/// evicted from the bounded store. `Ok(Some(fields))` means a baseline was
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
    let Some(field_name) = resolve_field_name::<M>(field) else {
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
mod tests {
    use super::*;
    // The macro-generated entity module emits `Result<_, DbErr>`, so it must not see
    // tideorm's own one-parameter `Result<T>` alias that `use super::*` brings in here.
    use std::result::Result;

    #[tideorm::model(table = "dirty_tracking_baseline_users")]
    struct BaselineUser {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
    }

    fn baseline_user() -> BaselineUser {
        BaselineUser {
            id: 7,
            name: "Alice".to_string(),
        }
    }

    #[test]
    fn a_missing_baseline_is_distinguishable_from_an_unchanged_model() {
        let model = baseline_user();
        forget_model(&model).expect("forgetting an absent baseline should succeed");

        // No baseline: nothing was compared, so there is no field list at all.
        assert_eq!(
            changed_fields(&model).expect("dirty check should succeed"),
            None
        );
        assert_eq!(
            original_value(&model, "name").expect("original value lookup should succeed"),
            None
        );

        remember_model(&model).expect("remembering a baseline should succeed");

        // Baseline present and matching: an empty list, not a missing one.
        assert_eq!(
            changed_fields(&model).expect("dirty check should succeed"),
            Some(Vec::new())
        );
        assert_eq!(
            original_value(&model, "name").expect("original value lookup should succeed"),
            Some(Some(serde_json::json!("Alice")))
        );

        let mut edited = model.clone();
        edited.name = "Bob".to_string();
        assert_eq!(
            changed_fields(&edited).expect("dirty check should succeed"),
            Some(vec!["name"])
        );

        forget_model(&model).expect("forgetting a baseline should succeed");
        assert_eq!(
            changed_fields(&model).expect("dirty check should succeed"),
            None
        );
    }

    #[test]
    fn an_unsaved_model_reports_no_baseline() {
        let mut model = baseline_user();
        model.id = 0;

        assert_eq!(
            changed_fields(&model).expect("dirty check should succeed"),
            None
        );
        assert_eq!(
            original_value(&model, "name").expect("original value lookup should succeed"),
            None
        );
    }
}
