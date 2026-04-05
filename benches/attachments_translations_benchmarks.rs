//! Attachments and Translations Benchmarks for TideORM
//!
//! These benchmarks measure the performance of file attachment and
//! translation operations.
//!
//! Run with: cargo bench --bench attachments_translations_benchmarks

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hint::black_box;
use tideorm::attachments::{AttachmentError, FileAttachment, FilesData, HasAttachments};
use tideorm::translations::{
    HasTranslations, TranslationError, TranslationInput, TranslationsData,
};

#[path = "attachments_translations_benchmarks/combined_operations.rs"]
mod combined_operations;
#[path = "attachments_translations_benchmarks/models.rs"]
mod models;

use combined_operations::*;
use models::*;

// =============================================================================
// FILE ATTACHMENT BENCHMARKS
// =============================================================================

fn bench_file_attachment_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_attachment_creation");

    group.bench_function("create_simple", |b| {
        b.iter(|| black_box(FileAttachment::new("uploads/2024/01/image.jpg")));
    });

    group.bench_function("create_with_metadata", |b| {
        b.iter(|| {
            black_box(FileAttachment::with_metadata(
                "uploads/2024/01/document.pdf",
                Some("My Document.pdf"),
                Some(1024 * 1024),
                Some("application/pdf"),
            ))
        });
    });

    group.bench_function("create_with_custom_metadata", |b| {
        b.iter(|| {
            black_box(
                FileAttachment::new("uploads/image.jpg")
                    .add_metadata("width", 1920)
                    .add_metadata("height", 1080)
                    .add_metadata("format", "jpeg"),
            )
        });
    });

    group.finish();
}

fn bench_files_data_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("files_data_operations");

    // Benchmark hasOne operations
    group.bench_function("set_one", |b| {
        let mut files = FilesData::new();
        b.iter(|| {
            files.set_one("thumbnail", FileAttachment::new("thumb.jpg"));
            black_box(&files);
        });
    });

    group.bench_function("get_one", |b| {
        let mut files = FilesData::new();
        files.set_one("thumbnail", FileAttachment::new("thumb.jpg"));
        b.iter(|| black_box(files.get_one("thumbnail")));
    });

    // Benchmark hasMany operations
    group.bench_function("add_many_single", |b| {
        let mut files = FilesData::new();
        let mut counter = 0u64;
        b.iter(|| {
            counter += 1;
            files.add_many(
                "images",
                FileAttachment::new(&format!("img{}.jpg", counter)),
            );
            black_box(&files);
        });
    });

    group.bench_function("get_many_10_items", |b| {
        let mut files = FilesData::new();
        for i in 0..10 {
            files.add_many("images", FileAttachment::new(&format!("img{}.jpg", i)));
        }
        b.iter(|| black_box(files.get_many("images")));
    });

    group.bench_function("get_many_100_items", |b| {
        let mut files = FilesData::new();
        for i in 0..100 {
            files.add_many("images", FileAttachment::new(&format!("img{}.jpg", i)));
        }
        b.iter(|| black_box(files.get_many("images")));
    });

    group.finish();
}

