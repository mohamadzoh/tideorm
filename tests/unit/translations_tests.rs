use super::*;

#[test]
fn test_translations_data_round_trips_through_json() {
    let mut data = TranslationsData::new();
    data.set("name", "en", "Product");
    data.set("name", "ar", "منتج");
    data.set("description", "en", "A great product");

    let json = serde_json::json!({
        "name": {"en": "Product", "ar": "منتج"},
        "description": {"en": "A great product"}
    });
    assert_eq!(data.to_json(), json);

    let data = TranslationsData::from_json(&json);
    assert_eq!(data.get("name", "en"), Some(&serde_json::json!("Product")));
    assert_eq!(data.get("name", "ar"), Some(&serde_json::json!("منتج")));
    assert_eq!(
        data.get("description", "en"),
        Some(&serde_json::json!("A great product"))
    );
    assert_eq!(data.get("name", "fr"), None);
}

#[test]
fn test_translations_data_from_json_skips_only_malformed_fields() {
    let json = serde_json::json!({
        "name": {"en": "Product"},
        "legacy": "not a language map",
        "empty": null
    });

    let data = TranslationsData::from_json(&json);

    assert_eq!(data.get("name", "en"), Some(&serde_json::json!("Product")));
    assert_eq!(data.fields().len(), 1);
    assert_eq!(
        data.to_json(),
        serde_json::json!({"name": {"en": "Product"}})
    );
    assert!(
        TranslationsData::from_json(&serde_json::json!([1, 2]))
            .fields()
            .is_empty()
    );
}

#[test]
fn test_translation_input_from_json_rejects_a_non_object() {
    let error = TranslationInput::from_json(&serde_json::json!("name"))
        .expect_err("a payload that is not an object cannot carry translations");
    assert!(matches!(error, TranslationError::ParseError(_)));
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
fn test_translation_lookups_fall_back_to_the_fallback_language_then_the_default() {
    let mut model = TranslatableProbe::default();
    model.set_translation("name", "en", "Product").unwrap();

    assert_eq!(
        model.get_translated("name", "ar").unwrap(),
        serde_json::json!("Product")
    );

    model.set_translation("name", "ar", "منتج").unwrap();
    assert_eq!(
        model.get_translated("name", "ar").unwrap(),
        serde_json::json!("منتج")
    );

    model.clear_translations().unwrap();
    assert_eq!(
        model.get_translated("name", "ar").unwrap(),
        serde_json::Value::Null
    );
}

/// Attacker-chosen field and language keys never reach the translations
/// payload, and a partly invalid batch is rejected as a whole.
#[test]
fn test_apply_translations_stores_nothing_from_an_invalid_batch() {
    type IsExpected = fn(&TranslationError) -> bool;
    let invalid_field: IsExpected = |error| matches!(error, TranslationError::InvalidField(_));
    let invalid_language: IsExpected =
        |error| matches!(error, TranslationError::InvalidLanguage(_));
    for (entries, expected) in [
        (vec![("evil_field", "en")], invalid_field),
        (vec![("name", "zz-junk")], invalid_language),
        (vec![("name", "en"), ("name", "zz-junk")], invalid_language),
    ] {
        let mut model = TranslatableProbe::default();
        let mut input = TranslationInput::new();
        for (field, language) in &entries {
            input.add(field, language, "value");
        }

        let error = model
            .apply_translations(input)
            .expect_err("the batch must be rejected");

        assert!(expected(&error), "{entries:?}: {error:?}");
        assert_eq!(model.translations, serde_json::Value::Null, "{entries:?}");
    }
}

#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RenamedTranslatableProbe {
    meta_title: String,
    translations: serde_json::Value,
}

impl HasTranslations for RenamedTranslatableProbe {
    fn translatable_fields() -> Vec<&'static str> {
        vec!["meta_title"]
    }

    fn allowed_languages() -> Vec<String> {
        vec!["en".to_string()]
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
        Ok(serde_json::json!(self.meta_title))
    }

    fn serialized_key(field: &'static str) -> &'static str {
        match field {
            "meta_title" => "metaTitle",
            other => other,
        }
    }
}

#[test]
fn translated_json_writes_the_translation_under_the_serialized_key() {
    let mut model = RenamedTranslatableProbe {
        meta_title: "Default".to_string(),
        ..Default::default()
    };
    model
        .set_translation("meta_title", "en", "Translated")
        .unwrap();

    let json = model.to_translated_json(None);

    assert_eq!(json["metaTitle"], "Translated");
    assert!(json.get("meta_title").is_none(), "{json}");
    assert!(json.get("translations").is_none(), "{json}");
}

#[derive(serde::Serialize)]
struct FailingTranslationAccessor {}
impl HasTranslations for FailingTranslationAccessor {
    fn fallback_language() -> String {
        "en".into()
    }
    fn translatable_fields() -> Vec<&'static str> {
        vec!["name"]
    }
    fn allowed_languages() -> Vec<String> {
        vec!["en".into()]
    }
    fn get_translations_data(&self) -> Result<TranslationsData, TranslationError> {
        Err(TranslationError::ParseError("bad data".into()))
    }
    fn set_translations_data(&mut self, _: TranslationsData) -> Result<(), TranslationError> {
        Ok(())
    }
    fn get_default_value(&self, _: &str) -> Result<serde_json::Value, TranslationError> {
        Ok(serde_json::Value::Null)
    }
}
#[test]
fn fallible_translation_rendering_reports_accessor_errors() {
    assert!(
        FailingTranslationAccessor {}
            .try_to_translated_json(None)
            .unwrap_err()
            .to_string()
            .contains("bad data")
    );
}
