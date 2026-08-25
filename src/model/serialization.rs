#![allow(missing_docs)]

use std::collections::HashMap;

use super::{Model, ModelMeta};

pub(crate) fn to_json<M>(model: &M, options: Option<&HashMap<String, String>>) -> serde_json::Value
where
    M: Model + serde::Serialize,
{
    #[cfg(feature = "translations")]
    let fallback = M::fallback_language();
    #[cfg(feature = "translations")]
    let current_language = options
        .and_then(|opts| opts.get("language"))
        .map(|s| s.as_str())
        .unwrap_or(&fallback);
    let _current_presenter = options
        .and_then(|opts| opts.get("presenter"))
        .map(|s| s.as_str())
        .unwrap_or(M::default_presenter());

    let hidden = M::hidden_attributes();
    let global_hidden = crate::config::Config::get_hidden_attributes();
    #[cfg(feature = "translations")]
    let translatable = M::translatable_fields();
    #[cfg(feature = "attachments")]
    let file_relations = M::files_relations();

    let mut json = match model_to_object(model) {
        Ok(map) => map,
        Err(error) => {
            crate::tide_warn!("to_json for `{}` failed: {}", M::table_name(), error);
            return serde_json::json!({});
        }
    };

    for attr in &hidden {
        json.remove(*attr);
    }

    for attr in &global_hidden {
        json.remove(attr.as_str());
    }

    #[cfg(feature = "translations")]
    if M::has_translations() {
        if let Some(translations) = json.get("translations").cloned() {
            if let Some(trans_obj) = translations.as_object() {
                for field in &translatable {
                    if let Some(field_trans) = trans_obj.get(*field) {
                        if let Some(field_obj) = field_trans.as_object() {
                            if let Some(value) = field_obj
                                .get(current_language)
                                .or_else(|| field_obj.get(&fallback))
                            {
                                json.insert(field.to_string(), value.clone());
                            }
                        }
                    }
                }
            }
            json.remove("translations");
        }
    }

    #[cfg(feature = "attachments")]
    if M::has_file_attachments() {
        let url_generator = M::file_url_generator();
        if let Some(files) = json.remove("files") {
            if let Some(files_obj) = files.as_object() {
                for relation in &file_relations {
                    if let Some(file_data) = files_obj.get(*relation) {
                        let processed =
                            process_file_for_json(relation, file_data, &hidden, url_generator);
                        json.insert(relation.to_string(), processed);
                    }
                }
            }
        }
    }

    strip_hidden_from_non_column_payloads::<M>(&mut json, &global_hidden);

    serde_json::Value::Object(json)
}

