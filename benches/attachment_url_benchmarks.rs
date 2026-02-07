//! Benchmarks for File Attachment URL Generation
//!
//! This module benchmarks the file URL generation feature to ensure
//! it performs efficiently under various conditions.
//!
//! Run with: cargo bench --bench attachment_url_benchmarks

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hint::black_box;

// ============================================================================
// FILE ATTACHMENT STRUCT (mirrors TideORM's FileAttachment)
// ============================================================================

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
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl FileAttachment {
    fn new(key: &str) -> Self {
        let filename = key.split('/').next_back().unwrap_or(key).to_string();
        Self {
            key: key.to_string(),
            filename,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            original_filename: None,
            size: None,
            mime_type: None,
            metadata: HashMap::new(),
        }
    }
    
    fn with_metadata(key: &str, original_filename: Option<&str>, size: Option<u64>, mime_type: Option<&str>) -> Self {
        let mut attachment = Self::new(key);
        attachment.original_filename = original_filename.map(|s| s.to_string());
        attachment.size = size;
        attachment.mime_type = mime_type.map(|s| s.to_string());
        attachment
    }
    
    fn with_full_metadata(key: &str) -> Self {
        let mut attachment = Self::new(key);
        attachment.original_filename = Some("original_file.jpg".to_string());
        attachment.size = Some(1_500_000);
        attachment.mime_type = Some("image/jpeg".to_string());
        attachment.metadata.insert("width".to_string(), serde_json::json!(1920));
        attachment.metadata.insert("height".to_string(), serde_json::json!(1080));
        attachment.metadata.insert("format".to_string(), serde_json::json!("jpeg"));
        attachment
    }
}

type FileUrlGenerator = fn(field_name: &str, file: &FileAttachment) -> String;

// ============================================================================
// URL GENERATORS
// ============================================================================

/// Simple key-only URL generator (baseline)
#[inline]
fn simple_url_generator(_field_name: &str, file: &FileAttachment) -> String {
    format!("https://cdn.example.com/{}", file.key)
}

/// Generator that uses field_name for routing
#[inline]
fn field_based_url_generator(field_name: &str, file: &FileAttachment) -> String {
    match field_name {
        "thumbnail" => format!("https://thumbs.example.com/{}", file.key),
        "avatar" => format!("https://avatars.example.com/{}", file.key),
        "video" => format!("https://stream.example.com/{}", file.key),
        _ => format!("https://cdn.example.com/{}", file.key),
    }
}

/// Generator that accesses mime_type
#[inline]
fn mime_based_url_generator(_field_name: &str, file: &FileAttachment) -> String {
    match file.mime_type.as_deref() {
        Some(m) if m.starts_with("video/") => format!("https://stream.example.com/{}", file.key),
        Some(m) if m.starts_with("image/") => format!("https://images.example.com/{}", file.key),
        _ => format!("https://cdn.example.com/{}", file.key),
    }
}

/// Generator that accesses multiple fields (field_name + metadata)
#[inline]
fn complex_url_generator(field_name: &str, file: &FileAttachment) -> String {
    // Field-specific routing takes priority
    match field_name {
        "thumbnail" => {
            let quality = if file.size.unwrap_or(0) > 500_000 { "60" } else { "auto" };
            return format!("https://thumbs.example.com/q_{}/{}", quality, file.key);
        }
        "avatar" => {
            return format!("https://avatars.example.com/w_200,h_200/{}", file.key);
        }
        _ => {}
    }
    
    // Fall back to mime_type routing
    let base = match file.mime_type.as_deref() {
        Some(m) if m.starts_with("video/") => "stream",
        Some(m) if m.starts_with("image/") => "images",
        Some("application/pdf") => "docs",
        _ => "cdn",
    };
    
    let quality = if file.size.unwrap_or(0) > 1_000_000 { "80" } else { "auto" };
    let prefix = if file.original_filename.is_some() { "originals" } else { "processed" };
    
    format!("https://{}.example.com/{}/q_{}/{}", base, prefix, quality, file.key)
}

/// Generator with signed URL (most complex)
#[inline]
fn signed_url_generator(field_name: &str, file: &FileAttachment) -> String {
    let access_level = match field_name {
        "private_document" | "secure_file" => "restricted",
        "internal_only" => "internal",
        _ => "public",
    };
    
    let size_hash = file.size.unwrap_or(0) % 1000;
    let filename_part = &file.filename[..3.min(file.filename.len())];
    let token = format!("{}_{:03}_{}", access_level, size_hash, filename_part);
    
    format!(
        "https://secure-cdn.example.com/{}?token={}&expires=3600",
        file.key.trim_start_matches('/'),
        token
    )
}

// ============================================================================
// MOCK CONFIG
// ============================================================================

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

// ============================================================================
// BENCHMARKS
// ============================================================================

