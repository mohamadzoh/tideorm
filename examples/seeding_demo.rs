//! Database Seeding Demo
//!
//! This example demonstrates how to use TideORM's seeding system.
//! Seeds are tracked in the database to prevent duplicate runs.
//!
//! Run with: `cargo run --example seeding_demo`

use tideorm::prelude::*;
use tideorm::seeding::{Seed, Seeder, async_trait};
use tideorm::Result;

// ============================================================================
// SEED DEFINITIONS
// ============================================================================

/// Seed for creating admin users
struct AdminUserSeeder;

#[async_trait]
impl Seed for AdminUserSeeder {
    fn name(&self) -> &str {
        "admin_user_seeder"
    }

    fn priority(&self) -> u32 {
        10 // Run early
    }

    async fn run(&self, _db: &Database) -> Result<()> {
        println!("  Creating admin users...");
        
        // In a real app, you would insert data like:
        // db.execute_raw(r#"
        //     INSERT INTO users (email, name, role, active)
        //     VALUES 
        //         ('admin@example.com', 'Admin User', 'admin', true),
        //         ('superadmin@example.com', 'Super Admin', 'superadmin', true)
        //     ON CONFLICT (email) DO NOTHING
        // "#).await?;

        // For demo purposes, just print what we would do
        println!("    Would insert: admin@example.com (Admin User)");
        println!("    Would insert: superadmin@example.com (Super Admin)");
        
        Ok(())
    }

    async fn rollback(&self, _db: &Database) -> Result<()> {
        println!("  Removing admin users...");
        
        // db.execute_raw(r#"
        //     DELETE FROM users WHERE email IN ('admin@example.com', 'superadmin@example.com')
        // "#).await?;

        println!("    Would delete: admin@example.com");
        println!("    Would delete: superadmin@example.com");
        
        Ok(())
    }
}

/// Seed for creating categories
struct CategorySeeder;

#[async_trait]
impl Seed for CategorySeeder {
    fn name(&self) -> &str {
        "category_seeder"
    }

    fn priority(&self) -> u32 {
        20 // Run after users
    }

    async fn run(&self, _db: &Database) -> Result<()> {
        println!("  Creating categories...");
        
        // db.execute_raw(r#"
        //     INSERT INTO categories (name, slug, description)
        //     VALUES 
        //         ('Electronics', 'electronics', 'Electronic devices and gadgets'),
        //         ('Clothing', 'clothing', 'Apparel and fashion items'),
        //         ('Books', 'books', 'Books and publications'),
        //         ('Home & Garden', 'home-garden', 'Home improvement and garden supplies')
        //     ON CONFLICT (slug) DO NOTHING
        // "#).await?;

        let categories = ["Electronics", "Clothing", "Books", "Home & Garden"];
        for cat in categories {
            println!("    Would insert category: {}", cat);
        }
        
        Ok(())
    }

    async fn rollback(&self, _db: &Database) -> Result<()> {
        println!("  Removing categories...");
        
        // db.execute_raw(r#"
        //     DELETE FROM categories WHERE slug IN ('electronics', 'clothing', 'books', 'home-garden')
        // "#).await?;

        println!("    Would delete seeded categories");
        
        Ok(())
    }
}

/// Seed for creating sample products
struct ProductSeeder;

#[async_trait]
impl Seed for ProductSeeder {
    fn name(&self) -> &str {
        "product_seeder"
    }

    fn priority(&self) -> u32 {
        30 // Run after categories
    }

    fn depends_on(&self) -> Vec<&str> {
        vec!["category_seeder"] // Requires categories to exist
    }

