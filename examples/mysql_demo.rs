//! MySQL/MariaDB Demo
//!
//! This example demonstrates TideORM with MySQL/MariaDB database.
//!
//! ## Prerequisites
//!
//! 1. MySQL/MariaDB server running
//! 2. Create a database: `CREATE DATABASE tideorm_demo;`
//! 3. Set environment variable: `MYSQL_DATABASE_URL=mysql://user:pass@localhost/tideorm_demo`
//!
//! ## Running
//!
//! ```bash
//! cargo run --example mysql_demo --features "mysql runtime-tokio" --no-default-features
//! ```

use std::env;
use tideorm::prelude::*;

#[derive(Model)]
#[tide(table = "users")]
#[index("email")]
#[unique_index("email")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub active: bool,
    pub age: Option<i32>,
}

#[derive(Model)]
#[tide(table = "products")]
#[index("category")]
pub struct Product {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub category: String,
    pub price: i64,
    /// JSON column for metadata (MySQL 5.7+)
    #[tide(nullable)]
    pub metadata: Option<serde_json::Value>,
}

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    println!("🐬 TideORM MySQL Demo\n");

    // Get database URL from environment
    let database_url = env::var("MYSQL_DATABASE_URL")
        .unwrap_or_else(|_| "mysql://root:root@localhost/tideorm_test".to_string());

    println!("Connecting to MySQL...");

    // Initialize TideORM with MySQL
    TideConfig::init()
        .database_type(DatabaseType::MySQL)
        .database(&database_url)
        .max_connections(10)
        .min_connections(2)
        .sync(true) // Auto-sync schema (development only!)
        .models::<(User, Product)>()  // Register models for sync
        .connect()
        .await?;

    println!("✓ Connected successfully!\n");

    // Check database features
    let db_type = Database::global().backend();
    println!("Database Features:");
    println!("  - JSON support: {}", db_type.supports_json());
    println!("  - Arrays: {} (use JSON instead)", db_type.supports_arrays());
    println!("  - RETURNING clause: {}", db_type.supports_returning());
    println!("  - Upsert: {}", db_type.supports_upsert());
    println!("  - Window functions: {}", db_type.supports_window_functions());
    println!("  - Parameter style: {}", db_type.param_style());
    println!();

    // =====================
    // CRUD Operations
    // =====================

    println!("=== CRUD Operations ===\n");

    // Create
    let user = User {
        id: 0,
        email: "john@example.com".to_string(),
        name: "John Doe".to_string(),
        active: true,
        age: Some(30),
    };
    println!("Creating user: {:?}", user);
    let user = user.save().await?;
    println!("✓ Created user with ID: {}\n", user.id);

    // Read
    let found = User::find(user.id).await?;
    println!("Found user: {:?}\n", found);

    // Update
    let mut user = user;
    user.name = "John Updated".to_string();
    let user = user.update().await?;
    println!("✓ Updated user: {:?}\n", user);

    // Query with conditions
    let active_users = User::query()
        .where_eq("active", true)
        .where_gt("age", 18)
        .order_by("name", Order::Asc)
        .get()
        .await?;
    println!("Active users over 18: {} found\n", active_users.len());

    // =====================
    // JSON Operations (MySQL 5.7+)
    // =====================

    println!("=== JSON Operations ===\n");

    let product = Product {
        id: 0,
        name: "Laptop".to_string(),
        category: "Electronics".to_string(),
        price: 999,
        metadata: Some(serde_json::json!({
            "brand": "TechCorp",
            "features": ["fast", "lightweight"],
            "specs": {
                "ram": "16GB",
                "storage": "512GB SSD"
            }
        })),
    };
    let product = product.save().await?;
    println!("✓ Created product with JSON metadata: {}\n", product.id);

    // Query JSON with MySQL JSON_CONTAINS
    println!("Querying products where metadata contains 'brand': 'TechCorp'...");
    let tech_products = Product::query()
        .where_json_contains("metadata", serde_json::json!({"brand": "TechCorp"}))
        .get()
        .await?;
    println!("Found {} products\n", tech_products.len());

    // Check for JSON key existence
    println!("Checking for key existence...");
    let with_specs = Product::query()
        .where_json_key_exists("metadata", "specs")
        .get()
        .await?;
    println!("Products with 'specs' key: {}\n", with_specs.len());

    // =====================
    // Aggregations
    // =====================

    println!("=== Aggregations ===\n");

    // Create more products for aggregation demo
    for i in 1..=5 {
        Product {
            id: 0,
            name: format!("Product {}", i),
            category: if i % 2 == 0 { "Category A" } else { "Category B" }.to_string(),
            price: i * 100,
            metadata: None,
        }
        .save()
        .await?;
    }

    let count = Product::query().count().await?;
    println!("Total products: {}", count);

    let avg_price = Product::query().avg("price").await?;
    println!("Average price: ${:.2}", avg_price);

    let max_price = Product::query().max("price").await?;
    println!("Max price: ${:.2}", max_price);

    let sum_price = Product::query()
        .where_eq("category", "Category A")
        .sum("price")
        .await?;
    println!("Sum of Category A prices: ${:.2}\n", sum_price);

    // =====================
    // Pagination
    // =====================

    println!("=== Pagination ===\n");

    let page1 = Product::query()
        .order_by("id", Order::Asc)
        .page(1, 3)
        .get()
        .await?;
    println!("Page 1 (3 per page): {} products", page1.len());

    let page2 = Product::query()
        .order_by("id", Order::Asc)
        .page(2, 3)
        .get()
        .await?;
    println!("Page 2 (3 per page): {} products\n", page2.len());

    // =====================
    // Cleanup
    // =====================

    println!("=== Cleanup ===\n");

    // Bulk delete
    let deleted = Product::query().delete().await?;
    println!("Deleted {} products", deleted);

    User::destroy(user.id).await?;
    println!("Deleted test user\n");

    println!("✓ MySQL demo completed successfully!");

    Ok(())
}
