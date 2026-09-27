#![allow(missing_docs)]

use std::collections::HashMap;

use super::{Model, ModelMeta};

pub(crate) fn to_json<M>(model: &M, options: Option<&HashMap<String, String>>) -> serde_json::Value
where
    M: Model,
{
    let hidden = M::hidden_attributes();
    let global_hidden = crate::config::Config::get_hidden_attributes();

    let mut json = match model_to_object(model) {
        Ok(map) => map,
        Err(error) => {
            crate::tide_warn!("to_json for `{}` failed: {}", M::table_name(), error);
            return serde_json::json!({});
        }
    };

    if M::has_translations()
        && let Some(translations) = json.remove(M::serialized_name("translations"))
        && let Some(translations) = translations.as_object()
    {
        let language = options
            .and_then(|opts| opts.get("language"))
            .map(String::as_str);
        for (field, value) in translated_values::<M>(translations, language) {
            json.insert(M::serialized_name(field).to_string(), value);
        }
    }

    // After translation resolution, which writes translatable fields back in:
    // a field that is both hidden and translatable must stay hidden.
    for attr in &hidden {
        json.remove(M::serialized_name(attr));
    }

    for attr in &global_hidden {
        json.remove(M::serialized_name(attr));
    }

    #[cfg(feature = "attachments")]
    if M::has_file_attachments()
        && let Some(files) = json.remove(M::serialized_name("files"))
        && let Some(files_obj) = files.as_object()
    {
        let url_generator = M::file_url_generator();
        for relation in M::files_relations() {
            // An attachment named among the hidden attributes stays hidden.
            if hidden.contains(&relation)
                || global_hidden.iter().any(|attr| attr.as_str() == relation)
            {
                continue;
            }
            if let Some(file_data) = files_obj.get(relation) {
                let processed = process_file_for_json(relation, file_data, &hidden, url_generator);
                json.insert(relation.to_string(), processed);
            }
        }
    }

    strip_hidden_from_non_column_payloads::<M>(&mut json, &global_hidden);

    serde_json::Value::Object(json)
}

