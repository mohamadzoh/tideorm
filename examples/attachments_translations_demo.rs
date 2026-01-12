//! File Attachments and Translations Demo
//!
//! This example demonstrates how to use TideORM's file attachments
//! and translations features together.
//!
//! Run with: cargo run --example attachments_translations_demo

use tideorm::prelude::*;
use std::collections::HashMap;

// ============================================================================
// MODEL DEFINITION
// ============================================================================

/// Product model with translations and file attachments
/// 
/// Database table should have:
/// - `id` - Primary key
/// - `name` - Default name (fallback)
/// - `description` - Default description (fallback)
/// - `price` - Decimal price
/// - `translations` - JSONB column for translations
/// - `files` - JSONB column for file attachments
/// 
/// Note: In a real application, use the derive macro attributes:
/// #[derive(Model, Clone, Debug, Serialize, Deserialize)]
/// #[tide(table = "products")]
/// #[tide(translatable = "name,description")]
/// #[tide(has_one_files = "thumbnail")]
/// #[tide(has_many_files = "images,documents")]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Product {
    /// Primary key
    pub id: i64,
    
    /// Default product name (fallback when translation not available)
    pub name: String,
    
    /// Default description (fallback when translation not available)
    pub description: String,
    
    /// Product price
    pub price: f64,
    
    /// Translations stored as JSONB
    /// Format: {"name": {"en": "...", "ar": "..."}, "description": {"en": "...", "ar": "..."}}
    pub translations: Option<Json>,
    
    /// File attachments stored as JSONB
    /// Format: {"thumbnail": {...}, "images": [...], "documents": [...]}
    pub files: Option<Json>,
}

// ============================================================================
// IMPLEMENT HasTranslations TRAIT
// ============================================================================

impl HasTranslations for Product {
    fn translatable_fields() -> Vec<&'static str> {
        vec!["name", "description"]
    }
    
    fn allowed_languages() -> Vec<String> {
        vec!["en".to_string(), "ar".to_string(), "fr".to_string()]
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
            _ => Err(TranslationError::InvalidField(format!("Unknown field: {}", field))),
        }
    }
}

// ============================================================================
// IMPLEMENT HasAttachments TRAIT
// ============================================================================

