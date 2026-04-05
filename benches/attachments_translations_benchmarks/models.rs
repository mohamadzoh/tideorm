use super::*;

// =============================================================================
// TEST MODELS (without database dependency)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchProduct {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub price: f64,
    pub translations: Option<serde_json::Value>,
    pub files: Option<serde_json::Value>,
}

impl HasTranslations for BenchProduct {
    fn translatable_fields() -> Vec<&'static str> {
        vec!["name", "description"]
    }

    fn allowed_languages() -> Vec<String> {
        vec![
            "en".to_string(),
            "ar".to_string(),
            "fr".to_string(),
            "es".to_string(),
            "de".to_string(),
        ]
    }

    fn fallback_language() -> String {
        "en".to_string()
    }

    fn get_translations_data(&self) -> Result<TranslationsData, TranslationError> {
        match &self.translations {
            Some(json) => Ok(TranslationsData::from_json(json)),
            None => Ok(TranslationsData::new()),
        }
    }

    fn set_translations_data(&mut self, data: TranslationsData) -> Result<(), TranslationError> {
        self.translations = Some(data.to_json());
        Ok(())
    }

    fn get_default_value(&self, field: &str) -> Result<serde_json::Value, TranslationError> {
        match field {
            "name" => Ok(serde_json::json!(self.name)),
            "description" => Ok(serde_json::json!(self.description)),
            _ => Err(TranslationError::InvalidField(format!(
                "Unknown field: {}",
                field
            ))),
        }
    }
}

impl HasAttachments for BenchProduct {
    fn has_one_files() -> Vec<&'static str> {
        vec!["thumbnail", "cover"]
    }

    fn has_many_files() -> Vec<&'static str> {
        vec!["images", "documents"]
    }

    fn get_files_data(&self) -> Result<FilesData, AttachmentError> {
        match &self.files {
            Some(json) => Ok(FilesData::from_json(json)),
            None => Ok(FilesData::new()),
        }
    }

    fn set_files_data(&mut self, data: FilesData) -> Result<(), AttachmentError> {
        self.files = Some(data.to_json());
        Ok(())
    }
}

impl BenchProduct {
    pub(super) fn new(id: i64) -> Self {
        Self {
            id,
            name: format!("Product {}", id),
            description: format!("Description for product {}", id),
            price: 99.99,
            translations: None,
            files: None,
        }
    }
}
