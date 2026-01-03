//! Translations System
//!
//! This module provides translation functionality for TideORM models,
//! inspired by Laravel Translatable and Rails Globalize patterns.
//!
//! ## Overview
//!
//! Translations allow you to store localized content in a JSONB column with
//! the format: `{field_name: {lang_code: "value", ...}, ...}`
//!
//! Example JSONB structure:
//! ```json
//! {
//!     "name": {"en": "Product Name", "ar": "اسم المنتج"},
//!     "description": {"en": "Description", "ar": "الوصف"}
//! }
//! ```
//!
//! ## Setup
//!
//! Add a `translations` JSONB column to your table and use the `#[tide(translatable)]` attribute:
//!
//! ```rust,ignore
//! #[derive(Model, Clone, Debug, Serialize, Deserialize)]
//! #[tide(table = "products")]
//! #[tide(translatable = "name,description")]
//! pub struct Product {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     
//!     // Default/fallback values stored directly on the model
//!     pub name: String,
//!     pub description: String,
//!     
//!     pub price: f64,
//!     
//!     // JSONB column storing translations
//!     pub translations: Option<Json>,
//! }
//! ```
//!
//! ## Usage
//!
//! ### Setting Translations
//!
//! ```rust,ignore
//! // Set translation for a field
//! product.set_translation("name", "ar", "اسم المنتج")?;
//! product.set_translation("description", "ar", "وصف المنتج")?;
//!
//! // Set multiple translations at once
//! product.set_translations("name", hashmap! {
//!     "en" => "Product Name",
//!     "ar" => "اسم المنتج",
//!     "fr" => "Nom du produit",
//! })?;
//!
//! product.update().await?;
//! ```
//!
//! ### Getting Translations
//!
//! ```rust,ignore
//! // Get translation for specific language
//! let name_ar = product.get_translation("name", "ar")?;
//!
//! // Get translation with fallback chain
//! let name = product.get_translated("name", "ar")?; // Falls back to default if not found
//!
//! // Get all translations for a field
//! let all_names = product.get_all_translations("name")?;
//! ```
//!
//! ### JSON Output with Translations
//!
//! ```rust,ignore
//! // Get model as JSON with translated fields
//! let mut opts = HashMap::new();
//! opts.insert("language".to_string(), "ar".to_string());
//! let json = product.to_json(Some(opts));
//!
//! // Result: {"id": 1, "name": "اسم المنتج", "description": "وصف المنتج", ...}
//! // Note: The translations JSONB column is removed from output
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
        Self { translations: HashMap::new() }
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
        Self { fields: HashMap::new() }
    }
    
    /// Create from JSON value
    pub fn from_json(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Object(map) => {
                let mut fields = HashMap::new();
                for (field_name, field_value) in map {
                    if let serde_json::Value::Object(lang_map) = field_value {
                        let mut translations = HashMap::new();
                        for (lang, trans_value) in lang_map {
                            translations.insert(lang.clone(), trans_value.clone());
                        }
                        fields.insert(field_name.clone(), FieldTranslations { translations });
                    }
                }
                Self { fields }
            }
            _ => Self::new(),
        }
    }
    
    /// Convert to JSON value
    pub fn to_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for (field, trans) in &self.fields {
            let mut lang_map = serde_json::Map::new();
            for (lang, value) in &trans.translations {
                lang_map.insert(lang.clone(), value.clone());
            }
            map.insert(field.clone(), serde_json::Value::Object(lang_map));
        }
        serde_json::Value::Object(map)
    }
    
    /// Get translations for a field
    pub fn get_field(&self, field: &str) -> Option<&FieldTranslations> {
        self.fields.get(field)
    }
    
    /// Get mutable translations for a field
    pub fn get_field_mut(&mut self, field: &str) -> &mut FieldTranslations {
        self.fields.entry(field.to_string()).or_insert_with(FieldTranslations::new)
    }
    
    /// Get translation for a specific field and language
    pub fn get(&self, field: &str, lang: &str) -> Option<&serde_json::Value> {
        self.fields.get(field)?.get(lang)
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
        self.fields.get(field)
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
/// This trait is automatically implemented for models with translation configuration.
/// You can also implement it manually for custom behavior.
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
    
    // =========================================================================
    // SET TRANSLATION METHODS
    // =========================================================================
    
    /// Set a translation for a field in a specific language
    ///
    /// # Example
    /// ```rust,ignore
    /// product.set_translation("name", "ar", "اسم المنتج")?;
    /// product.set_translation("name", "fr", "Nom du produit")?;
    /// product.update().await?;
    /// ```
    fn set_translation(&mut self, field: &str, lang: &str, value: impl Into<serde_json::Value>) -> Result<(), TranslationError> {
        self.validate_field(field)?;
        self.validate_language(lang)?;
        
        let mut data = self.get_translations_data()?;
        data.set(field, lang, value);
        self.set_translations_data(data)
    }
    
    /// Set multiple translations for a field at once
    ///
    /// # Example
    /// ```rust,ignore
    /// let mut names = HashMap::new();
    /// names.insert("en", "Product Name");
    /// names.insert("ar", "اسم المنتج");
    /// names.insert("fr", "Nom du produit");
    /// product.set_translations("name", names)?;
    /// ```
    fn set_translations<V: Into<serde_json::Value>>(&mut self, field: &str, translations: HashMap<&str, V>) -> Result<(), TranslationError> {
        self.validate_field(field)?;
        
        let mut data = self.get_translations_data()?;
        for (lang, value) in translations {
            self.validate_language(lang)?;
            data.set(field, lang, value);
        }
        self.set_translations_data(data)
    }
    
    /// Set all translations for a field from a map (replaces existing)
    ///
    /// # Example
    /// ```rust,ignore
    /// product.sync_translations("name", hashmap! {
    ///     "en" => "New Name",
    ///     "ar" => "اسم جديد",
    /// })?;
    /// ```
    fn sync_translations<V: Into<serde_json::Value>>(&mut self, field: &str, translations: HashMap<&str, V>) -> Result<(), TranslationError> {
        self.validate_field(field)?;
        
        let mut data = self.get_translations_data()?;
        data.remove_field(field);
        for (lang, value) in translations {
            self.validate_language(lang)?;
            data.set(field, lang, value);
        }
        self.set_translations_data(data)
    }
    
    // =========================================================================
    // GET TRANSLATION METHODS
    // =========================================================================
    
    /// Get the translation for a field in a specific language
    ///
    /// Returns `None` if no translation exists for that language.
    ///
    /// # Example
    /// ```rust,ignore
    /// if let Some(name) = product.get_translation("name", "ar")? {
    ///     println!("Arabic name: {}", name);
    /// }
    /// ```
    fn get_translation(&self, field: &str, lang: &str) -> Result<Option<serde_json::Value>, TranslationError> {
        let data = self.get_translations_data()?;
        Ok(data.get(field, lang).cloned())
    }
    
    /// Get the translation with fallback chain
    ///
    /// Tries: requested language -> fallback language -> default field value
    ///
    /// # Example
    /// ```rust,ignore
    /// // If Arabic not available, falls back to English, then to field value
    /// let name = product.get_translated("name", "ar")?;
    /// ```
    fn get_translated(&self, field: &str, lang: &str) -> Result<serde_json::Value, TranslationError> {
        let data = self.get_translations_data()?;
        let fallback = Self::fallback_language();
        
        // Try requested language
        if let Some(value) = data.get(field, lang) {
            return Ok(value.clone());
        }
        
        // Try fallback language
        if lang != fallback {
            if let Some(value) = data.get(field, &fallback) {
                return Ok(value.clone());
            }
        }
        
        // Fall back to default field value
        self.get_default_value(field)
    }
    
    /// Get all translations for a field
    ///
    /// # Example
    /// ```rust,ignore
    /// let all_names = product.get_all_translations("name")?;
    /// for (lang, value) in all_names {
    ///     println!("{}: {}", lang, value);
    /// }
    /// ```
    fn get_all_translations(&self, field: &str) -> Result<HashMap<String, serde_json::Value>, TranslationError> {
        let data = self.get_translations_data()?;
        Ok(data.get_field(field)
            .map(|f| f.all().clone())
            .unwrap_or_default())
    }
    
    /// Get translations for all fields in a specific language
    ///
    /// # Example
    /// ```rust,ignore
    /// let arabic = product.get_translations_for_language("ar")?;
    /// // Returns: {"name": "اسم المنتج", "description": "وصف المنتج"}
    /// ```
    fn get_translations_for_language(&self, lang: &str) -> Result<HashMap<String, serde_json::Value>, TranslationError> {
        let data = self.get_translations_data()?;
        let mut result = HashMap::new();
        
        for field in Self::translatable_fields() {
            if let Some(value) = data.get(field, lang) {
                result.insert(field.to_string(), value.clone());
            }
        }
        
        Ok(result)
    }
    
    // =========================================================================
    // REMOVE TRANSLATION METHODS
    // =========================================================================
    
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
    
    // =========================================================================
    // CHECK METHODS
    // =========================================================================
    
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
        Ok(data.get_field(field)
            .map(|f| f.languages().into_iter().cloned().collect())
            .unwrap_or_default())
    }
    
    // =========================================================================
    // JSON OUTPUT
    // =========================================================================
    
    /// Convert model to JSON with translations applied
    ///
    /// This method:
    /// 1. Serializes the model to JSON
    /// 2. Replaces translatable fields with their translated values
    /// 3. Removes the raw `translations` JSONB column from output
    ///
    /// # Arguments
    /// * `options` - Optional HashMap with:
    ///   - `language`: Language code (e.g., "en", "ar", "fr")
    ///
    /// # Example
    /// ```rust,ignore
    /// // Without options (uses default values)
    /// let json = product.to_translated_json(None);
    ///
    /// // With Arabic translations
    /// let mut opts = HashMap::new();
    /// opts.insert("language".to_string(), "ar".to_string());
    /// let json = product.to_translated_json(Some(opts));
    /// ```
    fn to_translated_json(&self, options: Option<HashMap<String, String>>) -> serde_json::Value
    where
        Self: serde::Serialize,
    {
        let opts = options.unwrap_or_default();
        let fallback = Self::fallback_language();
        let requested_lang = opts.get("language")
            .map(|s| s.as_str())
            .unwrap_or(&fallback);
        
        // Serialize model to JSON
        let mut json = match serde_json::to_value(self) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => return serde_json::json!({}),
        };
        
        // Get translations data
        let translations = json.get("translations")
            .map(|v| TranslationsData::from_json(v))
            .unwrap_or_default();
        
        // Apply translations to translatable fields
        for field in Self::translatable_fields() {
            // Try requested language first
            if let Some(value) = translations.get(field, requested_lang) {
                json.insert(field.to_string(), value.clone());
            } else if requested_lang != fallback {
                // Try fallback language
                if let Some(value) = translations.get(field, &fallback) {
                    json.insert(field.to_string(), value.clone());
                }
                // Otherwise keep the default value already in json
            }
        }
        
        // Remove the raw translations column from output
        json.remove("translations");
        
        serde_json::Value::Object(json)
    }
    
    /// Convert to JSON including all translations
    ///
    /// Useful for admin interfaces or APIs that need to show all translations.
    ///
    /// # Example
    /// ```rust,ignore
    /// let json = product.to_json_with_all_translations();
    /// // Result includes: {"name": "Default", "translations": {"name": {"en": "...", "ar": "..."}}}
    /// ```
    fn to_json_with_all_translations(&self) -> serde_json::Value
    where
        Self: serde::Serialize,
    {
        serde_json::to_value(self).unwrap_or(serde_json::json!({}))
    }
    
    // =========================================================================
    // HELPER METHODS
    // =========================================================================
    
    /// Validate that a field is translatable
    fn validate_field(&self, field: &str) -> Result<(), TranslationError> {
        if !Self::translatable_fields().contains(&field) {
            return Err(TranslationError::InvalidField(
                format!("'{}' is not a translatable field. Available: {:?}", field, Self::translatable_fields())
            ));
        }
        Ok(())
    }
    
    /// Validate that a language is allowed
    fn validate_language(&self, lang: &str) -> Result<(), TranslationError> {
        let allowed = Self::allowed_languages();
        if !allowed.iter().any(|l| l == lang) {
            return Err(TranslationError::InvalidLanguage(
                format!("'{}' is not an allowed language. Allowed: {:?}", lang, allowed)
            ));
        }
        Ok(())
    }
}

