//! Benchmarks for TideORM tokenization APIs.
//!
//! These benches exercise the crate's real tokenization surface instead of a
//! separate local implementation.

use async_trait::async_trait;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::OnceLock;
use tideorm::{Error, Result, TokenConfig, Tokenizable};

const MODEL_NAME: &str = "TokenBenchModel";
const ENCRYPTION_KEY: &str = "bench-tokenization-key-32-characters-minimum";

static TOKENIZATION_READY: OnceLock<()> = OnceLock::new();

#[derive(Clone)]
struct TokenBenchModel {
    id: i64,
}

#[async_trait]
impl Tokenizable for TokenBenchModel {
    type TokenPrimaryKey = i64;

    fn token_model_name() -> &'static str {
        MODEL_NAME
    }

    fn token_primary_key(&self) -> Self::TokenPrimaryKey {
        self.id
    }

    async fn from_token(_token: &str) -> Result<Self> {
        Err(Error::tokenization(
            "Database-backed token lookup is not part of benchmark coverage",
        ))
    }
}

fn init_tokenization() {
    TOKENIZATION_READY.get_or_init(|| {
        TokenConfig::reset();
        TokenConfig::set_encryption_key(ENCRYPTION_KEY);
    });
}

fn serialized_id(id: i64) -> String {
    serde_json::to_string(&id).expect("Failed to serialize benchmark token payload")
}

fn encode_payload(id: i64, model_name: &str) -> String {
    TokenConfig::encode(&serialized_id(id), model_name).expect("Failed to encode benchmark token")
}

fn tamper_token(token: &str) -> String {
    let mut chars: Vec<char> = token.chars().collect();
    if let Some(ch) = chars.get_mut(5) {
        *ch = if *ch == 'a' { 'b' } else { 'a' };
    }
    chars.into_iter().collect()
}

fn bench_token_config_encoding(c: &mut Criterion) {
    init_tokenization();

    let mut group = c.benchmark_group("token_config_encoding");

    for id in [1_i64, 42, 999_999, i64::MAX] {
        let payload = serialized_id(id);
        group.bench_with_input(BenchmarkId::new("encode", id), &payload, |b, payload| {
            b.iter(|| {
                black_box(
                    TokenConfig::encode(black_box(payload), black_box(MODEL_NAME))
                        .expect("TokenConfig::encode failed"),
                )
            })
        });
    }

    group.finish();
}

fn bench_token_config_decoding(c: &mut Criterion) {
    init_tokenization();

    let mut group = c.benchmark_group("token_config_decoding");

    for id in [1_i64, 42, 999_999, i64::MAX] {
        let token = encode_payload(id, MODEL_NAME);
        group.bench_with_input(BenchmarkId::new("decode", id), &token, |b, token| {
            b.iter(|| {
                black_box(
                    TokenConfig::decode(black_box(token), black_box(MODEL_NAME))
                        .expect("TokenConfig::decode failed"),
                )
            })
        });
    }

    group.finish();
}

fn bench_tokenizable_helpers(c: &mut Criterion) {
    init_tokenization();

    let model = TokenBenchModel { id: 4242 };
    let token = model
        .tokenize()
        .expect("Failed to build token for helper bench");

    let mut group = c.benchmark_group("tokenizable_helpers");

    group.bench_function("instance_tokenize", |b| {
        b.iter(|| black_box(model.tokenize().expect("tokenize failed")))
    });

    group.bench_function("static_tokenize_id", |b| {
        b.iter(|| {
            black_box(TokenBenchModel::tokenize_id(black_box(4242)).expect("tokenize_id failed"))
        })
    });

    group.bench_function("decode_token", |b| {
        b.iter(|| {
            black_box(
                TokenBenchModel::decode_token(black_box(&token)).expect("decode_token failed"),
            )
        })
    });

    group.bench_function("regenerate_token", |b| {
        b.iter(|| black_box(model.regenerate_token().expect("regenerate_token failed")))
    });

    group.finish();
}

fn bench_invalid_token_handling(c: &mut Criterion) {
    init_tokenization();

    let valid_token = encode_payload(99, MODEL_NAME);
    let tampered_token = tamper_token(&valid_token);

    let mut group = c.benchmark_group("invalid_token_handling");

    for (name, token) in [
        ("empty", String::new()),
        (
            "truncated",
            valid_token[..valid_token.len() / 2].to_string(),
        ),
        ("tampered", tampered_token),
        ("invalid_base64", "***not-a-token***".to_string()),
    ] {
        group.bench_with_input(BenchmarkId::new("decode", name), &token, |b, token| {
            b.iter(|| {
                black_box(
                    TokenConfig::decode(black_box(token), black_box(MODEL_NAME))
                        .expect("Invalid-token decode path should not error"),
                )
            })
        });
    }

    group.bench_function("wrong_model", |b| {
        b.iter(|| {
            black_box(
                TokenConfig::decode(black_box(&valid_token), black_box("OtherTokenModel"))
                    .expect("Wrong-model decode path should not error"),
            )
        })
    });

    group.finish();
}

fn bench_batch_round_trip(c: &mut Criterion) {
    init_tokenization();

    let ids: Vec<i64> = (1..=100).collect();
    let tokens: Vec<String> = ids
        .iter()
        .map(|id| TokenBenchModel::tokenize_id(*id).expect("Failed to precompute benchmark token"))
        .collect();

    let mut group = c.benchmark_group("token_batch_round_trip");
    group.throughput(Throughput::Elements(ids.len() as u64));

    group.bench_function("encode_100", |b| {
        b.iter(|| {
            black_box(
                ids.iter()
                    .map(|id| TokenBenchModel::tokenize_id(*id).expect("batch encode failed"))
                    .collect::<Vec<_>>(),
            )
        })
    });

    group.bench_function("decode_100", |b| {
        b.iter(|| {
            black_box(
                tokens
                    .iter()
                    .map(|token| TokenBenchModel::decode_token(token).expect("batch decode failed"))
                    .collect::<Vec<_>>(),
            )
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_token_config_encoding,
    bench_token_config_decoding,
    bench_tokenizable_helpers,
    bench_invalid_token_handling,
    bench_batch_round_trip,
);

criterion_main!(benches);
