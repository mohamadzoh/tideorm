//! Advanced PostgreSQL Integration Tests for TideORM
//!
//! Tests for JSON/JSONB operators, array operators, and relations.
//!
//! These tests require a running PostgreSQL instance with:
//! - Host: localhost
//! - Port: 5432
//! - User: postgres
//! - Password: postgres
//! - Database: test_tide_orm
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

// =============================================================================
// TEST MODELS WITH JSON AND ARRAY COLUMNS
// =============================================================================

#[tideorm::model(table = "test_documents")]
pub struct TestDocument {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub metadata: serde_json::Value, // JSONB column
    pub tags: Vec<String>,           // Array column
    pub ratings: Vec<i32>,           // Array of integers
}

#[tideorm::model(table = "test_authors")]
pub struct TestAuthor {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub country: String,

    // HasMany relation - author has many books
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

    // BelongsTo relation - book belongs to an author
    #[tideorm(belongs_to = "TestAuthor", foreign_key = "author_id")]
    pub author: BelongsTo<TestAuthor>,

    // HasOne relation - book has one detail record
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

    // BelongsTo relation - detail belongs to a book
    #[tideorm(belongs_to = "TestBook", foreign_key = "book_id")]
    pub book: BelongsTo<TestBook>,
}

// =============================================================================
// MAIN TEST SUITE
// =============================================================================

#[tokio::test]
async fn postgres_advanced_tests() {
    println!(" Starting Advanced PostgreSQL Integration Tests...\n");

    // Setup database
    TideConfig::init()
        .database(test_database_url())
        .max_connections(10)
        .min_connections(2)
        .connect()
        .await
        .expect("Failed to connect to database");

    // =========================================================================
    // SETUP: CREATE TABLES
    // =========================================================================
    setup_tables().await;

    // =========================================================================
    // JSON/JSONB OPERATOR TESTS
    // =========================================================================
    test_json_operators().await;

    // =========================================================================
    // ARRAY OPERATOR TESTS
    // =========================================================================
    test_array_operators().await;

    // =========================================================================
    // RELATION TESTS
    // =========================================================================
    relations::test_relations().await;

    // =========================================================================
    // CLEANUP
    // =========================================================================
    cleanup_tables().await;

    println!("\n All advanced PostgreSQL tests passed!\n");
}

// =============================================================================
// SETUP & CLEANUP FUNCTIONS
// =============================================================================

async fn setup_tables() {
    println!("📋 Setting up test tables...");

    // Drop existing tables
    let _ = Database::execute("DROP TABLE IF EXISTS test_book_details CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_books CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_authors CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_documents CASCADE").await;

    // Create documents table with JSONB and array columns
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

    // Create authors table
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

    // Create books table
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

    // Create book_details table
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

    println!("   ✓ Tables created\n");
}

async fn cleanup_tables() {
    println!("🧹 Cleaning up test tables...");
    let _ = Database::execute("DROP TABLE IF EXISTS test_book_details CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_books CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_authors CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_documents CASCADE").await;
}

// =============================================================================
// JSON/JSONB TESTS
// =============================================================================

async fn test_json_operators() {
    println!(" Testing: JSON/JSONB Operators");

    // Seed test data
    let _ = Database::execute("TRUNCATE TABLE test_documents RESTART IDENTITY CASCADE").await;

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

    // Test JSON contains (@>)
    {
        let docs = TestDocument::query()
            .where_json_contains("metadata", json!({"role": "admin"}))
            .get()
            .await
            .expect("Query failed");

        assert_eq!(docs.len(), 1, "Should find 1 admin document");
        assert_eq!(docs[0].title, "User Profile");
        println!("   ✓ where_json_contains");
    }

    // Test JSON contained by (<@)
    {
        let search_obj = json!({
            "role": "admin",
            "settings": {
                "theme": "dark",
                "notifications": true
            },
            "age": 30,
            "extra": "ignored"
        });

        let _docs = TestDocument::query()
            .where_json_contained_by("metadata", search_obj)
            .get()
            .await
            .expect("Query failed");

        // Query executed successfully
        println!("   ✓ where_json_contained_by");
    }

    // Test JSON key exists (?)
    {
        let docs = TestDocument::query()
            .where_json_key_exists("metadata", "permissions")
            .get()
            .await
            .expect("Query failed");

        assert_eq!(docs.len(), 1, "Should find 1 document with permissions key");
        assert_eq!(docs[0].title, "Moderator Profile");
        println!("   ✓ where_json_key_exists");
    }

    // Test JSON path query
    {
        let docs = TestDocument::query()
            .where_json_path_exists("metadata", "$.settings.theme")
            .get()
            .await
            .expect("Query failed");

        assert_eq!(docs.len(), 3, "All documents should have settings.theme");
        println!("   ✓ where_json_path_exists");
    }

    println!();
}

// =============================================================================
// ARRAY OPERATOR TESTS
// =============================================================================

async fn test_array_operators() {
    println!("📊 Testing: Array Operators");

    // Test array contains (@>)
    {
        let docs = TestDocument::query()
            .where_array_contains("tags", vec!["admin".to_string()])
            .get()
            .await
            .expect("Query failed");

        assert_eq!(docs.len(), 1, "Should find 1 document with admin tag");
        assert_eq!(docs[0].title, "User Profile");
        println!("   ✓ where_array_contains");
    }

    // Test array overlaps (&&)
    {
        let docs = TestDocument::query()
            .where_array_overlaps("tags", vec!["moderator".to_string(), "guest".to_string()])
            .get()
            .await
            .expect("Query failed");

        assert_eq!(
            docs.len(),
            2,
            "Should find 2 documents with overlapping tags"
        );
        println!("   ✓ where_array_overlaps");
    }

    // Test array contains any
    {
        let docs = TestDocument::query()
            .where_array_contains_any("tags", vec!["admin".to_string(), "guest".to_string()])
            .get()
            .await
            .expect("Query failed");

        assert!(docs.len() >= 2, "Should find at least 2 documents");
        println!("   ✓ where_array_contains_any");
    }

    // Test with integer arrays
    {
        let docs = TestDocument::query()
            .where_array_contains("ratings", vec![5])
            .get()
            .await
            .expect("Query failed");

        assert!(docs.len() >= 2, "Should find documents with rating 5");
        println!("   ✓ array operations with integers");
    }

    println!();
}

// =============================================================================
// RELATION TESTS
// =============================================================================