/// Return whether `name` is one of the model's own persisted attributes.
///
/// Anything else in the serialized payload is relation or attachment data that
/// belongs to another model, so it still needs hidden-attribute filtering.
fn is_persisted_attribute<M>(name: &str) -> bool
where
    M: ModelMeta,
{
    M::field_names().contains(&name) || M::column_names().contains(&name)
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
/// runtime) keep the older behaviour of reusing the owning model's list.
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
                map.remove(attr);
            }

            for attr in global_hidden {
                map.remove(attr.as_str());
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
pub(crate) fn process_file_for_json(
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
    M: Model + serde::Serialize,
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
    M: Model + serde::Serialize,
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

fn overwrite_model_from_object<M>(
    model: &mut M,
    object: serde_json::Map<String, serde_json::Value>,
) -> std::result::Result<(), String>
where
    M: Model,
{
    let mut updated: M = serde_json::from_value(serde_json::Value::Object(object))
        .map_err(|error| format!("Failed to deserialize model: {}", error))?;
    updated.refresh_runtime_relations_from(model);
    *model = updated;
    Ok(())
}

/// Resolve a field or database column name to the model's serde field name,
/// which is the key used by the JSON representation of the model.
fn canonical_field_name<M>(name: &str) -> Option<&'static str>
where
    M: Model,
{
    if let Some(field_name) = M::field_names()
        .iter()
        .copied()
        .find(|field| *field == name)
    {
        return Some(field_name);
    }

    M::field_names()
        .iter()
        .copied()
        .zip(M::column_names().iter().copied())
        .find_map(|(field_name, column_name)| (column_name == name).then_some(field_name))
}

fn changes_to_object<M>(
    changes: HashMap<String, serde_json::Value>,
    object: &mut serde_json::Map<String, serde_json::Value>,
) -> std::result::Result<(), String>
where
    M: Model,
{
    for (name, value) in changes {
        let field_name = canonical_field_name::<M>(&name)
            .ok_or_else(|| format!("Unknown field '{}' for '{}'", name, M::table_name()))?;
        object.insert(field_name.to_string(), value);
    }

    Ok(())
}

/// Apply accumulated attribute changes to `model` in place.
///
/// Keys accept either the Rust field name or the database column name.
pub(crate) fn apply_changes<M>(
    model: &mut M,
    changes: HashMap<String, serde_json::Value>,
) -> std::result::Result<(), String>
where
    M: Model,
{
    if changes.is_empty() {
        return Ok(());
    }

    let mut object = model_to_object(model)?;
    changes_to_object::<M>(changes, &mut object)?;

    overwrite_model_from_object(model, object)
}

/// Build a brand new model from accumulated attribute values.
///
/// Keys accept either the Rust field name or the database column name.
pub(crate) fn model_from_values<M>(
    values: HashMap<String, serde_json::Value>,
) -> std::result::Result<M, String>
where
    M: Model,
{
    let mut object = serde_json::Map::new();
    changes_to_object::<M>(values, &mut object)?;

    serde_json::from_value(serde_json::Value::Object(object))
        .map_err(|error| format!("Failed to build model from values: {}", error))
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

    let mut object = model_to_object(model)?;
    let fallback = M::fallback_language();

    let translations = object
        .get("translations")
        .and_then(serde_json::Value::as_object)
        .cloned();

    if let Some(translations) = translations {
        for field in M::translatable_fields() {
            if let Some(value) = translations
                .get(field)
                .and_then(serde_json::Value::as_object)
                .and_then(|by_language| {
                    by_language
                        .get(language)
                        .or_else(|| by_language.get(fallback.as_str()))
                })
            {
                object.insert(field.to_string(), value.clone());
            }
        }
    }

    overwrite_model_from_object(model, object)
}

#[cfg(feature = "translations")]
pub(crate) fn extract_translations<M>(
    data: &mut HashMap<String, serde_json::Value>,
) -> std::result::Result<serde_json::Value, String>
where
    M: Model,
{
    if !M::has_translations() {
        return Ok(serde_json::json!({}));
    }

    let mut translations = serde_json::Map::new();
    let translatable = M::translatable_fields();

    for field in translatable {
        if let Some(value) = data.get(field) {
            if value.is_object() {
                translations.insert(field.to_string(), value.clone());
                data.remove(field);
            }
        }
    }

    Ok(serde_json::Value::Object(translations))
}

#[cfg(not(feature = "translations"))]
pub(crate) fn extract_translations<M>(
    data: &mut HashMap<String, serde_json::Value>,
) -> std::result::Result<serde_json::Value, String>
where
    M: Model,
{
    let _ = std::marker::PhantomData::<M>;
    let _ = data;
    Ok(serde_json::json!({}))
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

    let mut object = model_to_object(model)?;
    match object.remove("files") {
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

    let mut object = model_to_object(model)?;
    object.insert(
        "files".to_string(),
        serde_json::Value::Object(files.into_iter().collect()),
    );

    overwrite_model_from_object(model, object)
}

pub(crate) fn attach_file<M>(
    relation_type: &str,
    file_key: &str,
    files: &mut HashMap<String, serde_json::Value>,
) -> std::result::Result<(), String>
where
    M: Model,
{
    if !M::has_file_attachments() {
        return Err("Model does not support file attachments".to_string());
    }

    let file_metadata = serde_json::json!({
        "key": file_key,
        "filename": file_key.split('/').next_back().unwrap_or(file_key),
        "created_at": chrono::Utc::now().to_rfc3339(),
    });

    if M::has_one_attached_file().contains(&relation_type) {
        files.insert(relation_type.to_string(), file_metadata);
    } else if M::has_many_attached_files().contains(&relation_type) {
        let mut array = files
            .get(relation_type)
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        array.push(file_metadata);
        files.insert(relation_type.to_string(), serde_json::Value::Array(array));
    } else {
        return Err(format!("Unknown file relation: {}", relation_type));
    }

    Ok(())
}

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

pub(crate) fn attach_files<M>(
    relation_type: &str,
    file_keys: Vec<&str>,
    files: &mut HashMap<String, serde_json::Value>,
) -> std::result::Result<(), String>
where
    M: Model,
{
    if !M::has_file_attachments() {
        return Err("Model does not support file attachments".to_string());
    }

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
    if !M::has_file_attachments() {
        return Err("Model does not support file attachments".to_string());
    }

    match file_relation_kind::<M>(relation_type)? {
        FileRelationKind::HasOne => {
            if let Some(key) = file_key {
                if let Some(current) = files.get(relation_type) {
                    if current.get("key").and_then(|k| k.as_str()) == Some(key) {
                        files.insert(relation_type.to_string(), serde_json::Value::Null);
                    }
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
    if !M::has_file_attachments() {
        return Err("Model does not support file attachments".to_string());
    }

    match file_relation_kind::<M>(relation_type)? {
        FileRelationKind::HasOne => {
            if file_keys.is_empty() {
                files.insert(relation_type.to_string(), serde_json::Value::Null);
                return Ok(());
            }

            let file_metadata = serde_json::json!({
                "key": file_keys[0],
                "filename": file_keys[0].split('/').next_back().unwrap_or(file_keys[0]),
                "created_at": chrono::Utc::now().to_rfc3339(),
            });
            files.insert(relation_type.to_string(), file_metadata);
        }
        FileRelationKind::HasMany => {
            if file_keys.is_empty() {
                files.insert(relation_type.to_string(), serde_json::Value::Array(vec![]));
                return Ok(());
            }

            let file_array: Vec<serde_json::Value> = file_keys
                .iter()
                .map(|key| {
                    serde_json::json!({
                        "key": key,
                        "filename": key.split('/').next_back().unwrap_or(key),
                        "created_at": chrono::Utc::now().to_rfc3339(),
                    })
                })
                .collect();
            files.insert(
                relation_type.to_string(),
                serde_json::Value::Array(file_array),
            );
        }
    }

    Ok(())
}

#[cfg(feature = "attachments")]
pub(crate) fn extract_files<M>(
    data: &mut HashMap<String, serde_json::Value>,
) -> std::result::Result<serde_json::Value, String>
where
    M: Model,
{
    if !M::has_file_attachments() {
        return Ok(serde_json::json!({}));
    }

    let mut files = serde_json::Map::new();
    let file_relations = M::files_relations();

    for relation in file_relations {
        if let Some(value) = data.remove(relation) {
            files.insert(relation.to_string(), value);
        }
    }

    Ok(serde_json::Value::Object(files))
}

#[cfg(not(feature = "attachments"))]
pub(crate) fn extract_files<M>(
    data: &mut HashMap<String, serde_json::Value>,
) -> std::result::Result<serde_json::Value, String>
where
    M: Model,
{
    let _ = std::marker::PhantomData::<M>;
    let _ = data;
    Ok(serde_json::json!({}))
}

#[cfg(test)]
#[path = "../../tests/unit/model_serialization_tests.rs"]
mod tests;
