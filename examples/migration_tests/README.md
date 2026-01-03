# Migration Tests

This folder contains comprehensive tests for TideORM's migration system.

## Structure

```
migration_tests/
├── mod.rs           # Module exports
├── migrations.rs    # Sample test migrations
├── test_utils.rs    # Helper functions for testing
└── README.md        # This file
```

## Running the Tests

### Prerequisites

1. Have a PostgreSQL database running
2. Create a test database:
   ```bash
   createdb migration_tests
   ```

### Run the Test Runner

```bash
# Using default database URL (postgres://localhost/migration_tests)
cargo run --example migration_test_runner

# With custom database URL
POSTGRESQL_DATABASE_URL=postgres://user:pass@localhost/migration_tests cargo run --example migration_test_runner
```

## Test Coverage

The migration test runner covers:

### Basic Operations
- ✅ Migration registration
- ✅ Running pending migrations
- ✅ Skipping already applied migrations
- ✅ Migration ordering by version

### Table Operations
- ✅ Creating tables with columns
- ✅ Adding indexes
- ✅ Adding unique constraints
- ✅ Foreign key columns

### Alter Table Operations
- ✅ Adding columns to existing tables
- ✅ Dropping columns from tables

### Rollback Operations
- ✅ Rolling back the last migration
- ✅ Rolling back multiple steps
- ✅ Resetting all migrations (rollback all)
- ✅ Refresh (reset + run)

### Status Tracking
- ✅ Migration status reporting
- ✅ Tracking applied migrations in `_migrations` table

## Sample Migrations

The test suite includes 5 sample migrations:

1. **CreateProductsTable** (`20260106_001`)
   - Creates `test_products` table with basic columns
   - Adds indexes on `active` and `sku` columns

2. **CreateOrdersTable** (`20260106_002`)
   - Creates `test_orders` table
   - Adds indexes on `status` and `order_number`

3. **CreateOrderItemsTable** (`20260106_003`)
   - Creates `test_order_items` pivot table
   - Links orders to products

4. **AddDescriptionToProducts** (`20260106_004`)
   - Demonstrates ALTER TABLE
   - Adds `description` and `category` columns

5. **CreateInventoryTable** (`20260106_005`)
   - Creates `test_inventory` table
   - Uses composite unique index

## Writing Your Own Migration Tests

You can use the test utilities to write additional tests:

```rust
use migration_tests::test_utils::*;

// Check if a table exists
let exists = table_exists("my_table").await;

// Check if a column exists
let has_column = column_exists("my_table", "my_column").await;

// Check if an index exists
let has_index = index_exists("idx_my_table_column").await;

// Get migration count
let count = get_migration_count().await;

// Get all applied versions
let versions = get_applied_versions().await;

// Clean up test tables
cleanup_test_tables().await?;
```

## Expected Output

When running successfully, you should see output like:

```
🧪 TideORM Migration Test Runner
=================================

Database: postgres://localhost/migration_tests

============================================================
📋 SETUP: Cleaning up previous test data
============================================================
  🧹 Cleaned up test tables and migration records

============================================================
📋 TEST 1: Migrator Creation and Initial Status
============================================================
  ✅ All migrations should be pending initially
  ✅ Should have 5 migrations registered

... (more test output)

============================================================
📊 TEST RESULTS
============================================================

  ✅ Passed: 32
  ❌ Failed: 0
  📋 Total:  32

🎉 All migration tests passed!
```
