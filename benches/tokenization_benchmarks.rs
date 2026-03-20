//! Benchmarks for Record Tokenization
//!
//! This module benchmarks the tokenization feature to ensure
//! it performs efficiently under various conditions.
//!
//! Run with: cargo bench --bench tokenization_benchmarks

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

// ============================================================================
// TOKENIZATION IMPLEMENTATION (mirrors TideORM's implementation)
// ============================================================================

const IV_SIZE: usize = 16;
const HMAC_SIZE: usize = 8;
const ID_SIZE: usize = 8;
const TOKEN_DATA_SIZE: usize = IV_SIZE + ID_SIZE + HMAC_SIZE;

/// Base64-URL encode without padding
#[inline]
fn base64_url_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut result = String::with_capacity((data.len() * 4).div_ceil(3));

    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;

        result.push(ALPHABET[b0 >> 2] as char);
        result.push(ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)] as char);

        if chunk.len() > 1 {
            result.push(ALPHABET[((b1 & 0x0f) << 2) | (b2 >> 6)] as char);
        }
        if chunk.len() > 2 {
            result.push(ALPHABET[b2 & 0x3f] as char);
        }
    }

    result
}

/// Base64-URL decode
#[inline]
fn base64_url_decode(input: &str) -> Option<Vec<u8>> {
    fn char_to_value(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }

    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return Some(Vec::new());
    }

    let mut result = Vec::with_capacity((bytes.len() * 3) / 4);

    let mut i = 0;
    while i < bytes.len() {
        let b0 = char_to_value(bytes[i])?;
        let b1 = if i + 1 < bytes.len() {
            char_to_value(bytes[i + 1])?
        } else {
            0
        };
        let b2 = if i + 2 < bytes.len() {
            char_to_value(bytes[i + 2])?
        } else {
            0
        };
        let b3 = if i + 3 < bytes.len() {
            char_to_value(bytes[i + 3])?
        } else {
            0
        };

        result.push((b0 << 2) | (b1 >> 4));
        if i + 2 < bytes.len() {
            result.push((b1 << 4) | (b2 >> 2));
        }
        if i + 3 < bytes.len() {
            result.push((b2 << 6) | b3);
        }

        i += 4;
    }

    Some(result)
}

/// Simple hash function for key derivation
#[inline]
fn derive_key(key: &str) -> [u8; 32] {
    let mut result = [0u8; 32];
    let key_bytes = key.as_bytes();

    for (i, &b) in key_bytes.iter().enumerate() {
        result[i % 32] ^= b;
        result[(i + 7) % 32] = result[(i + 7) % 32].wrapping_add(b);
        result[(i + 13) % 32] = result[(i + 13) % 32].wrapping_mul(b.wrapping_add(1));
    }

    for i in 0..32 {
        result[i] = result[i].wrapping_add(result[(i + 17) % 32]);
    }

    result
}

/// Generate pseudo-random IV
#[inline]
fn generate_iv() -> [u8; IV_SIZE] {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut hasher = DefaultHasher::new();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    now.as_nanos().hash(&mut hasher);
    std::process::id().hash(&mut hasher);

    let hash = hasher.finish();
    let mut iv = [0u8; IV_SIZE];

    for (i, item) in iv.iter_mut().enumerate().take(IV_SIZE) {
        *item = ((hash >> ((i % 8) * 8)) & 0xFF) as u8;
        *item ^= (i as u8).wrapping_mul(17);
    }

    iv
}

/// XOR encryption
#[inline]
fn xor_encrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, &b)| b ^ key[i % key.len()] ^ iv[i % iv.len()])
        .collect()
}

/// Compute HMAC for integrity
#[inline]
fn compute_hmac(data: &[u8], key: &[u8]) -> [u8; HMAC_SIZE] {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    key.hash(&mut hasher);

    let hash = hasher.finish();
    let mut hmac = [0u8; HMAC_SIZE];
    for (i, item) in hmac.iter_mut().enumerate().take(HMAC_SIZE) {
        *item = ((hash >> (i * 8)) & 0xFF) as u8;
    }
    hmac
}

/// Default token encoder
#[inline]
fn default_encode(id: i64, model_name: &str, key: &[u8]) -> String {
    let iv = generate_iv();
    let id_bytes = id.to_le_bytes();
    let encrypted = xor_encrypt(&id_bytes, key, &iv);

    let mut data_for_hmac = Vec::with_capacity(IV_SIZE + ID_SIZE + model_name.len());
    data_for_hmac.extend_from_slice(&iv);
    data_for_hmac.extend_from_slice(&encrypted);
    data_for_hmac.extend_from_slice(model_name.as_bytes());

    let hmac = compute_hmac(&data_for_hmac, key);

    let mut token_data = Vec::with_capacity(TOKEN_DATA_SIZE);
    token_data.extend_from_slice(&iv);
    token_data.extend_from_slice(&encrypted);
    token_data.extend_from_slice(&hmac);

    base64_url_encode(&token_data)
}

