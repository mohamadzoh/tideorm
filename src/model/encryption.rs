use serde::{Serialize, de::DeserializeOwned};

use crate::error::{Error, Result};

use super::{ModelMeta, UpdateValue};

pub(crate) fn encrypt_model_field<T>(
    value: T,
    table_name: &str,
    field_name: &str,
    column_name: &str,
) -> Result<T>
where
    T: Serialize + DeserializeOwned,
{
    let json = serde_json::to_value(&value).map_err(|error| {
        Error::conversion(format!(
            "Failed to serialize encrypted field '{}' before write: {}",
            encrypted_field_label(field_name, column_name),
            error
        ))
    })?;

    if json.is_null() {
        return Ok(value);
    }

    let encrypted =
        crate::types::__encrypt_json_value_for_attribute(&json, table_name, column_name)
            .map_err(|error| annotate_crypto_error(error, "encrypt", field_name, column_name))?;
    serde_json::from_value(serde_json::Value::String(encrypted)).map_err(|error| {
        Error::configuration(format!(
            "Encrypted field '{}' must use String/Text storage or Option<String>/Option<Text>: {}",
            encrypted_field_label(field_name, column_name),
            error
        ))
    })
}

pub(crate) fn decrypt_model_field<T>(
    value: T,
    table_name: &str,
    field_name: &str,
    column_name: &str,
) -> Result<T>
where
    T: Serialize + DeserializeOwned,
{
    let json = serde_json::to_value(&value).map_err(|error| {
        Error::conversion(format!(
            "Failed to inspect encrypted field '{}' after read: {}",
            encrypted_field_label(field_name, column_name),
            error
        ))
    })?;

    match json {
        serde_json::Value::Null => Ok(value),
        serde_json::Value::String(text) => {
            if !crate::types::__is_encrypted_json_value(&text) {
                return Err(unencrypted_field_data_error(field_name, column_name));
            }

            let decrypted =
                crate::types::__decrypt_json_value_for_attribute(&text, table_name, column_name)
                    .map_err(|error| {
                        annotate_crypto_error(error, "decrypt", field_name, column_name)
                    })?;
            serde_json::from_value(decrypted).map_err(|error| {
                Error::conversion(format!(
                    "Failed to deserialize decrypted field '{}': {}",
                    encrypted_field_label(field_name, column_name),
                    error
                ))
            })
        }
        _ => Err(Error::configuration(format!(
            "Encrypted field '{}' must load from a string or null database column",
            encrypted_field_label(field_name, column_name)
        ))),
    }
}

fn unencrypted_field_data_error(field_name: &str, column_name: &str) -> Error {
    Error::conversion(format!(
        "Encrypted field '{}' loaded plaintext data; expected a TideORM encrypted payload or null",
        encrypted_field_label(field_name, column_name)
    ))
}

pub(crate) fn prepare_batch_update_value<M: ModelMeta>(
    field_or_column: &str,
    value: UpdateValue,
) -> Result<UpdateValue> {
    let Some((field_name, column_name)) = resolve_encrypted_field::<M>(field_or_column)? else {
        return Ok(value);
    };

    match value {
        UpdateValue::Value(value) => Ok(UpdateValue::Value(encrypt_batch_json_value(
            value,
            M::table_name(),
            field_name,
            column_name,
        )?)),
        UpdateValue::Coalesce(value) => Ok(UpdateValue::Coalesce(encrypt_batch_json_value(
            value,
            M::table_name(),
            field_name,
            column_name,
        )?)),
        UpdateValue::UnsafeRaw(_) => {
            unsupported_batch_operation("trusted raw SQL", field_name, column_name)
        }
        UpdateValue::Increment(_) => {
            unsupported_batch_operation("increment", field_name, column_name)
        }
        UpdateValue::Decrement(_) => {
            unsupported_batch_operation("decrement", field_name, column_name)
        }
        UpdateValue::Multiply(_) => {
            unsupported_batch_operation("multiply", field_name, column_name)
        }
        UpdateValue::Divide(_) => unsupported_batch_operation("divide", field_name, column_name),
        UpdateValue::ArrayAppend(_) => {
            unsupported_batch_operation("array append", field_name, column_name)
        }
        UpdateValue::ArrayRemove(_) => {
            unsupported_batch_operation("array remove", field_name, column_name)
        }
        UpdateValue::JsonSet(_, _) => {
            unsupported_batch_operation("json set", field_name, column_name)
        }
    }
}

/// Pair the encrypted field name with its column name.
///
/// The two `ModelMeta` lists are declared independently, so a mismatched length
/// is possible in hand-written metadata. Zipping them would silently drop the
/// trailing entries, and a dropped entry looks exactly like "this column is not
/// encrypted" — which would write plaintext into an encrypted column. Refuse
/// instead.
fn resolve_encrypted_field<M: ModelMeta>(
    name: &str,
) -> Result<Option<(&'static str, &'static str)>> {
    let encrypted_fields = M::encrypted_fields();
    let encrypted_columns = M::encrypted_column_names();

    if encrypted_fields.len() != encrypted_columns.len() {
        return Err(Error::configuration(format!(
            "Model '{}' declares {} encrypted field name(s) but {} encrypted column name(s); \
             encrypted_fields() and encrypted_column_names() must describe the same columns in \
             the same order, otherwise an encrypted column can be written as plaintext",
            M::table_name(),
            encrypted_fields.len(),
            encrypted_columns.len()
        )));
    }

    Ok(encrypted_fields
        .into_iter()
        .zip(encrypted_columns)
        .find(|(field_name, column_name)| *field_name == name || *column_name == name))
}