impl HasAttachments for Product {
    fn has_one_files() -> Vec<&'static str> {
        vec!["thumbnail"]
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

// ============================================================================
// MAIN DEMO
// ============================================================================

fn main() {
    println!("=== TideORM Attachments & Translations Demo ===\n");
    
    // Create a new product with defaults
    let mut product = Product {
        id: 1,
        name: "Wireless Headphones".to_string(),
        description: "High-quality wireless headphones with noise cancellation".to_string(),
        price: 199.99,
        translations: None,
        files: None,
    };
    
    // ========================================================================
    // TRANSLATIONS DEMO
    // ========================================================================
    
    println!("--- Setting Translations ---\n");
    
    // Set individual translations
    product.set_translation("name", "ar", "سماعات لاسلكية").unwrap();
    product.set_translation("name", "fr", "Écouteurs sans fil").unwrap();
    
    product.set_translation("description", "ar", "سماعات لاسلكية عالية الجودة مع إلغاء الضوضاء").unwrap();
    product.set_translation("description", "fr", "Écouteurs sans fil de haute qualité avec réduction de bruit").unwrap();
    
    // Or set multiple at once
    let mut name_translations = HashMap::new();
    name_translations.insert("en", "Wireless Headphones Pro");
    // product.set_translations("name", name_translations).unwrap();
    
    println!("Translations set successfully!\n");
    
    // Get translations
    println!("--- Getting Translations ---\n");
    
    let name_en = product.get_translated("name", "en").unwrap();
    let name_ar = product.get_translated("name", "ar").unwrap();
    let name_fr = product.get_translated("name", "fr").unwrap();
    
    println!("Name (EN): {}", name_en);
    println!("Name (AR): {}", name_ar);
    println!("Name (FR): {}", name_fr);
    println!();
    
    // Get all translations for a field
    let all_names = product.get_all_translations("name").unwrap();
    println!("All name translations: {:?}", all_names);
    println!();
    
    // Check available languages
    let langs = product.available_languages("name").unwrap();
    println!("Available languages for 'name': {:?}", langs);
    println!();
    
    // ========================================================================
    // FILE ATTACHMENTS DEMO
    // ========================================================================
    
    println!("--- Attaching Files ---\n");
    
    // Attach a thumbnail (hasOne)
    product.attach("thumbnail", "uploads/products/1/thumb.jpg").unwrap();
    println!("Thumbnail attached!");
    
    // Attach multiple images (hasMany)
    product.attach("images", "uploads/products/1/img1.jpg").unwrap();
    product.attach("images", "uploads/products/1/img2.jpg").unwrap();
    product.attach("images", "uploads/products/1/img3.jpg").unwrap();
    println!("Images attached!");
    
    // Or attach many at once
    product.attach_many("documents", vec![
        "uploads/products/1/manual.pdf",
        "uploads/products/1/warranty.pdf",
    ]).unwrap();
    println!("Documents attached!\n");
    
    // Get files
    println!("--- Getting Files ---\n");
    
    if let Some(thumb) = product.get_file("thumbnail").unwrap() {
        println!("Thumbnail: {} ({})", thumb.filename, thumb.key);
    }
    
    let images = product.get_files("images").unwrap();
    println!("Images ({}):", images.len());
    for img in &images {
        println!("  - {}", img.filename);
    }
    
    let docs = product.get_files("documents").unwrap();
    println!("Documents ({}):", docs.len());
    for doc in &docs {
        println!("  - {}", doc.filename);
    }
    println!();
    
    // Detach a specific file
    println!("--- Detaching Files ---\n");
    product.detach("images", Some("uploads/products/1/img2.jpg")).unwrap();
    println!("Removed img2.jpg");
    
    let images = product.get_files("images").unwrap();
    println!("Remaining images: {:?}", images.iter().map(|i| &i.filename).collect::<Vec<_>>());
    println!();
    
    // Sync files (replace all)
    println!("--- Syncing Files ---\n");
    product.sync("images", vec![
        "uploads/products/1/new1.jpg",
        "uploads/products/1/new2.jpg",
    ]).unwrap();
    println!("Images synced to new files");
    
    let images = product.get_files("images").unwrap();
    println!("New images: {:?}", images.iter().map(|i| &i.filename).collect::<Vec<_>>());
    println!();
    
    // ========================================================================
    // JSON OUTPUT DEMO
    // ========================================================================
    
    println!("--- JSON Output ---\n");
    
    // Default JSON (uses fallback language)
    println!("Default JSON (no language specified):");
    let json = product.to_translated_json(None);
    println!("{}\n", serde_json::to_string_pretty(&json).unwrap());
    
    // Arabic JSON
    println!("Arabic JSON:");
    let mut opts = HashMap::new();
    opts.insert("language".to_string(), "ar".to_string());
    let json_ar = product.to_translated_json(Some(opts));
    println!("{}\n", serde_json::to_string_pretty(&json_ar).unwrap());
    
    // French JSON  
    println!("French JSON:");
    let mut opts = HashMap::new();
    opts.insert("language".to_string(), "fr".to_string());
    let json_fr = product.to_translated_json(Some(opts));
    println!("{}\n", serde_json::to_string_pretty(&json_fr).unwrap());
    
    // Full JSON with all translations (for admin)
    println!("Full JSON with all translations:");
    let full_json = product.to_json_with_all_translations();
    println!("{}\n", serde_json::to_string_pretty(&full_json).unwrap());
    
    // ========================================================================
    // TRANSLATION INPUT DEMO
    // ========================================================================
    
    println!("--- Translation Input (from API request) ---\n");
    
    // Simulate receiving translation data from an API request
    let api_translations = serde_json::json!({
        "name": {
            "en": "Updated Headphones",
            "ar": "سماعات محدثة",
            "fr": "Écouteurs mis à jour"
        },
        "description": {
            "en": "New and improved description",
            "ar": "وصف جديد ومحسن"
        }
    });
    
    let input = TranslationInput::from_json(&api_translations).unwrap();
    product.apply_translations(input).unwrap();
    println!("Applied translations from API request");
    
    let updated_name = product.get_translated("name", "ar").unwrap();
    println!("Updated Arabic name: {}\n", updated_name);
    
    println!("=== Demo Complete ===");
}

// ============================================================================
// ASYNC EXAMPLE (for real database usage)
// ============================================================================

/// Example of using with a real database (not run in this demo)
/// 
/// To use with a real database, you would:
/// 1. Derive the Model trait on your struct
/// 2. Initialize the database connection
/// 3. Use the async model methods like save(), find(), update(), etc.
#[allow(dead_code)]
async fn async_example() -> tideorm::Result<()> {
    // Initialize database
    TideConfig::init()
        .database("postgres://localhost/myapp")
        .languages(&["en", "ar", "fr"])
        .fallback_language("en")
        .connect()
        .await?;
    
    // In a real app with #[tideorm::model], you would:
    // let mut product = Product { ... };
    // product.set_translation("name", "ar", "اسم عربي")?;
    // product.attach("thumbnail", "uploads/thumb.jpg")?;
    // let product = product.save().await?;
    // 
    // if let Some(found) = Product::find(product.id).await? {
    //     let mut opts = HashMap::new();
    //     opts.insert("language".to_string(), "ar".to_string());
    //     let json = found.to_translated_json(Some(opts));
    //     println!("Found product: {}", json);
    // }
    
    println!("Database example - see comments for usage");
    
    Ok(())
}
