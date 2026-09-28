use serde::{Serialize, de::DeserializeOwned};

use crate::error::{Error, Result};
use crate::types::encrypted;

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

    let encrypted = encrypted::encrypt_json_value_for_attribute(&json, table_name, column_name)
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
            if !encrypted::is_encrypted_json_value(&text) {
                return Err(Error::conversion(format!(
                    "Encrypted field '{}' loaded plaintext data; expected a TideORM encrypted payload or null",
                    encrypted_field_label(field_name, column_name)
                )));
            }

            let decrypted =
                encrypted::decrypt_json_value_for_attribute(&text, table_name, column_name)
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

pub(crate) fn prepare_batch_update_value<M: ModelMeta>(
    field_or_column: &str,
    value: UpdateValue,
) -> Result<UpdateValue> {
    let Some((field_name, column_name)) = resolve_encrypted_field::<M>(field_or_column)? else {
        return Ok(value);
    };

    let encrypt = |value| encrypt_batch_json_value(value, M::table_name(), field_name, column_name);
    let operation = match value {
        UpdateValue::Value(value) => return Ok(UpdateValue::Value(encrypt(value)?)),
        UpdateValue::Coalesce(value) => return Ok(UpdateValue::Coalesce(encrypt(value)?)),
        UpdateValue::UnsafeRaw(_) => "trusted raw SQL",
        UpdateValue::Increment(_) => "increment",
        UpdateValue::Decrement(_) => "decrement",
        UpdateValue::Multiply(_) => "multiply",
        UpdateValue::Divide(_) => "divide",
        UpdateValue::ArrayAppend(_) => "array append",
        UpdateValue::ArrayRemove(_) => "array remove",
        UpdateValue::JsonSet(_, _) => "json set",
    };
    unsupported_batch_operation(operation, field_name, column_name)
}

/// The encrypted field `name`, a field or column name, names, with its column.
fn resolve_encrypted_field<M: ModelMeta>(
    name: &str,
) -> Result<Option<(&'static str, &'static str)>> {
    Ok(super::encrypted_field_columns::<M>()?
        .into_iter()
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
    // The field is a `String` and decrypts back into one: any other JSON value
    // would be stored, then fail every load of the row.
    if !value.is_string() {
        return Err(Error::query(format!(
            "Encrypted field '{}' takes a string or null, not a JSON {}",
            encrypted_field_label(field_name, column_name),
            json_kind(&value)
        )));
    }

    encrypted::encrypt_json_value_for_attribute(&value, table_name, column_name)
        .map(serde_json::Value::String)
        .map_err(|error| annotate_crypto_error(error, "encrypt", field_name, column_name))
}

fn json_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn unsupported_batch_operation<T>(
    operation: &str,
    field_name: &str,
    column_name: &str,
) -> Result<T> {
    Err(Error::query(format!(
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

/// Name the field in a payload error while keeping its error class.
///
/// The payload helpers only produce configuration (no key), conversion (JSON)
/// and tokenization (payload or cipher) errors; anything else passes through.
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
        Error::Tokenization { .. } => Error::tokenization(message),
        other => other,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/model_encryption_tests.rs"]
mod tests;
