//! # TideORM Query Builder Example
//!
//! **Category:** Query Building
//!
//! This example demonstrates all query builder features:
//! - WHERE conditions (eq, not, gt, gte, lt, lte, like, in, null, between)
//! - Efficient COUNT queries
//! - Bulk DELETE operations
//! - ORDER BY, LIMIT, OFFSET
//! - Pagination helpers
//!
//! ## Run this example
//!
//! ```bash
//! cargo run --example query_builder
//! ```

use tideorm::prelude::*;

// =============================================================================
// MODEL DEFINITIONS
// =============================================================================

/// Product model for e-commerce examples
#[derive(Model)]
#[tide(table = "products")]
#[index("category")]
#[index("active")]
#[index(name = "idx_price_range", columns = "price,active")]
pub struct Product {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub category: String,
    pub price: i64,  // Price in cents
    pub stock: i32,
    pub active: bool,
    #[tide(nullable)]
    pub description: Option<String>,
}

/// CustomerOrder model with various statuses
#[derive(Model)]
#[tide(table = "customer_orders")]
#[index("user_id")]
#[index("status")]
#[unique_index("order_number")]
pub struct CustomerOrder {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub order_number: String,
    pub user_id: i64,
    pub status: String,  // pending, processing, shipped, delivered, cancelled
    pub total: i64,      // Total in cents
}

// =============================================================================
// MAIN EXAMPLE
// =============================================================================

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    // Load database URL from .env file
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("POSTGRESQL_DATABASE_URL")
        .unwrap();
    
    // Connect with force_sync to recreate tables (in case schema changed)
    // WARNING: force_sync drops and recreates tables - only use in development!
    match TideConfig::init()
        .database_type(DatabaseType::Postgres)
        .database(&db_url)
        .max_connections(10)
        .sync(true)
        .force_sync(true)  // Drop and recreate tables to handle schema changes
        .models::<(Product, CustomerOrder)>()
        .connect()
        .await
    {
        Ok(_) => {
            println!("✓ Connected to database!\n");
            run_examples().await?;
        }
        Err(e) => {
            println!("Database connection failed: {}\n", e);
            println!("Showing Query Builder API examples:\n");
            show_api_documentation();
        }
    }
    
    Ok(())
}

