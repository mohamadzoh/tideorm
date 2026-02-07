use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use std::hint::black_box;

// =============================================================================
// STABILITY AND EDGE CASE BENCHMARKS
// =============================================================================

fn bench_empty_result_handling(c: &mut Criterion) {
    c.bench_function("empty_vec_iteration", |b| {
        b.iter(|| {
            let empty: Vec<i32> = black_box(vec![]);
            empty.len()
        });
    });

    c.bench_function("empty_vec_first", |b| {
        b.iter(|| {
            let empty: Vec<i32> = black_box(vec![]);
            !empty.is_empty()
        });
    });

    c.bench_function("option_unwrap_or", |b| {
        b.iter(|| {
            let none: Option<i32> = black_box(None);
            none.unwrap_or(0)
        });
    });
}

fn bench_null_handling(c: &mut Criterion) {
    c.bench_function("option_is_some_check", |b| {
        b.iter(|| {
            let opt: Option<String> = black_box(Some("value".to_string()));
            opt.is_some()
        });
    });

    c.bench_function("option_is_none_check", |b| {
        b.iter(|| {
            let opt: Option<String> = black_box(None);
            opt.is_none()
        });
    });
}

fn bench_large_data(c: &mut Criterion) {
    c.bench_function("large_string_creation", |b| {
        b.iter(|| {
            let large = "x".repeat(black_box(10000));
            large.len()
        });
    });

    let mut group = c.benchmark_group("large_vector");
    for size in [100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let mut vec = Vec::new();
                for i in 0..black_box(size) {
                    vec.push(i);
                }
                vec.len()
            });
        });
    }
    group.finish();
}

fn bench_json_operations(c: &mut Criterion) {
    use serde_json::json;

    c.bench_function("json_creation", |b| {
        b.iter(|| {
            json!({
                "name": "test",
                "age": 30,
                "active": true
            })
        });
    });

    c.bench_function("json_nesting", |b| {
        b.iter(|| {
            json!({
                "level1": {
                    "level2": {
                        "level3": {
                            "value": 42
                        }
                    }
                }
            })
        });
    });

    c.bench_function("json_serialization", |b| {
        let obj = json!({
            "name": "test",
            "age": 30,
            "items": [1, 2, 3, 4, 5]
        });
        b.iter(|| {
            serde_json::to_string(&obj).unwrap()
        });
    });

    c.bench_function("json_deserialization", |b| {
        let json_str = r#"{"name":"test","age":30,"items":[1,2,3,4,5]}"#;
        b.iter(|| {
            serde_json::from_str::<serde_json::Value>(black_box(json_str)).unwrap()
        });
    });
}

fn bench_special_characters(c: &mut Criterion) {
    c.bench_function("unicode_string_operations", |b| {
        b.iter(|| {
            let unicode = black_box("Привет 世界 مرحبا ");
            unicode.chars().count()
        });
    });

    c.bench_function("escape_sensitive_chars", |b| {
        b.iter(|| {
            let dangerous = black_box("'; DROP TABLE users; --");
            dangerous.contains(';')
        });
    });

    c.bench_function("multiline_text_split", |b| {
        b.iter(|| {
            let multiline = black_box("line1\nline2\nline3\nline4\nline5");
            multiline.lines().count()
        });
    });
}

fn bench_numeric_operations(c: &mut Criterion) {
    use rust_decimal::Decimal;

    c.bench_function("i64_max_check", |b| {
        b.iter(|| {
            let max = black_box(i64::MAX);
            max > 0
        });
    });

    c.bench_function("decimal_creation", |b| {
        b.iter(|| {
            Decimal::from(black_box(123456789i64))
        });
    });

    c.bench_function("decimal_to_string", |b| {
        let dec = Decimal::from(123456789i64);
        b.iter(|| {
            dec.to_string()
        });
    });
}