    async fn run(&self, _db: &Database) -> Result<()> {
        println!("  Creating sample products...");
        
        // In a real app:
        // db.execute_raw(r#"
        //     INSERT INTO products (name, category_id, price, stock)
        //     SELECT 'Laptop', id, 999.99, 50 FROM categories WHERE slug = 'electronics'
        //     ON CONFLICT DO NOTHING
        // "#).await?;

        let products = [
            ("Laptop", "Electronics", 999.99),
            ("Smartphone", "Electronics", 699.99),
            ("T-Shirt", "Clothing", 29.99),
            ("Rust Programming Book", "Books", 49.99),
        ];
        
        for (name, category, price) in products {
            println!("    Would insert product: {} in {} @ ${:.2}", name, category, price);
        }
        
        Ok(())
    }

    async fn rollback(&self, _db: &Database) -> Result<()> {
        println!("  Removing sample products...");
        println!("    Would delete seeded products");
        Ok(())
    }
}

/// Seed for development/testing data
struct DevDataSeeder;

#[async_trait]
impl Seed for DevDataSeeder {
    fn name(&self) -> &str {
        "dev_data_seeder"
    }

    fn priority(&self) -> u32 {
        100 // Run last
    }

    fn depends_on(&self) -> Vec<&str> {
        vec!["admin_user_seeder", "product_seeder"]
    }

    async fn run(&self, _db: &Database) -> Result<()> {
        println!("  Creating development test data...");
        
        // Create fake users, orders, reviews, etc. for testing
        println!("    Would create 100 fake users");
        println!("    Would create 500 fake orders");
        println!("    Would create 1000 fake reviews");
        
        Ok(())
    }

    async fn rollback(&self, _db: &Database) -> Result<()> {
        println!("  Removing development test data...");
        println!("    Would delete all fake test data");
        Ok(())
    }
}

// ============================================================================
// MAIN
// ============================================================================

