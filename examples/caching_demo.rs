//! Caching Demo for TideORM
//!
//! This example demonstrates how to use TideORM's query caching and
//! prepared statement caching features to improve database performance.
//!
//! ## Running the Example
//!
//! ```bash
//! # Set database URL
//! export DATABASE_URL="postgres://user:pass@localhost/testdb"
//!
//! # Run the example
//! cargo run --example caching_demo
//! ```

use std::time::Duration;
use tideorm::prelude::*;

// =============================================================================
// MODEL DEFINITIONS
// =============================================================================

#[tideorm::model]
#[tide(table = "users")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub role: String,
    pub active: bool,
}

#[tideorm::model]
#[tide(table = "products")]
pub struct Product {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub category: String,
    pub price: f64,
    pub in_stock: bool,
}

// =============================================================================
// MAIN
// =============================================================================

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    println!(" TideORM Caching Demo\n");
    
    // Check for database URL
    let db_url = std::env::var("POSTGRESQL_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/test_tide_orm".to_string());
    
    // Initialize database
    println!(" Connecting to database...");
    TideConfig::init()
        .database(&db_url)
        .max_connections(10)
        .connect()
        .await?;
    
    println!(" Connected!\n");
    
    // =========================================================================
    // QUERY CACHE DEMONSTRATION
    // =========================================================================
    
    demo_query_cache_basic().await;
    demo_query_cache_with_key().await;
    demo_query_cache_invalidation().await;
    demo_prepared_statement_cache().await;
    demo_cache_statistics().await;
    
    println!("\n Demo completed successfully!");
    Ok(())
}

// =============================================================================
// DEMO FUNCTIONS
// =============================================================================

