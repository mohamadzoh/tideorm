//! File Attachment URL Generation Demo
//!
//! This example demonstrates the file attachment URL generation feature in TideORM.
//! It shows how to:
//! - Configure a global file base URL
//! - Use a custom URL generator function with field name and full file metadata
//! - Override URL generation per-model
//! - Access generated URLs in JSON output
//! - Generate different URLs based on field name and file properties
//!
//! Run with: cargo run --example attachment_url_demo

use serde::{Deserialize, Serialize};

// ============================================================================
// HELPER MACROS FOR TESTING
// ============================================================================

macro_rules! test_section {
    ($name:expr) => {
        println!("\n{}", "=".repeat(70));
        println!("📋 {}", $name);
        println!("{}", "=".repeat(70));
    };
}

macro_rules! test_case {
    ($name:expr) => {
        println!("\n  🧪 {}", $name);
        println!("  {}", "-".repeat(60));
    };
}

macro_rules! verify {
    ($condition:expr, $msg:expr) => {
        if $condition {
            println!("    ✅ {}", $msg);
        } else {
            println!("    ❌ FAILED: {}", $msg);
            panic!("Verification failed: {}", $msg);
        }
    };
}

macro_rules! verify_eq {
    ($actual:expr, $expected:expr, $msg:expr) => {
        if $actual == $expected {
            println!("    ✅ {} (value: {:?})", $msg, $actual);
        } else {
            println!("    ❌ FAILED: {}", $msg);
            println!("       Expected: {:?}", $expected);
            println!("       Actual:   {:?}", $actual);
            panic!("Verification failed: {}", $msg);
        }
    };
}

// ============================================================================
// FILE ATTACHMENT STRUCT (mirrors TideORM's FileAttachment)
// ============================================================================

/// File attachment metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAttachment {
    pub key: String,
    pub filename: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

impl FileAttachment {
    pub fn new(key: &str) -> Self {
        let filename = key.split('/').next_back().unwrap_or(key).to_string();
        Self {
            key: key.to_string(),
            filename,
            created_at: chrono::Utc::now().to_rfc3339(),
            original_filename: None,
            size: None,
            mime_type: None,
        }
    }
    
    pub fn with_metadata(key: &str, original_filename: Option<&str>, size: Option<u64>, mime_type: Option<&str>) -> Self {
        let mut attachment = Self::new(key);
        attachment.original_filename = original_filename.map(|s| s.to_string());
        attachment.size = size;
        attachment.mime_type = mime_type.map(|s| s.to_string());
        attachment
    }
}

/// File URL generator function type (takes field_name and full FileAttachment)
type FileUrlGenerator = fn(field_name: &str, file: &FileAttachment) -> String;

/// Simulated global configuration
struct MockConfig {
    file_base_url: Option<String>,
    custom_generator: Option<FileUrlGenerator>,
}

impl MockConfig {
    fn new() -> Self {
        Self {
            file_base_url: None,
            custom_generator: None,
        }
    }
    
    fn with_base_url(mut self, url: &str) -> Self {
        self.file_base_url = Some(url.to_string());
        self
    }
    
    #[allow(dead_code)]
    fn with_generator(mut self, generator: FileUrlGenerator) -> Self {
        self.custom_generator = Some(generator);
        self
    }
    
    #[inline]
    fn generate_url(&self, field_name: &str, file: &FileAttachment) -> String {
        if let Some(generator) = self.custom_generator {
            generator(field_name, file)
        } else if let Some(base_url) = &self.file_base_url {
            let base = base_url.trim_end_matches('/');
            let key = file.key.trim_start_matches('/');
            format!("{}/{}", base, key)
        } else {
            file.key.clone()
        }
    }
}

/// Simulate processing file data for JSON output
#[inline]
fn process_file_for_json(
    field_name: &str,
    file_data: &serde_json::Value,
    hidden_attrs: &[&str],
    url_generator: FileUrlGenerator,
) -> serde_json::Value {
    match file_data {
        serde_json::Value::Object(obj) => {
            let mut cleaned = serde_json::Map::new();
            for (key, value) in obj {
                if !hidden_attrs.contains(&key.as_str()) {
                    cleaned.insert(key.clone(), value.clone());
                }
            }
            // Add URL field by deserializing the file attachment
            if let Ok(file_attachment) = serde_json::from_value::<FileAttachment>(serde_json::Value::Object(obj.clone())) {
                let url = url_generator(field_name, &file_attachment);
                cleaned.insert("url".to_string(), serde_json::Value::String(url));
            }
            serde_json::Value::Object(cleaned)
        }
        serde_json::Value::Array(arr) => {
            let cleaned: Vec<serde_json::Value> = arr
                .iter()
                .map(|item| process_file_for_json(field_name, item, hidden_attrs, url_generator))
                .collect();
            serde_json::Value::Array(cleaned)
        }
        other => other.clone(),
    }
}

