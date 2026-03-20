#![allow(missing_docs)]

use std::collections::HashMap;

use super::Model;

pub(crate) fn to_json<M>(model: &M, options: Option<HashMap<String, String>>) -> serde_json::Value
where
    M: Model + serde::Serialize,
{
    let opts = options.unwrap_or_default();
    #[cfg(feature = "translations")]
    let fallback = M::fallback_language();
    #[cfg(feature = "translations")]
    let current_language = opts
        .get("language")
        .map(|s| s.as_str())
        .unwrap_or(&fallback);
    let _current_presenter = opts
        .get("presenter")
        .map(|s| s.as_str())
        .unwrap_or(M::default_presenter());

    let hidden = M::hidden_attributes();
    #[cfg(feature = "translations")]
    let translatable = M::translatable_fields();
    #[cfg(feature = "attachments")]
    let file_relations = M::files_relations();

    let mut json = match serde_json::to_value(model) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => return serde_json::json!({}),
    };

    for attr in &hidden {
        json.remove(*attr);
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

    serde_json::Value::Object(json)
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
            .map(|model| to_json(model, options.clone()))
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
    *model = serde_json::from_value(serde_json::Value::Object(object))
        .map_err(|error| format!("Failed to deserialize model: {}", error))?;
    Ok(())
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

pub(crate) fn load_all_translations<M>(model: &mut M) -> std::result::Result<(), String>
where
    M: Model,
{
    let _ = model;
    if !M::has_translations() {
        return Err("Model does not support translations".to_string());
    }

    Err(
        "Loading all translations into model fields is not supported; use to_json_with_all_translations() or translation accessors instead".to_string(),
    )
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

    let object = model_to_object(model)?;
    match object.get("files") {
        None | Some(serde_json::Value::Null) => Ok(HashMap::new()),
        Some(serde_json::Value::Object(map)) => Ok(map
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()),
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

    if !M::has_many_attached_files().contains(&relation_type) {
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

    if let Some(key) = file_key {
        if M::has_one_attached_file().contains(&relation_type) {
            if let Some(current) = files.get(relation_type) {
                if current.get("key").and_then(|k| k.as_str()) == Some(key) {
                    files.insert(relation_type.to_string(), serde_json::Value::Null);
                }
            }
        } else if M::has_many_attached_files().contains(&relation_type) {
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
        }
    } else if M::has_one_attached_file().contains(&relation_type) {
        files.insert(relation_type.to_string(), serde_json::Value::Null);
    } else if M::has_many_attached_files().contains(&relation_type) {
        files.insert(relation_type.to_string(), serde_json::Value::Array(vec![]));
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

    if file_keys.is_empty() {
        if M::has_one_attached_file().contains(&relation_type) {
            files.insert(relation_type.to_string(), serde_json::Value::Null);
        } else if M::has_many_attached_files().contains(&relation_type) {
            files.insert(relation_type.to_string(), serde_json::Value::Array(vec![]));
        }
        return Ok(());
    }

    if M::has_one_attached_file().contains(&relation_type) {
        let file_metadata = serde_json::json!({
            "key": file_keys[0],
            "filename": file_keys[0].split('/').next_back().unwrap_or(file_keys[0]),
            "created_at": chrono::Utc::now().to_rfc3339(),
        });
        files.insert(relation_type.to_string(), file_metadata);
    } else if M::has_many_attached_files().contains(&relation_type) {
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
    } else {
        return Err(format!("Unknown file relation: {}", relation_type));
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
#[path = "../testing/model_serialization_tests.rs"]
mod tests;