/// Demonstrate basic query caching
async fn demo_query_cache_basic() {
    println!("═══════════════════════════════════════════════════════════════");
    println!("  DEMO 1: Basic Query Caching");
    println!("═══════════════════════════════════════════════════════════════\n");
    
    // Enable the global query cache
    QueryCache::global()
        .set_max_entries(1000)
        .set_default_ttl(Duration::from_secs(60))
        .set_strategy(CacheStrategy::LRU)
        .enable();
    
    println!(" Query cache enabled with:");
    println!("   - Max entries: 1000");
    println!("   - Default TTL: 60 seconds");
    println!("   - Strategy: LRU (Least Recently Used)\n");
    
    // Example: Cache a query for 5 minutes
    println!("📝 Example: Caching a query\n");
    println!("```rust");
    println!(r#"// Cache results for 5 minutes
let active_users = User::query()
    .where_eq("active", true)
    .cache(Duration::from_secs(300))
    .get()
    .await?;

// Second call hits the cache (no database query)
let active_users = User::query()
    .where_eq("active", true)
    .cache(Duration::from_secs(300))
    .get()
    .await?;"#);
    println!("```\n");
}

/// Demonstrate caching with custom keys
async fn demo_query_cache_with_key() {
    println!("═══════════════════════════════════════════════════════════════");
    println!("  DEMO 2: Caching with Custom Keys");
    println!("═══════════════════════════════════════════════════════════════\n");
    
    println!("📝 Example: Using custom cache keys for easier invalidation\n");
    println!("```rust");
    println!(r#"// Cache with a meaningful key
let admin_users = User::query()
    .where_eq("role", "admin")
    .cache_with_key("admin_users", Duration::from_secs(600))
    .get()
    .await?;

// Later, when admin data changes, invalidate by key
QueryCache::global().invalidate("admin_users");

// Next query will hit the database and refresh the cache
let admin_users = User::query()
    .where_eq("role", "admin")
    .cache_with_key("admin_users", Duration::from_secs(600))
    .get()
    .await?;"#);
    println!("```\n");
    
    println!("💡 Tip: Use descriptive keys like 'featured_products', 'active_subscriptions'\n");
}

/// Demonstrate cache invalidation
async fn demo_query_cache_invalidation() {
    println!("═══════════════════════════════════════════════════════════════");
    println!("  DEMO 3: Cache Invalidation Strategies");
    println!("═══════════════════════════════════════════════════════════════\n");
    
    println!("📝 Example: Different invalidation methods\n");
    println!("```rust");
    println!(r#"// 1. Invalidate by specific key
QueryCache::global().invalidate("admin_users");

// 2. Invalidate all cached queries for a model/table
QueryCache::global().invalidate_model("users");

// 3. Clear the entire cache
QueryCache::global().clear();

// 4. Evict expired entries (cleanup)
QueryCache::global().evict_expired();

// 5. Disable caching for a specific query
let users = User::query()
    .where_eq("active", true)
    .no_cache()  // Skip cache even if globally enabled
    .get()
    .await?;"#);
    println!("```\n");
    
    // Demonstrate invalidation
    println!("🔄 Live demonstration:\n");
    
    // Add some cache entries
    QueryCache::global().set("demo_key_1", &vec!["value1"], None, "demo").ok();
    QueryCache::global().set("demo_key_2", &vec!["value2"], None, "demo").ok();
    QueryCache::global().set("other_key", &vec!["value3"], None, "other").ok();
    
    println!("   Cache entries: {}", QueryCache::global().len());
    
    // Invalidate by model
    QueryCache::global().invalidate_model("demo");
    println!("   After invalidating 'demo' model: {}", QueryCache::global().len());
    
    // Clear all
    QueryCache::global().clear();
    println!("   After clearing all: {}\n", QueryCache::global().len());
}

/// Demonstrate prepared statement caching
async fn demo_prepared_statement_cache() {
    println!("═══════════════════════════════════════════════════════════════");
    println!("  DEMO 4: Prepared Statement Caching");
    println!("═══════════════════════════════════════════════════════════════\n");
    
    // Enable prepared statement cache
    PreparedStatementCache::global()
        .set_max_statements(500)
        .set_max_age(Duration::from_secs(3600))
        .enable();
    
    println!(" Prepared statement cache enabled with:");
    println!("   - Max statements: 500");
    println!("   - Max age: 1 hour\n");
    
    println!("📝 Example: Statements are automatically cached\n");
    println!("```rust");
    println!(r#"// Enable prepared statement caching
PreparedStatementCache::global()
    .set_max_statements(500)
    .enable();

// First execution - statement is prepared and cached
let users = User::find(1).await?;

// Subsequent executions reuse the cached prepared statement
// This avoids repeated parsing and planning
for id in 2..100 {{
    let user = User::find(id).await?;
}}

// View cache statistics
let stats = PreparedStatementCache::global().stats();
println!("Hits: {{}}, Misses: {{}}", stats.hits, stats.misses);"#);
    println!("```\n");
    
    // Demonstrate statement caching
    let sql1 = "SELECT * FROM users WHERE id = $1";
    let sql2 = "SELECT * FROM users WHERE active = $1";
    
    // First call - misses
    let (_, cached) = PreparedStatementCache::global().get_or_prepare(sql1);
    println!("   First call to '{}...':", &sql1[..30]);
    println!("   - Cached: {}", cached);
    
    // Second call - hits
    let (_, cached) = PreparedStatementCache::global().get_or_prepare(sql1);
    println!("   Second call to same query:");
    println!("   - Cached: {}\n", cached);
    
    // Different query - misses
    let (_, cached) = PreparedStatementCache::global().get_or_prepare(sql2);
    println!("   Call to different query '{}...':", &sql2[..30]);
    println!("   - Cached: {}\n", cached);
}

/// Demonstrate cache statistics
async fn demo_cache_statistics() {
    println!("═══════════════════════════════════════════════════════════════");
    println!("  DEMO 5: Cache Statistics & Monitoring");
    println!("═══════════════════════════════════════════════════════════════\n");
    
    // Simulate some cache activity
    QueryCache::global().enable();
    for i in 0..10 {
        let key = format!("stats_demo_{}", i);
        QueryCache::global().set(&key, &i, None, "demo").ok();
    }
    
    // Get first few to generate hits
    for i in 0..5 {
        let key = format!("stats_demo_{}", i);
        let _: Option<i32> = QueryCache::global().get(&key);
    }
    
    // Get some non-existent keys to generate misses
    for i in 20..25 {
        let key = format!("stats_demo_{}", i);
        let _: Option<i32> = QueryCache::global().get(&key);
    }
    
    println!("📊 Query Cache Statistics:\n");
    let stats = QueryCache::global().stats();
    println!("   Entries: {}", stats.entries);
    println!("   Hits: {}", stats.hits);
    println!("   Misses: {}", stats.misses);
    println!("   Hit Ratio: {:.1}%", stats.hit_ratio() * 100.0);
    println!("   Evictions: {}", stats.evictions);
    println!("   Invalidations: {}", stats.invalidations);
    
    println!("\n📊 Prepared Statement Cache Statistics:\n");
    let stmt_stats = PreparedStatementCache::global().stats();
    println!("   Cached Statements: {}", stmt_stats.cached_count);
    println!("   Hits: {}", stmt_stats.hits);
    println!("   Misses: {}", stmt_stats.misses);
    println!("   Hit Ratio: {:.1}%", stmt_stats.hit_ratio() * 100.0);
    println!("   Total Executions: {}", stmt_stats.total_executions);
    
    println!("\n📝 Example: Monitoring cache performance\n");
    println!("```rust");
    println!(r#"// Get query cache stats
let stats = QueryCache::global().stats();
println!("Hit ratio: {{:.1}}%", stats.hit_ratio() * 100.0);

// Get prepared statement stats
let stmt_stats = PreparedStatementCache::global().stats();
println!("Statement cache hit ratio: {{:.1}}%", stmt_stats.hit_ratio() * 100.0);

// View cached statements
let statements = PreparedStatementCache::global().cached_statements_info();
for stmt in statements {{
    println!("SQL: {{}}", stmt.sql_preview);
    println!("  Executions: {{}}", stmt.execution_count);
    println!("  Avg time: {{}}µs", stmt.avg_execution_time_us);
}}

// Reset stats
QueryCache::global().reset_stats();
PreparedStatementCache::global().reset_stats();"#);
    println!("```\n");
    
    // Clean up
    QueryCache::global().clear();
    PreparedStatementCache::global().clear();
}

// =============================================================================
// ADDITIONAL EXAMPLES (Code snippets)
// =============================================================================

/// Example: Cache warming at startup
#[allow(dead_code)]
async fn example_cache_warming() -> tideorm::Result<()> {
    // Pre-populate cache with frequently accessed data
    let _featured_products = Product::query()
        .where_eq("in_stock", true)
        .order_by("name", Order::Asc)
        .limit(100)
        .cache_with_key("featured_products", Duration::from_secs(3600))
        .get()
        .await?;
    
    let _admin_users = User::query()
        .where_eq("role", "admin")
        .cache_with_key("admin_users", Duration::from_secs(3600))
        .get()
        .await?;
    
    println!("Cache warmed with {} entries", QueryCache::global().len());
    Ok(())
}

/// Example: Cache key builder
#[allow(dead_code)]
fn example_cache_key_builder() {
    // Build cache keys programmatically
    let key = CacheKeyBuilder::new()
        .table("products")
        .condition("category", "electronics")
        .condition("in_stock", true)
        .order("price", "asc")
        .limit(50)
        .build();
    
    println!("Generated cache key: {}", key);
    
    // Or get a hash for the key
    let hash = CacheKeyBuilder::new()
        .table("users")
        .condition("active", true)
        .build_hash();
    
    println!("Cache key hash: {}", hash);
}

/// Example: Custom cache options with tags
#[allow(dead_code)]
async fn example_cache_with_tags() -> tideorm::Result<()> {
    let options = CacheOptions::new(Duration::from_secs(300))
        .with_key("active_premium_users")
        .with_tags(&["users", "premium", "active"]);
    
    let _users = User::query()
        .where_eq("active", true)
        .where_eq("role", "premium")
        .cache_with_options(options)
        .get()
        .await?;
    
    Ok(())
}