fn bench_simple_url_generation(c: &mut Criterion) {
    let file = FileAttachment::new("uploads/2024/01/image.jpg");
    
    c.bench_function("url_gen_simple", |b| {
        b.iter(|| simple_url_generator(black_box("thumbnail"), black_box(&file)))
    });
}

fn bench_field_based_url_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_gen_field_based");
    
    let file = FileAttachment::new("uploads/image.jpg");
    
    group.bench_function("thumbnail", |b| {
        b.iter(|| field_based_url_generator(black_box("thumbnail"), black_box(&file)))
    });
    
    group.bench_function("avatar", |b| {
        b.iter(|| field_based_url_generator(black_box("avatar"), black_box(&file)))
    });
    
    group.bench_function("video", |b| {
        b.iter(|| field_based_url_generator(black_box("video"), black_box(&file)))
    });
    
    group.bench_function("unknown", |b| {
        b.iter(|| field_based_url_generator(black_box("other"), black_box(&file)))
    });
    
    group.finish();
}

fn bench_mime_based_url_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_gen_mime_based");
    
    let image = FileAttachment::with_metadata("media/image.jpg", None, None, Some("image/jpeg"));
    let video = FileAttachment::with_metadata("media/video.mp4", None, None, Some("video/mp4"));
    let other = FileAttachment::new("media/file.bin");
    
    group.bench_function("image", |b| {
        b.iter(|| mime_based_url_generator(black_box("gallery"), black_box(&image)))
    });
    
    group.bench_function("video", |b| {
        b.iter(|| mime_based_url_generator(black_box("gallery"), black_box(&video)))
    });
    
    group.bench_function("unknown", |b| {
        b.iter(|| mime_based_url_generator(black_box("gallery"), black_box(&other)))
    });
    
    group.finish();
}

fn bench_complex_url_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_gen_complex");
    
    let file = FileAttachment::with_full_metadata("products/premium/item.jpg");
    
    group.bench_function("thumbnail_field", |b| {
        b.iter(|| complex_url_generator(black_box("thumbnail"), black_box(&file)))
    });
    
    group.bench_function("avatar_field", |b| {
        b.iter(|| complex_url_generator(black_box("avatar"), black_box(&file)))
    });
    
    group.bench_function("fallback_to_mime", |b| {
        b.iter(|| complex_url_generator(black_box("gallery"), black_box(&file)))
    });
    
    group.finish();
}

fn bench_signed_url_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_gen_signed");
    
    let file = FileAttachment::with_metadata(
        "private/documents/report.pdf",
        Some("Q4 Report.pdf"),
        Some(2_500_000),
        Some("application/pdf"),
    );
    
    group.bench_function("private", |b| {
        b.iter(|| signed_url_generator(black_box("private_document"), black_box(&file)))
    });
    
    group.bench_function("internal", |b| {
        b.iter(|| signed_url_generator(black_box("internal_only"), black_box(&file)))
    });
    
    group.bench_function("public", |b| {
        b.iter(|| signed_url_generator(black_box("thumbnail"), black_box(&file)))
    });
    
    group.finish();
}

fn bench_config_url_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_gen_via_config");
    
    let file = FileAttachment::new("uploads/image.jpg");
    
    // No config (returns key)
    let no_config = MockConfig::new();
    group.bench_function("no_config", |b| {
        b.iter(|| no_config.generate_url(black_box("thumbnail"), black_box(&file)))
    });
    
    // Base URL config
    let base_config = MockConfig::new().with_base_url("https://cdn.example.com/uploads");
    group.bench_function("base_url", |b| {
        b.iter(|| base_config.generate_url(black_box("thumbnail"), black_box(&file)))
    });
    
    // Custom generator
    let custom_config = MockConfig::new().with_generator(field_based_url_generator);
    group.bench_function("custom_generator", |b| {
        b.iter(|| custom_config.generate_url(black_box("thumbnail"), black_box(&file)))
    });
    
    group.finish();
}

fn bench_batch_url_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_gen_batch");
    
    let fields = ["thumbnail", "avatar", "gallery", "document", "video"];
    
    for count in [10, 100, 1000].iter() {
        let files: Vec<FileAttachment> = (0..*count)
            .map(|i| FileAttachment::with_metadata(
                &format!("files/{}/image.jpg", i),
                Some("original.jpg"),
                Some(i as u64 * 1000),
                Some("image/jpeg"),
            ))
            .collect();
        
        group.throughput(Throughput::Elements(*count as u64));
        
        group.bench_with_input(BenchmarkId::new("simple", count), &files, |b, files| {
            b.iter(|| {
                files.iter().map(|f| simple_url_generator(black_box("thumbnail"), black_box(f))).collect::<Vec<_>>()
            })
        });
        
        group.bench_with_input(BenchmarkId::new("field_based", count), &files, |b, files| {
            b.iter(|| {
                files.iter().enumerate().map(|(i, f)| {
                    let field = fields[i % fields.len()];
                    field_based_url_generator(black_box(field), black_box(f))
                }).collect::<Vec<_>>()
            })
        });
        
        group.bench_with_input(BenchmarkId::new("complex", count), &files, |b, files| {
            b.iter(|| {
                files.iter().enumerate().map(|(i, f)| {
                    let field = fields[i % fields.len()];
                    complex_url_generator(black_box(field), black_box(f))
                }).collect::<Vec<_>>()
            })
        });
    }
    
    group.finish();
}

