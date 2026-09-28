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
use tideorm::prelude::Model;

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

#[tideorm::model(table = "test_array_documents")]
struct TestArrayDocument {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    title: String,
    tags: Vec<String>,
    ratings: Vec<i32>,
}

/// Native `TEXT[]` and `INTEGER[]` columns, which only PostgreSQL has; the
/// parity scenarios run the array filters over JSON arrays.
#[tokio::test]
async fn array_filters_read_native_array_columns() {
    if !backend::connect().await {
        return;
    }
    Database::execute("DROP TABLE IF EXISTS test_array_documents")
        .await
        .expect("failed to drop test_array_documents");
    Database::execute(
        "CREATE TABLE test_array_documents (
            id BIGSERIAL PRIMARY KEY,
            title TEXT NOT NULL,
            tags TEXT[] NOT NULL,
            ratings INTEGER[] NOT NULL
        )",
    )
    .await
    .expect("failed to create test_array_documents");

    for (title, tags, ratings) in [
        ("User Profile", ["user", "admin"], [5, 4, 5]),
        ("Guest Profile", ["user", "guest"], [3, 3, 4]),
        ("Moderator Profile", ["user", "moderator"], [4, 5, 4]),
    ] {
        TestArrayDocument {
            id: 0,
            title: title.to_string(),
            tags: tags.map(String::from).to_vec(),
            ratings: ratings.to_vec(),
        }
        .save()
        .await
        .expect("failed to save a document");
    }

    let titles = |docs: Vec<TestArrayDocument>| {
        let mut titles: Vec<String> = docs.into_iter().map(|doc| doc.title).collect();
        titles.sort();
        titles
    };

    let docs = TestArrayDocument::query()
        .where_array_contains("tags", vec!["admin".to_string()])
        .get()
        .await
        .expect("array contains failed");
    assert_eq!(titles(docs), ["User Profile"]);

    let docs = TestArrayDocument::query()
        .where_array_overlaps("tags", vec!["moderator".to_string(), "guest".to_string()])
        .get()
        .await
        .expect("array overlap failed");
    assert_eq!(titles(docs), ["Guest Profile", "Moderator Profile"]);

    let docs = TestArrayDocument::query()
        .where_array_contains("ratings", vec![5])
        .get()
        .await
        .expect("integer array contains failed");
    assert_eq!(titles(docs), ["Moderator Profile", "User Profile"]);

    Database::execute("DROP TABLE test_array_documents")
        .await
        .expect("failed to drop test_array_documents");
}

#[tideorm::model(table = "test_nulls_not_distinct_contacts")]
struct NullsNotDistinctContact {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    email: Option<String>,
    name: String,
}

/// Whether a NULL conflict value conflicts is the constraint's to decide: under
/// `NULLS NOT DISTINCT` a second NULL email is the stored row's, and the upsert
/// updates it rather than failing on a plain insert.
#[tokio::test]
async fn an_upsert_follows_a_nulls_not_distinct_constraint() {
    if !backend::connect().await {
        return;
    }
    Database::execute("DROP TABLE IF EXISTS test_nulls_not_distinct_contacts")
        .await
        .expect("failed to drop test_nulls_not_distinct_contacts");
    Database::execute(
        "CREATE TABLE test_nulls_not_distinct_contacts (
            id BIGSERIAL PRIMARY KEY,
            email TEXT UNIQUE NULLS NOT DISTINCT,
            name TEXT NOT NULL
        )",
    )
    .await
    .expect("failed to create test_nulls_not_distinct_contacts");

    let contact = |name: &str| NullsNotDistinctContact {
        id: 0,
        email: None,
        name: name.to_string(),
    };
    let first = NullsNotDistinctContact::insert_or_update(contact("first"), vec!["email"])
        .await
        .expect("the first upsert inserts");
    let second = NullsNotDistinctContact::insert_or_update(contact("second"), vec!["email"])
        .await
        .expect("the second upsert updates the row holding NULL");

    assert_eq!(second.id, first.id);
    assert_eq!(second.name, "second");
    assert_eq!(
        NullsNotDistinctContact::count()
            .await
            .expect("count failed"),
        1
    );

    Database::execute("DROP TABLE test_nulls_not_distinct_contacts")
        .await
        .expect("failed to drop test_nulls_not_distinct_contacts");
}