// ============================================================================
// CUSTOM URL GENERATORS (Now with field_name AND full file metadata access)
// ============================================================================

/// Simple CDN URL generator (ignores field_name)
fn cdn_url_generator(_field_name: &str, file: &FileAttachment) -> String {
    format!("https://cdn.example.com/{}", file.key.trim_start_matches('/'))
}

/// S3 URL generator with bucket
fn s3_url_generator(_field_name: &str, file: &FileAttachment) -> String {
    format!("https://my-bucket.s3.amazonaws.com/{}", file.key.trim_start_matches('/'))
}

/// Field-aware URL generator - routes based on field name
fn field_aware_url_generator(field_name: &str, file: &FileAttachment) -> String {
    let key = file.key.trim_start_matches('/');
    
    match field_name {
        "thumbnail" | "avatar" | "profile_image" => {
            // Images go to image-specific CDN with transformations
            format!("https://images.example.com/w_auto,f_auto/{}", key)
        }
        "video" | "preview_video" => {
            // Videos go to streaming CDN
            format!("https://stream.example.com/{}", key)
        }
        "document" | "attachment" | "resume" => {
            // Documents go to secure document storage
            format!("https://docs.example.com/secure/{}", key)
        }
        "cover_image" | "banner" => {
            // Large images optimized for display
            format!("https://images.example.com/w_1200,q_80/{}", key)
        }
        _ => {
            // Default: general CDN
            format!("https://cdn.example.com/{}", key)
        }
    }
}

/// Combined field + metadata URL generator
fn smart_url_generator(field_name: &str, file: &FileAttachment) -> String {
    let key = file.key.trim_start_matches('/');
    
    // First, check field-specific routing
    match field_name {
        "thumbnail" => {
            // Thumbnails: Apply quality based on size
            let quality = if file.size.unwrap_or(0) > 500_000 { "60" } else { "auto" };
            return format!("https://thumbs.example.com/q_{}/{}", quality, key);
        }
        "avatar" => {
            // Avatars: Always compressed, square format
            return format!("https://avatars.example.com/w_200,h_200,c_fill/{}", key);
        }
        _ => {}
    }
    
    // Then, check mime_type for type-specific routing
    match file.mime_type.as_deref() {
        Some(m) if m.starts_with("video/") => {
            format!("https://stream.example.com/{}", key)
        }
        Some(m) if m.starts_with("image/") => {
            let quality = if file.size.unwrap_or(0) > 1_000_000 { "80" } else { "auto" };
            format!("https://images.example.com/q_{}/{}", quality, key)
        }
        Some(m) if m == "application/pdf" => {
            format!("https://docs.example.com/{}", key)
        }
        _ => {
            format!("https://cdn.example.com/{}", key)
        }
    }
}

/// Product model URL generator
fn product_url_generator(field_name: &str, file: &FileAttachment) -> String {
    let key = file.key.trim_start_matches('/');
    
    match field_name {
        "thumbnail" => format!("https://products-cdn.example.com/thumb/{}", key),
        "gallery" => format!("https://products-cdn.example.com/gallery/{}", key),
        "manual" => format!("https://products-cdn.example.com/docs/{}", key),
        _ => format!("https://products-cdn.example.com/assets/{}", key),
    }
}

/// User model URL generator
fn user_url_generator(field_name: &str, file: &FileAttachment) -> String {
    let key = file.key.trim_start_matches('/');
    
    match field_name {
        "avatar" => format!("https://avatars.example.com/{}", key),
        "cover_photo" => format!("https://covers.example.com/{}", key),
        "resume" => format!("https://secure-docs.example.com/resumes/{}", key),
        _ => format!("https://user-files.example.com/{}", key),
    }
}

/// Signed URL generator with field-specific tokens
fn signed_url_generator(field_name: &str, file: &FileAttachment) -> String {
    let key = file.key.trim_start_matches('/');
    
    // Generate token based on field and file
    let access_level = match field_name {
        "private_document" | "secure_file" => "restricted",
        "internal_only" => "internal",
        _ => "public",
    };
    
    let size_hash = file.size.unwrap_or(0) % 1000;
    let token = format!("{}_{:03}", access_level, size_hash);
    
    format!(
        "https://secure-cdn.example.com/{}?token={}&expires=3600",
        key, token
    )
}