async fn run_examples() -> tideorm::Result<()> {
    // =========================================================================
    // CREATE SAMPLE DATA
    // =========================================================================
    
    println!("Creating sample products...");
    
    let products = vec![
        Product { id: 0, name: "Laptop".into(), category: "Electronics".into(), price: 99900, stock: 50, active: true, description: Some("High-performance laptop".into()) },
        Product { id: 0, name: "Mouse".into(), category: "Electronics".into(), price: 2999, stock: 200, active: true, description: None },
        Product { id: 0, name: "Keyboard".into(), category: "Electronics".into(), price: 7999, stock: 100, active: true, description: Some("Mechanical keyboard".into()) },
        Product { id: 0, name: "T-Shirt".into(), category: "Clothing".into(), price: 1999, stock: 500, active: true, description: None },
        Product { id: 0, name: "Jeans".into(), category: "Clothing".into(), price: 4999, stock: 0, active: false, description: None },
        Product { id: 0, name: "Book".into(), category: "Books".into(), price: 1499, stock: 30, active: true, description: Some("Programming guide".into()) },
    ];
    
    for product in products {
        product.save().await?;
    }
    println!("✓ Created {} products\n", 6);
    
    // =========================================================================
    // WHERE CONDITIONS
    // =========================================================================
    
    println!("=== WHERE Conditions ===\n");
    
    // where_eq - exact match
    let electronics = Product::query()
        .where_eq("category", "Electronics")
        .get()
        .await?;
    println!("Electronics products: {}", electronics.len());
    
    // where_not - not equal
    let non_electronics = Product::query()
        .where_not("category", "Electronics")
        .get()
        .await?;
    println!("Non-electronics products: {}", non_electronics.len());
    
    // where_gt - greater than
    let expensive = Product::query()
        .where_gt("price", 5000)  // > $50.00
        .get()
        .await?;
    println!("Expensive products (>$50): {}", expensive.len());
    
    // where_gte - greater than or equal
    let mid_price = Product::query()
        .where_gte("price", 2000)  // >= $20.00
        .where_lte("price", 10000) // <= $100.00
        .get()
        .await?;
    println!("Mid-range products ($20-$100): {}", mid_price.len());
    
    // where_like - pattern matching
    let keyboards = Product::query()
        .where_like("name", "%board%")
        .get()
        .await?;
    println!("Products with 'board' in name: {}", keyboards.len());
    
    // where_in - multiple values
    let selected_categories = Product::query()
        .where_in("category", vec!["Electronics", "Books"])
        .get()
        .await?;
    println!("Electronics or Books: {}", selected_categories.len());
    
    // where_not_in - exclude multiple values
    let other_categories = Product::query()
        .where_not_in("category", vec!["Electronics", "Books"])
        .get()
        .await?;
    println!("Not Electronics or Books: {}", other_categories.len());
    
    // where_null - check for NULL
    let no_description = Product::query()
        .where_null("description")
        .get()
        .await?;
    println!("Products without description: {}", no_description.len());
    
    // where_not_null - check for NOT NULL
    let has_description = Product::query()
        .where_not_null("description")
        .get()
        .await?;
    println!("Products with description: {}", has_description.len());
    
    // where_between - range check
    let in_stock_range = Product::query()
        .where_between("stock", 10, 100)
        .get()
        .await?;
    println!("Products with stock 10-100: {}", in_stock_range.len());
    
    // Combined conditions (AND)
    let active_electronics = Product::query()
        .where_eq("category", "Electronics")
        .where_eq("active", true)
        .where_gt("stock", 0)
        .get()
        .await?;
    println!("Active electronics in stock: {}", active_electronics.len());
    
    println!();
    
    // =========================================================================
    // COUNT QUERIES
    // =========================================================================
    
    println!("=== COUNT Queries (Optimized) ===\n");
    
    // Basic count
    let total = Product::count().await?;
    println!("Total products: {}", total);
    
    // Count with conditions
    let active_count = Product::query()
        .where_eq("active", true)
        .count()
        .await?;
    println!("Active products: {}", active_count);
    
    // Count expensive items
    let expensive_count = Product::query()
        .where_gt("price", 5000)
        .count()
        .await?;
    println!("Expensive products count: {}", expensive_count);
    
    // exists() - efficient existence check
    let has_books = Product::query()
        .where_eq("category", "Books")
        .exists()
        .await?;
    println!("Has books in catalog: {}", has_books);
    
    println!();
    
    // =========================================================================
    // ORDER BY, LIMIT, OFFSET
    // =========================================================================
    
    println!("=== Ordering & Pagination ===\n");
    
    // Order by single column
    let by_price = Product::query()
        .order_by("price", tideorm::query::Order::Desc)
        .limit(3)
        .get()
        .await?;
    println!("Top 3 most expensive:");
    for p in &by_price {
        println!("  - {} (${:.2})", p.name, p.price as f64 / 100.0);
    }
    
    // Convenience methods
    let cheapest = Product::query()
        .order_asc("price")
        .first()
        .await?;
    if let Some(p) = cheapest {
        println!("Cheapest: {} (${:.2})", p.name, p.price as f64 / 100.0);
    }
    
    // Pagination
    let page_1 = Product::query()
        .where_eq("active", true)
        .order_by("name", tideorm::query::Order::Asc)
        .page(1, 3) // Page 1, 3 items per page
        .get()
        .await?;
    println!("Page 1 (3 per page): {} items", page_1.len());
    
    // skip/take (alias for offset/limit)
    let skipped = Product::query()
        .order_by("price", tideorm::query::Order::Asc)
        .skip(2)
        .take(2)
        .get()
        .await?;
    println!("Skip 2, take 2: {} items", skipped.len());
    
    println!();
    
    // =========================================================================
    // FIRST AND FIRST_OR_FAIL
    // =========================================================================
    
    println!("=== First Record Queries ===\n");
    
    // first() - returns Option<M>
    let first_active = Product::query()
        .where_eq("active", true)
        .order_by("name", tideorm::query::Order::Asc)
        .first()
        .await?;
    
    if let Some(p) = first_active {
        println!("First active product: {}", p.name);
    }
    
    // first_or_fail() - returns Result<M>
    match Product::query()
        .where_eq("category", "NonExistent")
        .first_or_fail()
        .await
    {
        Ok(p) => println!("Found: {}", p.name),
        Err(e) => println!("first_or_fail error (expected): {}", e),
    }
    
    println!();
    
    // =========================================================================
    // BULK DELETE
    // =========================================================================
    
    println!("=== Bulk DELETE ===\n");
    
    // Count before delete
    let inactive_count = Product::query()
        .where_eq("active", false)
        .count()
        .await?;
    println!("Inactive products before delete: {}", inactive_count);
    
    // Bulk delete inactive products
    let deleted = Product::query()
        .where_eq("active", false)
        .delete()
        .await?;
    println!("Deleted {} inactive products", deleted);
    
    // Delete products with no stock
    let deleted_no_stock = Product::query()
        .where_eq("stock", 0)
        .where_eq("active", true)
        .delete()
        .await?;
    println!("Deleted {} out-of-stock products", deleted_no_stock);
    
    // Verify deletion
    let remaining = Product::count().await?;
    println!("Remaining products: {}", remaining);
    
    println!("\n✓ Query builder examples completed!");
    
    Ok(())
}

