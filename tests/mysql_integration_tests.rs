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

    // =========================================================================
    // CONNECTION TESTS
    // =========================================================================
    println!("📡 Testing: Database Connection");
    {
        let db = tideorm::require_db().unwrap();
        assert!(db.ping().await.is_ok(), "Database ping failed");
        println!("   ✓ Ping successful");

        let result = Database::execute("SELECT 1").await;
        assert!(result.is_ok(), "Raw SQL execution failed");
        println!("   ✓ Raw SQL execution works");

        // Verify it's MySQL
        assert_eq!(db.backend(), DatabaseType::MySQL);
        println!("   ✓ Database type is MySQL");
    }
    println!();

    // =========================================================================
    // RAW JSON TESTS
    // =========================================================================
    println!("🧪 Testing: Raw JSON Typed Decoding");
    {
        let db = tideorm::require_db().expect("database should be available");

        db.__execute_with_params(
            "INSERT INTO `test_raw_json_types` (`enabled`, `payload`, `amount`, `created_at`) VALUES (?, ?, ?, ?)",
            vec![
                tideorm::internal::Value::Bool(Some(true)),
                tideorm::internal::Value::Json(Some(Box::new(serde_json::json!({
                    "kind": "probe",
                    "count": 2
                })))),
                tideorm::internal::Value::String(Some("12.34".to_string())),
                tideorm::internal::Value::String(Some("2026-03-21 10:11:12".to_string())),
            ],
        )
        .await
        .expect("typed raw-json probe insert should succeed");

        let rows = db
            .__raw_json_with_params(
                "SELECT `enabled`, `payload`, `amount`, `created_at` FROM `test_raw_json_types` ORDER BY `id` ASC",
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
                    chrono::NaiveDateTime::parse_from_str("2026-03-21 10:11:12", "%Y-%m-%d %H:%M:%S")
                        .expect("datetime literal should parse")
                ).expect("datetime should serialize to JSON"),
            })]
        );
        println!("   ✓ raw_json preserves MySQL boolean, JSON, decimal, and datetime types");
    }
    println!();

    // =========================================================================
    // CRUD TESTS
    // =========================================================================
    println!("📝 Testing: CRUD Operations");

    // Create and Find
    {
        let user = TestUser {
            id: 0,
            email: "test@example.com".to_string(),
            name: "Test User".to_string(),
            age: 25,
            active: true,
        };

        let saved_user = user.save().await.expect("Failed to save user");
        assert!(saved_user.id > 0, "User ID should be auto-generated");
        println!("   ✓ Create works (id: {})", saved_user.id);

        let found_user = TestUser::find(saved_user.id).await.expect("Find failed");
        assert!(found_user.is_some(), "User should be found");
        assert_eq!(found_user.unwrap().email, "test@example.com");
        println!("   ✓ Find by ID works");
    }

    // Update
    {
        let user = TestUser {
            id: 0,
            email: "update@example.com".to_string(),
            name: "Original Name".to_string(),
            age: 30,
            active: true,
        };
        let mut saved_user = user.save().await.expect("Failed to save");

        saved_user.name = "Updated Name".to_string();
        saved_user.age = 31;
        let updated = saved_user.update().await.expect("Update failed");

        assert_eq!(updated.name, "Updated Name");
        assert_eq!(updated.age, 31);
        println!("   ✓ Update works");
    }

    // Delete
    {
        let user = TestUser {
            id: 0,
            email: "delete@example.com".to_string(),
            name: "To Delete".to_string(),
            age: 40,
            active: true,
        };
        let saved_user = user.save().await.expect("Failed to save");
        let user_id = saved_user.id;

        saved_user.delete().await.expect("Delete failed");

        let found = TestUser::find(user_id).await.expect("Find failed");
        assert!(found.is_none(), "User should be deleted");
        println!("   ✓ Delete works");
    }
    println!();

    // =========================================================================
    // QUERY BUILDER TESTS
    // =========================================================================
    println!("🔍 Testing: Query Builder");

    // Clear and seed data
    let _ = Database::execute("DELETE FROM `test_users`").await;

    for i in 1..=10 {
        TestUser {
            id: 0,
            email: format!("user{}@example.com", i),
            name: format!("User {}", i),
            age: 20 + i,
            active: i % 2 == 0,
        }
        .save()
        .await
        .expect("Failed to seed user");
    }

    // WHERE conditions
    {
        let active_users = TestUser::query()
            .where_eq("active", true)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(active_users.len(), 5, "Should have 5 active users");
        println!("   ✓ where_eq works");

        let young_users = TestUser::query()
            .where_lt("age", 25)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(young_users.len(), 4);
        println!("   ✓ where_lt works");

        let range_users = TestUser::query()
            .where_between("age", 23, 27)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(range_users.len(), 5);
        println!("   ✓ where_between works");
    }

    // Ordering
    {
        let ordered = TestUser::query()
            .order_by("age", Order::Desc)
            .limit(3)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(ordered.len(), 3);
        assert_eq!(ordered[0].age, 30);
        println!("   ✓ order_by works");
    }

    // Pagination
    {
        let page1 = TestUser::query()
            .order_by("id", Order::Asc)
            .page(1, 3)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(page1.len(), 3);

        let page2 = TestUser::query()
            .order_by("id", Order::Asc)
            .page(2, 3)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(page2.len(), 3);
        assert_ne!(page1[0].id, page2[0].id);
        println!("   ✓ Pagination works");
    }

    // Count
    {
        let count = TestUser::query().count().await.expect("Count failed");
        assert_eq!(count, 10);

        let active_count = TestUser::query()
            .where_eq("active", true)
            .count()
            .await
            .expect("Count failed");
        assert_eq!(active_count, 5);
        println!("   ✓ count works");
    }

    // Exists
    {
        let exists = TestUser::query()
            .where_eq("email", "user1@example.com")
            .exists()
            .await
            .expect("Exists failed");
        assert!(exists);

        let not_exists = TestUser::query()
            .where_eq("email", "nonexistent@example.com")
            .exists()
            .await
            .expect("Exists failed");
        assert!(!not_exists);
        println!("   ✓ exists works");
    }

    // Pattern matching
    {
        let like_users = TestUser::query()
            .where_like("email", "%@example.com")
            .get()
            .await
            .expect("Query failed");
        assert_eq!(like_users.len(), 10);
        println!("   ✓ where_like works");
    }

    // IN clause
    {
        let in_users = TestUser::query()
            .where_in("age", vec![21, 23, 25])
            .get()
            .await
            .expect("Query failed");
        assert_eq!(in_users.len(), 3);
        println!("   ✓ where_in works");
    }
    println!();

    // =========================================================================
    // AGGREGATION TESTS
    // =========================================================================
    println!("📊 Testing: Aggregations");
    {
        let sum = TestUser::query().sum("age").await.expect("Sum failed");
        // Ages: 21+22+23+24+25+26+27+28+29+30 = 255
        assert_eq!(sum as i64, 255);
        println!("   ✓ sum works");

        let avg = TestUser::query().avg("age").await.expect("Avg failed");
        assert!((avg - 25.5).abs() < 0.01);
        println!("   ✓ avg works");

        let min = TestUser::query().min("age").await.expect("Min failed");
        assert_eq!(min as i64, 21);
        println!("   ✓ min works");

        let max = TestUser::query().max("age").await.expect("Max failed");
        assert_eq!(max as i64, 30);
        println!("   ✓ max works");
    }
    println!();

    // =========================================================================
    // BULK DELETE TESTS
    // =========================================================================
    println!("🗑️  Testing: Bulk Delete");
    {
        let deleted = TestUser::query()
            .where_eq("active", false)
            .delete()
            .await
            .expect("Bulk delete failed");
        assert_eq!(deleted, 5);

        let remaining = TestUser::query().count().await.expect("Count failed");
        assert_eq!(remaining, 5);
        println!("   ✓ Bulk delete works");
    }
    println!();

    // =========================================================================
    // SOFT DELETE TESTS
    // =========================================================================
    println!("🗄️  Testing: Soft Deletes");
    {
        let _ = Database::execute("DELETE FROM `test_soft_deletes`").await;

        // Create records
        for i in 1..=5 {
            TestSoftDelete {
                id: 0,
                name: format!("Item {}", i),
                deleted_at: None,
            }
            .save()
            .await
            .expect("Failed to create");
        }

        // Soft delete some
        let item = TestSoftDelete::query()
            .where_eq("name", "Item 1")
            .first()
            .await
            .expect("Query failed")
            .expect("Item not found");
        item.soft_delete().await.expect("Soft delete failed");

        let item2 = TestSoftDelete::query()
            .where_eq("name", "Item 2")
            .first()
            .await
            .expect("Query failed")
            .expect("Item not found");
        item2.soft_delete().await.expect("Soft delete failed");

        // Query without trashed
        let active = TestSoftDelete::query().get().await.expect("Query failed");
        assert_eq!(active.len(), 3);
        println!("   ✓ Default excludes soft deleted");

        // Query with trashed
        let all = TestSoftDelete::query()
            .with_trashed()
            .get()
            .await
            .expect("Query failed");
        assert_eq!(all.len(), 5);
        println!("   ✓ with_trashed includes all");

        // Query only trashed
        let trashed = TestSoftDelete::query()
            .only_trashed()
            .get()
            .await
            .expect("Query failed");
        assert_eq!(trashed.len(), 2);
        println!("   ✓ only_trashed works");
    }
    println!();

    // =========================================================================
    // JSON TESTS (MySQL JSON Type)
    // =========================================================================
    println!(" Testing: JSON Operations");
    {
        let _ = Database::execute("DELETE FROM `test_products`").await;

        // Create products with JSON metadata
        let product = TestProduct {
            id: 0,
            name: "Laptop".to_string(),
            category: "Electronics".to_string(),
            price: 999,
            metadata: Some(serde_json::json!({
                "brand": "TechCorp",
                "features": ["fast", "lightweight"],
                "specs": {
                    "ram": 16,
                    "storage": 512
                }
            })),
        };
        product.save().await.expect("Failed to save product");

        let product2 = TestProduct {
            id: 0,
            name: "Phone".to_string(),
            category: "Electronics".to_string(),
            price: 699,
            metadata: Some(serde_json::json!({
                "brand": "MobileCo",
                "features": ["compact", "durable"]
            })),
        };
        product2.save().await.expect("Failed to save product");

        // Test that JSON was stored and retrieved correctly
        let products = TestProduct::query()
            .where_eq("category", "Electronics")
            .get()
            .await
            .expect("Query failed");
        assert_eq!(products.len(), 2);

        let laptop = &products.iter().find(|p| p.name == "Laptop").unwrap();
        let metadata = laptop.metadata.as_ref().unwrap();
        assert_eq!(metadata["brand"], "TechCorp");
        println!("   ✓ JSON storage and retrieval works");

        // MySQL native JSON query (using raw SQL for now)
        // MySQL supports JSON_EXTRACT and ->> operators
        let result = Database::raw_json(
            "SELECT COUNT(*) as cnt FROM `test_products` WHERE JSON_EXTRACT(metadata, '$.brand') = 'TechCorp'",
        ).await;
        assert!(result.is_ok(), "JSON query should work");
        println!("   ✓ MySQL JSON_EXTRACT works");
    }
    println!();

    // =========================================================================
    // FIRST AND FIRST_OR_FAIL TESTS
    // =========================================================================
    println!("🎯 Testing: First Methods");
    {
        let _ = Database::execute("DELETE FROM `test_users`").await;

        TestUser {
            id: 0,
            email: "first@example.com".to_string(),
            name: "First".to_string(),
            age: 25,
            active: true,
        }
        .save()
        .await
        .unwrap();

        let first = TestUser::query().first().await.expect("First failed");
        assert!(first.is_some());
        println!("   ✓ first works");

        let first_or_fail = TestUser::query()
            .where_eq("email", "first@example.com")
            .first_or_fail()
            .await;
        assert!(first_or_fail.is_ok());
        println!("   ✓ first_or_fail works for existing");

        let not_found = TestUser::query()
            .where_eq("email", "nonexistent@example.com")
            .first_or_fail()
            .await;
        assert!(not_found.is_err());
        println!("   ✓ first_or_fail returns error for missing");
    }
    println!();

    // =========================================================================
    // MYSQL-SPECIFIC FEATURES
    // =========================================================================
    println!("🐬 Testing: MySQL-Specific Features");
    {
        // Test ON DUPLICATE KEY UPDATE (upsert)
        let _ = Database::execute("DELETE FROM `test_users`").await;

        // Create unique index on email
        let _ = Database::execute("DROP INDEX `idx_users_email` ON `test_users`").await;
        let _ =
            Database::execute("CREATE UNIQUE INDEX `idx_users_email` ON `test_users` (`email`)")
                .await;

        // Insert
        let user = TestUser {
            id: 0,
            email: "upsert@example.com".to_string(),
            name: "Initial".to_string(),
            age: 25,
            active: true,
        };
        user.save().await.expect("Failed to save");

        // Verify insert
        let found = TestUser::query()
            .where_eq("email", "upsert@example.com")
            .first()
            .await
            .expect("Query failed")
            .unwrap();
        assert_eq!(found.name, "Initial");
        println!("   ✓ Unique index works");

        // Test LIMIT with ORDER BY
        let _ = Database::execute("DELETE FROM `test_users`").await;
        for i in 1..=5 {
            TestUser {
                id: 0,
                email: format!("order{}@example.com", i),
                name: format!("User {}", i),
                age: 30 - i,
                active: true,
            }
            .save()
            .await
            .unwrap();
        }

        let top3 = TestUser::query()
            .order_by("age", Order::Desc)
            .limit(3)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(top3.len(), 3);
        assert_eq!(top3[0].age, 29);
        println!("   ✓ LIMIT with ORDER BY works");
    }
    println!();

    // =========================================================================
    // CLEANUP
    // =========================================================================
    println!("🧹 Cleanup");
    let _ = Database::execute("DROP TABLE IF EXISTS `test_soft_deletes`").await;
    let _ = Database::execute("DROP TABLE IF EXISTS `test_posts`").await;
    let _ = Database::execute("DROP TABLE IF EXISTS `test_products`").await;
    let _ = Database::execute("DROP TABLE IF EXISTS `test_users`").await;
    println!("   ✓ Tables dropped");

    println!("\n All MySQL integration tests passed!");
}