fn bench_iteration_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("iteration");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::new("simple_loop", size), size, |b, &size| {
            b.iter(|| {
                let vec: Vec<i32> = (0..black_box(size)).collect();
                vec.iter().sum::<i32>()
            });
        });

        group.bench_with_input(BenchmarkId::new("filter_map", size), size, |b, &size| {
            b.iter(|| {
                let vec: Vec<i32> = (0..black_box(size)).collect();
                vec.iter()
                    .filter(|x| **x % 2 == 0)
                    .map(|x| x * 2)
                    .sum::<i32>()
            });
        });

        group.bench_with_input(BenchmarkId::new("find_first", size), size, |b, &size| {
            b.iter(|| {
                let vec: Vec<i32> = (0..black_box(size)).collect();
                vec.iter().any(|x| *x > size / 2)
            });
        });
    }
    group.finish();
}

fn bench_comparison_operations(c: &mut Criterion) {
    c.bench_function("string_comparison", |b| {
        b.iter(|| {
            let a = black_box("abc");
            let b_str = black_box("abd");
            a < b_str
        });
    });

    c.bench_function("numeric_comparison", |b| {
        b.iter(|| {
            let a = black_box(100i64);
            let b_val = black_box(200i64);
            a < b_val
        });
    });

    c.bench_function("option_comparison", |b| {
        b.iter(|| {
            let a: Option<i32> = black_box(Some(1));
            let b_val: Option<i32> = black_box(Some(2));
            a < b_val
        });
    });
}

fn bench_clone_and_copy(c: &mut Criterion) {
    c.bench_function("vec_clone", |b| {
        b.iter(|| {
            let original = black_box(vec![1, 2, 3, 4, 5]);
            original.clone()
        });
    });

    c.bench_function("string_clone", |b| {
        b.iter(|| {
            let original = black_box(String::from("test string"));
            original.clone()
        });
    });

    c.bench_function("large_vec_clone", |b| {
        b.iter(|| {
            let original: Vec<i32> = (0..1000).collect();
            black_box(original).clone()
        });
    });
}

fn bench_hash_operations(c: &mut Criterion) {
    use std::collections::HashMap;

    c.bench_function("hashmap_insert", |b| {
        b.iter(|| {
            let mut map = HashMap::new();
            for i in 0..100 {
                map.insert(black_box(i), i * 2);
            }
            map
        });
    });

    c.bench_function("hashmap_lookup", |b| {
        let mut map = HashMap::new();
        for i in 0..100 {
            map.insert(i, i * 2);
        }
        b.iter(|| {
            map.get(&black_box(50))
        });
    });

    c.bench_function("hashmap_contains", |b| {
        let mut map = HashMap::new();
        for i in 0..100 {
            map.insert(i, i * 2);
        }
        b.iter(|| {
            map.contains_key(&black_box(50))
        });
    });
}

fn bench_timestamp_operations(c: &mut Criterion) {
    use chrono::Utc;

    c.bench_function("utc_now", |b| {
        b.iter(|| {
            Utc::now()
        });
    });

    c.bench_function("timestamp_comparison", |b| {
        let time1 = Utc::now();
        let time2 = Utc::now();
        b.iter(|| {
            black_box(time1) < black_box(time2)
        });
    });

    c.bench_function("timestamp_format", |b| {
        let now = Utc::now();
        b.iter(|| {
            now.to_rfc3339()
        });
    });
}

#[allow(clippy::unnecessary_literal_unwrap)]
fn bench_error_handling(c: &mut Criterion) {
    c.bench_function("result_ok_unwrap", |b| {
        b.iter(|| {
            let result: Result<i32, String> = Ok(black_box(42));
            result.unwrap()
        });
    });

    c.bench_function("result_err_unwrap_or", |b| {
        b.iter(|| {
            let result: Result<i32, String> = Err(black_box("error".to_string()));
            result.unwrap_or(0)
        });
    });

    c.bench_function("option_unwrap_or_else", |b| {
        b.iter(|| {
            let none: Option<i32> = black_box(None);
            none.unwrap_or(99)
        });
    });
}

criterion_group!(
    benches,
    bench_empty_result_handling,
    bench_null_handling,
    bench_large_data,
    bench_json_operations,
    bench_special_characters,
    bench_numeric_operations,
    bench_iteration_patterns,
    bench_comparison_operations,
    bench_clone_and_copy,
    bench_hash_operations,
    bench_timestamp_operations,
    bench_error_handling,
);

criterion_main!(benches);
