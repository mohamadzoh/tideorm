//! SQLite Integration Tests for TideORM
//!
//! These tests use an in-memory or file-based SQLite database.
//! No external database server required.
//!
//! Run with: cargo test --test sqlite_integration_tests --features sqlite --no-default-features

use std::future::Future;
use tideorm::prelude::*;
use tideorm::profiling::GlobalProfiler;
use tideorm::{Database, TideConfig};

#[path = "sqlite_integration_tests/aggregations_and_profiler.rs"]
mod aggregations_and_profiler;
#[path = "sqlite_integration_tests/bulk_delete.rs"]
mod bulk_delete;
#[path = "sqlite_integration_tests/cleanup.rs"]
mod cleanup;
#[path = "sqlite_integration_tests/connection.rs"]
mod connection;
#[path = "sqlite_integration_tests/crud.rs"]
mod crud;
#[path = "sqlite_integration_tests/first_methods.rs"]
mod first_methods;
#[path = "sqlite_integration_tests/json_operations.rs"]
mod json_operations;
#[path = "sqlite_integration_tests/query_builder.rs"]
mod query_builder;
#[path = "sqlite_integration_tests/soft_delete.rs"]
mod soft_delete;
#[path = "support/sqlite_test_config.rs"]
mod test_config;

use test_config::{should_run_sqlite_tests, sqlite_database_url};

// =============================================================================
// TEST MODELS
// =============================================================================

#[derive(Model, PartialEq)]
#[tideorm(table = "test_users")]
pub struct TestUser {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub age: i32,
    pub active: bool,
}

#[tideorm::model(table = "test_posts")]
pub struct TestPost {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub content: String,
    pub published: bool,
}

#[tideorm::model(table = "test_products")]
pub struct TestProduct {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub category: String,
    pub price: i64,
    #[tideorm(nullable)]
    pub metadata: Option<serde_json::Value>,
}

#[tideorm::model(table = "test_soft_deletes", soft_delete)]
pub struct TestSoftDelete {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn assert_profiled_operation<T, Fut>(label: &str, future: Fut) -> T
where
    Fut: Future<Output = tideorm::Result<T>>,
{
    GlobalProfiler::enable();
    GlobalProfiler::reset();
    GlobalProfiler::set_slow_threshold(0);

    let result = future
        .await
        .unwrap_or_else(|err| panic!("{} failed during profiler test: {}", label, err));

    let profiler_stats = GlobalProfiler::stats();
    assert!(
        profiler_stats.total_queries >= 1,
        "expected {} to increment total_queries, got {:?}",
        label,
        profiler_stats
    );
    assert!(
        profiler_stats.slow_queries >= 1,
        "expected {} to increment slow_queries when threshold is zero, got {:?}",
        label,
        profiler_stats
    );

    GlobalProfiler::disable();
    GlobalProfiler::reset();
    GlobalProfiler::set_slow_threshold(100);

    result
}

// =============================================================================
// SQLITE INTEGRATION TEST
// =============================================================================

#[tokio::test]
async fn sqlite_integration_tests() {
    if !should_run_sqlite_tests() {
        println!("⏭️  Skipping SQLite tests (SKIP_SQLITE_TESTS is set)");
        return;
    }

    println!("🪶 Starting SQLite Integration Tests...\n");

    // Use in-memory database for tests
    let db_url = if sqlite_database_url().contains("mode=memory") {
        sqlite_database_url().to_string()
    } else {
        // Use in-memory for faster tests
        "sqlite::memory:".to_string()
    };

    let connect_result = TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .database(&db_url)
        .max_connections(1) // SQLite works best with single connection for tests
        .connect()
        .await;

    if let Err(e) = connect_result {
        println!("⚠️  SQLite connection failed: {}", e);
        println!("   This is expected if SQLite feature is not enabled");
        return;
    }

    // Verify database type
    let db_type = tideorm::require_db().unwrap().backend();
    assert_eq!(db_type, DatabaseType::SQLite, "Expected SQLite database");
    println!(" Connected to SQLite\n");

    // Create tables (SQLite syntax)
    let _ = Database::execute("DROP TABLE IF EXISTS test_soft_deletes").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_posts").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_products").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_users").await;

    Database::execute(
        r#"
        CREATE TABLE test_users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            email TEXT NOT NULL,
            name TEXT NOT NULL,
            age INTEGER NOT NULL,
            active INTEGER NOT NULL DEFAULT 1
        )
    "#,
    )
    .await
    .expect("Failed to create test_users table");

    Database::execute(
        r#"
        CREATE TABLE test_posts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            published INTEGER NOT NULL DEFAULT 0
        )
    "#,
    )
    .await
    .expect("Failed to create test_posts table");

    Database::execute(
        r#"
        CREATE TABLE test_products (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            category TEXT NOT NULL,
            price INTEGER NOT NULL,
            metadata TEXT
        )
    "#,
    )
    .await
    .expect("Failed to create test_products table");

    Database::execute(
        r#"
        CREATE TABLE test_soft_deletes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            deleted_at TEXT
        )
    "#,
    )
    .await
    .expect("Failed to create test_soft_deletes table");

    println!(" Database setup complete\n");

    connection::run().await;
    crud::run().await;
    query_builder::run().await;
    aggregations_and_profiler::run().await;
    bulk_delete::run().await;
    soft_delete::run().await;
    json_operations::run().await;
    first_methods::run().await;
    cleanup::run().await;
}
