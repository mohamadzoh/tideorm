use super::*;

pub(super) fn bench_combined_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("combined_operations");
    group.sample_size(50);

    group.bench_function("full_product_setup", |b| {
        b.iter(|| {
            let mut product = BenchProduct::new(1);

            product
                .set_translation("name", "en", "Wireless Headphones")
                .unwrap();
            product
                .set_translation("name", "ar", "سماعات لاسلكية")
                .unwrap();
            product
                .set_translation("description", "en", "High-quality wireless headphones")
                .unwrap();
            product
                .set_translation("description", "ar", "سماعات لاسلكية عالية الجودة")
                .unwrap();

            product.attach("thumbnail", "uploads/thumb.jpg").unwrap();
            product
                .attach_many("images", vec!["img1.jpg", "img2.jpg", "img3.jpg"])
                .unwrap();
            product
                .attach_many("documents", vec!["manual.pdf", "warranty.pdf"])
                .unwrap();

            black_box(product)
        });
    });

    group.bench_function("full_json_output", |b| {
        let mut product = BenchProduct::new(1);
        product
            .set_translation("name", "en", "Wireless Headphones")
            .unwrap();
        product
            .set_translation("name", "ar", "سماعات لاسلكية")
            .unwrap();
        product
            .set_translation("description", "en", "High-quality wireless headphones")
            .unwrap();
        product
            .set_translation("description", "ar", "سماعات لاسلكية عالية الجودة")
            .unwrap();
        product.attach("thumbnail", "uploads/thumb.jpg").unwrap();
        product
            .attach_many("images", vec!["img1.jpg", "img2.jpg", "img3.jpg"])
            .unwrap();

        let mut opts_ar = HashMap::new();
        opts_ar.insert("language".to_string(), "ar".to_string());

        b.iter(|| {
            black_box(product.to_translated_json(Some(opts_ar.clone())));
            black_box(product.to_json_with_all_translations());
        });
    });

    for size in [10, 50, 100].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        group.bench_with_input(
            BenchmarkId::new("setup_collection", size),
            size,
            |b, &size| {
                b.iter(|| {
                    let products: Vec<BenchProduct> = (0..size)
                        .map(|i| {
                            let mut product = BenchProduct::new(i as i64);
                            product
                                .set_translation("name", "en", format!("Product {}", i))
                                .unwrap();
                            product
                                .set_translation("name", "ar", format!("منتج {}", i))
                                .unwrap();
                            product
                                .attach("thumbnail", &format!("thumb{}.jpg", i))
                                .unwrap();
                            product
                        })
                        .collect();
                    black_box(products)
                });
            },
        );
    }

    group.finish();
}
