//! Migration System Example
//!
//! This example demonstrates TideORM's migration system for managing database
//! schema changes. Migrations provide a versioned, reversible way to modify
//! your database structure.
//!
//! ## Features Demonstrated
//!
//! - Creating tables with various column types
//! - Adding indexes and unique constraints
//! - Altering tables (add/drop/rename columns)
//! - Dropping tables
//! - Migration rollback
//! - Migration status tracking
//! - Two ways to run migrations:
//!   1. Via TideConfig (automatic on connect)
//!   2. Via Migrator (manual control)
//!
//! ## Running the Example
//!
//! ```bash
//! # Set up a PostgreSQL database
//! createdb migration_demo
//!
//! # Run the example
//! cargo run --example migrations
//! ```

use tideorm::prelude::*;

// ============================================================================
// MIGRATION 1: Create Users Table
// ============================================================================

#[derive(Default)]
struct CreateUsersTable;

#[async_trait]
impl Migration for CreateUsersTable {
    fn version(&self) -> &str {
        "20260106_001"
    }

    fn name(&self) -> &str {
        "create_users_table"
    }

    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema
            .create_table("users", |t| {
                t.id(); // Auto-incrementing BIGSERIAL primary key
                t.string("email").unique().not_null();
                t.string("name").not_null();
                t.string("password_hash").not_null();
                t.boolean("active").default(true).not_null();
                t.string("role").default("user").not_null();
                t.timestamps(); // created_at and updated_at
                t.soft_deletes(); // deleted_at for soft delete

                // Add indexes
                t.index(&["active"]);
                t.index(&["role"]);
            })
            .await
    }

    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.drop_table("users").await
    }
}

// ============================================================================
// MIGRATION 2: Create Posts Table
// ============================================================================

#[derive(Default)]
struct CreatePostsTable;

#[async_trait]
impl Migration for CreatePostsTable {
    fn version(&self) -> &str {
        "20260106_002"
    }

    fn name(&self) -> &str {
        "create_posts_table"
    }

    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema
            .create_table("posts", |t| {
                t.id();
                t.foreign_id("user_id").not_null(); // References users.id
                t.string("title").not_null();
                t.string("slug").unique().not_null();
                t.text("content");
                t.text("excerpt");
                t.boolean("published").default(false);
                
                // Use timestamptz for DateTime<Utc> fields
                t.timestamptz("published_at").nullable();

                // PostgreSQL-specific types
                t.jsonb("metadata"); // JSONB column
                t.text_array("tags"); // TEXT[] column

                t.timestamps(); // created_at, updated_at as TIMESTAMPTZ
                t.soft_deletes(); // deleted_at as TIMESTAMPTZ

                // Indexes
                t.index(&["user_id"]);
                t.index(&["published"]);
                t.index(&["published_at"]);
            })
            .await
    }

    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.drop_table("posts").await
    }
}

// ============================================================================
// MIGRATION 3: Create Comments Table
// ============================================================================

#[derive(Default)]
struct CreateCommentsTable;

#[async_trait]
impl Migration for CreateCommentsTable {
    fn version(&self) -> &str {
        "20260106_003"
    }

    fn name(&self) -> &str {
        "create_comments_table"
    }

    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema
            .create_table("comments", |t| {
                t.id();
                t.foreign_id("post_id").not_null();
                t.foreign_id("user_id").not_null();
                t.foreign_id("parent_id"); // For nested comments (nullable)
                t.text("body").not_null();
                t.boolean("approved").default(false);
                t.timestamps();

                // Composite index for efficient queries
                t.index(&["post_id", "approved"]);
                t.index(&["user_id"]);
            })
            .await
    }

    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.drop_table("comments").await
    }
}

// ============================================================================
// MIGRATION 4: Add Profile Fields to Users
// ============================================================================

#[derive(Default)]
struct AddProfileFieldsToUsers;

#[async_trait]
impl Migration for AddProfileFieldsToUsers {
    fn version(&self) -> &str {
        "20260106_004"
    }

    fn name(&self) -> &str {
        "add_profile_fields_to_users"
    }

    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema
            .alter_table("users", |t| {
                t.add_column("avatar_url", ColumnType::String).nullable();
                t.add_column("bio", ColumnType::Text).nullable();
                t.add_column("website", ColumnType::String).nullable();
                t.add_column("location", ColumnType::String).nullable();
                t.add_column("settings", ColumnType::Jsonb).default(DefaultValue::Raw("'{}'::jsonb".to_string()));
            })
            .await
    }

    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema
            .alter_table("users", |t| {
                t.drop_column("avatar_url");
                t.drop_column("bio");
                t.drop_column("website");
                t.drop_column("location");
                t.drop_column("settings");
            })
            .await
    }
}

// ============================================================================
// MIGRATION 5: Create Categories Table
// ============================================================================

#[derive(Default)]
struct CreateCategoriesTable;

#[async_trait]
impl Migration for CreateCategoriesTable {
    fn version(&self) -> &str {
        "20260106_005"
    }