/// Default token decoder
#[inline]
fn default_decode(token: &str, model_name: &str, key: &[u8]) -> Option<i64> {
    let data = base64_url_decode(token)?;

    if data.len() != TOKEN_DATA_SIZE {
        return None;
    }

    let iv = &data[0..IV_SIZE];
    let encrypted = &data[IV_SIZE..IV_SIZE + ID_SIZE];
    let provided_hmac = &data[IV_SIZE + ID_SIZE..];

    let mut data_for_hmac = Vec::with_capacity(IV_SIZE + ID_SIZE + model_name.len());
    data_for_hmac.extend_from_slice(iv);
    data_for_hmac.extend_from_slice(encrypted);
    data_for_hmac.extend_from_slice(model_name.as_bytes());

    let expected_hmac = compute_hmac(&data_for_hmac, key);

    if provided_hmac != expected_hmac {
        return None;
    }

    let decrypted = xor_encrypt(encrypted, key, iv);

    let id_bytes: [u8; 8] = decrypted.try_into().ok()?;
    Some(i64::from_le_bytes(id_bytes))
}

// ============================================================================
// ALTERNATIVE ENCODERS FOR BENCHMARKING
// ============================================================================

/// Simple prefix-based encoder (no encryption)
#[inline]
fn simple_prefix_encode(id: i64, model_name: &str) -> String {
    format!("{}-{}", model_name.to_lowercase(), id)
}

/// Simple prefix-based decoder
#[inline]
fn simple_prefix_decode(token: &str, model_name: &str) -> Option<i64> {
    let prefix = format!("{}-", model_name.to_lowercase());
    token.strip_prefix(&prefix)?.parse().ok()
}

/// Hex-based encoder (lightweight obfuscation)
#[inline]
fn hex_encode(id: i64, model_name: &str) -> String {
    let combined = format!("{}{:016x}", model_name, id);
    hex::encode(combined)
}

/// Hex-based decoder
#[inline]
fn hex_decode(token: &str, model_name: &str) -> Option<i64> {
    let decoded = hex::decode(token).ok()?;
    let s = String::from_utf8(decoded).ok()?;
    if !s.starts_with(model_name) {
        return None;
    }
    let hex_part = &s[model_name.len()..];
    i64::from_str_radix(hex_part, 16).ok()
}

// Simple hex implementation for benchmark (no external dependency)
mod hex {
    pub fn encode(data: impl AsRef<str>) -> String {
        data.as_ref()
            .as_bytes()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }

    pub fn decode(s: &str) -> Result<Vec<u8>, ()> {
        if s.len() % 2 != 0 {
            return Err(());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
            .collect()
    }
}

// ============================================================================
// BENCHMARKS
// ============================================================================

fn bench_base64_encoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("base64_encoding");

    // Various data sizes
    let sizes = [8, 16, 32, 64, 128, 256];

    for size in sizes {
        let data: Vec<u8> = (0..size).map(|i| i as u8).collect();

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("encode", size), &data, |b, data| {
            b.iter(|| black_box(base64_url_encode(black_box(data))))
        });
    }

    group.finish();
}

fn bench_base64_decoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("base64_decoding");

    let sizes = [8, 16, 32, 64, 128, 256];

    for size in sizes {
        let data: Vec<u8> = (0..size).map(|i| i as u8).collect();
        let encoded = base64_url_encode(&data);

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("decode", size), &encoded, |b, encoded| {
            b.iter(|| black_box(base64_url_decode(black_box(encoded))))
        });
    }

    group.finish();
}

fn bench_key_derivation(c: &mut Criterion) {
    let mut group = c.benchmark_group("key_derivation");

    let keys = [
        "short",
        "medium-length-key",
        "this-is-a-longer-encryption-key-for-testing",
        "very-long-encryption-key-that-exceeds-32-characters-significantly",
    ];

    for key in keys {
        group.bench_with_input(BenchmarkId::new("derive", key.len()), &key, |b, key| {
            b.iter(|| black_box(derive_key(black_box(key))))
        });
    }

    group.finish();
}

