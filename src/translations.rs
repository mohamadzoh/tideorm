//! Translations System
//!
//! This module stores per-language field values in a JSON or JSONB column.
//!
//! The data shape is:
//! `{field_name: {lang_code: value, ...}, ...}`
//!
//! Use it when the model keeps a default value on the struct but also needs
//! per-language overrides.
//!
//! If a translation lookup returns the fallback value unexpectedly, check that:
//! - the field is listed in `#[tideorm(translatable = ...)]`
//! - the language key was stored under the expected field name
//! - the model was saved after mutating the translations payload
//!
//! Typical workflow:
//! - declare the `translations` JSON or JSONB column plus `#[tideorm(translatable = ...)]`
//! - use `set_translation()` or `set_translations()` to update per-language overrides
//! - use `get_translated()` when the caller wants the fallback chain applied automatically
//! - save the model afterward so the updated translations payload is persisted

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod errors;
mod input;

pub use errors::TranslationError;
pub use input::{ApplyTranslations, TranslationInput};

/// Translation data container
///
/// Stores translations in the format: `{field: {lang: value}}`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TranslationsData {
    #[serde(flatten)]
    fields: HashMap<String, FieldTranslations>,
}

/// Translations for a single field
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FieldTranslations {
    #[serde(flatten)]
    translations: HashMap<String, serde_json::Value>,
}

impl FieldTranslations {
    /// Create empty field translations
    pub fn new() -> Self {
        Self {
            translations: HashMap::new(),
        }
    }

    /// Get translation for a language
    pub fn get(&self, lang: &str) -> Option<&serde_json::Value> {
        self.translations.get(lang)
    }

    /// Set translation for a language
    pub fn set(&mut self, lang: &str, value: impl Into<serde_json::Value>) {
        self.translations.insert(lang.to_string(), value.into());
    }

    /// Remove translation for a language
    pub fn remove(&mut self, lang: &str) {
        self.translations.remove(lang);
    }

    /// Get all translations as HashMap
    pub fn all(&self) -> &HashMap<String, serde_json::Value> {
        &self.translations
    }

    /// Check if has translation for language
    pub fn has(&self, lang: &str) -> bool {
        self.translations.contains_key(lang)
    }

    /// Get available languages
    pub fn languages(&self) -> Vec<&String> {
        self.translations.keys().collect()
    }
}

impl TranslationsData {
    /// Create empty translations data
    pub fn new() -> Self {
        Self {
            fields: HashMap::new(),
        }
    }

    /// Create from JSON value
    ///
    /// A field whose value is not a `{lang: value}` object is skipped rather
    /// than failing the whole payload, so one malformed field cannot hide — or,
    /// once saved back, erase — every other field's translations.
    pub fn from_json(value: &serde_json::Value) -> Self {
        let fields = value
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(field, translations)| {
                let translations = serde_json::from_value(translations.clone()).ok()?;
                Some((field.clone(), translations))
            })
            .collect();
        Self { fields }
    }

    /// Convert to JSON value
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }

    /// Get translations for a field
    pub fn get_field(&self, field: &str) -> Option<&FieldTranslations> {
        self.fields.get(field)
    }

    /// Get mutable translations for a field
    pub fn get_field_mut(&mut self, field: &str) -> &mut FieldTranslations {
        self.fields.entry(field.to_string()).or_default()
    }

    /// Get translation for a specific field and language
    pub fn get(&self, field: &str, lang: &str) -> Option<&serde_json::Value> {
        self.fields.get(field)?.get(lang)
    }

    /// Look `field` up in `lang`, then in `fallback`.
    fn get_or_fallback(
        &self,
        field: &str,
        lang: &str,
        fallback: &str,
    ) -> Option<&serde_json::Value> {
        self.get(field, lang).or_else(|| self.get(field, fallback))
    }

    /// Set translation for a specific field and language
    pub fn set(&mut self, field: &str, lang: &str, value: impl Into<serde_json::Value>) {
        self.get_field_mut(field).set(lang, value);
    }

    /// Remove translation for a field and language
    pub fn remove(&mut self, field: &str, lang: &str) {
        if let Some(field_trans) = self.fields.get_mut(field) {
            field_trans.remove(lang);
        }
    }

    /// Remove all translations for a field
    pub fn remove_field(&mut self, field: &str) {
        self.fields.remove(field);
    }

    /// Check if field has translations
    pub fn has_translations(&self, field: &str) -> bool {
        self.fields
            .get(field)
            .map(|f| !f.translations.is_empty())
            .unwrap_or(false)
    }

    /// Get all translatable fields
    pub fn fields(&self) -> Vec<&String> {
        self.fields.keys().collect()
    }
}