    fn name(&self) -> &str {
        "create_categories_table"
    }

    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        // Create categories table
        schema
            .create_table("categories", |t| {
                t.id();
                t.string("name").not_null();
                t.string("slug").unique().not_null();
                t.text("description");
                t.foreign_id("parent_id"); // Self-referencing for nested categories
                t.integer("sort_order").default(0);
                t.timestamps();
            })
            .await?;

        // Create pivot table for post-category relationship (many-to-many)
        schema
            .create_table("post_categories", |t| {
                t.foreign_id("post_id").not_null();
                t.foreign_id("category_id").not_null();
                t.timestamps();

                // Composite unique index prevents duplicates
                t.unique_index(&["post_id", "category_id"]);
            })
            .await
    }

    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.drop_table("post_categories").await?;
        schema.drop_table("categories").await
    }
}

// ============================================================================
// MIGRATION 6: Add Full-Text Search Index (Raw SQL)
// ============================================================================

#[derive(Default)]
struct AddFullTextSearchToPosts;

#[async_trait]
impl Migration for AddFullTextSearchToPosts {
    fn version(&self) -> &str {
        "20260106_006"
    }

    fn name(&self) -> &str {
        "add_full_text_search_to_posts"
    }

    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        // Add a tsvector column for full-text search
        schema
            .raw(
                r#"
                ALTER TABLE "posts" 
                ADD COLUMN "search_vector" tsvector 
                GENERATED ALWAYS AS (
                    setweight(to_tsvector('english', coalesce("title", '')), 'A') ||
                    setweight(to_tsvector('english', coalesce("content", '')), 'B')
                ) STORED
            "#,
            )
            .await?;

        // Create a GIN index for fast full-text search
        schema
            .raw(r#"CREATE INDEX "idx_posts_search" ON "posts" USING GIN ("search_vector")"#)
            .await
    }

    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.drop_index("posts", "idx_posts_search").await?;
        schema
            .alter_table("posts", |t| {
                t.drop_column("search_vector");
            })
            .await
    }
}

// ============================================================================
// MAIN FUNCTION
// ============================================================================

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    // Get database URL from environment or use default
    let database_url =
        std::env::var("POSTGRESQL_DATABASE_URL").unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/test_tide_orm".to_string());

    println!(" TideORM Migration Example");
    println!("===========================\n");
    println!("Database: {}\n", database_url);

    // =========================================================================
    // APPROACH 1: Register migrations via TideConfig (recommended)
    // =========================================================================
    // 
    // This approach runs migrations automatically when connecting.
    // Migrations are tracked in `_migrations` table, so they only run once.
    //
    // ```rust
    // TideConfig::init()
    //     .database(&database_url)
    //     // Add migrations individually
    //     .migration(CreateUsersTable)
    //     .migration(CreatePostsTable)
    //     // Or add multiple via tuple (requires Default trait)
    //     .migrations::<(CreateCommentsTable, AddProfileFieldsToUsers)>()
    //     // Enable automatic migration execution
    //     .run_migrations(true)
    //     .connect()
    //     .await?;
    // ```
    //
    // For this demo, we'll use manual control to show status and rollback.

    // =========================================================================
    // APPROACH 2: Use Migrator directly (more control)
    // =========================================================================

    // Initialize TideORM (without auto-running migrations)
    TideConfig::init()
        .database(&database_url)
        .connect()
        .await?;

    // Clean up existing tables for demo purposes
    println!("🧹 Cleaning up existing tables...\n");
    let _ = Database::execute("DROP TABLE IF EXISTS post_categories CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS comments CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS posts CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS users CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS categories CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS _migrations CASCADE").await;


    // Create the migrator with all migrations
    let migrator = Migrator::new()
        .add(CreateUsersTable)
        .add(CreatePostsTable)
        .add(CreateCommentsTable)
        .add(AddProfileFieldsToUsers)
        .add(CreateCategoriesTable)
        .add(AddFullTextSearchToPosts);

    // Check migration status
    println!("📋 Migration Status (before):");
    println!("------------------------------");
    let status = migrator.status().await?;
    for s in &status {
        println!("  {}", s);
    }
    println!();

    // Run all pending migrations
    println!("▶️  Running migrations...\n");
    let result = migrator.run().await?;
    println!("{}", result);

    // Check migration status after running
    println!("📋 Migration Status (after):");
    println!("-----------------------------");
    let status = migrator.status().await?;
    for s in &status {
        println!("  {}", s);
    }
    println!();

    // Demonstrate rollback
    println!("🔙 Demonstrating rollback (last migration)...\n");
    let rollback_result = migrator.rollback().await?;
    println!("{}", rollback_result);

    // Check status after rollback
    println!("📋 Migration Status (after rollback):");
    println!("--------------------------------------");
    let status = migrator.status().await?;
    for s in &status {
        println!("  {}", s);
    }
    println!();

    // Re-run to restore
    println!("▶️  Re-running migrations...\n");
    let _result = migrator.run().await?;

    // Final status
    println!("📋 Final Migration Status:");
    println!("--------------------------");
    let status = migrator.status().await?;
    for s in &status {
        println!("  {}", s);
    }
    println!();

    println!(" Migration example completed!");

    Ok(())
}
