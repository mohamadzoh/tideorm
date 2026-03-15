//! Validation Benchmarks for TideORM
//!
//! These benchmarks measure the performance of validation operations.
//!
//! Run with: cargo bench --bench validation_benchmarks

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tideorm::validation::{ValidationBuilder, ValidationErrors, ValidationRule, Validator};

// =============================================================================
// SINGLE RULE BENCHMARKS
// =============================================================================

fn bench_validation_rules(c: &mut Criterion) {
    let mut group = c.benchmark_group("validation_rules");

    // Required validation
    group.bench_function("required_valid", |b| {
        let rule = ValidationRule::Required;
        let value = "hello world".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    group.bench_function("required_invalid", |b| {
        let rule = ValidationRule::Required;
        let value = "".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    // Email validation
    group.bench_function("email_valid", |b| {
        let rule = ValidationRule::Email;
        let value = "test@example.com".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    group.bench_function("email_invalid", |b| {
        let rule = ValidationRule::Email;
        let value = "not-an-email".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    // URL validation
    group.bench_function("url_valid", |b| {
        let rule = ValidationRule::Url;
        let value = "https://example.com/path?query=value".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    // Length validations
    group.bench_function("min_length_valid", |b| {
        let rule = ValidationRule::MinLength(5);
        let value = "hello world".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    group.bench_function("max_length_valid", |b| {
        let rule = ValidationRule::MaxLength(100);
        let value = "hello world".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    // Numeric validations
    group.bench_function("min_valid", |b| {
        let rule = ValidationRule::Min(18.0);
        let value = "25".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    group.bench_function("max_valid", |b| {
        let rule = ValidationRule::Max(100.0);
        let value = "50".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    group.bench_function("range_valid", |b| {
        let rule = ValidationRule::Range(1.0, 100.0);
        let value = "50".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    // Regex validation
    group.bench_function("regex_simple", |b| {
        let rule = ValidationRule::Regex(r"^\d{3}-\d{4}$".to_string());
        let value = "123-4567".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    group.bench_function("regex_complex", |b| {
        let rule =
            ValidationRule::Regex(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$".to_string());
        let value = "user.name+tag@example.co.uk".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    // Character class validations
    group.bench_function("alpha", |b| {
        let rule = ValidationRule::Alpha;
        let value = "HelloWorld".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    group.bench_function("alphanumeric", |b| {
        let rule = ValidationRule::Alphanumeric;
        let value = "Hello123World".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    group.bench_function("numeric", |b| {
        let rule = ValidationRule::Numeric;
        let value = "1234567890".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    // UUID validation
    group.bench_function("uuid_valid", |b| {
        let rule = ValidationRule::Uuid;
        let value = "550e8400-e29b-41d4-a716-446655440000".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    // In/NotIn validations
    group.bench_function("in_small_list", |b| {
        let rule = ValidationRule::In(vec![
            "red".to_string(),
            "green".to_string(),
            "blue".to_string(),
        ]);
        let value = "green".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    group.bench_function("in_large_list", |b| {
        let values: Vec<String> = (0..100).map(|i| format!("item_{}", i)).collect();
        let rule = ValidationRule::In(values);
        let value = "item_50".to_string();
        b.iter(|| Validator::validate_rule(&value, &rule, "field"))
    });

    group.finish();
}

// =============================================================================
// VALIDATOR BENCHMARKS
// =============================================================================

fn bench_validator(c: &mut Criterion) {
    let mut group = c.benchmark_group("validator");

    // Benchmark Validator static methods
    group.bench_function("is_valid_email_valid", |b| {
        b.iter(|| Validator::is_valid_email("test@example.com"))
    });

    group.bench_function("is_valid_email_invalid", |b| {
        b.iter(|| Validator::is_valid_email("invalid-email"))
    });

    group.bench_function("is_valid_url_valid", |b| {
        b.iter(|| Validator::is_valid_url("https://example.com/path"))
    });

    group.bench_function("is_valid_url_invalid", |b| {
        b.iter(|| Validator::is_valid_url("not-a-url"))
    });

    // Benchmark validate_rule for multiple fields
    group.bench_function("validate_rule_3_fields", |b| {
        let email_rule = ValidationRule::Email;
        let min_rule = ValidationRule::MinLength(2);
        let age_rule = ValidationRule::Min(18.0);
        let email = "test@example.com".to_string();
        let name = "John Doe".to_string();
        let age = "25".to_string();

        b.iter(|| {
            Validator::validate_rule(&email, &email_rule, "email");
            Validator::validate_rule(&name, &min_rule, "name");
            Validator::validate_rule(&age, &age_rule, "age");
        })
    });

    // Benchmark validate_rule for 6 fields
    group.bench_function("validate_rule_6_fields", |b| {
        let rules = vec![
            (
                ValidationRule::Email,
                "test@example.com".to_string(),
                "email",
            ),
            (
                ValidationRule::Alphanumeric,
                "johndoe123".to_string(),
                "username",
            ),
            (
                ValidationRule::MinLength(8),
                "securepassword".to_string(),
                "password",
            ),
            (ValidationRule::Range(18.0, 120.0), "30".to_string(), "age"),
            (
                ValidationRule::Url,
                "https://example.com".to_string(),
                "website",
            ),
            (
                ValidationRule::In(vec!["active".to_string()]),
                "active".to_string(),
                "status",
            ),
        ];

        b.iter(|| {
            for (rule, value, field) in &rules {
                Validator::validate_rule(value, rule, field);
            }
        })
    });

    group.finish();
}

// =============================================================================
// VALIDATION BUILDER BENCHMARKS
// =============================================================================

fn bench_validation_builder(c: &mut Criterion) {
    let mut group = c.benchmark_group("validation_builder");

    group.bench_function("build_simple", |b| {
        b.iter(|| ValidationBuilder::new("email").required().email().build())
    });

    group.bench_function("build_complex", |b| {
        b.iter(|| {
            ValidationBuilder::new("username")
                .required()
                .min_length(3)
                .max_length(20)
                .alphanumeric()
                .build()
        })
    });

    group.bench_function("build_numeric", |b| {
        b.iter(|| {
            ValidationBuilder::new("age")
                .required()
                .min(18.0)
                .max(120.0)
                .build()
        })
    });

    group.finish();
}

// =============================================================================
// VALIDATION ERRORS BENCHMARKS
// =============================================================================

fn bench_validation_errors(c: &mut Criterion) {
    let mut group = c.benchmark_group("validation_errors");

    group.bench_function("add_error", |b| {
        b.iter(|| {
            let mut errors = ValidationErrors::new();
            errors.add("email", "Invalid email format");
            errors
        })
    });

    group.bench_function("add_multiple_errors", |b| {
        b.iter(|| {
            let mut errors = ValidationErrors::new();
            for i in 0..10 {
                errors.add(format!("field_{}", i), format!("Error message {}", i));
            }
            errors
        })
    });

    group.bench_function("field_errors_lookup", |b| {
        let mut errors = ValidationErrors::new();
        for i in 0..100 {
            errors.add(format!("field_{}", i % 10), format!("Error {}", i));
        }

        b.iter(|| errors.field_errors("field_5"))
    });

    group.bench_function("display_formatting", |b| {
        let mut errors = ValidationErrors::new();
        errors.add("email", "Invalid email");
        errors.add("email", "Already taken");
        errors.add("password", "Too short");
        errors.add("username", "Required");

        b.iter(|| format!("{}", errors))
    });

    group.bench_function("has_errors_check", |b| {
        let mut errors = ValidationErrors::new();
        errors.add("field", "error");
        b.iter(|| errors.has_errors())
    });

    group.finish();
}

// =============================================================================
// THROUGHPUT BENCHMARKS
// =============================================================================

fn bench_validation_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("validation_throughput");

    let email_rule = ValidationRule::Email;
    let min_rule = ValidationRule::MinLength(2);
    let age_rule = ValidationRule::Min(18.0);

    for count in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*count as u64));

        let records: Vec<(String, String, String)> = (0..*count)
            .map(|i| {
                (
                    format!("user{}@example.com", i),
                    format!("User {}", i),
                    format!("{}", 20 + (i % 60)),
                )
            })
            .collect();

        group.bench_with_input(
            BenchmarkId::new("validate_batch", count),
            &records,
            |b, records| {
                b.iter(|| {
                    for (email, name, age) in records {
                        Validator::validate_rule(email, &email_rule, "email");
                        Validator::validate_rule(name, &min_rule, "name");
                        Validator::validate_rule(age, &age_rule, "age");
                    }
                })
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_validation_rules,
    bench_validator,
    bench_validation_builder,
    bench_validation_errors,
    bench_validation_throughput,
);

criterion_main!(benches);