fn main() {
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║  TideORM File Attachment URL Generation Demo (Field Name + Metadata) ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");
    
    let mut passed_tests = 0;
    let mut total_tests = 0;
    
    // ========================================================================
    // TEST 1: Default URL Generation (No base URL)
    // ========================================================================
    test_section!("TEST 1: Default URL Generation (No Base URL Configured)");
    
    test_case!("1.1: URL without base URL returns key as-is");
    {
        let config = MockConfig::new();
        let file = FileAttachment::new("uploads/2024/image.jpg");
        let url = config.generate_url("thumbnail", &file);
        verify_eq!(url, "uploads/2024/image.jpg", "URL equals key when no base URL");
        total_tests += 1;
        passed_tests += 1;
    }
    
    test_case!("1.2: File with metadata - URL still works");
    {
        let config = MockConfig::new();
        let file = FileAttachment::with_metadata(
            "uploads/doc.pdf",
            Some("Report.pdf"),
            Some(1024),
            Some("application/pdf"),
        );
        let url = config.generate_url("document", &file);
        verify_eq!(url, "uploads/doc.pdf", "URL from file with metadata");
        total_tests += 1;
        passed_tests += 1;
    }
    
    // ========================================================================
    // TEST 2: Base URL Configuration
    // ========================================================================
    test_section!("TEST 2: Base URL Configuration");
    
    test_case!("2.1: Simple base URL concatenation");
    {
        let config = MockConfig::new()
            .with_base_url("https://cdn.example.com/uploads");
        let file = FileAttachment::new("2024/image.jpg");
        let url = config.generate_url("thumbnail", &file);
        verify_eq!(url, "https://cdn.example.com/uploads/2024/image.jpg", "Base URL + key");
        total_tests += 1;
        passed_tests += 1;
    }
    
    test_case!("2.2: Trailing slash in base URL handled");
    {
        let config = MockConfig::new()
            .with_base_url("https://cdn.example.com/uploads/");
        let file = FileAttachment::new("2024/image.jpg");
        let url = config.generate_url("avatar", &file);
        verify_eq!(url, "https://cdn.example.com/uploads/2024/image.jpg", "Trailing slash trimmed");
        total_tests += 1;
        passed_tests += 1;
    }
    
    test_case!("2.3: Leading slash in key handled");
    {
        let config = MockConfig::new()
            .with_base_url("https://cdn.example.com/uploads");
        let file = FileAttachment::new("/2024/image.jpg");
        let url = config.generate_url("document", &file);
        verify_eq!(url, "https://cdn.example.com/uploads/2024/image.jpg", "Leading slash trimmed");
        total_tests += 1;
        passed_tests += 1;
    }
    
    // ========================================================================
    // TEST 3: Field-Aware URL Generation
    // ========================================================================
    test_section!("TEST 3: Field-Aware URL Generation");
    
    test_case!("3.1: Different URLs for different field names");
    {
        let file = FileAttachment::new("products/123/image.jpg");
        
        let thumb_url = field_aware_url_generator("thumbnail", &file);
        let video_url = field_aware_url_generator("video", &file);
        let doc_url = field_aware_url_generator("document", &file);
        let other_url = field_aware_url_generator("other", &file);
        
        verify!(thumb_url.contains("images.example.com"), "Thumbnail goes to images CDN");
        verify!(video_url.contains("stream.example.com"), "Video goes to streaming CDN");
        verify!(doc_url.contains("docs.example.com/secure"), "Document goes to secure docs");
        verify!(other_url.contains("cdn.example.com"), "Other goes to general CDN");
        
        total_tests += 1;
        passed_tests += 1;
    }
    
    test_case!("3.2: Avatar field gets special formatting");
    {
        let file = FileAttachment::new("users/avatar.jpg");
        let url = field_aware_url_generator("avatar", &file);
        verify!(url.contains("images.example.com/w_auto,f_auto"), "Avatar has transformations");
        total_tests += 1;
        passed_tests += 1;
    }
    
    test_case!("3.3: Cover image gets optimized resolution");
    {
        let file = FileAttachment::new("banners/hero.jpg");
        let url = field_aware_url_generator("cover_image", &file);
        verify!(url.contains("w_1200,q_80"), "Cover image optimized for large display");
        total_tests += 1;
        passed_tests += 1;
    }
    
    // ========================================================================
    // TEST 4: Combined Field + Metadata URL Generation
    // ========================================================================
    test_section!("TEST 4: Combined Field + Metadata URL Generation");
    
    test_case!("4.1: Thumbnail field takes priority over mime_type");
    {
        let file = FileAttachment::with_metadata("img.jpg", None, Some(100_000), Some("image/jpeg"));
        let url = smart_url_generator("thumbnail", &file);
        verify!(url.contains("thumbs.example.com"), "Thumbnail field routes to thumbs CDN");
        total_tests += 1;
        passed_tests += 1;
    }
    
    test_case!("4.2: Avatar field applies specific transformations");
    {
        let file = FileAttachment::with_metadata("avatar.png", None, Some(50_000), Some("image/png"));
        let url = smart_url_generator("avatar", &file);
        verify!(url.contains("w_200,h_200,c_fill"), "Avatar has square crop applied");
        total_tests += 1;
        passed_tests += 1;
    }
    
    test_case!("4.3: Non-special fields fall back to mime_type routing");
    {
        let video = FileAttachment::with_metadata("media/clip.mp4", None, None, Some("video/mp4"));
        let image = FileAttachment::with_metadata("media/photo.jpg", None, Some(2_000_000), Some("image/jpeg"));
        let pdf = FileAttachment::with_metadata("media/doc.pdf", None, None, Some("application/pdf"));
        
        let video_url = smart_url_generator("gallery", &video);
        let image_url = smart_url_generator("gallery", &image);
        let pdf_url = smart_url_generator("gallery", &pdf);
        
        verify!(video_url.contains("stream.example.com"), "Video to streaming");
        verify!(image_url.contains("images.example.com/q_80"), "Large image gets reduced quality");
        verify!(pdf_url.contains("docs.example.com"), "PDF to docs");
        
        total_tests += 1;
        passed_tests += 1;
    }
    
    // ========================================================================
    // TEST 5: Model-Specific URL Generators
    // ========================================================================
    test_section!("TEST 5: Model-Specific URL Generators");
    
    test_case!("5.1: Product model routes by field");
    {
        let file = FileAttachment::new("products/123/image.jpg");
        
        let thumb = product_url_generator("thumbnail", &file);
        let gallery = product_url_generator("gallery", &file);
        let manual = product_url_generator("manual", &file);
        let other = product_url_generator("specs", &file);
        
        verify!(thumb.contains("/thumb/"), "Thumbnail to thumb path");
        verify!(gallery.contains("/gallery/"), "Gallery to gallery path");
        verify!(manual.contains("/docs/"), "Manual to docs path");
        verify!(other.contains("/assets/"), "Other to assets path");
        
        total_tests += 1;
        passed_tests += 1;
    }
    
    test_case!("5.2: User model routes by field");
    {
        let file = FileAttachment::new("users/456/file.jpg");
        
        let avatar = user_url_generator("avatar", &file);
        let cover = user_url_generator("cover_photo", &file);
        let resume = user_url_generator("resume", &file);
        
        verify!(avatar.contains("avatars.example.com"), "Avatar to avatars CDN");
        verify!(cover.contains("covers.example.com"), "Cover to covers CDN");
        verify!(resume.contains("secure-docs.example.com/resumes"), "Resume to secure docs");
        
        total_tests += 1;
        passed_tests += 1;
    }
    
    // ========================================================================
    // TEST 6: Signed URLs with Field Context
    // ========================================================================
    test_section!("TEST 6: Signed URLs with Field Context");
    
    test_case!("6.1: Restricted access for private documents");
    {
        let file = FileAttachment::with_metadata("docs/secret.pdf", None, Some(5000), None);
        let url = signed_url_generator("private_document", &file);
        verify!(url.contains("token=restricted_"), "Private document gets restricted token");
        total_tests += 1;
        passed_tests += 1;
    }
    
    test_case!("6.2: Internal access for internal-only files");
    {
        let file = FileAttachment::with_metadata("internal/report.xlsx", None, Some(3000), None);
        let url = signed_url_generator("internal_only", &file);
        verify!(url.contains("token=internal_"), "Internal file gets internal token");
        total_tests += 1;
        passed_tests += 1;
    }
    
    test_case!("6.3: Public access for regular files");
    {
        let file = FileAttachment::with_metadata("public/image.jpg", None, Some(1234), None);
        let url = signed_url_generator("thumbnail", &file);
        verify!(url.contains("token=public_234"), "Public file gets public token with size hash");
        total_tests += 1;
        passed_tests += 1;
    }
    
    // ========================================================================
    // TEST 7: JSON Output with Field-Aware URLs
    // ========================================================================
    test_section!("TEST 7: JSON Output with Field-Aware URLs");
    
    test_case!("7.1: Single file attachment with field-specific URL");
    {
        let attachment = FileAttachment::with_metadata(
            "uploads/thumbnail.jpg",
            Some("Photo.jpg"),
            Some(50_000),
            Some("image/jpeg"),
        );
        let json = serde_json::to_value(&attachment).unwrap();
        
        let processed = process_file_for_json("thumbnail", &json, &[], smart_url_generator);
        
        verify!(processed.get("url").is_some(), "URL field added");
        let url = processed.get("url").unwrap().as_str().unwrap();
        verify!(url.contains("thumbs.example.com"), "Thumbnail field routed correctly");
        total_tests += 1;
        passed_tests += 1;
    }
    
    test_case!("7.2: Different fields get different URLs for same file");
    {
        let attachment = FileAttachment::with_metadata(
            "uploads/image.jpg",
            None,
            Some(100_000),
            Some("image/jpeg"),
        );
        let json = serde_json::to_value(&attachment).unwrap();
        
        let as_thumbnail = process_file_for_json("thumbnail", &json, &[], smart_url_generator);
        let as_avatar = process_file_for_json("avatar", &json, &[], smart_url_generator);
        let as_gallery = process_file_for_json("gallery", &json, &[], smart_url_generator);
        
        let thumb_url = as_thumbnail.get("url").unwrap().as_str().unwrap();
        let avatar_url = as_avatar.get("url").unwrap().as_str().unwrap();
        let gallery_url = as_gallery.get("url").unwrap().as_str().unwrap();
        
        verify!(thumb_url.contains("thumbs.example.com"), "Thumbnail to thumbs");
        verify!(avatar_url.contains("avatars.example.com"), "Avatar to avatars");
        verify!(gallery_url.contains("images.example.com"), "Gallery to images");
        
        total_tests += 1;
        passed_tests += 1;
    }
    
    test_case!("7.3: Hidden attributes excluded, URL still added");
    {
        let attachment = FileAttachment::with_metadata(
            "docs/secret.pdf",
            Some("Secret Document.pdf"),
            Some(1024 * 1024),
            Some("application/pdf"),
        );
        let json = serde_json::to_value(&attachment).unwrap();
        
        let processed = process_file_for_json("document", &json, &["size", "mime_type"], cdn_url_generator);
        
        verify!(processed.get("url").is_some(), "URL present");
        verify!(processed.get("key").is_some(), "Key present");
        verify!(processed.get("size").is_none(), "Size hidden");
        verify!(processed.get("mime_type").is_none(), "Mime type hidden");
        
        total_tests += 1;
        passed_tests += 1;
    }
    
    // ========================================================================
    // TEST 8: Edge Cases
    // ========================================================================
    test_section!("TEST 8: Edge Cases");
    
    test_case!("8.1: Empty field name");
    {
        let file = FileAttachment::new("file.jpg");
        let url = field_aware_url_generator("", &file);
        verify!(url.contains("cdn.example.com"), "Empty field name falls back to default");
        total_tests += 1;
        passed_tests += 1;
    }
    
    test_case!("8.2: Field name with special characters");
    {
        let file = FileAttachment::new("file.jpg");
        let url = field_aware_url_generator("user_avatar_2", &file);
        // Should fall through to default
        verify!(url.contains("cdn.example.com"), "Special field name handled");
        total_tests += 1;
        passed_tests += 1;
    }
    
    test_case!("8.3: Unicode in field name");
    {
        let file = FileAttachment::new("файл.jpg");
        let url = cdn_url_generator("изображение", &file);
        verify!(url.contains("файл.jpg"), "Unicode preserved in URL");
        total_tests += 1;
        passed_tests += 1;
    }
    
    // ========================================================================
    // TEST 9: Performance Verification
    // ========================================================================
    test_section!("TEST 9: Performance Verification");
    
    test_case!("9.1: Inline URL generation with field routing");
    {
        let file = FileAttachment::new("test.jpg");
        let fields = ["thumbnail", "avatar", "document", "video", "gallery"];
        
        let start = std::time::Instant::now();
        for _ in 0..10000 {
            for field in &fields {
                let _ = field_aware_url_generator(field, &file);
            }
        }
        let duration = start.elapsed();
        
        println!("    ⏱️  50,000 field-aware URL generations: {:?}", duration);
        verify!(duration.as_millis() < 200, "URL generation is fast (< 200ms for 50k)");
        
        total_tests += 1;
        passed_tests += 1;
    }
    
    test_case!("9.2: Complex generator with field + metadata access");
    {
        let files: Vec<_> = (0..1000).map(|i| {
            FileAttachment::with_metadata(
                &format!("files/{}.jpg", i),
                Some("original.jpg"),
                Some(i as u64 * 1000),
                Some("image/jpeg"),
            )
        }).collect();
        
        let fields = ["thumbnail", "avatar", "gallery"];
        
        let start = std::time::Instant::now();
        for file in &files {
            for field in &fields {
                let _ = smart_url_generator(field, file);
            }
        }
        let duration = start.elapsed();
        
        println!("    ⏱️  3,000 smart URL generations: {:?}", duration);
        verify!(duration.as_millis() < 50, "Smart generation is fast (< 50ms for 3k)");
        
        total_tests += 1;
        passed_tests += 1;
    }
    
    // ========================================================================
    // SUMMARY
    // ========================================================================
    println!("\n{}", "═".repeat(70));
    println!("📊 TEST SUMMARY");
    println!("{}", "═".repeat(70));
    println!("  Total Tests: {}", total_tests);
    println!("  Passed:      {}", passed_tests);
    println!("  Failed:      {}", total_tests - passed_tests);
    println!("{}", "═".repeat(70));
    
    if passed_tests == total_tests {
        println!("\n✅ ALL FILE ATTACHMENT URL TESTS PASSED!\n");
    } else {
        println!("\n❌ SOME TESTS FAILED!\n");
        std::process::exit(1);
    }
    
    // ========================================================================
    // USAGE EXAMPLES OUTPUT
    // ========================================================================
    println!("\n{}", "═".repeat(70));
    println!("📚 USAGE EXAMPLES (with field_name + FileAttachment access)");
    println!("{}", "═".repeat(70));
    
    println!("\n1️⃣  Configure global base URL:");
    println!(r#"
    TideConfig::init()
        .database("postgres://localhost/mydb")
        .file_base_url("https://cdn.example.com/uploads")
        .connect()
        .await?;
    "#);
    
    println!("\n2️⃣  Use field-aware URL generator:");
    println!(r#"
    fn smart_url_generator(field_name: &str, file: &FileAttachment) -> String {{
        // Route based on field name first
        match field_name {{
            "thumbnail" => format!("https://thumbs.example.com/{{}}", file.key),
            "avatar" => format!("https://avatars.example.com/{{}}", file.key),
            _ => {{
                // Fall back to mime_type routing
                match file.mime_type.as_deref() {{
                    Some(m) if m.starts_with("video/") => {{
                        format!("https://stream.example.com/{{}}", file.key)
                    }}
                    _ => format!("https://cdn.example.com/{{}}", file.key),
                }}
            }}
        }}
    }}
    
    TideConfig::init()
        .database("postgres://localhost/mydb")
        .file_url_generator(smart_url_generator)
        .connect()
        .await?;
    "#);
    
    println!("\n3️⃣  Override per model:");
    println!(r#"
    impl ModelMeta for Product {{
        fn file_url_generator() -> FileUrlGenerator {{
            |field_name, file| {{
                match field_name {{
                    "thumbnail" => format!("https://products-cdn.example.com/thumb/{{}}", file.key),
                    "gallery" => format!("https://products-cdn.example.com/gallery/{{}}", file.key),
                    _ => format!("https://products-cdn.example.com/assets/{{}}", file.key),
                }}
            }}
        }}
    }}
    "#);
    
    println!("\n4️⃣  Access URLs in JSON output:");
    println!(r#"
    let product = Product::find(1).await?;
    let json = product.to_json(None);
    
    // JSON output includes field-specific URLs:
    // {{
    //   "thumbnail": {{
    //     "key": "products/1/thumb.jpg",
    //     "filename": "thumb.jpg",
    //     "url": "https://products-cdn.example.com/thumb/products/1/thumb.jpg"
    //   }},
    //   "gallery": [
    //     {{
    //       "key": "products/1/img1.jpg",
    //       "url": "https://products-cdn.example.com/gallery/products/1/img1.jpg"
    //     }}
    //   ]
    // }}
    "#);
}