/// Helper to create translations from input data
///
/// Use this when processing form data or API requests that include translations.
///
/// # Example
/// ```rust,ignore
/// // Input format: {"name": {"en": "Name", "ar": "اسم"}, "description": {"en": "Desc"}}
/// let translations = TranslationInput::from_json(&input)?;
/// product.apply_translations(translations)?;
/// ```
#[derive(Debug, Clone)]
pub struct TranslationInput {
    /// Field translations
    pub fields: HashMap<String, HashMap<String, serde_json::Value>>,
}

impl TranslationInput {
    /// Create empty input
    pub fn new() -> Self {
        Self { fields: HashMap::new() }
    }
    
    /// Create from JSON value
    ///
    /// Expected format: `{"field": {"lang": "value", ...}, ...}`
    pub fn from_json(value: &serde_json::Value) -> Result<Self, TranslationError> {
        match value {
            serde_json::Value::Object(map) => {
                let mut fields = HashMap::new();
                for (field, trans) in map {
                    if let serde_json::Value::Object(lang_map) = trans {
                        let mut translations = HashMap::new();
                        for (lang, val) in lang_map {
                            translations.insert(lang.clone(), val.clone());
                        }
                        fields.insert(field.clone(), translations);
                    }
                }
                Ok(Self { fields })
            }
            _ => Err(TranslationError::ParseError("Expected JSON object".to_string())),
        }
    }
    