/// Each translatable field's value for `language` (default: the model's
/// fallback language), falling back to the fallback language for a field the
/// requested one does not define.
fn translated_values<M>(
    translations: &serde_json::Map<String, serde_json::Value>,
    language: Option<&str>,
) -> Vec<(&'static str, serde_json::Value)>
where
    M: ModelMeta,
{
    let fallback = M::fallback_language();
    let language = language.unwrap_or(&fallback);

    M::translatable_fields()
        .into_iter()
        .filter_map(|field| {
            let by_language = translations.get(field)?.as_object()?;
            let value = by_language
                .get(language)
                .or_else(|| by_language.get(fallback.as_str()))?;
            Some((field, value.clone()))
        })
        .collect()
}

/// Return whether `name` is one of the model's own persisted attributes.
///
/// Anything else in the serialized payload is relation or attachment data that
/// belongs to another model, so it still needs hidden-attribute filtering.
fn is_persisted_attribute<M>(name: &str) -> bool
where
    M: ModelMeta,
{
    M::field_names()
        .iter()
        .any(|field| *field == name || M::serialized_name(field) == name)
        || M::column_names().contains(&name)
}

/// Apply hidden-attribute filtering to eager-loaded relation and attachment
/// payloads, which serde emits verbatim from the related model.
///
/// Only non-column keys are visited so JSON-typed columns keep their contents.
///
/// A key `M` declares as a relation is filtered with the **target** model's
/// hidden list — via the function pointer in
/// `ModelMeta::relation_payload_filters` — and then descended recursively to
/// any depth, so `post.to_json(None)` after `.with("author")` hides what `User`
/// declares hidden rather than what `Post` does. Payloads with no declared
/// target (attachment blobs, and `MorphTo` fields whose model is only known at
/// runtime) are filtered with the owning model's list instead.
fn strip_hidden_from_non_column_payloads<M>(
    object: &mut serde_json::Map<String, serde_json::Value>,
    global_hidden: &[String],
) where
    M: ModelMeta,
{
    let hidden = M::hidden_attributes();
    let relation_filters = M::relation_payload_filters();

    for (key, value) in object.iter_mut() {
        if is_persisted_attribute::<M>(key) {
            continue;
        }

        let filter = relation_filters
            .iter()
            .find_map(|(field, filter)| (*field == key.as_str()).then_some(*filter));

        match filter {
            Some(filter) => filter(value, global_hidden),
            None => strip_hidden_attributes(value, &hidden, global_hidden),
        }
    }
}

/// Filter one already-serialized payload of model `M` in place: drop `M`'s own
/// hidden attributes plus the global list, then recurse through `M`'s relation
/// payloads.
///
/// Reached through `ModelMeta::__strip_hidden_payload`, which is what lets a
/// nested payload — untagged JSON by the time `to_json` sees it — be filtered by
/// the model that produced it. Recursion is driven by the JSON, not by the
/// relation graph, so a cyclic relation graph still terminates.
pub(crate) fn strip_model_payload<M>(value: &mut serde_json::Value, global_hidden: &[String])
where
    M: ModelMeta,
{
    match value {
        serde_json::Value::Object(map) => {
            for attr in M::hidden_attributes() {
                map.remove(M::serialized_name(attr));
            }

            for attr in global_hidden {
                map.remove(M::serialized_name(attr));
            }

            strip_hidden_from_non_column_payloads::<M>(map, global_hidden);
        }
        serde_json::Value::Array(items) => {
            for item in items {
                strip_model_payload::<M>(item, global_hidden);
            }
        }
        _ => {}
    }
}

fn strip_hidden_attributes(
    value: &mut serde_json::Value,
    hidden: &[&str],
    global_hidden: &[String],
) {
    match value {
        serde_json::Value::Object(map) => {
            for attr in hidden {
                map.remove(*attr);
            }

            for attr in global_hidden {
                map.remove(attr.as_str());
            }

            for nested in map.values_mut() {
                strip_hidden_attributes(nested, hidden, global_hidden);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                strip_hidden_attributes(item, hidden, global_hidden);
            }
        }
        _ => {}
    }
}

#[cfg(feature = "attachments")]
fn process_file_for_json(
    field_name: &str,
    file_data: &serde_json::Value,
    hidden_attrs: &[&str],
    url_generator: crate::config::FileUrlGenerator,
) -> serde_json::Value {
    match file_data {
        serde_json::Value::Object(obj) => {
            let mut cleaned = serde_json::Map::new();
            for (key, value) in obj {
                if !hidden_attrs.contains(&key.as_str()) {
                    cleaned.insert(key.clone(), value.clone());
                }
            }

            if let Ok(file_attachment) = serde_json::from_value::<crate::attachments::FileAttachment>(
                serde_json::Value::Object(obj.clone()),
            ) {
                let url = url_generator(field_name, &file_attachment);
                cleaned.insert("url".to_string(), serde_json::Value::String(url));
            }

            serde_json::Value::Object(cleaned)
        }
        serde_json::Value::Array(arr) => serde_json::Value::Array(
            arr.iter()
                .map(|item| process_file_for_json(field_name, item, hidden_attrs, url_generator))
                .collect(),
        ),
        other => other.clone(),
    }
}

pub(crate) fn collection_to_json<M>(
    models: Vec<M>,
    options: Option<HashMap<String, String>>,
) -> serde_json::Value
where
    M: Model,
{
    serde_json::Value::Array(
        models
            .iter()
            .map(|model| to_json(model, options.as_ref()))
            .collect(),
    )
}

pub(crate) fn to_hash_map<M>(model: &M) -> HashMap<String, String>
where
    M: Model,
{
    let json = to_json(model, None);
    let mut map = HashMap::new();

    if let Some(obj) = json.as_object() {
        for (key, value) in obj {
            let Some(output_key) = hash_map_output_key(key, value) else {
                continue;
            };

            let str_val = match value {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Null => "null".to_string(),
                _ => value.to_string(),
            };
            map.insert(output_key.to_string(), str_val);
        }
    }

    map
}

fn hash_map_output_key<'a>(key: &'a str, value: &serde_json::Value) -> Option<&'a str> {
    if key == "params" && (value.is_object() || value.is_array()) {
        None
    } else {
        Some(key)
    }
}

fn model_to_object<M>(
    model: &M,
) -> std::result::Result<serde_json::Map<String, serde_json::Value>, String>
where
    M: Model,
{
    match serde_json::to_value(model) {
        Ok(serde_json::Value::Object(map)) => Ok(map),
        Ok(_) => Err("Failed to serialize model into an object".to_string()),
        Err(error) => Err(format!("Failed to serialize model: {}", error)),
    }
}

