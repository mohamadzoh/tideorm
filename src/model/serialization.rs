#![allow(missing_docs)]

use std::collections::HashMap;

use super::{Model, ModelMeta};

pub(crate) fn to_json<M>(model: &M, options: Option<&HashMap<String, String>>) -> serde_json::Value
where
    M: Model,
{
    try_to_json(model, options).unwrap_or_else(|error| {
        crate::tide_warn!("to_json for `{}` failed: {}", M::table_name(), error);
        serde_json::json!({})
    })
}

pub(crate) fn try_to_json<M: Model>(
    model: &M,
    options: Option<&HashMap<String, String>>,
) -> crate::Result<serde_json::Value> {
    let global_hidden = crate::config::Config::get_hidden_attributes();
    if let Some(field) = unfilterable_flattened_field::<M>(&global_hidden) {
        return Err(crate::Error::configuration(format!(
            "Cannot safely filter flattened field '{field}' on '{}': remove #[serde(flatten)] or use #[serde(skip_serializing)]",
            M::table_name(),
        )));
    }
    #[allow(unused_mut)] // Optional translation and attachment renderers mutate this map.
    let mut json = model_to_object(model).map_err(crate::Error::internal)?;
    #[cfg(feature = "translations")]
    if M::has_translations() {
        let serialized = json.remove(M::serialized_name("translations"));
        let payload = model.__translation_payload()?.or(serialized);
        if let Some(payload) = payload.filter(|value| !value.is_null()) {
            // A malformed field, or a payload that is no object, is skipped:
            // it must not blank the whole model.
            let data = crate::translations::TranslationsData::from_json(&payload);
            let fallback = M::fallback_language();
            let language = options
                .and_then(|opts| opts.get("language"))
                .map(String::as_str)
                .unwrap_or(&fallback);
            // A field the model's own serde skips stays out, translated or not.
            let fields = M::translatable_fields()
                .into_iter()
                .filter(|field| !M::__skipped_fields().contains(field))
                .collect();
            crate::translations::render_translations(
                &mut json,
                &data,
                fields,
                language,
                &fallback,
                M::serialized_name,
            );
        }
    }
    #[cfg(not(feature = "translations"))]
    let _ = options;

    #[cfg(feature = "attachments")]
    if M::has_file_attachments()
        && let Some(files) = json.remove(M::serialized_name("files"))
        && let Some(files_obj) = files.as_object()
    {
        let hidden = M::hidden_attributes();
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

    // After translation resolution, which writes translatable fields back in:
    // a field that is both hidden and translatable must stay hidden. An
    // attachment named among the hidden attributes was left out above.
    let mut json = serde_json::Value::Object(json);
    strip_model_payload::<M>(&mut json, &global_hidden);
    Ok(json)
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
        let filter = relation_filters
            .iter()
            .find_map(|(field, filter)| (*field == key.as_str()).then_some(*filter));

        match filter {
            Some(filter) => filter(value, global_hidden),
            // Database column names may also name a relation in the JSON.
            // A declared relation always takes precedence over that heuristic.
            None if is_persisted_attribute::<M>(key) => {}
            None => strip_hidden_attributes(value, &hidden, global_hidden),
        }
    }
}

fn unfilterable_flattened_field<M: ModelMeta>(global_hidden: &[String]) -> Option<&'static str> {
    let hidden = M::hidden_attributes();
    let relations = M::relation_payload_filters();
    M::__flattened_fields().iter().copied().find(|field| {
        hidden.contains(field)
            || global_hidden.iter().any(|attr| attr == field)
            // Flattening removes the boundary needed to apply the target's
            // hidden list, including to its own nested relations. Its keys
            // can also overwrite the parent's, so filtering the merged map
            // with either model's metadata is unsafe.
            || relations.iter().any(|(key, _)| *key == M::serialized_name(field))
    })
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
            // Nested relations arrive already serialized. A flattened field's
            // keys may overlap any other field, so none of this object can be
            // trusted when that field is hidden or contains a typed relation.
            if unfilterable_flattened_field::<M>(global_hidden).is_some() {
                map.clear();
                return;
            }
            for attr in M::hidden_attributes() {
                map.remove(M::serialized_name(attr));
            }

            for attr in global_hidden {
                map.remove(M::serialized_name(attr));
            }

            strip_hidden_backing_entries::<M>(map, global_hidden);
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

/// Drop the entries a hidden field keeps in `M`'s raw `translations` column,
/// and those of a field `M`'s own serde skips, plus a hidden attachment's in
/// its `files` column.
///
/// `try_to_json` renders both columns, leaving those entries out, before it
/// filters its own payload, so there this finds nothing; a related model's
/// payload nested in another still carries them as serialized.
fn strip_hidden_backing_entries<M>(
    map: &mut serde_json::Map<String, serde_json::Value>,
    global_hidden: &[String],
) where
    M: ModelMeta,
{
    let hidden = M::hidden_attributes();
    let is_hidden =
        |name: &str| hidden.contains(&name) || global_hidden.iter().any(|attr| attr == name);
    if M::has_translations()
        && let Some(serde_json::Value::Object(entries)) =
            map.get_mut(M::serialized_name("translations"))
    {
        entries.retain(|field, _| {
            !is_hidden(field) && !M::__skipped_fields().contains(&field.as_str())
        });
    }
    if M::has_file_attachments()
        && let Some(serde_json::Value::Object(entries)) = map.get_mut(M::serialized_name("files"))
    {
        entries.retain(|relation, _| !is_hidden(relation));
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

            map.insert(output_key.to_string(), crate::internal::json_text(value));
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

#[cfg(test)]
#[path = "../../tests/unit/model_serialization_tests.rs"]
mod tests;