/// Trait for models with translations
///
/// **You implement this yourself** — the derive does not generate it. Declaring
/// `translatable` populates [`ModelMeta`](crate::model::ModelMeta) so the field
/// names are queryable, but the read/write half needs a body only you can supply:
/// it has to know which column holds the payload. Store it in a JSON column
/// (`translations: Option<Json>` by convention) and forward
/// `get_translations_data` / `set_translations_data` to it. A generated impl
/// would also collide with `ModelMeta`, which declares `translatable_fields`,
/// `allowed_languages` and `fallback_language` under the same names, and with
/// any impl written by hand on a `#[tideorm::model]` struct.
///
/// Declaring the three metadata methods here rather than borrowing `ModelMeta`'s
/// is deliberate: it lets a plain struct implement the trait with its own,
/// deterministic language policy instead of reading process-global config.
pub trait HasTranslations {
    /// Get the list of translatable field names
    fn translatable_fields() -> Vec<&'static str>;

    /// Get allowed languages for translations
    fn allowed_languages() -> Vec<String>;

    /// Get the fallback language
    fn fallback_language() -> String;

    /// Get the current translations data from the model
    fn get_translations_data(&self) -> Result<TranslationsData, TranslationError>;

    /// Set the translations data on the model
    fn set_translations_data(&mut self, data: TranslationsData) -> Result<(), TranslationError>;

    /// Get the default (non-translated) value for a field
    fn get_default_value(&self, field: &str) -> Result<serde_json::Value, TranslationError>;

    /// Set one translated value for one field and language.
    fn set_translation(
        &mut self,
        field: &str,
        lang: &str,
        value: impl Into<serde_json::Value>,
    ) -> Result<(), TranslationError> {
        self.validate_field(field)?;
        self.validate_language(lang)?;

        let mut data = self.get_translations_data()?;
        data.set(field, lang, value);
        self.set_translations_data(data)
    }

    /// Set multiple translated values for one field.
    fn set_translations<V: Into<serde_json::Value>>(
        &mut self,
        field: &str,
        translations: HashMap<&str, V>,
    ) -> Result<(), TranslationError> {
        self.validate_field(field)?;

        let mut data = self.get_translations_data()?;
        for (lang, value) in translations {
            self.validate_language(lang)?;
            data.set(field, lang, value);
        }
        self.set_translations_data(data)
    }

    /// Replace all stored translations for one field.
    fn sync_translations<V: Into<serde_json::Value>>(
        &mut self,
        field: &str,
        translations: HashMap<&str, V>,
    ) -> Result<(), TranslationError> {
        self.validate_field(field)?;

        let mut data = self.get_translations_data()?;
        data.remove_field(field);
        for (lang, value) in translations {
            self.validate_language(lang)?;
            data.set(field, lang, value);
        }
        self.set_translations_data(data)
    }

    /// Return the translation stored for one field and language.
    ///
    /// Returns `None` when that language has no stored override.
    fn get_translation(
        &self,
        field: &str,
        lang: &str,
    ) -> Result<Option<serde_json::Value>, TranslationError> {
        let data = self.get_translations_data()?;
        Ok(data.get(field, lang).cloned())
    }

    /// Return a translated value using the normal fallback chain.
    ///
    /// The lookup order is: requested language, fallback language, then the
    /// default value stored directly on the model.
    fn get_translated(
        &self,
        field: &str,
        lang: &str,
    ) -> Result<serde_json::Value, TranslationError> {
        let data = self.get_translations_data()?;

        match data.get_or_fallback(field, lang, &Self::fallback_language()) {
            Some(value) => Ok(value.clone()),
            None => self.get_default_value(field),
        }
    }

