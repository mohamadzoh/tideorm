use std::collections::HashMap;

use super::{HasTranslations, TranslationError};

/// Helper to create translations from input data
///
/// Use this when processing form data or API requests that include translations.
#[derive(Debug, Clone)]
pub struct TranslationInput {
    /// Field translations
    pub fields: HashMap<String, HashMap<String, serde_json::Value>>,
}

impl TranslationInput {
    /// Create empty input
    pub fn new() -> Self {
        Self {
            fields: HashMap::new(),
        }
    }

    /// Create from JSON value
    ///
    /// Expected format: `{"field": {"lang": "value", ...}, ...}`
    ///
    /// This only reshapes the payload; field and language keys are not checked
    /// here because the allowlists live on the model. `ApplyTranslations::apply_translations`
    /// performs that validation before anything is stored.
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
            _ => Err(TranslationError::ParseError(
                "Expected JSON object".to_string(),
            )),
        }
    }

    /// Add a translation
    pub fn add(&mut self, field: &str, lang: &str, value: impl Into<serde_json::Value>) {
        self.fields
            .entry(field.to_string())
            .or_default()
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
    /// Apply a batch of parsed translations to the model payload.
    ///
    /// Every field is checked against `translatable_fields()` and every language
    /// against `allowed_languages()`, exactly like `set_translation()`. This
    /// matters because `TranslationInput::from_json` is the documented entry
    /// point for API and form payloads, so the keys are usually attacker
    /// controlled.
    ///
    /// Nothing is written when any key is rejected: the payload is only stored
    /// once the whole batch validates.
    fn apply_translations(&mut self, input: TranslationInput) -> Result<(), TranslationError> {
        let mut data = self.get_translations_data()?;

        for (field, translations) in input.fields {
            self.validate_field(&field)?;
            for (lang, value) in translations {
                self.validate_language(&lang)?;
                data.set(&field, &lang, value);
            }
        }

        self.set_translations_data(data)
    }
}

impl<T: HasTranslations> ApplyTranslations for T {}