fn bench_encryption(c: &mut Criterion) {
    let mut group = c.benchmark_group("xor_encryption");

    let key = derive_key("benchmark-encryption-key-32-char!");
    let iv = generate_iv();

    let sizes = [8, 16, 32, 64, 128];

    for size in sizes {
        let data: Vec<u8> = (0..size).map(|i| i as u8).collect();

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("encrypt", size), &data, |b, data| {
            b.iter(|| black_box(xor_encrypt(black_box(data), &key, &iv)))
        });
    }

    group.finish();
}

fn bench_hmac_computation(c: &mut Criterion) {
    let mut group = c.benchmark_group("hmac_computation");

    let key = derive_key("benchmark-hmac-key-32-characters!");

    let sizes = [16, 32, 64, 128, 256];

    for size in sizes {
        let data: Vec<u8> = (0..size).map(|i| i as u8).collect();

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("compute", size), &data, |b, data| {
            b.iter(|| black_box(compute_hmac(black_box(data), &key)))
        });
    }

    group.finish();
}

fn bench_token_encoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("token_encoding");

    let key = derive_key("benchmark-token-key-32-characters!");

    // Test with various IDs
    let ids = [1i64, 100, 999999, i64::MAX, -1, 0];

    for id in ids {
        group.bench_with_input(BenchmarkId::new("default_encode", id), &id, |b, &id| {
            b.iter(|| black_box(default_encode(black_box(id), "BenchmarkModel", &key)))
        });
    }

    // Compare with simple prefix encoder
    for id in ids {
        group.bench_with_input(BenchmarkId::new("simple_prefix", id), &id, |b, &id| {
            b.iter(|| black_box(simple_prefix_encode(black_box(id), "BenchmarkModel")))
        });
    }

    // Compare with hex encoder
    for id in ids {
        group.bench_with_input(BenchmarkId::new("hex_encode", id), &id, |b, &id| {
            b.iter(|| black_box(hex_encode(black_box(id), "BenchmarkModel")))
        });
    }

    group.finish();
}

fn bench_token_decoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("token_decoding");

    let key = derive_key("benchmark-token-key-32-characters!");

    // Pre-generate tokens
    let ids = [1i64, 100, 999999, i64::MAX];
    let tokens: Vec<_> = ids
        .iter()
        .map(|&id| default_encode(id, "BenchmarkModel", &key))
        .collect();

    for (id, token) in ids.iter().zip(tokens.iter()) {
        group.bench_with_input(BenchmarkId::new("default_decode", id), token, |b, token| {
            b.iter(|| black_box(default_decode(black_box(token), "BenchmarkModel", &key)))
        });
    }

    // Compare with simple prefix decoder
    let prefix_tokens: Vec<_> = ids
        .iter()
        .map(|&id| simple_prefix_encode(id, "BenchmarkModel"))
        .collect();
    for (id, token) in ids.iter().zip(prefix_tokens.iter()) {
        group.bench_with_input(BenchmarkId::new("simple_prefix", id), token, |b, token| {
            b.iter(|| black_box(simple_prefix_decode(black_box(token), "BenchmarkModel")))
        });
    }

    // Compare with hex decoder
    let hex_tokens: Vec<_> = ids
        .iter()
        .map(|&id| hex_encode(id, "BenchmarkModel"))
        .collect();
    for (id, token) in ids.iter().zip(hex_tokens.iter()) {
        group.bench_with_input(BenchmarkId::new("hex_decode", id), token, |b, token| {
            b.iter(|| black_box(hex_decode(black_box(token), "BenchmarkModel")))
        });
    }

    group.finish();
}

fn bench_round_trip(c: &mut Criterion) {
    let mut group = c.benchmark_group("token_round_trip");

    let key = derive_key("benchmark-round-trip-key-32chars!");

    let ids = [42i64, 1000000, i64::MAX];

    for id in ids {
        group.bench_with_input(BenchmarkId::new("encode_decode", id), &id, |b, &id| {
            b.iter(|| {
                let token = default_encode(id, "RoundTrip", &key);
                black_box(default_decode(&token, "RoundTrip", &key))
            })
        });
    }

    group.finish();
}

fn bench_model_specificity(c: &mut Criterion) {
    let mut group = c.benchmark_group("model_specificity");

    let key = derive_key("benchmark-model-specificity-key!");
    let id = 42i64;

    let models = [
        "User",
        "Product",
        "Order",
        "Invoice",
        "VeryLongModelNameForTesting",
    ];

    for model in models {
        group.bench_with_input(BenchmarkId::new("encode", model), model, |b, model| {
            b.iter(|| black_box(default_encode(id, black_box(model), &key)))
        });
    }

    // Pre-generate tokens for each model
    let tokens: Vec<_> = models.iter().map(|m| default_encode(id, m, &key)).collect();

    for (model, token) in models.iter().zip(tokens.iter()) {
        group.bench_with_input(
            BenchmarkId::new("decode", model),
            &(model, token),
            |b, (model, token)| {
                b.iter(|| black_box(default_decode(black_box(token), black_box(model), &key)))
            },
        );
    }

    group.finish();
}

