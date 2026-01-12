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
use tideorm::relations::{HasOne, HasMany, BelongsTo};
use tideorm::{TideConfig, Database};

mod test_config;
use test_config::test_database_url;

// =============================================================================
// TEST MODELS WITH JSON AND ARRAY COLUMNS
// =============================================================================

#[tideorm::model]
#[tide(table = "test_documents")]
pub struct TestDocument {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub metadata: serde_json::Value,  // JSONB column
    pub tags: Vec<String>,            // Array column
    pub ratings: Vec<i32>,            // Array of integers
}

#[tideorm::model]
#[tide(table = "test_authors")]
pub struct TestAuthor {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub country: String,
    
    // HasMany relation - author has many books
    #[tide(has_many = "TestBook", foreign_key = "author_id")]
    pub books: HasMany<TestBook>,
}

#[tideorm::model]
#[tide(table = "test_books")]
pub struct TestBook {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub author_id: i64,
    pub title: String,
    pub year: i32,
    
    // BelongsTo relation - book belongs to an author
    #[tide(belongs_to = "TestAuthor", foreign_key = "author_id")]
    pub author: BelongsTo<TestAuthor>,
    
    // HasOne relation - book has one detail record
    #[tide(has_one = "TestBookDetail", foreign_key = "book_id")]
    pub detail: HasOne<TestBookDetail>,
}

#[tideorm::model]
#[tide(table = "test_book_details")]
pub struct TestBookDetail {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub book_id: i64,
    pub isbn: String,
    pub pages: i32,
    
    // BelongsTo relation - detail belongs to a book
    #[tide(belongs_to = "TestBook", foreign_key = "book_id")]
    pub book: BelongsTo<TestBook>,
}

// =============================================================================
// MAIN TEST SUITE
// =============================================================================

