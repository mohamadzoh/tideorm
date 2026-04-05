use std::any::TypeId;
use std::collections::HashMap;
use std::sync::OnceLock;

use parking_lot::RwLock;

use crate::error::{Error, Result};

use super::Model;

type IdentityKey = (TypeId, String);
type SnapshotValues = HashMap<String, serde_json::Value>;
type SnapshotStore = HashMap<IdentityKey, SnapshotValues>;

fn snapshot_store() -> &'static RwLock<SnapshotStore> {
    static STORE: OnceLock<RwLock<SnapshotStore>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn snapshot_key_for_primary_key<M: Model>(primary_key: &M::PrimaryKey) -> Result<Option<IdentityKey>> {
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
pub fn remember_model<M: Model>(model: &M) -> Result<()> {
    let Some(key) = snapshot_key_for_model(model)? else {
        return Ok(());
    };

    let snapshot = capture_snapshot(model)?;
    snapshot_store().write().insert(key, snapshot);
    Ok(())
}

/// Remember a collection of models as dirty-tracking baselines.
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
    snapshot_store()
        .write()
        .retain(|(type_id, _), _| *type_id != model_type);
}

/// Clear every remembered dirty-tracking baseline.
pub fn clear_all() {
    snapshot_store().write().clear();
}

pub(crate) fn changed_fields<M: Model>(model: &M) -> Result<Vec<&'static str>> {
    let Some(snapshot) = snapshot_values_for_model(model)? else {
        return Ok(Vec::new());
    };

    let mut changed = Vec::new();
    for field in M::field_names() {
        let previous = snapshot.get(*field).cloned();
        if model.field_json_value(field)? != previous {
            changed.push(*field);
        }
    }

    Ok(changed)
}

pub(crate) fn original_value<M: Model>(model: &M, field: &str) -> Result<Option<serde_json::Value>> {
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

    Ok(snapshot.get(field_name).cloned())
}