pub(crate) fn load_language_translations<M>(
    model: &mut M,
    language: &str,
) -> std::result::Result<(), String>
where
    M: Model,
{
    if !M::has_translations() {
        return Err("Model does not support translations".to_string());
    }

    let translations = model
        .field_json_value("translations")
        .map_err(|error| error.to_string())?;
    let Some(translations) = translations.as_ref().and_then(serde_json::Value::as_object) else {
        return Ok(());
    };
    for (field, value) in translated_values::<M>(translations, Some(language)) {
        model
            .set_field_json(field, value)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(crate) fn get_files_attribute<M>(
    model: &M,
) -> std::result::Result<HashMap<String, serde_json::Value>, String>
where
    M: Model,
{
    if !M::has_file_attachments() {
        return Err("Model does not support file attachments".to_string());
    }

    match model
        .field_json_value("files")
        .map_err(|error| error.to_string())?
    {
        None | Some(serde_json::Value::Null) => Ok(HashMap::new()),
        Some(serde_json::Value::Object(map)) => Ok(map.into_iter().collect()),
        Some(_) => Err("Model files attribute is not a JSON object".to_string()),
    }
}

pub(crate) fn set_files_attribute<M>(
    model: &mut M,
    files: HashMap<String, serde_json::Value>,
) -> std::result::Result<(), String>
where
    M: Model,
{
    if !M::has_file_attachments() {
        return Err("Model does not support file attachments".to_string());
    }

    let files = serde_json::Value::Object(files.into_iter().collect());
    match model.set_field_json("files", files) {
        Ok(true) => Ok(()),
        Ok(false) => Err("Model has no files field".to_string()),
        Err(error) => Err(error.to_string()),
    }
}

// The attachment editors below receive `files` from `get_files_attribute`,
// which has already rejected a model without attachments.

enum FileRelationKind {
    HasOne,
    HasMany,
}

fn file_relation_kind<M>(relation_type: &str) -> std::result::Result<FileRelationKind, String>
where
    M: Model,
{
    if M::has_one_attached_file().contains(&relation_type) {
        Ok(FileRelationKind::HasOne)
    } else if M::has_many_attached_files().contains(&relation_type) {
        Ok(FileRelationKind::HasMany)
    } else {
        Err(format!("Unknown file relation: {}", relation_type))
    }
}

/// Metadata recorded for one attached file: its storage key, the key's last
/// path segment as the file name, and the time it was attached.
fn file_metadata(file_key: &str) -> serde_json::Value {
    serde_json::json!({
        "key": file_key,
        "filename": file_key.split('/').next_back().unwrap_or(file_key),
        "created_at": chrono::Utc::now().to_rfc3339(),
    })
}

pub(crate) fn attach_file<M>(
    relation_type: &str,
    file_key: &str,
    files: &mut HashMap<String, serde_json::Value>,
) -> std::result::Result<(), String>
where
    M: Model,
{
    let metadata = file_metadata(file_key);

    match file_relation_kind::<M>(relation_type)? {
        FileRelationKind::HasOne => {
            files.insert(relation_type.to_string(), metadata);
        }
        FileRelationKind::HasMany => {
            let mut array = files
                .get(relation_type)
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            array.push(metadata);
            files.insert(relation_type.to_string(), serde_json::Value::Array(array));
        }
    }

    Ok(())
}

pub(crate) fn attach_files<M>(
    relation_type: &str,
    file_keys: Vec<&str>,
    files: &mut HashMap<String, serde_json::Value>,
) -> std::result::Result<(), String>
where
    M: Model,
{
    if !matches!(
        file_relation_kind::<M>(relation_type)?,
        FileRelationKind::HasMany
    ) {
        return Err(format!(
            "Relation '{}' is not a hasMany relation",
            relation_type
        ));
    }

    for file_key in file_keys {
        attach_file::<M>(relation_type, file_key, files)?;
    }

    Ok(())
}

pub(crate) fn detach_file<M>(
    relation_type: &str,
    file_key: Option<&str>,
    files: &mut HashMap<String, serde_json::Value>,
) -> std::result::Result<(), String>
where
    M: Model,
{
    match file_relation_kind::<M>(relation_type)? {
        FileRelationKind::HasOne => {
            if let Some(key) = file_key {
                if let Some(current) = files.get(relation_type)
                    && current.get("key").and_then(|k| k.as_str()) == Some(key)
                {
                    files.insert(relation_type.to_string(), serde_json::Value::Null);
                }
            } else {
                files.insert(relation_type.to_string(), serde_json::Value::Null);
            }
        }
        FileRelationKind::HasMany => {
            if let Some(key) = file_key {
                if let Some(array) = files.get(relation_type).and_then(|v| v.as_array()) {
                    let filtered: Vec<serde_json::Value> = array
                        .iter()
                        .filter(|item| item.get("key").and_then(|k| k.as_str()) != Some(key))
                        .cloned()
                        .collect();
                    files.insert(
                        relation_type.to_string(),
                        serde_json::Value::Array(filtered),
                    );
                }
            } else {
                files.insert(relation_type.to_string(), serde_json::Value::Array(vec![]));
            }
        }
    }

    Ok(())
}

pub(crate) fn sync_files<M>(
    relation_type: &str,
    file_keys: Vec<&str>,
    files: &mut HashMap<String, serde_json::Value>,
) -> std::result::Result<(), String>
where
    M: Model,
{
    let synced = match file_relation_kind::<M>(relation_type)? {
        FileRelationKind::HasOne => file_keys
            .first()
            .map_or(serde_json::Value::Null, |key| file_metadata(key)),
        FileRelationKind::HasMany => {
            serde_json::Value::Array(file_keys.into_iter().map(file_metadata).collect())
        }
    };
    files.insert(relation_type.to_string(), synced);

    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/model_serialization_tests.rs"]
mod tests;