fn bench_json_serialization_with_url(c: &mut Criterion) {
    let file = FileAttachment::with_full_metadata("products/item.jpg");
    
    c.bench_function("json_serialize_file", |b| {
        b.iter(|| serde_json::to_value(black_box(&file)).unwrap())
    });
}

fn bench_file_attachment_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_attachment_creation");
    
    group.bench_function("minimal", |b| {
        b.iter(|| FileAttachment::new(black_box("uploads/image.jpg")))
    });
    
    group.bench_function("with_metadata", |b| {
        b.iter(|| FileAttachment::with_metadata(
            black_box("uploads/image.jpg"),
            black_box(Some("original.jpg")),
            black_box(Some(1_500_000)),
            black_box(Some("image/jpeg")),
        ))
    });
    
    group.bench_function("with_full_metadata", |b| {
        b.iter(|| FileAttachment::with_full_metadata(black_box("uploads/image.jpg")))
    });
    
    group.finish();
}

fn bench_field_name_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("field_name_matching");
    
    let file = FileAttachment::new("uploads/image.jpg");
    let fields = ["thumbnail", "avatar", "video", "document", "gallery", "cover_image", "profile_photo", "attachment", "resume", "other"];
    
    // Matching known field
    group.bench_function("known_field", |b| {
        b.iter(|| field_based_url_generator(black_box("thumbnail"), black_box(&file)))
    });
    
    // Matching unknown field (goes to default)
    group.bench_function("unknown_field", |b| {
        b.iter(|| field_based_url_generator(black_box("some_random_field"), black_box(&file)))
    });
    
    // Cycling through multiple fields
    group.bench_function("multiple_fields", |b| {
        b.iter(|| {
            fields.iter().map(|field| {
                field_based_url_generator(black_box(field), black_box(&file))
            }).collect::<Vec<_>>()
        })
    });
    
    group.finish();
}

fn bench_metadata_access_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("metadata_access");
    
    let file = FileAttachment::with_full_metadata("uploads/image.jpg");
    
    // Just accessing key (baseline)
    group.bench_function("key_only", |b| {
        b.iter(|| {
            let _ = black_box(&file).key.as_str();
        })
    });
    
    // Accessing optional field
    group.bench_function("optional_field", |b| {
        b.iter(|| {
            let _ = black_box(&file).mime_type.as_deref();
        })
    });
    
    // Pattern matching on mime_type
    group.bench_function("mime_type_match", |b| {
        b.iter(|| {
            match black_box(&file).mime_type.as_deref() {
                Some(m) if m.starts_with("image/") => "image",
                Some(m) if m.starts_with("video/") => "video",
                _ => "other",
            }
        })
    });
    
    // Accessing HashMap metadata
    group.bench_function("hashmap_lookup", |b| {
        b.iter(|| {
            let _ = black_box(&file).metadata.get("width");
        })
    });
    
    group.finish();
}

fn bench_url_string_building(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_string_building");
    
    let key = "uploads/2024/01/images/product.jpg";
    let base_url = "https://cdn.example.com";
    let field = "thumbnail";
    
    // Using format! macro with field
    group.bench_function("format_with_field", |b| {
        b.iter(|| format!("https://{}.example.com/{}", black_box(field), black_box(key)))
    });
    
    // Using format! macro without field
    group.bench_function("format_no_field", |b| {
        b.iter(|| format!("{}/{}", black_box(base_url), black_box(key)))
    });
    
    // Pre-allocated String with field routing
    group.bench_function("preallocated_with_routing", |b| {
        b.iter(|| {
            let f = black_box(field);
            let k = black_box(key);
            let subdomain = match f {
                "thumbnail" => "thumbs",
                "avatar" => "avatars",
                _ => "cdn",
            };
            let mut url = String::with_capacity(40 + k.len());
            url.push_str("https://");
            url.push_str(subdomain);
            url.push_str(".example.com/");
            url.push_str(k);
            url
        })
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_simple_url_generation,
    bench_field_based_url_generation,
    bench_mime_based_url_generation,
    bench_complex_url_generation,
    bench_signed_url_generation,
    bench_config_url_generation,
    bench_batch_url_generation,
    bench_json_serialization_with_url,
    bench_file_attachment_creation,
    bench_field_name_matching,
    bench_metadata_access_patterns,
    bench_url_string_building,
);

criterion_main!(benches);
