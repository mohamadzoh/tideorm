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
    assert_eq!(
        data.get("description", "en"),
        Some(&serde_json::json!("A great product"))
    );
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

#[derive(Default)]
struct TranslatableProbe {
    translations: serde_json::Value,
}

impl HasTranslations for TranslatableProbe {
    fn translatable_fields() -> Vec<&'static str> {
        vec!["name"]
    }

    fn allowed_languages() -> Vec<String> {
        vec!["en".to_string(), "ar".to_string()]
    }

    fn fallback_language() -> String {
        "en".to_string()
    }

    fn get_translations_data(&self) -> Result<TranslationsData, TranslationError> {
        Ok(TranslationsData::from_json(&self.translations))
    }

    fn set_translations_data(&mut self, data: TranslationsData) -> Result<(), TranslationError> {
        self.translations = data.to_json();
        Ok(())
    }

    fn get_default_value(&self, _field: &str) -> Result<serde_json::Value, TranslationError> {
        Ok(serde_json::Value::Null)
    }
}

#[test]
fn test_apply_translations_accepts_allowed_field_and_language() {
    let mut model = TranslatableProbe::default();
    let mut input = TranslationInput::new();
    input.add("name", "en", "Product");
    input.add("name", "ar", "منتج");

    model.apply_translations(input).unwrap();

    assert_eq!(
        model.get_translation("name", "ar").unwrap(),
        Some(serde_json::json!("منتج"))
    );
}

#[test]
fn test_apply_translations_rejects_untranslatable_field() {
    let mut model = TranslatableProbe::default();
    let mut input = TranslationInput::new();
    input.add("evil_field", "en", "junk");

    let error = model
        .apply_translations(input)
        .expect_err("attacker-chosen field keys must not reach the translations payload");

    assert!(matches!(error, TranslationError::InvalidField(_)));
    assert_eq!(model.translations, serde_json::Value::Null);
}

#[test]
fn test_apply_translations_rejects_disallowed_language() {
    let mut model = TranslatableProbe::default();
    let mut input = TranslationInput::new();
    input.add("name", "zz-junk", "junk");

    let error = model
        .apply_translations(input)
        .expect_err("attacker-chosen language keys must not reach the translations payload");

    assert!(matches!(error, TranslationError::InvalidLanguage(_)));
    assert_eq!(model.translations, serde_json::Value::Null);
}

#[test]
fn test_apply_translations_stores_nothing_when_part_of_the_batch_is_invalid() {
    let mut model = TranslatableProbe::default();
    let mut input = TranslationInput::new();
    input.add("name", "en", "Product");
    input.add("name", "zz-junk", "junk");

    model
        .apply_translations(input)
        .expect_err("a partially invalid batch must be rejected as a whole");

    assert_eq!(model.translations, serde_json::Value::Null);
}