    /// Add a translation
    pub fn add(&mut self, field: &str, lang: &str, value: impl Into<serde_json::Value>) {
        self.fields
            .entry(field.to_string())
            .or_insert_with(HashMap::new)
            .insert(lang.to_string(), value.into());
    }
}

impl Default for TranslationInput {
    fn default() -> Self {
        Self::new()
    }
}

/// Extension trait for applying translation input to models
pub trait ApplyTranslations: HasTranslations {
    /// Apply translations from TranslationInput
    ///
    /// # Example
    /// ```rust,ignore
    /// let input = TranslationInput::from_json(&request_body["translations"])?;
    /// product.apply_translations(input)?;
    /// product.update().await?;
    /// ```
    fn apply_translations(&mut self, input: TranslationInput) -> Result<(), TranslationError> {
        let mut data = self.get_translations_data()?;
        
        for (field, translations) in input.fields {
            for (lang, value) in translations {
                data.set(&field, &lang, value);
            }
        }
        
        self.set_translations_data(data)
    }
}

// Implement ApplyTranslations for all HasTranslations
impl<T: HasTranslations> ApplyTranslations for T {}

/// Errors that can occur during translation operations
#[derive(Debug, Clone)]
pub enum TranslationError {
    /// Invalid or non-translatable field
    InvalidField(String),
    /// Invalid or disallowed language
    InvalidLanguage(String),
    /// Failed to parse translations data
    ParseError(String),
    /// Model doesn't support translations
    NotSupported,
}

