# Translations

Requires the `translations` feature.

Translations keep per-language values of a model's fields in a JSON column. The struct's own field holds the default value, and the translations override it per language.

```toml
[dependencies]
tideorm = { version = "0.17.8", default-features = false, features = ["postgres", "runtime-tokio", "translations"] }
```

## Model Setup

List the translatable fields on the model, and implement `HasTranslations` to say which column holds the translations:

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "products", translatable = "name,description")]
pub struct Product {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,

    // Default values
    pub name: String,
    pub description: String,

    pub price: f64,

    // JSON or JSONB column holding the translations
    pub translations: Option<JsonValue>,
}

impl HasTranslations for Product {
    fn translatable_fields() -> Vec<&'static str> {
        vec!["name", "description"]
    }

    fn allowed_languages() -> Vec<String> {
        vec!["en".to_string(), "ar".to_string(), "fr".to_string(), "de".to_string()]
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
            _ => Err(TranslationError::InvalidField(format!("Unknown field: {field}"))),
        }
    }
}
```

The `translatable` attribute records the fields on the model, which `to_json()` uses. The derive does not generate `HasTranslations`, because only you know which column holds the payload. Once your impl exists, the methods below are available on `Product` with `tideorm::prelude::*` in scope. `get_translated()` falls back to what your `get_default_value()` returns.

Name the column field `translations`: `Model::to_json()` reads the translations from the field named `translations`.

### Languages

`HasTranslations` declares its own `allowed_languages()` and `fallback_language()`, so a plain struct can implement it with a fixed policy. A model also has languages of its own, which `Model::to_json()` uses:

- the model attributes `languages = "en,ar,fr"` and `fallback_language = "en"`, or else
- the configured `TideConfig::languages(["en", "ar"])` and `TideConfig::fallback_language("en")`, whose defaults are `["en"]` and `"en"`.

To keep one policy, return the model's from your impl, as `<Self as ModelMeta>::allowed_languages()` and `<Self as ModelMeta>::fallback_language()`; the fully qualified form is needed because both traits name these methods.

## Setting Translations

```rust
product.set_translation("name", "ar", "اسم المنتج")?;
product.set_translation("name", "fr", "Nom du produit")?;
product.set_translation("description", "ar", "وصف المنتج")?;

// Several languages of one field: a HashMap or any (language, value) pairs
product.set_translations("name", [
    ("en", "Product Name"),
    ("ar", "اسم المنتج"),
    ("fr", "Nom du produit"),
])?;

// Replace every translation of a field
product.sync_translations("name", [("en", "New Product Name"), ("de", "Neuer Produktname")])?;

product.update().await?;
```

Every method changes the model in memory only; save the model afterwards. A field that is not translatable is a `TranslationError::InvalidField`, and a language that is not allowed a `TranslationError::InvalidLanguage`; `set_translations` and `sync_translations` set nothing when either is rejected.

### From a Request

`TranslationInput` reads a request or form payload shaped `{"field": {"lang": "value"}}`, and `apply_translations` stores it:

```rust
let input = TranslationInput::from_json(&body["translations"])?;
product.apply_translations(input)?;
product.update().await?;
```

`from_json` only reshapes the payload (anything other than a JSON object is a `TranslationError::ParseError`). `apply_translations` checks every field against `translatable_fields()` and every language against `allowed_languages()`, as `set_translation` does, and writes nothing unless the whole batch passes, since such keys usually come from a client. Build one by hand with `TranslationInput::new()` and `add(field, lang, value)`.

## Reading Translations

```rust
// One stored translation, or None
if let Some(name) = product.get_translation("name", "ar")? {
    println!("Arabic name: {name}");
}

// With the fallback chain: requested language, fallback language, default value
let name = product.get_translated("name", "ar")?;

// Every language of one field
for (lang, value) in product.get_all_translations("name")? {
    println!("{lang}: {value}");
}

// Every field in one language: {"name": "اسم المنتج", "description": "وصف المنتج"}
let arabic = product.get_translations_for_language("ar")?;
```

Values are `serde_json::Value`s.

## Checking Translations

```rust
if product.has_translation("name", "ar")? {
    println!("Arabic name available");
}

if product.has_any_translation("name")? {
    println!("Name has translations");
}

let languages = product.available_languages("name")?;
println!("Name available in: {languages:?}");
```

`validate_field(field)` and `validate_language(lang)` run the same checks the setters do.

## Removing Translations

```rust
product.remove_translation("name", "fr")?;     // one language of one field
product.remove_field_translations("name")?;    // every language of one field
product.clear_translations()?;                 // everything

product.update().await?;
```

## JSON Output

```rust
// Translatable fields in Arabic; the raw translations column is removed
let json = product.to_translated_json_in("ar");
// {"id": 1, "name": "اسم المنتج", "description": "وصف المنتج", "price": 99.99}

// In the fallback language, or the one an options map names under "language"
let json = product.to_translated_json(None);

// Model::to_json with its translatable fields in one language, with hidden
// attributes and attachment URLs applied as for any to_json
let json = product.to_json_in("ar");

// The whole model, raw translations included (for admin interfaces)
let json = product.to_json_with_all_translations();
```

A field with no translation in the requested language takes the fallback language's, and then keeps the model's own value. The plain forms log a failure and return `{}`; `try_to_translated_json`, `try_to_translated_json_in`, `try_to_json_with_all_translations` and `Model::try_to_json_in` return it instead.

`Model::to_json()` itself renders translatable fields in the language its options name (`"language"`), else in the model's fallback language, and removes the raw `translations` column. A malformed field in the stored payload is skipped rather than failing the whole model, and a field that is both hidden and translatable stays hidden.

If your model's serde derive renames fields (`rename_all = "camelCase"`), tell `HasTranslations` where each field lands, so the translation replaces it there:

```rust
fn serialized_key(field: &'static str) -> &'static str {
    <Self as ModelMeta>::serialized_name(field)
}
```

There is no `load_all_translations()` that writes every translation onto the struct's fields; it was removed because it was lossy. Use `get_all_translations()` per field, `get_translations_for_language()` per language, or `to_json_with_all_translations()` for the full payload.

## Errors

`TranslationError` has three variants, `InvalidField`, `InvalidLanguage` and `ParseError`, and is `#[non_exhaustive]`, so match it with a wildcard arm. It converts into `tideorm::Error` with `?`: a rejected field or language becomes a validation error naming `field` or `language`, and a parse failure a conversion error.

## Storage Format

The column holds one JSON object, a member per field, each holding a member per language:

```json
{
  "name": {
    "en": "Wireless Headphones",
    "ar": "سماعات لاسلكية",
    "fr": "Écouteurs sans fil"
  },
  "description": {
    "en": "High-quality wireless headphones",
    "ar": "سماعات لاسلكية عالية الجودة",
    "fr": "Écouteurs sans fil de haute qualité"
  }
}
```

`TranslationsData::from_json` skips a member that is not such an object, so one malformed field cannot hide, or once saved back erase, the others. `TranslationsData` and `FieldTranslations` also offer direct access (`get`, `set`, `remove`, `remove_field`, `has_translations`, `fields`; `all`, `has`, `languages`).

To use translations together with file attachments, see [File Attachments](attachments.md).
