//! Migration Test Runner
//!
//! This example runs comprehensive tests for TideORM's migration system.
//! It tests creating tables, altering tables, rolling back migrations,
//! and various edge cases.
//!
//! ## Running the Tests
//!
//! ```bash
//! # Set up a PostgreSQL database for testing
//! createdb migration_tests
//!
//! # Run with default database URL
//! cargo run --example migration_test_runner
//!
//! # Or specify a custom database URL
//! POSTGRESQL_DATABASE_URL=postgres://user:pass@localhost/migration_tests cargo run --example migration_test_runner
//! ```
//!
//! ## Test Coverage
//!
//! -  Migration registration and ordering
//! -  Running pending migrations
//! -  Skipping already applied migrations
//! -  Table creation verification
//! -  Column and index creation
//! -  Single migration rollback
//! -  Multiple step rollback
//! -  Complete reset (rollback all)
//! -  Refresh (reset + run)
//! -  Migration status tracking
//! -  Alter table operations (add/drop columns)

use tideorm::prelude::*;

// Include the migration tests module
mod migration_tests;
use migration_tests::migrations::*;
use migration_tests::test_utils::*;

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    // Load .env file if present
    let _ = dotenvy::dotenv();
    
    // Get database URL from environment or use default
    let database_url = std::env::var("POSTGRESQL_DATABASE_URL")
        .expect("POSTGRESQL_DATABASE_URL must be set in environment or .env file");

    println!("🧪 TideORM Migration Test Runner");
    println!("=================================\n");
    println!("Database: {}\n", database_url);

    // Connect to database
    TideConfig::init()
        .database(&database_url)
        .connect()
        .await?;

    // Track test results
    let mut passed = 0;
    let mut failed = 0;

    // =========================================================================
    // SETUP: Clean up any previous test artifacts
    // =========================================================================
    print_section("SETUP: Cleaning up previous test data");
    cleanup_test_tables().await?;
    println!("  🧹 Cleaned up test tables and migration records\n");

    // =========================================================================
    // TEST 1: Create Migrator and Check Initial Status
    // =========================================================================
    print_section("TEST 1: Migrator Creation and Initial Status");
    
    let migrator = Migrator::new()
        .add(CreateProductsTable)
        .add(CreateOrdersTable)
        .add(CreateOrderItemsTable)
        .add(AddDescriptionToProducts)
        .add(CreateInventoryTable);

    let status = migrator.status().await?;
    let all_pending = status.iter().all(|s| !s.applied);
    
    print_test_result("All migrations should be pending initially", all_pending);
    if all_pending { passed += 1; } else { failed += 1; }

    let correct_count = status.len() == 5;
    print_test_result("Should have 5 migrations registered", correct_count);
    if correct_count { passed += 1; } else { failed += 1; }

    // =========================================================================
    // TEST 2: Run All Migrations
    // =========================================================================
    print_section("TEST 2: Running All Migrations");
    
    let result = migrator.run().await?;
    
    let applied_5 = result.applied.len() == 5;
    print_test_result("Should apply 5 migrations", applied_5);
    if applied_5 { passed += 1; } else { failed += 1; }

    let skipped_0 = result.skipped.is_empty();
    print_test_result("Should skip 0 migrations", skipped_0);
    if skipped_0 { passed += 1; } else { failed += 1; }

    // Verify tables were created
    let products_exists = table_exists("test_products").await;
    print_test_result("test_products table should exist", products_exists);
    if products_exists { passed += 1; } else { failed += 1; }

    let orders_exists = table_exists("test_orders").await;
    print_test_result("test_orders table should exist", orders_exists);
    if orders_exists { passed += 1; } else { failed += 1; }

    let order_items_exists = table_exists("test_order_items").await;
    print_test_result("test_order_items table should exist", order_items_exists);
    if order_items_exists { passed += 1; } else { failed += 1; }

    let inventory_exists = table_exists("test_inventory").await;
    print_test_result("test_inventory table should exist", inventory_exists);
    if inventory_exists { passed += 1; } else { failed += 1; }

    // =========================================================================
    // TEST 3: Verify Column Creation
    // =========================================================================
    print_section("TEST 3: Verify Column Creation");

    let name_col = column_exists("test_products", "name").await;
    print_test_result("test_products.name column should exist", name_col);
    if name_col { passed += 1; } else { failed += 1; }

    let sku_col = column_exists("test_products", "sku").await;
    print_test_result("test_products.sku column should exist", sku_col);
    if sku_col { passed += 1; } else { failed += 1; }

    // Verify alter table added columns
    let desc_col = column_exists("test_products", "description").await;
    print_test_result("test_products.description column should exist (from alter table)", desc_col);
    if desc_col { passed += 1; } else { failed += 1; }

    let cat_col = column_exists("test_products", "category").await;
    print_test_result("test_products.category column should exist (from alter table)", cat_col);
    if cat_col { passed += 1; } else { failed += 1; }

    // =========================================================================
    // TEST 4: Run Migrations Again (Should Skip All)
    // =========================================================================
    print_section("TEST 4: Re-run Migrations (Should Skip All)");

    let result2 = migrator.run().await?;
    
    let applied_0 = result2.applied.is_empty();
    print_test_result("Should apply 0 migrations on re-run", applied_0);
    if applied_0 { passed += 1; } else { failed += 1; }

    let skipped_5 = result2.skipped.len() == 5;
    print_test_result("Should skip 5 migrations on re-run", skipped_5);
    if skipped_5 { passed += 1; } else { failed += 1; }

    // =========================================================================
    // TEST 5: Check Migration Status
    // =========================================================================
    print_section("TEST 5: Migration Status Tracking");

    let status = migrator.status().await?;
    let all_applied = status.iter().all(|s| s.applied);
    print_test_result("All migrations should show as applied", all_applied);
    if all_applied { passed += 1; } else { failed += 1; }

    let migration_count = get_test_migration_count().await;
    let correct_db_count = migration_count == 5;
    print_test_result("_migrations table should have 5 test records", correct_db_count);
    if correct_db_count { passed += 1; } else { failed += 1; }

    // =========================================================================
    // TEST 6: Rollback Last Migration
    // =========================================================================
    print_section("TEST 6: Rollback Last Migration");

    let rollback_result = migrator.rollback().await?;
    
    let rolled_back_1 = rollback_result.rolled_back.len() == 1;
    print_test_result("Should rollback 1 migration", rolled_back_1);
    if rolled_back_1 { passed += 1; } else { failed += 1; }

    let correct_version = rollback_result.rolled_back.first()
        .map(|m| m.version == "20260106_005")
        .unwrap_or(false);
    print_test_result("Should rollback version 20260106_005", correct_version);
    if correct_version { passed += 1; } else { failed += 1; }

    // Verify inventory table was dropped
    let inventory_dropped = !table_exists("test_inventory").await;
    print_test_result("test_inventory table should be dropped after rollback", inventory_dropped);
    if inventory_dropped { passed += 1; } else { failed += 1; }

    // =========================================================================
    // TEST 7: Rollback Multiple Steps
    // =========================================================================
    print_section("TEST 7: Rollback Multiple Steps");

    let multi_rollback = migrator.rollback_steps(2).await?;
    
    let rolled_back_2 = multi_rollback.rolled_back.len() == 2;
    print_test_result("Should rollback 2 migrations", rolled_back_2);
    if rolled_back_2 { passed += 1; } else { failed += 1; }

    // Verify description column was dropped (migration 4 rolled back)
    let desc_dropped = !column_exists("test_products", "description").await;
    print_test_result("description column should be dropped", desc_dropped);
    if desc_dropped { passed += 1; } else { failed += 1; }

    // Verify order_items table was dropped (migration 3 rolled back)
    let order_items_dropped = !table_exists("test_order_items").await;
    print_test_result("test_order_items table should be dropped", order_items_dropped);
    if order_items_dropped { passed += 1; } else { failed += 1; }

    // =========================================================================
    // TEST 8: Re-run Pending Migrations
    // =========================================================================
    print_section("TEST 8: Re-run Pending Migrations");

    let rerun_result = migrator.run().await?;
    
    let reapplied_3 = rerun_result.applied.len() == 3;
    print_test_result("Should re-apply 3 migrations", reapplied_3);
    if reapplied_3 { passed += 1; } else { failed += 1; }

    let skipped_2 = rerun_result.skipped.len() == 2;
    print_test_result("Should skip 2 already applied migrations", skipped_2);
    if skipped_2 { passed += 1; } else { failed += 1; }

    // =========================================================================
    // TEST 9: Reset All Migrations
    // =========================================================================
    print_section("TEST 9: Reset All Migrations");

    let reset_result = migrator.reset().await?;
    
    let reset_5 = reset_result.rolled_back.len() == 5;
    print_test_result("Should rollback all 5 migrations", reset_5);
    if reset_5 { passed += 1; } else { failed += 1; }

    // Verify all tables are dropped
    let all_dropped = !table_exists("test_products").await
        && !table_exists("test_orders").await
        && !table_exists("test_order_items").await
        && !table_exists("test_inventory").await;
    print_test_result("All test tables should be dropped after reset", all_dropped);
    if all_dropped { passed += 1; } else { failed += 1; }

    let zero_migrations = get_test_migration_count().await == 0;
    print_test_result("_migrations table should have 0 test records after reset", zero_migrations);
    if zero_migrations { passed += 1; } else { failed += 1; }

    // =========================================================================
    // TEST 10: Refresh Migrations
    // =========================================================================
    print_section("TEST 10: Refresh Migrations (Reset + Run)");

    // First, apply some migrations
    migrator.run().await?;
    
    // Then refresh
    let refresh_result = migrator.refresh().await?;
    
    let refresh_rolled_5 = refresh_result.rolled_back.len() == 5;
    print_test_result("Refresh should rollback 5 migrations", refresh_rolled_5);
    if refresh_rolled_5 { passed += 1; } else { failed += 1; }

    let refresh_applied_5 = refresh_result.applied.len() == 5;
    print_test_result("Refresh should re-apply 5 migrations", refresh_applied_5);
    if refresh_applied_5 { passed += 1; } else { failed += 1; }

    // =========================================================================
    // TEST 11: Migration Ordering
    // =========================================================================
    print_section("TEST 11: Migration Version Ordering");

    let versions = get_test_applied_versions().await;
    let correctly_ordered = versions == vec![
        "20260106_001",
        "20260106_002",
        "20260106_003",
        "20260106_004",
        "20260106_005",
    ];
    print_test_result("Test migrations should be ordered by version", correctly_ordered);
    if correctly_ordered { passed += 1; } else { failed += 1; }

    // =========================================================================
    // CLEANUP
    // =========================================================================
    print_section("CLEANUP");
    cleanup_test_tables().await?;
    println!("  🧹 Cleaned up all test tables and migration records\n");

    // =========================================================================
    // RESULTS
    // =========================================================================
    println!("\n{}", "=".repeat(60));
    println!("📊 TEST RESULTS");
    println!("{}", "=".repeat(60));
    println!();
    println!("   Passed: {}", passed);
    println!("  ❌ Failed: {}", failed);
    println!("  📋 Total:  {}", passed + failed);
    println!();

    if failed == 0 {
        println!("🎉 All migration tests passed!\n");
    } else {
        println!("⚠️  Some tests failed. Please review the output above.\n");
    }

    Ok(())
}