    /// Return every stored translation for one field.
    fn get_all_translations(
        &self,
        field: &str,
    ) -> Result<HashMap<String, serde_json::Value>, TranslationError> {
        let data = self.get_translations_data()?;
        Ok(data
            .get_field(field)
            .map(|f| f.all().clone())
            .unwrap_or_default())
    }

    /// Return all translated fields that have a value for one language.
    fn get_translations_for_language(
        &self,
        lang: &str,
    ) -> Result<HashMap<String, serde_json::Value>, TranslationError> {
        let data = self.get_translations_data()?;
        let mut result = HashMap::new();

        for field in Self::translatable_fields() {
            if let Some(value) = data.get(field, lang) {
                result.insert(field.to_string(), value.clone());
            }
        }

        Ok(result)
    }

    /// Remove a translation for a field in a specific language
    fn remove_translation(&mut self, field: &str, lang: &str) -> Result<(), TranslationError> {
        let mut data = self.get_translations_data()?;
        data.remove(field, lang);
        self.set_translations_data(data)
    }

    /// Remove all translations for a field
    fn remove_field_translations(&mut self, field: &str) -> Result<(), TranslationError> {
        let mut data = self.get_translations_data()?;
        data.remove_field(field);
        self.set_translations_data(data)
    }

    /// Clear all translations
    fn clear_translations(&mut self) -> Result<(), TranslationError> {
        self.set_translations_data(TranslationsData::new())
    }

    /// Check if field has a translation for a language
    fn has_translation(&self, field: &str, lang: &str) -> Result<bool, TranslationError> {
        let data = self.get_translations_data()?;
        Ok(data.get(field, lang).is_some())
    }

    /// Check if field has any translations
    fn has_any_translation(&self, field: &str) -> Result<bool, TranslationError> {
        let data = self.get_translations_data()?;
        Ok(data.has_translations(field))
    }

    /// Get languages available for a field
    fn available_languages(&self, field: &str) -> Result<Vec<String>, TranslationError> {
        let data = self.get_translations_data()?;
        Ok(data
            .get_field(field)
            .map(|f| f.languages().into_iter().cloned().collect())
            .unwrap_or_default())
    }

    /// Serialize the model with translated values applied.
    ///
    /// The raw `translations` column is removed from the returned JSON.
    /// Pass `options["language"]` to choose the requested language.
    fn to_translated_json(&self, options: Option<HashMap<String, String>>) -> serde_json::Value
    where
        Self: serde::Serialize,
    {
        let opts = options.unwrap_or_default();
        let fallback = Self::fallback_language();
        let requested_lang = opts
            .get("language")
            .map(|s| s.as_str())
            .unwrap_or(&fallback);

        let mut json = match serde_json::to_value(self) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => return serde_json::json!({}),
        };

        // Read through the accessor, as every translation lookup does: the
        // serialized JSON need not carry the raw column under that name, or at
        // all, when the model's own serde renames or skips it.
        let translations = self.get_translations_data().unwrap_or_default();

        // A field without a translation keeps the default value already in `json`.
        for field in Self::translatable_fields() {
            if let Some(value) = translations.get_or_fallback(field, requested_lang, &fallback) {
                json.insert(field.to_string(), value.clone());
            }
        }

        json.remove("translations");

        serde_json::Value::Object(json)
    }

    /// Serialize the model without removing the raw translations payload.
    fn to_json_with_all_translations(&self) -> serde_json::Value
    where
        Self: serde::Serialize,
    {
        serde_json::to_value(self).unwrap_or(serde_json::json!({}))
    }

    /// Validate that a field is translatable
    fn validate_field(&self, field: &str) -> Result<(), TranslationError> {
        if !Self::translatable_fields().contains(&field) {
            return Err(TranslationError::InvalidField(format!(
                "'{}' is not a translatable field. Available: {:?}",
                field,
                Self::translatable_fields()
            )));
        }
        Ok(())
    }

    /// Validate that a language is allowed
    fn validate_language(&self, lang: &str) -> Result<(), TranslationError> {
        let allowed = Self::allowed_languages();
        if !allowed.iter().any(|l| l == lang) {
            return Err(TranslationError::InvalidLanguage(format!(
                "'{}' is not an allowed language. Allowed: {:?}",
                lang, allowed
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/translations_tests.rs"]
mod tests;
