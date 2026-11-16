//! PostgreSQL integration tests: the scenarios every backend runs, in
//! `support/integration_parity.rs`; those of the server backends, in
//! `support/review_backend.rs`; and below, raw JSON decoding of PostgreSQL's
//! own types, native array columns, a `NULLS NOT DISTINCT` upsert, the
//! closing of statements a full statement cache evicts, a batch `coalesce`
//! on `json` and `jsonb` columns, and the width of a migration's integers.
//!
//! Opt-in: set `RUN_POSTGRES_TESTS`, `TEST_DATABASE_URL` or
//! `POSTGRESQL_DATABASE_URL`; `SKIP_POSTGRES_TESTS` turns them off again. Once
//! enabled, an unreachable server fails the run. The URL is `TEST_DATABASE_URL`,
//! then `POSTGRESQL_DATABASE_URL`, then
//! `postgres://postgres:postgres@localhost:5432/test_tide_orm`.
//!
//! Run with: cargo test --test postgres_integration_tests

use tideorm::Database;
use tideorm::prelude::DbValue;
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

#[path = "support/schema_script.rs"]
mod schema_script;

#[path = "support/integration_parity.rs"]
mod parity;

#[path = "support/catalog_parity.rs"]
mod catalog_parity;

#[path = "support/review_backend.rs"]
mod review_backend;

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

