//! MySQL Integration Tests for TideORM
//!
//! These tests require a running MySQL/MariaDB database.
//!
//! Setup:
//! 1. Create a MySQL test database:
//!    CREATE DATABASE tideorm_test;
//!
//! 2. Set the environment variable:
//!    set MYSQL_DATABASE_URL=mysql://user:password@localhost:3306/tideorm_test
//!
//! Run with:
//! cargo test --test mysql_integration_tests --features mysql --no-default-features

use tideorm::prelude::*;
use tideorm::{Database, TideConfig};

#[path = "mysql_integration_tests/aggregations.rs"]
mod aggregations;
#[path = "mysql_integration_tests/bulk_delete.rs"]
mod bulk_delete;
#[path = "mysql_integration_tests/cleanup.rs"]
mod cleanup;
#[path = "mysql_integration_tests/connection_and_raw_json.rs"]
mod connection_and_raw_json;
#[path = "mysql_integration_tests/crud.rs"]
mod crud;
#[path = "mysql_integration_tests/json_and_first_methods.rs"]
mod json_and_first_methods;
#[path = "mysql_integration_tests/mysql_specific.rs"]
mod mysql_specific;
#[path = "mysql_integration_tests/query_builder.rs"]
mod query_builder;
#[path = "mysql_integration_tests/soft_delete.rs"]
mod soft_delete;
#[path = "support/mysql_test_config.rs"]
mod test_config;

use test_config::{mysql_database_url, should_run_mysql_tests};

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

// =============================================================================
// MYSQL INTEGRATION TEST
// =============================================================================

#[tokio::test]
async fn mysql_integration_tests() {
    if !should_run_mysql_tests() {
        println!(
            "⏭️  Skipping MySQL tests (MYSQL_DATABASE_URL not set or SKIP_MYSQL_TESTS is set)"
        );
        return;
    }

    println!("🐬 Starting MySQL Integration Tests...\n");

    let db_url = mysql_database_url();

    let connect_result = TideConfig::init()
        .database_type(DatabaseType::MySQL)
        .database(db_url)
        .max_connections(5)
        .connect()
        .await;

    if let Err(e) = connect_result {
        println!("⚠️  MySQL connection failed: {}", e);
        println!("   Make sure MySQL is running and MYSQL_DATABASE_URL is set correctly");
        println!("   Example: mysql://user:password@localhost:3306/tideorm_test");
        return;
    }

    // Verify database type
    let db_type = tideorm::require_db().unwrap().backend();
    assert_eq!(db_type, DatabaseType::MySQL, "Expected MySQL database");
    println!(" Connected to MySQL\n");

    // Create tables (MySQL syntax with backticks)
    let _ = Database::execute("DROP TABLE IF EXISTS `test_soft_deletes`").await;
    let _ = Database::execute("DROP TABLE IF EXISTS `test_posts`").await;
    let _ = Database::execute("DROP TABLE IF EXISTS `test_products`").await;
    let _ = Database::execute("DROP TABLE IF EXISTS `test_raw_json_types`").await;
    let _ = Database::execute("DROP TABLE IF EXISTS `test_users`").await;

    Database::execute(
        r#"
        CREATE TABLE `test_users` (
            `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
            `email` VARCHAR(255) NOT NULL,
            `name` VARCHAR(255) NOT NULL,
            `age` INT NOT NULL,
            `active` BOOLEAN NOT NULL DEFAULT TRUE
        ) ENGINE=InnoDB
    "#,
    )
    .await
    .expect("Failed to create test_users table");

    Database::execute(
        r#"
        CREATE TABLE `test_posts` (
            `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
            `user_id` BIGINT NOT NULL,
            `title` VARCHAR(255) NOT NULL,
            `content` TEXT NOT NULL,
            `published` BOOLEAN NOT NULL DEFAULT FALSE
        ) ENGINE=InnoDB
    "#,
    )
    .await
    .expect("Failed to create test_posts table");

    Database::execute(
        r#"
        CREATE TABLE `test_products` (
            `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
            `name` VARCHAR(255) NOT NULL,
            `category` VARCHAR(255) NOT NULL,
            `price` BIGINT NOT NULL,
            `metadata` JSON
        ) ENGINE=InnoDB
    "#,
    )
    .await
    .expect("Failed to create test_products table");

    Database::execute(
        r#"
        CREATE TABLE `test_soft_deletes` (
            `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
            `name` VARCHAR(255) NOT NULL,
            `deleted_at` DATETIME
        ) ENGINE=InnoDB
    "#,
    )
    .await
    .expect("Failed to create test_soft_deletes table");

    Database::execute(
        r#"
        CREATE TABLE `test_raw_json_types` (
            `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
            `enabled` BOOLEAN NOT NULL,
            `payload` JSON NOT NULL,
            `amount` DECIMAL(10,2) NOT NULL,
            `created_at` DATETIME NOT NULL
        ) ENGINE=InnoDB
    "#,
    )
    .await
    .expect("Failed to create test_raw_json_types table");

    println!(" Database setup complete\n");

    connection_and_raw_json::run().await;
    crud::run().await;
    query_builder::run().await;
    aggregations::run().await;
    bulk_delete::run().await;
    soft_delete::run().await;
    json_and_first_methods::run().await;
    mysql_specific::run().await;
    cleanup::run().await;
}