fn bench_attachment_trait_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("attachment_trait_operations");

    group.bench_function("attach_single", |b| {
        let mut product = BenchProduct::new(1);
        let mut counter = 0u64;
        b.iter(|| {
            counter += 1;
            product
                .attach("images", &format!("uploads/img{}.jpg", counter))
                .unwrap();
            black_box(&product);
        });
    });

    group.bench_function("attach_many_10", |b| {
        b.iter(|| {
            let mut product = BenchProduct::new(1);
            let keys: Vec<&str> = vec![
                "img1.jpg",
                "img2.jpg",
                "img3.jpg",
                "img4.jpg",
                "img5.jpg",
                "img6.jpg",
                "img7.jpg",
                "img8.jpg",
                "img9.jpg",
                "img10.jpg",
            ];
            product.attach_many("images", keys).unwrap();
            black_box(product)
        });
    });

    group.bench_function("sync_files", |b| {
        let mut product = BenchProduct::new(1);
        // Pre-populate with some files
        product
            .attach_many("images", vec!["old1.jpg", "old2.jpg", "old3.jpg"])
            .unwrap();

        b.iter(|| {
            product
                .sync("images", vec!["new1.jpg", "new2.jpg"])
                .unwrap();
            black_box(&product);
        });
    });

    group.bench_function("detach_file", |b| {
        b.iter_batched(
            || {
                let mut product = BenchProduct::new(1);
                product
                    .attach_many("images", vec!["img1.jpg", "img2.jpg", "img3.jpg"])
                    .unwrap();
                product
            },
            |mut product| {
                product.detach("images", Some("img2.jpg")).unwrap();
                black_box(product)
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_files_data_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("files_data_serialization");

    // Small files data
    group.bench_function("to_json_small", |b| {
        let mut files = FilesData::new();
        files.set_one("thumbnail", FileAttachment::new("thumb.jpg"));
        files.add_many("images", FileAttachment::new("img1.jpg"));
        files.add_many("images", FileAttachment::new("img2.jpg"));

        b.iter(|| black_box(files.to_json()));
    });

    // Large files data
    group.bench_function("to_json_large", |b| {
        let mut files = FilesData::new();
        files.set_one("thumbnail", FileAttachment::new("thumb.jpg"));
        files.set_one("cover", FileAttachment::new("cover.jpg"));
        for i in 0..50 {
            files.add_many("images", FileAttachment::new(&format!("img{}.jpg", i)));
        }
        for i in 0..20 {
            files.add_many("documents", FileAttachment::new(&format!("doc{}.pdf", i)));
        }

        b.iter(|| black_box(files.to_json()));
    });

    // Deserialize
    group.bench_function("from_json", |b| {
        let json = serde_json::json!({
            "thumbnail": {"key": "thumb.jpg", "filename": "thumb.jpg", "created_at": "2024-01-01T00:00:00Z"},
            "images": [
                {"key": "img1.jpg", "filename": "img1.jpg", "created_at": "2024-01-01T00:00:00Z"},
                {"key": "img2.jpg", "filename": "img2.jpg", "created_at": "2024-01-01T00:00:00Z"},
                {"key": "img3.jpg", "filename": "img3.jpg", "created_at": "2024-01-01T00:00:00Z"},
            ]
        });

        b.iter(|| {
            black_box(FilesData::from_json(&json))
        });
    });

    group.finish();
}

// =============================================================================
// TRANSLATION BENCHMARKS
// =============================================================================

fn bench_translations_data_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("translations_data_operations");

    group.bench_function("set_translation", |b| {
        let mut data = TranslationsData::new();
        b.iter(|| {
            data.set("name", "en", "Product Name");
            black_box(&data);
        });
    });

    group.bench_function("get_translation", |b| {
        let mut data = TranslationsData::new();
        data.set("name", "en", "Product Name");
        data.set("name", "ar", "اسم المنتج");

        b.iter(|| black_box(data.get("name", "en")));
    });

    group.bench_function("set_multiple_fields", |b| {
        b.iter(|| {
            let mut data = TranslationsData::new();
            data.set("name", "en", "Product");
            data.set("name", "ar", "منتج");
            data.set("name", "fr", "Produit");
            data.set("description", "en", "Description");
            data.set("description", "ar", "الوصف");
            data.set("description", "fr", "La description");
            black_box(data)
        });
    });

    group.bench_function("has_translations_check", |b| {
        let mut data = TranslationsData::new();
        data.set("name", "en", "Product");
        data.set("name", "ar", "منتج");

        b.iter(|| {
            black_box(data.has_translations("name"));
            black_box(data.has_translations("description"));
        });
    });

    group.finish();
}

fn bench_translation_trait_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("translation_trait_operations");

    group.bench_function("set_translation_via_trait", |b| {
        let mut product = BenchProduct::new(1);
        b.iter(|| {
            product.set_translation("name", "ar", "منتج").unwrap();
            black_box(&product);
        });
    });

    group.bench_function("get_translated_with_fallback", |b| {
        let mut product = BenchProduct::new(1);
        product.set_translation("name", "ar", "منتج").unwrap();
        product.set_translation("name", "en", "Product").unwrap();

        b.iter(|| {
            // Get existing translation
            black_box(product.get_translated("name", "ar").unwrap());
            // Trigger fallback
            black_box(product.get_translated("name", "de").unwrap());
        });
    });

    group.bench_function("get_all_translations", |b| {
        let mut product = BenchProduct::new(1);
        product.set_translation("name", "en", "Product").unwrap();
        product.set_translation("name", "ar", "منتج").unwrap();
        product.set_translation("name", "fr", "Produit").unwrap();
        product.set_translation("name", "es", "Producto").unwrap();
        product.set_translation("name", "de", "Produkt").unwrap();

        b.iter(|| black_box(product.get_all_translations("name").unwrap()));
    });

    group.bench_function("to_translated_json", |b| {
        let mut product = BenchProduct::new(1);
        product.set_translation("name", "ar", "منتج").unwrap();
        product
            .set_translation("description", "ar", "وصف المنتج")
            .unwrap();
        product.set_translation("name", "en", "Product").unwrap();
        product
            .set_translation("description", "en", "Product description")
            .unwrap();

        let mut opts = HashMap::new();
        opts.insert("language".to_string(), "ar".to_string());

        b.iter(|| black_box(product.to_translated_json(Some(opts.clone()))));
    });

    group.finish();
}

fn bench_translation_input(c: &mut Criterion) {
    let mut group = c.benchmark_group("translation_input");

    group.bench_function("from_json_small", |b| {
        let json = serde_json::json!({
            "name": {"en": "Product", "ar": "منتج"},
            "description": {"en": "Description"}
        });

        b.iter(|| black_box(TranslationInput::from_json(&json).unwrap()));
    });

    group.bench_function("from_json_large", |b| {
        let json = serde_json::json!({
            "name": {"en": "Product", "ar": "منتج", "fr": "Produit", "es": "Producto", "de": "Produkt"},
            "description": {"en": "Description", "ar": "الوصف", "fr": "La description", "es": "Descripción", "de": "Beschreibung"},
            "short_desc": {"en": "Short", "ar": "قصير", "fr": "Court", "es": "Corto", "de": "Kurz"},
            "meta_title": {"en": "Title", "ar": "عنوان", "fr": "Titre", "es": "Título", "de": "Titel"},
            "meta_desc": {"en": "Meta", "ar": "ميتا", "fr": "Méta", "es": "Meta", "de": "Meta"}
        });

        b.iter(|| {
            black_box(TranslationInput::from_json(&json).unwrap())
        });
    });

    group.bench_function("add_translations", |b| {
        b.iter(|| {
            let mut input = TranslationInput::new();
            input.add("name", "en", "Product");
            input.add("name", "ar", "منتج");
            input.add("name", "fr", "Produit");
            input.add("description", "en", "Description");
            input.add("description", "ar", "الوصف");
            black_box(input)
        });
    });

    group.finish();
}

fn bench_translations_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("translations_serialization");

    group.bench_function("to_json_small", |b| {
        let mut data = TranslationsData::new();
        data.set("name", "en", "Product");
        data.set("name", "ar", "منتج");

        b.iter(|| black_box(data.to_json()));
    });

    group.bench_function("to_json_large", |b| {
        let mut data = TranslationsData::new();
        for field in &[
            "name",
            "description",
            "short_desc",
            "meta_title",
            "meta_desc",
        ] {
            for lang in &["en", "ar", "fr", "es", "de"] {
                data.set(field, lang, format!("{} in {}", field, lang));
            }
        }

        b.iter(|| black_box(data.to_json()));
    });

    group.bench_function("from_json", |b| {
        let json = serde_json::json!({
            "name": {"en": "Product", "ar": "منتج", "fr": "Produit"},
            "description": {"en": "Description", "ar": "الوصف", "fr": "La description"}
        });

        b.iter(|| black_box(TranslationsData::from_json(&json)));
    });

    group.finish();
}

// =============================================================================
// COMBINED OPERATIONS BENCHMARKS
// =============================================================================

// =============================================================================
// BENCHMARK GROUPS
// =============================================================================

criterion_group!(
    file_attachment_benches,
    bench_file_attachment_creation,
    bench_files_data_operations,
    bench_attachment_trait_operations,
    bench_files_data_serialization,
);

criterion_group!(
    translation_benches,
    bench_translations_data_operations,
    bench_translation_trait_operations,
    bench_translation_input,
    bench_translations_serialization,
);

criterion_group!(combined_benches, bench_combined_operations,);

criterion_main!(
    file_attachment_benches,
    translation_benches,
    combined_benches,
);