#[tokio::test]
async fn postgres_advanced_tests() {
    println!("🚀 Starting Advanced PostgreSQL Integration Tests...\n");
    
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
    test_relations().await;
    
    // =========================================================================
    // CLEANUP
    // =========================================================================
    cleanup_tables().await;
    
    println!("\n✅ All advanced PostgreSQL tests passed!\n");
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
    Database::execute(r#"
        CREATE TABLE test_documents (
            id BIGSERIAL PRIMARY KEY,
            title VARCHAR(255) NOT NULL,
            metadata JSONB NOT NULL,
            tags TEXT[] NOT NULL,
            ratings INTEGER[] NOT NULL
        )
    "#).await.expect("Failed to create test_documents table");
    
    // Create authors table
    Database::execute(r#"
        CREATE TABLE test_authors (
            id BIGSERIAL PRIMARY KEY,
            name VARCHAR(255) NOT NULL,
            country VARCHAR(100) NOT NULL
        )
    "#).await.expect("Failed to create test_authors table");
    
    // Create books table
    Database::execute(r#"
        CREATE TABLE test_books (
            id BIGSERIAL PRIMARY KEY,
            author_id BIGINT NOT NULL,
            title VARCHAR(255) NOT NULL,
            year INTEGER NOT NULL
        )
    "#).await.expect("Failed to create test_books table");
    
    // Create book_details table
    Database::execute(r#"
        CREATE TABLE test_book_details (
            id BIGSERIAL PRIMARY KEY,
            book_id BIGINT NOT NULL,
            isbn VARCHAR(50) NOT NULL,
            pages INTEGER NOT NULL
        )
    "#).await.expect("Failed to create test_book_details table");
    
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
    println!("📦 Testing: JSON/JSONB Operators");
    
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
        
        assert_eq!(docs.len(), 2, "Should find 2 documents with overlapping tags");
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

async fn test_relations() {
    println!("🔗 Testing: Relations");
    
    // Seed test data
    let _ = Database::execute("TRUNCATE TABLE test_book_details RESTART IDENTITY CASCADE").await;
    let _ = Database::execute("TRUNCATE TABLE test_books RESTART IDENTITY CASCADE").await;
    let _ = Database::execute("TRUNCATE TABLE test_authors RESTART IDENTITY CASCADE").await;
    
    // Create authors
    let author1 = TestAuthor {
        id: 0,
        name: "J.K. Rowling".into(),
        country: "UK".into(),
        ..Default::default()
    }.save().await.expect("Failed to save author 1");
    
    let author2 = TestAuthor {
        id: 0,
        name: "George R.R. Martin".into(),
        country: "USA".into(),
        ..Default::default()
    }.save().await.expect("Failed to save author 2");
    
    // Create books
    let book1 = TestBook {
        id: 0,
        author_id: author1.id,
        title: "Harry Potter and the Philosopher's Stone".into(),
        year: 1997,
        ..Default::default()
    }.save().await.expect("Failed to save book 1");
    
    let book2 = TestBook {
        id: 0,
        author_id: author1.id,
        title: "Harry Potter and the Chamber of Secrets".into(),
        year: 1998,
        ..Default::default()
    }.save().await.expect("Failed to save book 2");
    
    let book3 = TestBook {
        id: 0,
        author_id: author2.id,
        title: "A Game of Thrones".into(),
        year: 1996,
        ..Default::default()
    }.save().await.expect("Failed to save book 3");
    
    // Create book details
    let _detail1 = TestBookDetail {
        id: 0,
        book_id: book1.id,
        isbn: "978-0747532699".into(),
        pages: 223,
        ..Default::default()
    }.save().await.expect("Failed to save detail 1");
    
    let _detail2 = TestBookDetail {
        id: 0,
        book_id: book2.id,
        isbn: "978-0747538493".into(),
        pages: 251,
        ..Default::default()
    }.save().await.expect("Failed to save detail 2");
    
    let _detail3 = TestBookDetail {
        id: 0,
        book_id: book3.id,
        isbn: "978-0553103540".into(),
        pages: 694,
        ..Default::default()
    }.save().await.expect("Failed to save detail 3");
    
    // Test BelongsTo relation
    {
        let books = TestBook::query()
            .where_eq("author_id", author1.id)
            .get()
            .await
            .expect("Query failed");
        
        assert_eq!(books.len(), 2, "J.K. Rowling should have 2 books");
        println!("   ✓ BelongsTo - query by foreign key");
    }
    
    // Test HasMany relation concept (query books by author)
    {
        let author2_books = TestBook::query()
            .where_eq("author_id", author2.id)
            .get()
            .await
            .expect("Query failed");
        
        assert_eq!(author2_books.len(), 1, "George R.R. Martin should have 1 book");
        println!("   ✓ HasMany - query related records");
    }
    
    // Test HasOne relation concept (book detail for a book)
    {
        let detail = TestBookDetail::query()
            .where_eq("book_id", book1.id)
            .first()
            .await
            .expect("Query failed");
        
        assert!(detail.is_some(), "Book 1 should have details");
        let detail = detail.unwrap();
        assert_eq!(detail.isbn, "978-0747532699");
        assert_eq!(detail.pages, 223);
        println!("   ✓ HasOne - query related record");
    }
    
    // Test querying across relations
    {
        let uk_author_books = TestBook::query()
            .inner_join("test_authors", "test_books.author_id", "test_authors.id")
            .where_eq("test_authors.country", "UK")
            .get()
            .await
            .expect("Query failed");
        
        assert_eq!(uk_author_books.len(), 2, "UK authors should have 2 books");
        println!("   ✓ JOIN across relations");
    }
    
    // Test complex join with book details
    {
        let books_with_many_pages = TestBook::query()
            .inner_join("test_book_details", "test_books.id", "test_book_details.book_id")
            .where_gt("test_book_details.pages", 500)
            .get()
            .await
            .expect("Query failed");
        
        assert_eq!(books_with_many_pages.len(), 1, "Should find 1 book with > 500 pages");
        println!("   ✓ JOIN with conditions on related table");
    }
    
    // Test field-based relation loading
    {
        let mut rowling = TestAuthor::query()
            .where_eq("name", "J.K. Rowling")
            .first()
            .await
            .expect("Query failed")
            .expect("Author should exist");
        
        // Set the parent PK on the relation field before loading
        rowling.books = HasMany::new("author_id", "id").with_parent_pk(serde_json::json!(rowling.id));
        let rowling_books = rowling.books.load().await.expect("Failed to load has_many");
        assert_eq!(rowling_books.len(), 2, "Rowling should have 2 books via load()");
        
        let mut got_book = TestBook::query()
            .where_eq("title", "A Game of Thrones")
            .first()
            .await
            .expect("Query failed")
            .expect("Book should exist");
        
        // Set up belongs_to relation with FK value
        got_book.author = BelongsTo::new("author_id", "id").with_fk_value(serde_json::json!(got_book.author_id));
        let got_author = got_book.author.load().await.expect("Failed to load belongs_to");
        assert_eq!(got_author.unwrap().name, "George R.R. Martin", "BelongsTo should fetch correct author");
        
        // Set up has_one relation with parent PK
        got_book.detail = HasOne::new("book_id", "id").with_parent_pk(serde_json::json!(got_book.id));
        let got_detail = got_book.detail.load().await.expect("Failed to load has_one");
        assert!(got_detail.is_some(), "HasOne should return a detail");
        let got_detail = got_detail.unwrap();
        assert_eq!(got_detail.isbn, "978-0553103540");
        assert_eq!(got_detail.pages, 694);
        println!("   ✓ Field-based relation loading (belongs_to / has_one / has_many)");
    }
    
    println!();
}