fn bench_invalid_tokens(c: &mut Criterion) {
    let mut group = c.benchmark_group("invalid_token_rejection");

    let key = derive_key("benchmark-invalid-token-key-32c!");

    let invalid_tokens = [
        ("empty", ""),
        ("too_short", "abc"),
        ("invalid_base64", "!!!invalid!!!"),
        ("wrong_length", "YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXo"),
        ("random", "abcdefghijklmnopqrstuvwxyzABCDEF"),
    ];

    for (name, token) in invalid_tokens {
        group.bench_with_input(BenchmarkId::new("reject", name), &token, |b, token| {
            b.iter(|| black_box(default_decode(black_box(token), "TestModel", &key)))
        });
    }

    group.finish();
}

fn bench_tamper_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("tamper_detection");

    let key = derive_key("benchmark-tamper-detection-key!!");
    let id = 12345i64;
    let token = default_encode(id, "TamperTest", &key);

    // Create tampered version by flipping a character
    let mut tampered_bytes: Vec<u8> = token.bytes().collect();
    if tampered_bytes.len() > 5 {
        tampered_bytes[5] = if tampered_bytes[5] == b'a' {
            b'b'
        } else {
            b'a'
        };
    }
    let tampered_char = String::from_utf8(tampered_bytes).unwrap_or_default();

    group.bench_function("valid_token", |b| {
        b.iter(|| black_box(default_decode(black_box(&token), "TamperTest", &key)))
    });

    group.bench_function("tampered_token", |b| {
        b.iter(|| {
            black_box(default_decode(
                black_box(&tampered_char),
                "TamperTest",
                &key,
            ))
        })
    });

    // Wrong model name (should fail HMAC)
    group.bench_function("wrong_model", |b| {
        b.iter(|| black_box(default_decode(black_box(&token), "WrongModel", &key)))
    });

    group.finish();
}

fn bench_batch_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_operations");

    let key = derive_key("benchmark-batch-operations-key!!");
    let ids: Vec<i64> = (1..=100).collect();

    group.throughput(Throughput::Elements(100));

    group.bench_function("batch_encode_100", |b| {
        b.iter(|| {
            black_box(
                ids.iter()
                    .map(|&id| default_encode(id, "BatchModel", &key))
                    .collect::<Vec<_>>(),
            )
        })
    });

    let tokens: Vec<_> = ids
        .iter()
        .map(|&id| default_encode(id, "BatchModel", &key))
        .collect();

    group.bench_function("batch_decode_100", |b| {
        b.iter(|| {
            black_box(
                tokens
                    .iter()
                    .map(|t| default_decode(t, "BatchModel", &key))
                    .collect::<Vec<_>>(),
            )
        })
    });

    group.finish();
}

fn bench_key_lengths(c: &mut Criterion) {
    let mut group = c.benchmark_group("key_length_impact");

    let keys = [
        ("16_chars", "1234567890123456"),
        ("32_chars", "12345678901234567890123456789012"),
        (
            "64_chars",
            "1234567890123456789012345678901234567890123456789012345678901234",
        ),
        (
            "128_chars",
            "12345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678",
        ),
    ];

    let id = 42i64;

    for (name, key_str) in keys {
        let key = derive_key(key_str);

        group.bench_with_input(BenchmarkId::new("encode", name), &key, |b, key| {
            b.iter(|| black_box(default_encode(id, "KeyTest", key)))
        });

        let token = default_encode(id, "KeyTest", &key);
        group.bench_with_input(
            BenchmarkId::new("decode", name),
            &(key, token),
            |b, (key, token)| {
                b.iter(|| black_box(default_decode(black_box(token), "KeyTest", key)))
            },
        );
    }

    group.finish();
}

fn bench_iv_generation(c: &mut Criterion) {
    c.bench_function("iv_generation", |b| b.iter(|| black_box(generate_iv())));
}

criterion_group!(
    benches,
    bench_base64_encoding,
    bench_base64_decoding,
    bench_key_derivation,
    bench_encryption,
    bench_hmac_computation,
    bench_token_encoding,
    bench_token_decoding,
    bench_round_trip,
    bench_model_specificity,
    bench_invalid_tokens,
    bench_tamper_detection,
    bench_batch_operations,
    bench_key_lengths,
    bench_iv_generation,
);

criterion_main!(benches);
