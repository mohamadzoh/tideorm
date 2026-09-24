//! The [`Model`] trait and its supporting types: metadata ([`ModelMeta`]),
//! bulk updates ([`BatchUpdateBuilder`]), upserts ([`OnConflictBuilder`]) and
//! nested saves ([`NestedSave`]).
//!
//! `find`, `create`, `update`, `save` and `delete` are emitted per model by the
//! derive. The trait's default methods delegate to `crud` for table-wide reads,
//! `nested` for relation-aware saves, and `serialization` for `to_json` and the
//! attachment and translation attribute helpers.

mod api;
mod batch;
mod builders;
mod crud;
#[cfg(feature = "dirty-tracking")]
mod dirty_tracking;
#[cfg(feature = "encrypted-fields")]
mod encryption;
mod meta;
mod nested;
mod serialization;

pub use api::Model;
pub use batch::{BatchUpdateBuilder, UpdateValue};
pub use builders::OnConflictBuilder;
pub use meta::{IndexDefinition, ModelMeta, RelationPayloadFilter};
pub use nested::{NestedSave, NestedSaveBuilder, SavedRelation};

#[doc(hidden)]
#[cfg(feature = "encrypted-fields")]
pub const fn __assert_encrypted_fields_feature_enabled() {}

// Deliberately not named like `__assert_encrypted_fields_feature_enabled`:
// rustc would suggest it in the "enable the feature" compile error that
// `tests/ui/invalid_encrypted_fields_without_feature.stderr` pins.
#[cfg(not(feature = "encrypted-fields"))]
fn encryption_unavailable() -> crate::Error {
    crate::Error::configuration(
        "Model auto-encryption requires the `encrypted-fields` feature. Enable it with `tideorm = { features = [\"encrypted-fields\"] }` before using #[tideorm(encrypted = ...)].",
    )
}

#[doc(hidden)]
pub fn __encrypt_model_field<T>(
    value: T,
    table_name: &str,
    field_name: &str,
    column_name: &str,
) -> crate::error::Result<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    #[cfg(feature = "encrypted-fields")]
    {
        encryption::encrypt_model_field(value, table_name, field_name, column_name)
    }

    #[cfg(not(feature = "encrypted-fields"))]
    {
        let _ = (value, table_name, field_name, column_name);
        Err(encryption_unavailable())
    }
}

#[doc(hidden)]
pub fn __decrypt_model_field<T>(
    value: T,
    table_name: &str,
    field_name: &str,
    column_name: &str,
) -> crate::error::Result<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    #[cfg(feature = "encrypted-fields")]
    {
        encryption::decrypt_model_field(value, table_name, field_name, column_name)
    }

    #[cfg(not(feature = "encrypted-fields"))]
    {
        let _ = (value, table_name, field_name, column_name);
        Err(encryption_unavailable())
    }
}

#[doc(hidden)]
pub fn __prepare_batch_update_value<M: ModelMeta>(
    field_or_column: &str,
    value: UpdateValue,
) -> crate::error::Result<UpdateValue> {
    #[cfg(feature = "encrypted-fields")]
    {
        encryption::prepare_batch_update_value::<M>(field_or_column, value)
    }

    #[cfg(not(feature = "encrypted-fields"))]
    {
        if M::has_encrypted_fields() {
            return Err(encryption_unavailable());
        }

        let _ = field_or_column;
        Ok(value)
    }
}

/// `left == right`, usable in the `const` column assertions generated for
/// relation keys.
#[doc(hidden)]
pub const fn __str_eq(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }

    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }

    true
}

/// Whether a primary-key component still holds its type's default, i.e. the row
/// has not been keyed yet.
#[doc(hidden)]
pub fn __is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// The key a `Uuid` primary key is inserted with: `key` itself, or a random
/// (v4) one when it is still nil, which no row should be keyed by.
#[doc(hidden)]
pub fn __uuid_key(key: uuid::Uuid) -> uuid::Uuid {
    if key.is_nil() {
        uuid::Uuid::new_v4()
    } else {
        key
    }
}

// These wrappers stay available in all builds because macro-generated code may
// expand into downstream crates, where TideORM dependency features are not
// directly visible through `cfg(feature = ...)` checks.
//
// Dirty tracking is best-effort bookkeeping beside a statement that already
// succeeded, so a baseline that cannot be recorded or dropped — its primary key
// failed to serialize — is logged rather than failing the operation.
#[cfg(feature = "dirty-tracking")]
fn warn_on_snapshot_error(action: &str, result: crate::error::Result<()>) {
    if let Err(error) = result {
        crate::tide_warn!("dirty tracking could not {} a baseline: {}", action, error);
    }
}

#[doc(hidden)]
pub fn __clear_dirty_snapshots() {
    #[cfg(feature = "dirty-tracking")]
    dirty_tracking::clear_all();
}

#[doc(hidden)]
#[cfg_attr(not(feature = "dirty-tracking"), allow(unused_variables))]
pub fn __forget_dirty_snapshot<M: Model>(model: &M) {
    #[cfg(feature = "dirty-tracking")]
    warn_on_snapshot_error("forget", dirty_tracking::forget_model(model));
}

#[doc(hidden)]
#[cfg_attr(not(feature = "dirty-tracking"), allow(unused_variables))]
pub fn __forget_dirty_snapshot_by_pk<M: Model>(primary_key: &M::PrimaryKey) {
    #[cfg(feature = "dirty-tracking")]
    warn_on_snapshot_error(
        "forget",
        dirty_tracking::forget_primary_key::<M>(primary_key),
    );
}

#[doc(hidden)]
pub fn __invalidate_dirty_snapshots<M: Model>() {
    #[cfg(feature = "dirty-tracking")]
    dirty_tracking::invalidate_model::<M>();
}

#[doc(hidden)]
#[cfg_attr(not(feature = "dirty-tracking"), allow(unused_variables))]
pub fn __remember_dirty_snapshots<M: Model>(models: &[M]) {
    #[cfg(feature = "dirty-tracking")]
    warn_on_snapshot_error("record", dirty_tracking::remember_collection(models));
}

#[doc(hidden)]
#[cfg_attr(not(feature = "dirty-tracking"), allow(unused_variables))]
pub fn __remember_dirty_snapshot<M: Model>(model: &M) {
    #[cfg(feature = "dirty-tracking")]
    warn_on_snapshot_error("record", dirty_tracking::remember_model(model));
}

#[cfg(test)]
#[path = "../../tests/unit/model_tests.rs"]
mod tests;
