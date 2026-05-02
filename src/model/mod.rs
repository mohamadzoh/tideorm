//! Model APIs and shared model-side helpers.
//!
//! This module is the entry point for CRUD helpers, batch updates, nested save
//! behavior, and model metadata.
//!
//! If model behavior looks wrong, start in the submodule that matches the
//! failing operation:
//! - `crud` for normal create, update, delete, and lookup paths
//! - `batch` for bulk update behavior
//! - `nested` for relation-aware saves
//! - `serialization` when attributes are not loading or persisting as expected

#[allow(missing_docs)]
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
pub use builders::{CreateBuilder, OnConflictBuilder, UpdateBuilder};
pub use meta::{IndexDefinition, ModelMeta};
pub use nested::{NestedSave, NestedSaveBuilder, SavedRelation};

#[doc(hidden)]
#[cfg(feature = "encrypted-fields")]
pub const fn __assert_encrypted_fields_feature_enabled() {}

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
        Err(crate::Error::configuration(
            "Model auto-encryption requires the `encrypted-fields` feature. Enable it with `tideorm = { features = [\"encrypted-fields\"] }` before using #[tideorm(encrypted = ...)].",
        ))
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
        Err(crate::Error::configuration(
            "Model auto-encryption requires the `encrypted-fields` feature. Enable it with `tideorm = { features = [\"encrypted-fields\"] }` before using #[tideorm(encrypted = ...)].",
        ))
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
            return Err(crate::Error::configuration(
                "Model auto-encryption requires the `encrypted-fields` feature. Enable it with `tideorm = { features = [\"encrypted-fields\"] }` before using #[tideorm(encrypted = ...)].",
            ));
        }

        let _ = field_or_column;
        Ok(value)
    }
}

// These wrappers stay available in all builds because macro-generated code may
// expand into downstream crates, where TideORM dependency features are not
// directly visible through `cfg(feature = ...)` checks.
#[doc(hidden)]
#[inline]
pub fn __dirty_tracking_enabled() -> bool {
    cfg!(feature = "dirty-tracking")
}

#[doc(hidden)]
pub fn __clear_dirty_snapshots() {
    #[cfg(feature = "dirty-tracking")]
    dirty_tracking::clear_all();
}

#[doc(hidden)]
pub fn __forget_dirty_snapshot<M: Model>(model: &M) -> crate::error::Result<()> {
    #[cfg(feature = "dirty-tracking")]
    {
        dirty_tracking::forget_model(model)
    }

    #[cfg(not(feature = "dirty-tracking"))]
    {
        let _ = model;
        Ok(())
    }
}

#[doc(hidden)]
pub fn __forget_dirty_snapshot_by_pk<M: Model>(
    primary_key: &M::PrimaryKey,
) -> crate::error::Result<()> {
    #[cfg(feature = "dirty-tracking")]
    {
        dirty_tracking::forget_primary_key::<M>(primary_key)
    }

    #[cfg(not(feature = "dirty-tracking"))]
    {
        let _ = primary_key;
        Ok(())
    }
}

#[doc(hidden)]
pub fn __invalidate_dirty_snapshots<M: Model>() {
    #[cfg(feature = "dirty-tracking")]
    dirty_tracking::invalidate_model::<M>();
}

#[doc(hidden)]
pub fn __remember_dirty_snapshots<M: Model>(models: &[M]) -> crate::error::Result<()> {
    #[cfg(feature = "dirty-tracking")]
    {
        dirty_tracking::remember_collection(models)
    }

    #[cfg(not(feature = "dirty-tracking"))]
    {
        let _ = models;
        Ok(())
    }
}

#[doc(hidden)]
pub fn __remember_dirty_snapshot<M: Model>(model: &M) -> crate::error::Result<()> {
    #[cfg(feature = "dirty-tracking")]
    {
        dirty_tracking::remember_model(model)
    }

    #[cfg(not(feature = "dirty-tracking"))]
    {
        let _ = model;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/model_tests.rs"]
mod tests;