#[tokio::test]
async fn raw_json_preserves_postgres_nonfinite_float_arrays() {
    if !backend::connect().await {
        return;
    }
    let matrix = !parity::skip_on(
        &[tideorm::config::DatabaseServer::CockroachDb],
        "multidimensional array",
    );
    let db = tideorm::require_db().unwrap();
    let rows = db
        .query_raw_json(&format!(
            "SELECT ARRAY[1.5::real, 'NaN', 'Infinity', '-Infinity'] AS reals, \
                ARRAY[1.5::double precision, 'NaN', 'Infinity', '-Infinity'] AS doubles, \
                ARRAY[NULL, 'NaN', 'Infinity', '-Infinity']::double precision[] AS nullable, \
                {}ARRAY[]::double precision[] AS empty, NULL::real[] AS absent",
            if matrix {
                "ARRAY[[1.5::real, 'NaN'], ['Infinity'::real, '-Infinity']] AS matrix, "
            } else {
                ""
            }
        ))
        .await
        .unwrap();
    let mut expected = serde_json::json!({
        "reals": [1.5, "NaN", "inf", "-inf"],
        "doubles": [1.5, "NaN", "inf", "-inf"],
        "nullable": [null, "NaN", "inf", "-inf"],
        "empty": [],
        "absent": null
    });
    if matrix {
        expected["matrix"] = serde_json::json!([[1.5, "NaN"], ["inf", "-inf"]]);
    }
    assert_eq!(rows, vec![expected]);
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
    if !backend::connect().await
        || parity::skip_on(
            &[tideorm::config::DatabaseServer::CockroachDb],
            "NULLS NOT DISTINCT",
        )
    {
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

/// A statement a full cache evicts is closed on the server, in the round trip
/// of the next statement run: the server holds no more prepared statements
/// than the cache, and every statement still answers.
#[tokio::test]
async fn an_evicted_statement_is_closed_with_the_next_one_run() {
    if !test_config::should_run_postgres_tests() {
        return;
    }
    let db = Database::builder()
        .url(backend::database_url())
        .max_connections(1)
        .statement_cache_capacity(1)
        .build()
        .await
        .expect("failed to connect to PostgreSQL");
    for round in 0..3_i64 {
        for (sql, column) in [
            ("SELECT $1::int8 AS a", "a"),
            ("SELECT $1::int8 + 1 AS b", "b"),
        ] {
            let rows = db
                .query_raw_json_with_params(sql, [DbValue::from(round)])
                .await
                .expect("a statement runs after an eviction");
            assert_eq!(rows[0][column], round + i64::from(column == "b"), "{sql}");
        }
    }
    let prepared = db
        .query_raw_json_with_params(
            "SELECT count(*)::int8 AS n FROM pg_prepared_statements WHERE $1",
            [DbValue::from(true)],
        )
        .await
        .expect("prepared statements are listed");
    assert_eq!(prepared[0]["n"], 1, "only the cached statement is prepared");
    db.close().await.expect("the pool closes");
}

#[tideorm::model(table = "test_coalesced_documents")]
struct CoalescedDocument {
    #[tideorm(primary_key)]
    id: i64,
    doc: Option<serde_json::Value>,
    docb: Option<serde_json::Value>,
}

/// A batch `coalesce` fills a `json` column as it does a `jsonb` one, though
/// the default is bound as `jsonb`, which COALESCE cannot unify with `json`.
#[tokio::test]
async fn a_batch_coalesce_fills_json_and_jsonb_columns() {
    if !backend::connect().await {
        return;
    }
    Database::execute("DROP TABLE IF EXISTS test_coalesced_documents")
        .await
        .expect("failed to drop test_coalesced_documents");
    Database::execute(
        "CREATE TABLE test_coalesced_documents (
            id BIGINT PRIMARY KEY,
            doc JSON,
            docb JSONB
        )",
    )
    .await
    .expect("failed to create test_coalesced_documents");
    CoalescedDocument {
        id: 1,
        doc: None,
        docb: None,
    }
    .save()
    .await
    .expect("failed to save a document");

    let updated = CoalescedDocument::query()
        .where_eq("id", 1)
        .update_all()
        .coalesce("doc", serde_json::json!({"a": 1}))
        .coalesce("docb", serde_json::json!({"b": 2}))
        .execute()
        .await
        .expect("coalesce failed");
    assert_eq!(updated, 1);
    let document = CoalescedDocument::find(1)
        .await
        .expect("find failed")
        .expect("the document was stored");
    assert_eq!(document.doc, Some(serde_json::json!({"a": 1})));
    assert_eq!(document.docb, Some(serde_json::json!({"b": 2})));

    Database::execute("DROP TABLE test_coalesced_documents")
        .await
        .expect("failed to drop test_coalesced_documents");
}

/// A migration's integers keep their width on every server: CockroachDB reads
/// a bare `INT` or `SERIAL` as 64-bit, so migrations write `INT4`/`SERIAL4`,
/// and its connections leave `default_int_size` at 8, which at 4 capped every
/// sequence, `id()`'s too, at 2^31 - 1.
#[tokio::test]
async fn migration_integers_keep_their_width() {
    if !backend::connect().await {
        return;
    }
    parity::fresh_table("width_small_keys", |t| {
        t.increments("id");
        t.integer("score");
    })
    .await;
    parity::fresh_table("width_big_keys", |t| {
        t.id();
    })
    .await;

    let db = tideorm::require_db().expect("database should be available");
    for (table, expected) in [
        (
            "width_small_keys",
            &[("id", "integer"), ("score", "integer")][..],
        ),
        ("width_big_keys", &[("id", "bigint")]),
    ] {
        let columns = db.catalog().columns(table).await.expect("columns read");
        let columns: Vec<_> = columns
            .iter()
            .map(|column| (column.name.as_str(), column.data_type.as_str()))
            .collect();
        assert_eq!(columns, expected, "{table}");
    }

    let sequence = Database::raw_json(
        "SELECT CAST(maximum_value AS TEXT) AS maximum FROM information_schema.sequences \
         WHERE sequence_name = 'width_big_keys_id_seq'",
    )
    .await
    .expect("sequence read");
    assert_eq!(sequence[0]["maximum"], "9223372036854775807");

    Database::execute("DROP TABLE width_small_keys, width_big_keys")
        .await
        .expect("drop failed");
}

/// The catalog reads a table back by the name `tables()` lists: a table of
/// the current schema whose name holds a dot is listed bare, and another
/// schema's as `schema.table`.
#[tokio::test]
async fn catalog_reads_a_dotted_table_name_as_listed() {
    if !backend::connect().await {
        return;
    }
    for sql in [
        "DROP TABLE IF EXISTS \"catalog.dotted\"",
        "DROP SCHEMA IF EXISTS catalog_other CASCADE",
        "CREATE TABLE \"catalog.dotted\" (id INT PRIMARY KEY, label TEXT)",
        "CREATE SCHEMA catalog_other",
        "CREATE TABLE catalog_other.plain (code INT PRIMARY KEY)",
    ] {
        Database::execute(sql).await.expect(sql);
    }

    let db = tideorm::require_db().expect("database should be available");
    let catalog = db.catalog();
    let tables = catalog.tables().await.expect("tables read");
    for (table, columns) in [
        ("catalog.dotted", &["id", "label"][..]),
        ("catalog_other.plain", &["code"]),
    ] {
        assert!(
            tables.iter().any(|listed| listed == table),
            "{table} in {tables:?}"
        );
        let read: Vec<_> = catalog
            .columns(table)
            .await
            .expect("columns read")
            .into_iter()
            .map(|column| column.name)
            .collect();
        assert_eq!(read, columns, "{table}");
        let indexes = catalog.indexes(table).await.expect("indexes read");
        assert!(
            indexes.iter().any(|index| index.primary),
            "{table}: {indexes:?}"
        );
    }

    Database::execute("DROP TABLE \"catalog.dotted\"")
        .await
        .expect("drop failed");
    Database::execute("DROP SCHEMA catalog_other CASCADE")
        .await
        .expect("drop failed");
}

/// A notification for a `LISTEN` run as plain SQL, which no listener takes,
/// leaves the session's next statement alone.
#[tokio::test]
async fn an_unheard_notification_leaves_the_next_statement_alone() {
    if !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().expect("database should be available");
    // CockroachDB and YugabyteDB have no LISTEN.
    if db.server() != Some(tideorm::DatabaseServer::Postgres) {
        return;
    }
    let session = db.pin_session().await.expect("pin a session");
    session
        .exec_raw("LISTEN tideorm_unheard")
        .await
        .expect("listen");
    session
        .exec_raw("NOTIFY tideorm_unheard, 'payload'")
        .await
        .expect("notify");
    let rows = session
        .query_raw_json("SELECT 1 AS one")
        .await
        .expect("the statement after the notification");
    assert_eq!(rows[0]["one"], 1);
}