fn show_api_documentation() {
    println!(r#"
╔══════════════════════════════════════════════════════════════════════════════╗
║                       TideORM Query Builder API                              ║
╚══════════════════════════════════════════════════════════════════════════════╝

WHERE CONDITIONS
────────────────
User::query()
    .where_eq("status", "active")      // status = 'active'
    .where_not("role", "admin")        // role != 'admin'
    .where_gt("age", 18)               // age > 18
    .where_gte("age", 18)              // age >= 18
    .where_lt("age", 65)               // age < 65
    .where_lte("age", 65)              // age <= 65
    .where_like("name", "%John%")      // name LIKE '%John%'
    .where_not_like("email", "%spam%") // email NOT LIKE '%spam%'
    .where_in("role", vec!["admin", "mod"])     // role IN (...)
    .where_not_in("status", vec!["banned"])    // status NOT IN (...)
    .where_null("deleted_at")          // deleted_at IS NULL
    .where_not_null("email_verified")  // email_verified IS NOT NULL
    .where_between("age", 18, 65)      // age BETWEEN 18 AND 65
    .get()
    .await?;

ORDERING
────────
User::query()
    .order_by("created_at", Order::Desc)  // ORDER BY created_at DESC
    .order_by("name", Order::Asc)         // Then by name ASC
    .get()
    .await?;

// Convenience methods
User::query().order_asc("name")       // ORDER BY name ASC
User::query().order_desc("created_at") // ORDER BY created_at DESC
User::query().latest()                 // ORDER BY created_at DESC
User::query().oldest()                 // ORDER BY created_at ASC

PAGINATION
──────────
User::query()
    .limit(10)                    // LIMIT 10
    .offset(20)                   // OFFSET 20
    .get()
    .await?;

// Page-based pagination
User::query()
    .page(3, 25)                  // Page 3, 25 per page
    .get()
    .await?;

// Aliases
User::query().take(10).skip(20)   // Same as limit(10).offset(20)

EXECUTION METHODS
─────────────────
let users = User::query().where_eq("active", true).get().await?;       // Vec<User>
let user = User::query().where_eq("id", 1).first().await?;            // Option<User>
let user = User::query().where_eq("id", 1).first_or_fail().await?;    // Result<User>
let count = User::query().where_eq("active", true).count().await?;    // u64
let exists = User::query().where_eq("role", "admin").exists().await?; // bool
let deleted = User::query().where_eq("status", "spam").delete().await?; // u64

EFFICIENT COUNT
───────────────
// Uses optimized COUNT(*) SQL - doesn't fetch all records
let total = User::count().await?;
let active = User::query().where_eq("active", true).count().await?;

BULK DELETE
───────────
// Single efficient DELETE statement with conditions
let deleted = User::query()
    .where_eq("status", "inactive")
    .where_lt("last_login", "2023-01-01")
    .delete()
    .await?;
println!("Deleted {{}} records", deleted);

COMPLEX QUERIES
───────────────
// All conditions are ANDed together
let results = Product::query()
    .where_eq("category", "Electronics")
    .where_eq("active", true)
    .where_gt("stock", 0)
    .where_between("price", 1000, 50000)
    .where_not_null("description")
    .order_by("price", Order::Asc)
    .limit(20)
    .get()
    .await?;
"#);
}
