//! The [`Model`] trait and its supporting types: metadata ([`ModelMeta`]),
//! bulk updates ([`BatchUpdateBuilder`]), upserts ([`OnConflictBuilder`]) and
//! nested saves ([`NestedSave`]).
//!
//! `find`, `create`, `update`, `save` and `delete` are emitted per model by the
//! derive. The trait's default methods delegate to `internal::QueryExecutor`
//! for table-wide reads, `nested` for relation-aware saves, and
//! `serialization` for `to_json`.

mod api;
mod batch;
mod builders;
mod crud;
pub(crate) use crud::reload_by_key;
#[doc(hidden)]
pub use crud::{__delete_row, __insert_row, __update_row};
#[cfg(feature = "dirty-tracking")]
mod dirty_tracking;
#[cfg(feature = "encrypted-fields")]
mod encryption;
mod meta;
mod nested;
mod serialization;
mod upsert;

pub use api::Model;
pub use batch::{BatchUpdateBuilder, UpdateValue};
pub use builders::OnConflictBuilder;
pub use meta::{IndexDefinition, ModelMeta, RelationPayloadFilter};
pub use nested::{NestedSave, NestedSaveBuilder, SavedRelation};
#[doc(hidden)]
pub use upsert::__upsert;

/// A type a model field's column can hold; the derive names one per field.
#[doc(hidden)]
pub struct __ColumnType<T: crate::orm::sql_builder::ValueType>(std::marker::PhantomData<T>);

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

/// An encrypted field's column value for a write: the payload sealing its
/// JSON, or NULL for a `null` document.
#[doc(hidden)]
pub fn __encrypt_model_field<M: ModelMeta, T: serde::Serialize>(
    value: &T,
    field_name: &str,
    column_name: &str,
) -> crate::Result<Option<String>> {
    #[cfg(feature = "encrypted-fields")]
    {
        encryption::encrypt_model_field(value, M::table_name(), field_name, column_name)
    }

    #[cfg(not(feature = "encrypted-fields"))]
    {
        let _ = (value, field_name, column_name);
        Err(encryption_unavailable())
    }
}

/// An encrypted field decrypted from its column's text as the row decodes, so
/// a decoded model never holds ciphertext. NULL reads as a nullable column's
/// does; a failure is the `DbErr::Decode` of the column, carrying the
/// TideORM error, whose class `translate_error` keeps.
#[doc(hidden)]
pub fn __decrypt_model_field<M: ModelMeta, T: serde::de::DeserializeOwned>(
    stored: Option<String>,
    field_name: &str,
    column_name: &str,
) -> std::result::Result<T, crate::orm::DbErr> {
    #[cfg(feature = "encrypted-fields")]
    let value = encryption::decrypt_model_field(stored, M::table_name(), field_name, column_name);
    #[cfg(not(feature = "encrypted-fields"))]
    let value: crate::Result<Option<T>> = {
        let _ = (stored, field_name);
        Err(encryption_unavailable())
    };
    match value {
        Ok(Some(value)) => Ok(value),
        Ok(None) => Err(crate::orm::DbErr::Type(format!(
            "Missing value for column '{column_name}'"
        ))),
        Err(error) => Err(crate::orm::DbErr::Decode {
            column: column_name.to_owned(),
            source: std::sync::Arc::new(error),
        }),
    }
}

/// An encrypted field's plaintext as `ModelTrait::get` reads it, which
/// comparisons and dirty-tracking baselines hold: text as a string, `null` as
/// NULL, any other document as JSON.
#[doc(hidden)]
pub fn __encrypted_field_value<T: serde::Serialize>(value: &T) -> crate::orm::Value {
    use crate::orm::Value;

    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => Value::String(Some(text)),
        Ok(serde_json::Value::Null) => Value::String(None),
        Ok(document) => Value::Json(Some(Box::new(document))),
        // The field's write reports why it does not serialize.
        Err(_) => Value::Json(None),
    }
}

/// `value` as an encrypted field's plaintext, for `ModelTrait::try_set`.
#[doc(hidden)]
pub fn __encrypted_field_from_value<T: serde::de::DeserializeOwned>(
    value: crate::orm::Value,
) -> std::result::Result<T, crate::orm::DbErr> {
    serde_json::from_value(crate::internal::value_document(&value))
        .map_err(|error| crate::orm::DbErr::Type(error.to_string()))
}

/// An encrypted column a projection reads: its model's table, which with the
/// column scopes the key, and the field it decrypts into. Public only because
/// the macro registers [`__encrypted_column`] for every model.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct EncryptedColumn {
    pub(crate) table: &'static str,
    pub(crate) field: &'static str,
    pub(crate) column: &'static str,
}

impl EncryptedColumn {
    /// The error refusing `operation` over the column, whose randomized
    /// ciphertext matches no plaintext, `reason` saying what that breaks.
    pub(crate) fn refuse(&self, operation: &str, reason: &str) -> crate::Error {
        crate::Error::query(format!(
            "{operation} is not supported for encrypted column {}.{}: {reason}",
            self.table, self.column
        ))
    }

