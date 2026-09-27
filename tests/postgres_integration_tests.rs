//! PostgreSQL integration tests: the shared scenarios in
//! `support/integration_parity.rs` plus PostgreSQL-only type decoding.
//!
//! Opt-in: set `RUN_POSTGRES_TESTS`, `TEST_DATABASE_URL` or
//! `POSTGRESQL_DATABASE_URL`; `SKIP_POSTGRES_TESTS` turns them off again. Once
//! enabled, an unreachable server fails the run. The URL is `TEST_DATABASE_URL`,
//! then `POSTGRESQL_DATABASE_URL`, then
//! `postgres://postgres:postgres@localhost:5432/test_tide_orm`.
//!
//! Run with: cargo test --test postgres_integration_tests

use tideorm::Database;

#[path = "support/postgres_test_config.rs"]
mod test_config;

mod backend {
    use tideorm::TideConfig;
    use tideorm::config::DatabaseType;

    pub const DATABASE_TYPE: DatabaseType = DatabaseType::Postgres;

    pub fn database_url() -> &'static str {
        super::test_config::test_database_url()
    }

    pub async fn connect() -> bool {
        if !super::test_config::should_run_postgres_tests() {
            println!("{}", super::test_config::SKIPPED);
            return false;
        }
        TideConfig::init()
            .database(super::test_config::test_database_url())
            .max_connections(10)
            .connect()
            .await
            .expect("failed to connect to PostgreSQL");
        true
    }
}

#[path = "support/integration_parity.rs"]
mod parity;

#[tokio::test]
async fn raw_json_preserves_postgres_types() {
    if !backend::connect().await {
        return;
    }
    Database::execute("DROP TABLE IF EXISTS test_raw_json_types")
        .await
        .expect("failed to drop test_raw_json_types");
    Database::execute(
        "CREATE TABLE test_raw_json_types (
            id BIGSERIAL PRIMARY KEY,
            enabled BOOLEAN NOT NULL,
            payload JSONB NOT NULL,
            amount NUMERIC(10,2) NOT NULL,
            created_at TIMESTAMPTZ NOT NULL,
            uuid_value UUID NOT NULL
        )",
    )
    .await
    .expect("failed to create test_raw_json_types");

    let probe_uuid = uuid::Uuid::parse_str("6d8f4a4e-5f60-4c5f-b8fb-7ddc7310df2a")
        .expect("UUID literal should parse");
    let db = tideorm::require_db().expect("database should be available");

    db.__execute_with_params(
        "INSERT INTO test_raw_json_types (enabled, payload, amount, created_at, uuid_value) VALUES ($1, $2, $3::numeric, $4::timestamptz, $5::uuid)",
        vec![
            tideorm::internal::Value::Bool(Some(true)),
            tideorm::internal::Value::Json(Some(Box::new(serde_json::json!({
                "kind": "probe",
                "count": 2
            })))),
            tideorm::internal::Value::String(Some("12.34".to_string())),
            tideorm::internal::Value::String(Some("2026-03-21T10:11:12+00:00".to_string())),
            tideorm::internal::Value::String(Some(probe_uuid.to_string())),
        ],
    )
    .await
    .expect("typed raw-json probe insert should succeed");

    let rows = db
        .__raw_json_with_params(
            "SELECT enabled, payload, amount, created_at, uuid_value FROM test_raw_json_types ORDER BY id ASC",
            vec![],
        )
        .await
        .expect("typed raw-json probe query should succeed");

    assert_eq!(
        rows,
        vec![serde_json::json!({
            "enabled": true,
            "payload": {
                "kind": "probe",
                "count": 2
            },
            "amount": serde_json::to_value(
                rust_decimal::Decimal::from_str_exact("12.34")
                    .expect("decimal literal should parse")
            ).expect("decimal should serialize to JSON"),
            "created_at": serde_json::to_value(
                chrono::DateTime::parse_from_rfc3339("2026-03-21T10:11:12+00:00")
                    .expect("timestamp literal should parse")
            ).expect("timestamp should serialize to JSON"),
            "uuid_value": serde_json::to_value(probe_uuid)
                .expect("uuid should serialize to JSON"),
        })]
    );
}