fn encrypt_batch_json_value(
    value: serde_json::Value,
    table_name: &str,
    field_name: &str,
    column_name: &str,
) -> Result<serde_json::Value> {
    if value.is_null() {
        return Ok(serde_json::Value::Null);
    }

    crate::types::__encrypt_json_value_for_attribute(&value, table_name, column_name)
        .map(serde_json::Value::String)
        .map_err(|error| annotate_crypto_error(error, "encrypt", field_name, column_name))
}

fn unsupported_batch_operation<T>(
    operation: &str,
    field_name: &str,
    column_name: &str,
) -> Result<T> {
    Err(Error::invalid_query(format!(
        "Batch operation '{}' is not supported for encrypted field '{}'",
        operation,
        encrypted_field_label(field_name, column_name)
    )))
}

fn encrypted_field_label(field_name: &str, column_name: &str) -> String {
    if field_name == column_name {
        field_name.to_string()
    } else {
        format!("{} ({})", field_name, column_name)
    }
}

fn annotate_crypto_error(
    error: Error,
    operation: &str,
    field_name: &str,
    column_name: &str,
) -> Error {
    let message = format!(
        "Failed to {} encrypted field '{}': {}",
        operation,
        encrypted_field_label(field_name, column_name),
        error
    );

    match error {
        Error::Configuration { .. } => Error::configuration(message),
        Error::Conversion { .. } => Error::conversion(message),
        Error::Query { .. } => Error::query(message),
        Error::Tokenization { .. } => Error::tokenization(message),
        Error::InvalidToken { .. } => Error::invalid_token(message),
        Error::Validation { field, .. } => Error::validation(field, message),
        Error::Connection { .. } => Error::connection(message),
        Error::Transaction { .. } => Error::transaction(message),
        Error::NotFound { .. } => Error::query(message),
        Error::BackendNotSupported { backend, .. } => {
            Error::backend_not_supported(message, backend)
        }
        Error::PrimaryKeyNotSet { model, .. } => Error::primary_key_not_set(message, model),
        Error::InsertReturningNotSupported { backend, .. } => {
            Error::insert_returning_not_supported(message, backend)
        }
        // Authorization failures keep their own classification: the caller needs the
        // 403 and the permission/resource pair, not a 500 with a decorated message.
        Error::AccessDenied {
            permission,
            resource,
        } => Error::access_denied(permission, resource),
        Error::Rbac { .. } => Error::rbac(message),
        Error::Internal { .. } => Error::internal(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Metadata whose two encrypted lists disagree. Macro-generated models
    /// cannot produce this, but the trait lets it be written by hand and the
    /// failure mode is silent plaintext, so it has to be rejected.
    #[derive(Clone)]
    struct MismatchedEncryptedMeta;

    impl ModelMeta for MismatchedEncryptedMeta {
        type PrimaryKey = i64;

        fn table_name() -> &'static str {
            "mismatched_encrypted_models"
        }

        fn primary_key_names() -> &'static [&'static str] {
            &["id"]
        }

        fn primary_key_display(primary_key: &Self::PrimaryKey) -> String {
            primary_key.to_string()
        }

        fn column_names() -> &'static [&'static str] {
            &["id", "secret_column", "other_column"]
        }

        fn field_names() -> &'static [&'static str] {
            &["id", "secret", "other"]
        }

        fn encrypted_fields() -> Vec<&'static str> {
            vec!["secret", "other"]
        }

        fn encrypted_column_names() -> Vec<&'static str> {
            vec!["secret_column"]
        }
    }

    #[test]
    fn mismatched_encrypted_metadata_is_rejected_instead_of_truncated() {
        // `other` is the entry `zip` used to drop, which made it look like an
        // unencrypted column and let plaintext through.
        let error = prepare_batch_update_value::<MismatchedEncryptedMeta>(
            "other",
            UpdateValue::Value(serde_json::Value::String("plaintext".to_string())),
        )
        .expect_err("a metadata mismatch must not fall through to plaintext");

        assert!(
            error.to_string().contains("encrypted column name"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn mismatched_encrypted_metadata_is_rejected_for_unrelated_columns_too() {
        let error = prepare_batch_update_value::<MismatchedEncryptedMeta>(
            "id",
            UpdateValue::Value(serde_json::Value::from(1)),
        )
        .expect_err("a metadata mismatch must be reported, not skipped");

        assert!(
            error.to_string().contains("encrypted field name"),
            "unexpected error: {error}"
        );
    }
}
