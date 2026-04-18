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

    let encrypted = crate::types::__encrypt_json_value_for_attribute(&json, table_name, column_name)
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

            let decrypted = crate::types::__decrypt_json_value_for_attribute(
                &text,
                table_name,
                column_name,
            )
                .map_err(|error| annotate_crypto_error(error, "decrypt", field_name, column_name))?;
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
    let Some((field_name, column_name)) = resolve_encrypted_field::<M>(field_or_column) else {
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
        UpdateValue::UnsafeRaw(_) => unsupported_batch_operation(
            "trusted raw SQL",
            field_name,
            column_name,
        ),
        UpdateValue::Increment(_) => unsupported_batch_operation(
            "increment",
            field_name,
            column_name,
        ),
        UpdateValue::Decrement(_) => unsupported_batch_operation(
            "decrement",
            field_name,
            column_name,
        ),
        UpdateValue::Multiply(_) => unsupported_batch_operation(
            "multiply",
            field_name,
            column_name,
        ),
        UpdateValue::Divide(_) => unsupported_batch_operation(
            "divide",
            field_name,
            column_name,
        ),
        UpdateValue::ArrayAppend(_) => unsupported_batch_operation(
            "array append",
            field_name,
            column_name,
        ),
        UpdateValue::ArrayRemove(_) => unsupported_batch_operation(
            "array remove",
            field_name,
            column_name,
        ),
        UpdateValue::JsonSet(_, _) => unsupported_batch_operation(
            "json set",
            field_name,
            column_name,
        ),
    }
}

fn resolve_encrypted_field<M: ModelMeta>(name: &str) -> Option<(&'static str, &'static str)> {
    let encrypted_fields = M::encrypted_fields();
    let encrypted_columns = M::encrypted_column_names();

    encrypted_fields
        .into_iter()
        .zip(encrypted_columns)
        .find(|(field_name, column_name)| *field_name == name || *column_name == name)
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
        Error::BackendNotSupported { backend, .. } => Error::backend_not_supported(message, backend),
        Error::PrimaryKeyNotSet { model, .. } => Error::primary_key_not_set(message, model),
        Error::InsertReturningNotSupported { backend, .. } => {
            Error::insert_returning_not_supported(message, backend)
        }
        Error::Internal { .. } => Error::internal(message),
    }
}