fn main() {
    println!("=== TideORM Database Seeding Demo ===\n");

    println!("1. Registering seeds via TideConfig:\n");
    
    println!("   // Seeds are registered as a tuple in TideConfig");
    println!("   TideConfig::init()");
    println!("       .database(\"postgres://localhost/myapp\")");
    println!("       .seeds::<(AdminUserSeeder, CategorySeeder, ProductSeeder, DevDataSeeder)>()");
    println!("       .run_seeds(true)  // Enable automatic seeding on connect");
    println!("       .connect()");
    println!("       .await?;");
    
    println!("\n   Or manually add individual seeds:");
    println!("   TideConfig::init()");
    println!("       .database(\"postgres://localhost/myapp\")");
    println!("       .seed(AdminUserSeeder)");
    println!("       .seed(CategorySeeder)");
    println!("       .run_seeds(true)");
    println!("       .connect()");
    println!("       .await?;");
    
    println!("\n   Or use the Seeder directly for more control:");
    let _seeder = Seeder::new()
        .add(AdminUserSeeder)
        .add(CategorySeeder)
        .add(ProductSeeder)
        .add(DevDataSeeder);
    
    println!("\n   Seeds registered:");
    println!("   - admin_user_seeder (priority: 10)");
    println!("   - category_seeder (priority: 20)");
    println!("   - product_seeder (priority: 30, depends on: category_seeder)");
    println!("   - dev_data_seeder (priority: 100, depends on: admin_user_seeder, product_seeder)");

    println!("\n2. How seeding works:\n");
    
    println!("   First run (seeder.run().await):");
    println!("   ┌─────────────────────────────────────────────────┐");
    println!("   │ 1. Creates _seeds table if not exists           │");
    println!("   │ 2. Checks which seeds have already run          │");
    println!("   │ 3. Runs pending seeds in priority order         │");
    println!("   │ 4. Records each seed in _seeds table            │");
    println!("   └─────────────────────────────────────────────────┘");
    
    println!("\n   Second run (seeder.run().await):");
    println!("   ┌─────────────────────────────────────────────────┐");
    println!("   │ All seeds skipped - already in _seeds table     │");
    println!("   └─────────────────────────────────────────────────┘");

    println!("\n3. Database tracking table structure:\n");
    
    println!("   _seeds table:");
    println!("   ┌────┬─────────────────────┬─────────────────────┐");
    println!("   │ id │ name                │ executed_at         │");
    println!("   ├────┼─────────────────────┼─────────────────────┤");
    println!("   │ 1  │ admin_user_seeder   │ 2026-01-09 10:00:00 │");
    println!("   │ 2  │ category_seeder     │ 2026-01-09 10:00:01 │");
    println!("   │ 3  │ product_seeder      │ 2026-01-09 10:00:02 │");
    println!("   │ 4  │ dev_data_seeder     │ 2026-01-09 10:00:03 │");
    println!("   └────┴─────────────────────┴─────────────────────┘");

    println!("\n4. Available seeder operations:\n");
    
    println!("   // Run all pending seeds");
    println!("   seeder.run().await?;");
    println!();
    println!("   // Run a specific seed (even if already executed)");
    println!("   seeder.run_seed(\"product_seeder\").await?;");
    println!();
    println!("   // Rollback the last seed");
    println!("   seeder.rollback().await?;");
    println!();
    println!("   // Rollback a specific seed");
    println!("   seeder.rollback_seed(\"dev_data_seeder\").await?;");
    println!();
    println!("   // Rollback multiple seeds");
    println!("   seeder.rollback_steps(3).await?;");
    println!();
    println!("   // Reset all seeds (rollback everything)");
    println!("   seeder.reset().await?;");
    println!();
    println!("   // Refresh (reset + run all)");
    println!("   seeder.refresh().await?;");
    println!();
    println!("   // Get status of all seeds");
    println!("   let status = seeder.status().await?;");
    println!("   for s in status {{");
    println!("       println!(\"{{}}\", s); // [✓] admin_user_seeder (priority: 10)");
    println!("   }}");

    println!("\n5. Example seed implementation:\n");
    
    println!("   struct UserSeeder;");
    println!("   ");
    println!("   #[async_trait]");
    println!("   impl Seed for UserSeeder {{");
    println!("       fn name(&self) -> &str {{ \"user_seeder\" }}");
    println!("       ");
    println!("       fn priority(&self) -> u32 {{ 10 }}  // Lower = runs first");
    println!("       ");
    println!("       fn depends_on(&self) -> Vec<&str> {{");
    println!("           vec![\"role_seeder\"]  // Requires roles to exist");
    println!("       }}");
    println!("       ");
    println!("       async fn run(&self, db: &Database) -> Result<()> {{");
    println!("           db.execute_raw(r#\"");
    println!("               INSERT INTO users (email, name, role_id)");
    println!("               SELECT 'admin@test.com', 'Admin', id ");
    println!("               FROM roles WHERE name = 'admin'");
    println!("               ON CONFLICT DO NOTHING");
    println!("           \"#).await?;");
    println!("           Ok(())");
    println!("       }}");
    println!("       ");
    println!("       async fn rollback(&self, db: &Database) -> Result<()> {{");
    println!("           db.execute_raw(r#\"");
    println!("               DELETE FROM users WHERE email = 'admin@test.com'");
    println!("           \"#).await?;");
    println!("           Ok(())");
    println!("       }}");
    println!("   }}");

    println!("\n6. SeedResult output example:\n");
    
    println!("   Executed seeds:");
    println!("     ✓ admin_user_seeder");
    println!("     ✓ category_seeder");
    println!("     ✓ product_seeder");
    println!("   Skipped seeds (already executed):");
    println!("     - dev_data_seeder");

    println!("\n=== Demo Complete ===");
    println!("\nTo use in a real application:");
    println!("1. Define your Seed implementations with #[derive(Default)]");
    println!("2. Register seeds via TideConfig::init().seeds::<(Seed1, Seed2)>()");
    println!("3. Set .run_seeds(true) to auto-run on connect");
    println!("4. Or use Seeder::new().add(seed).run().await? for manual control");
    println!("5. Seeds are tracked in _seeds table - won't run twice!");
}