impl std::fmt::Display for TranslationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TranslationError::InvalidField(msg) => write!(f, "Invalid field: {}", msg),
            TranslationError::InvalidLanguage(msg) => write!(f, "Invalid language: {}", msg),
            TranslationError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            TranslationError::NotSupported => write!(f, "Model does not support translations"),
        }
    }
}

impl std::error::Error for TranslationError {}

impl From<TranslationError> for crate::Error {
    fn from(err: TranslationError) -> Self {
        crate::Error::query(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_translations_data_basic() {
        let mut data = TranslationsData::new();
        
        data.set("name", "en", "Product");
        data.set("name", "ar", "منتج");
        
        assert_eq!(data.get("name", "en"), Some(&serde_json::json!("Product")));
        assert_eq!(data.get("name", "ar"), Some(&serde_json::json!("منتج")));
        assert_eq!(data.get("name", "fr"), None);
    }
    
    #[test]
    fn test_translations_data_from_json() {
        let json = serde_json::json!({
            "name": {"en": "Product", "ar": "منتج"},
            "description": {"en": "A great product"}
        });
        
        let data = TranslationsData::from_json(&json);
        
        assert_eq!(data.get("name", "en"), Some(&serde_json::json!("Product")));
        assert_eq!(data.get("name", "ar"), Some(&serde_json::json!("منتج")));
        assert_eq!(data.get("description", "en"), Some(&serde_json::json!("A great product")));
    }
    
    #[test]
    fn test_translations_data_to_json() {
        let mut data = TranslationsData::new();
        data.set("name", "en", "Product");
        data.set("name", "ar", "منتج");
        
        let json = data.to_json();
        let expected = serde_json::json!({
            "name": {"en": "Product", "ar": "منتج"}
        });
        
        assert_eq!(json, expected);
    }
    
    #[test]
    fn test_field_translations() {
        let mut field = FieldTranslations::new();
        
        field.set("en", "Hello");
        field.set("ar", "مرحبا");
        
        assert!(field.has("en"));
        assert!(field.has("ar"));
        assert!(!field.has("fr"));
        
        assert_eq!(field.languages().len(), 2);
        
        field.remove("ar");
        assert!(!field.has("ar"));
    }
    
    #[test]
    fn test_translation_input() {
        let mut input = TranslationInput::new();
        input.add("name", "en", "Product");
        input.add("name", "ar", "منتج");
        input.add("description", "en", "Description");
        
        assert_eq!(input.fields.len(), 2);
        assert_eq!(input.fields.get("name").unwrap().len(), 2);
    }
    
    #[test]
    fn test_translation_input_from_json() {
        let json = serde_json::json!({
            "name": {"en": "Product", "ar": "منتج"},
            "description": {"en": "A product"}
        });
        
        let input = TranslationInput::from_json(&json).unwrap();
        
        assert_eq!(input.fields.len(), 2);
        assert_eq!(
            input.fields.get("name").unwrap().get("en"),
            Some(&serde_json::json!("Product"))
        );
    }
}
