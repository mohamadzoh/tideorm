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