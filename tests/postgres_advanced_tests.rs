//! PostgreSQL-only integration tests: JSONB and array operators, joins, and
//! relation loading.
//!
//! Opt-in like `postgres_integration_tests`: set `RUN_POSTGRES_TESTS`,
//! `TEST_DATABASE_URL` or `POSTGRESQL_DATABASE_URL`. Once enabled, an
//! unreachable server fails the run.
//!
//! Run with: cargo test --test postgres_advanced_tests

use tideorm::prelude::*;
use tideorm::relations::{BelongsTo, HasMany, HasOne};
use tideorm::{Database, TideConfig};

#[path = "postgres_advanced_tests/relations.rs"]
mod relations;
#[path = "support/postgres_test_config.rs"]
mod test_config;

use test_config::test_database_url;

#[tideorm::model(table = "test_documents")]
pub struct TestDocument {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub metadata: serde_json::Value,
    pub tags: Vec<String>,
    pub ratings: Vec<i32>,
}

#[tideorm::model(table = "test_authors")]
pub struct TestAuthor {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub country: String,

    #[tideorm(has_many = "TestBook", foreign_key = "author_id")]
    pub books: HasMany<TestBook>,
}

#[tideorm::model(table = "test_books")]
pub struct TestBook {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub author_id: i64,
    pub title: String,
    pub year: i32,

    #[tideorm(belongs_to = "TestAuthor", foreign_key = "author_id")]
    pub author: BelongsTo<TestAuthor>,

    #[tideorm(has_one = "TestBookDetail", foreign_key = "book_id")]
    pub detail: HasOne<TestBookDetail>,
}

#[tideorm::model(table = "test_book_details")]
pub struct TestBookDetail {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub book_id: i64,
    pub isbn: String,
    pub pages: i32,

    #[tideorm(belongs_to = "TestBook", foreign_key = "book_id")]
    pub book: BelongsTo<TestBook>,
}

#[tokio::test]
async fn postgres_advanced_tests() {
    if !test_config::should_run_postgres_tests() {
        println!("{}", test_config::SKIPPED);
        return;
    }
    TideConfig::init()
        .database(test_database_url())
        .max_connections(10)
        .min_connections(2)
        .connect()
        .await
        .expect("Failed to connect to database");

    setup_tables().await;
    test_json_operators().await;
    test_array_operators().await;
    relations::test_relations().await;
    cleanup_tables().await;
}

async fn setup_tables() {
    cleanup_tables().await;

    Database::execute(
        r#"
        CREATE TABLE test_documents (
            id BIGSERIAL PRIMARY KEY,
            title VARCHAR(255) NOT NULL,
            metadata JSONB NOT NULL,
            tags TEXT[] NOT NULL,
            ratings INTEGER[] NOT NULL
        )
    "#,
    )
    .await
    .expect("Failed to create test_documents table");

    Database::execute(
        r#"
        CREATE TABLE test_authors (
            id BIGSERIAL PRIMARY KEY,
            name VARCHAR(255) NOT NULL,
            country VARCHAR(100) NOT NULL
        )
    "#,
    )
    .await
    .expect("Failed to create test_authors table");

    Database::execute(
        r#"
        CREATE TABLE test_books (
            id BIGSERIAL PRIMARY KEY,
            author_id BIGINT NOT NULL,
            title VARCHAR(255) NOT NULL,
            year INTEGER NOT NULL
        )
    "#,
    )
    .await
    .expect("Failed to create test_books table");

    Database::execute(
        r#"
        CREATE TABLE test_book_details (
            id BIGSERIAL PRIMARY KEY,
            book_id BIGINT NOT NULL,
            isbn VARCHAR(50) NOT NULL,
            pages INTEGER NOT NULL
        )
    "#,
    )
    .await
    .expect("Failed to create test_book_details table");
}

async fn cleanup_tables() {
    for table in [
        "test_book_details",
        "test_books",
        "test_authors",
        "test_documents",
    ] {
        Database::execute(&format!("DROP TABLE IF EXISTS {table} CASCADE"))
            .await
            .expect("Failed to drop test table");
    }
}

async fn test_json_operators() {
    let docs = vec![
        TestDocument {
            id: 0,
            title: "User Profile".into(),
            metadata: json!({
                "role": "admin",
                "settings": {
                    "theme": "dark",
                    "notifications": true
                },
                "age": 30
            }),
            tags: vec!["user".into(), "admin".into()],
            ratings: vec![5, 4, 5],
        },
        TestDocument {
            id: 0,
            title: "Guest Profile".into(),
            metadata: json!({
                "role": "guest",
                "settings": {
                    "theme": "light",
                    "notifications": false
                },
                "age": 25
            }),
            tags: vec!["user".into(), "guest".into()],
            ratings: vec![3, 3, 4],
        },
        TestDocument {
            id: 0,
            title: "Moderator Profile".into(),
            metadata: json!({
                "role": "moderator",
                "settings": {
                    "theme": "dark",
                    "notifications": true
                },
                "permissions": ["read", "write", "moderate"]
            }),
            tags: vec!["user".into(), "moderator".into()],
            ratings: vec![4, 5, 4],
        },
    ];

    for doc in docs {
        doc.save().await.expect("Failed to save document");
    }

    let docs = TestDocument::query()
        .where_json_contains("metadata", json!({"role": "admin"}))
        .get()
        .await
        .expect("Query failed");
    assert_eq!(docs.len(), 1, "Should find 1 admin document");
    assert_eq!(docs[0].title, "User Profile");

    // Only the admin document is a subset of this object: the guest one has a
    // different role and the moderator one an extra key.
    let docs = TestDocument::query()
        .where_json_contained_by(
            "metadata",
            json!({
                "role": "admin",
                "settings": {
                    "theme": "dark",
                    "notifications": true
                },
                "age": 30,
                "extra": "ignored"
            }),
        )
        .get()
        .await
        .expect("Query failed");
    assert_eq!(docs.len(), 1, "only the admin metadata is contained");
    assert_eq!(docs[0].title, "User Profile");

    let docs = TestDocument::query()
        .where_json_key_exists("metadata", "permissions")
        .get()
        .await
        .expect("Query failed");
    assert_eq!(docs.len(), 1, "Should find 1 document with permissions key");
    assert_eq!(docs[0].title, "Moderator Profile");

    let docs = TestDocument::query()
        .where_json_path_exists("metadata", "$.settings.theme")
        .get()
        .await
        .expect("Query failed");
    assert_eq!(docs.len(), 3, "All documents should have settings.theme");
}

async fn test_array_operators() {
    let docs = TestDocument::query()
        .where_array_contains("tags", vec!["admin".to_string()])
        .get()
        .await
        .expect("Query failed");
    assert_eq!(docs.len(), 1, "Should find 1 document with admin tag");
    assert_eq!(docs[0].title, "User Profile");

    let docs = TestDocument::query()
        .where_array_overlaps("tags", vec!["moderator".to_string(), "guest".to_string()])
        .get()
        .await
        .expect("Query failed");
    assert_eq!(docs.len(), 2, "moderator and guest documents overlap");

    let docs = TestDocument::query()
        .where_array_contains_any("tags", vec!["admin".to_string(), "guest".to_string()])
        .get()
        .await
        .expect("Query failed");
    assert_eq!(docs.len(), 2, "admin and guest documents");

    let docs = TestDocument::query()
        .where_array_contains("ratings", vec![5])
        .get()
        .await
        .expect("Query failed");
    assert_eq!(docs.len(), 2, "user and moderator documents rate a 5");
}