    /// `M`'s encrypted columns.
    pub(crate) fn of<M: ModelMeta>() -> impl Iterator<Item = Self> {
        M::encrypted_fields()
            .iter()
            .map(|&(field, column)| EncryptedColumn {
                table: M::table_name(),
                field,
                column,
            })
    }
}

/// `M`'s encrypted column named `column` (by column or field name), if any.
#[doc(hidden)]
pub fn __encrypted_column<M: ModelMeta>(column: &str) -> Option<EncryptedColumn> {
    let column = M::column_named(column);
    EncryptedColumn::of::<M>().find(|encrypted| encrypted.column == column)
}

/// Decrypt explicitly resolved projections, plus implicitly selected model fields.
/// An explicit plain output with an encrypted field's name must remain plain.
pub(crate) fn decrypt_json_rows<M: ModelMeta>(
    rows: &mut [serde_json::Value],
    outputs: &[(String, Option<EncryptedColumn>)],
) -> crate::error::Result<()> {
    let mut encrypted_outputs: Vec<_> = outputs
        .iter()
        .filter_map(|(name, source)| source.map(|source| (name.clone(), source)))
        .collect();
    for encrypted in EncryptedColumn::of::<M>() {
        if !outputs.iter().any(|(name, _)| name == encrypted.column) {
            encrypted_outputs.push((encrypted.column.to_string(), encrypted));
        }
    }
    for row in rows {
        let Some(members) = row.as_object_mut() else {
            continue;
        };
        for (name, source) in &encrypted_outputs {
            if let Some(value) = members.get_mut(name.as_str()) {
                *value =
                    decrypt_json_value(value.take(), source.table, source.field, source.column)?;
            }
        }
    }
    Ok(())
}

/// One JSON row's encrypted value decrypted into the document it seals.
fn decrypt_json_value(
    value: serde_json::Value,
    table_name: &str,
    field_name: &str,
    column_name: &str,
) -> crate::Result<serde_json::Value> {
    #[cfg(feature = "encrypted-fields")]
    {
        encryption::decrypt_json_value(value, table_name, field_name, column_name)
    }

    #[cfg(not(feature = "encrypted-fields"))]
    {
        let _ = (value, table_name, field_name, column_name);
        Err(encryption_unavailable())
    }
}

pub(crate) fn prepare_batch_update_value<M: ModelMeta>(
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

/// The value an auto-increment key is inserted with: unset while it is still
/// the unsaved default, for the database to number, and otherwise the key
/// the model holds.
#[doc(hidden)]
pub fn __auto_increment_key<V>(new: bool, key: V) -> crate::orm::ActiveValue<V>
where
    V: Into<crate::orm::Value>,
{
    if new {
        crate::orm::ActiveValue::NotSet
    } else {
        crate::orm::ActiveValue::Set(key)
    }
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
#[cfg(feature = "dirty-tracking")]
pub(crate) fn clear_dirty_snapshots() {
    dirty_tracking::clear_all();
}

/// Whether a row with this primary key exists, trashed or not: how `save()`
/// chooses between an update and an insert.
pub(crate) async fn exists_including_trashed<M: Model>(
    primary_key: &M::PrimaryKey,
) -> crate::Result<bool> {
    M::query()
        .with_trashed()
        .where_primary_key(primary_key)?
        .exists()
        .await
}

/// Mark the row with this primary key deleted, as `Model::delete` does for a
/// soft-delete model; a trashed row is left alone.
#[doc(hidden)]
pub async fn __soft_delete_by_primary_key<M: Model>(
    primary_key: &M::PrimaryKey,
) -> crate::error::Result<u64> {
    M::query()
        .where_primary_key(primary_key)?
        .soft_delete()
        .await
}

#[doc(hidden)]
#[cfg_attr(not(feature = "dirty-tracking"), allow(unused_variables))]
pub fn __forget_dirty_snapshot<M: Model>(model: &M) {
    #[cfg(feature = "dirty-tracking")]
    dirty_tracking::forget_model(model);
}

pub(crate) fn invalidate_dirty_snapshots<M: Model>() {
    #[cfg(feature = "dirty-tracking")]
    dirty_tracking::invalidate_model::<M>();
}

/// The last step of every load: `model`, its relations built, was read
/// through `connection`, which for `find_with(id, db)` need not be the
/// scope's. Its relations query the pool the row came from, never the
/// transaction it was read in, which they would keep from committing, and
/// its dirty-tracking baseline is that pool's, recorded in that transaction
/// so it goes back if the transaction rolls back.
#[doc(hidden)]
pub fn __finish_load<M: Model>(model: &mut M, connection: &crate::database::ConnectionRef) {
    model.__attach_query_database(&crate::database::Database::from_handle(
        crate::database::ConnectionRef::Database(connection.pool().clone()),
    ));
    #[cfg(feature = "dirty-tracking")]
    connection
        .recording(|| dirty_tracking::remember_model_from(model, Some(connection.identity())));
}

#[cfg(test)]
#[path = "../../tests/unit/model_tests.rs"]
mod tests;
