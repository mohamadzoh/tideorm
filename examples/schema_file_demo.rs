//! Schema File Generation Example
//!
//! This example demonstrates TideORM's schema file generation feature, which
//! automatically generates SQL schema files from your database or model definitions.
//!
//! ## Features Demonstrated
//!
//! - Automatic schema file generation on connect
//! - Manual schema file generation
//! - Schema generation from database introspection
//! - Using SchemaGenerator and TableSchemaBuilder
//! - Generating schema for multiple database types
//!
//! ## Running the Example
//!
//! ```bash
//! # Set up a PostgreSQL database
//! createdb schema_demo
//!
//! # Run the example
//! cargo run --example schema_file_demo
//! ```

use tideorm::prelude::*;
use tideorm::schema::{SchemaGenerator, TableSchemaBuilder, ColumnSchema, SchemaWriter};
use tideorm::model::IndexDefinition;

// ============================================================================
// MODEL DEFINITIONS
// ============================================================================

/// User model with various field types
#[derive(Debug, Clone, Model, Serialize, Deserialize)]
#[tide(table = "users", hidden = "password_hash")]
#[index("email")]
#[index("status")]
#[unique_index("email")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub password_hash: String,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Post model with soft delete
#[derive(Debug, Clone, Model, Serialize, Deserialize)]
#[tide(table = "posts", soft_delete)]
#[index("author_id")]
#[index(name = "idx_posts_status_published", columns = "status,published_at")]
#[unique_index("slug")]
pub struct Post {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub author_id: i64,
    pub slug: String,
    pub title: String,
    pub content: String,
    pub status: String,
    pub published_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Category model
#[derive(Debug, Clone, Model, Serialize, Deserialize)]
#[tide(table = "categories")]
#[index("parent_id")]
#[unique_index("slug")]
pub struct Category {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub sort_order: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// ============================================================================
// MAIN EXAMPLE
// ============================================================================

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    println!("🌊 TideORM Schema File Generation Demo\n");
    println!("==========================================\n");

    // Demo 1: Generate schema programmatically (no database connection needed)
    demo_programmatic_schema_generation().await?;

    // Demo 2: Connect with auto schema file generation
    demo_auto_schema_file().await?;

    // Demo 3: Manual schema file generation
    demo_manual_schema_generation().await?;

    println!("\n==========================================");
    println!("✨ Demo completed successfully!");
    println!();
    println!("Generated files:");
    println!("  - schema_programmatic.sql (from model definitions)");
    println!("  - schema_auto.sql (auto-generated on connect)");
    println!("  - schema_manual.sql (manual generation)");

    Ok(())
}

// ============================================================================
// DEMO 1: Programmatic Schema Generation
// ============================================================================

async fn demo_programmatic_schema_generation() -> tideorm::Result<()> {
    println!("📝 Demo 1: Programmatic Schema Generation");
    println!("   (No database connection required)\n");

    // Create a schema generator for PostgreSQL
    let mut generator = SchemaGenerator::new(DatabaseType::Postgres);

    // Build users table schema
    let users_table = TableSchemaBuilder::new("users")
        .column(ColumnSchema::new("id", "BIGINT").primary_key().auto_increment())
        .column(ColumnSchema::new("email", "VARCHAR(255)").not_null())
        .column(ColumnSchema::new("name", "VARCHAR(255)").not_null())
        .column(ColumnSchema::new("password_hash", "TEXT").not_null())
        .column(ColumnSchema::new("status", "VARCHAR(50)").not_null().default("'active'"))
        .column(ColumnSchema::new("created_at", "TIMESTAMPTZ").not_null().default("NOW()"))
        .column(ColumnSchema::new("updated_at", "TIMESTAMPTZ").not_null().default("NOW()"))
        .index(IndexDefinition::new("idx_users_email", vec!["email".to_string()], false))
        .index(IndexDefinition::new("idx_users_status", vec!["status".to_string()], false))
        .index(IndexDefinition::new("uidx_users_email", vec!["email".to_string()], true))
        .build();

    generator.add_table(users_table);

    // Build posts table schema
    let posts_table = TableSchemaBuilder::new("posts")
        .column(ColumnSchema::new("id", "BIGINT").primary_key().auto_increment())
        .column(ColumnSchema::new("author_id", "BIGINT").not_null())
        .column(ColumnSchema::new("slug", "VARCHAR(255)").not_null())
        .column(ColumnSchema::new("title", "VARCHAR(255)").not_null())
        .column(ColumnSchema::new("content", "TEXT"))
        .column(ColumnSchema::new("status", "VARCHAR(50)").not_null().default("'draft'"))
        .column(ColumnSchema::new("published_at", "TIMESTAMPTZ"))
        .column(ColumnSchema::new("created_at", "TIMESTAMPTZ").not_null().default("NOW()"))
        .column(ColumnSchema::new("updated_at", "TIMESTAMPTZ").not_null().default("NOW()"))
        .column(ColumnSchema::new("deleted_at", "TIMESTAMPTZ"))
        .index(IndexDefinition::new("idx_posts_author_id", vec!["author_id".to_string()], false))
        .index(IndexDefinition::new("idx_posts_status_published", vec!["status".to_string(), "published_at".to_string()], false))
        .index(IndexDefinition::new("uidx_posts_slug", vec!["slug".to_string()], true))
        .build();

    generator.add_table(posts_table);

    // Build categories table schema
    let categories_table = TableSchemaBuilder::new("categories")
        .column(ColumnSchema::new("id", "BIGINT").primary_key().auto_increment())
        .column(ColumnSchema::new("parent_id", "BIGINT"))
        .column(ColumnSchema::new("name", "VARCHAR(255)").not_null())
        .column(ColumnSchema::new("slug", "VARCHAR(255)").not_null())
        .column(ColumnSchema::new("description", "TEXT"))
        .column(ColumnSchema::new("sort_order", "INTEGER").not_null().default("0"))
        .column(ColumnSchema::new("created_at", "TIMESTAMPTZ").not_null().default("NOW()"))
        .index(IndexDefinition::new("idx_categories_parent_id", vec!["parent_id".to_string()], false))
        .index(IndexDefinition::new("uidx_categories_slug", vec!["slug".to_string()], true))
        .build();

    generator.add_table(categories_table);

    // Generate SQL
    let sql = generator.generate();

    // Write to file
    std::fs::write("schema_programmatic.sql", &sql)
        .expect("Failed to write schema file");

    println!("   ✅ Generated schema_programmatic.sql");
    println!("   Preview:");
    println!("   ----------------------------------------");
    for line in sql.lines().take(15) {
        println!("   {}", line);
    }
    println!("   ...\n");

    Ok(())
}

// ============================================================================
// DEMO 2: Auto Schema File Generation on Connect
// ============================================================================

async fn demo_auto_schema_file() -> tideorm::Result<()> {
    println!("📝 Demo 2: Auto Schema File Generation on Connect\n");

    // Load database URL from .env file
    let _ = dotenvy::dotenv();
    let db_url = match std::env::var("POSTGRESQL_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            println!("   ⚠️  POSTGRESQL_DATABASE_URL not set, skipping database demo");
            println!("   Set it in .env file to enable this demo\n");
            return Ok(());
        }
    };

    // Connect with auto schema file generation
    TideConfig::init()
        .database_type(DatabaseType::Postgres)
        .database(&db_url)
        .max_connections(5)
        .min_connections(1)
        .schema_file("schema_auto.sql")  // Auto-generate schema file on connect
        .connect()
        .await?;

    println!("   ✅ Connected to database");
    println!("   ✅ Generated schema_auto.sql from database introspection");

    // Verify file was created
    if std::path::Path::new("schema_auto.sql").exists() {
        let content = std::fs::read_to_string("schema_auto.sql")
            .expect("Failed to read schema file");
        println!("   Preview:");
        println!("   ----------------------------------------");
        for line in content.lines().take(15) {
            println!("   {}", line);
        }
        println!("   ...\n");
    }

    Ok(())
}

// ============================================================================
// DEMO 3: Manual Schema Generation
// ============================================================================

async fn demo_manual_schema_generation() -> tideorm::Result<()> {
    println!("📝 Demo 3: Manual Schema File Generation\n");

    // Check if database is connected
    if !TideConfig::is_connected() {
        println!("   ⚠️  Database not connected, skipping manual generation demo\n");
        return Ok(());
    }

    // Manually trigger schema generation
    SchemaWriter::write_schema("schema_manual.sql").await?;

    println!("   ✅ Generated schema_manual.sql");

    // Also demonstrate generating schema for different database types
    println!("\n   Generating schemas for different database types:");

    // MySQL schema
    let mut mysql_gen = SchemaGenerator::new(DatabaseType::MySQL);
    mysql_gen.add_table(TableSchemaBuilder::new("example")
        .column(ColumnSchema::new("id", "BIGINT").primary_key().auto_increment())
        .column(ColumnSchema::new("name", "VARCHAR(255)").not_null())
        .build());
    let mysql_sql = mysql_gen.generate();
    std::fs::write("schema_mysql_example.sql", &mysql_sql)
        .expect("Failed to write MySQL schema file");
    println!("   ✅ Generated schema_mysql_example.sql (MySQL syntax)");

    // SQLite schema
    let mut sqlite_gen = SchemaGenerator::new(DatabaseType::SQLite);
    sqlite_gen.add_table(TableSchemaBuilder::new("example")
        .column(ColumnSchema::new("id", "INTEGER").primary_key().auto_increment())
        .column(ColumnSchema::new("name", "TEXT").not_null())
        .build());
    let sqlite_sql = sqlite_gen.generate();
    std::fs::write("schema_sqlite_example.sql", &sqlite_sql)
        .expect("Failed to write SQLite schema file");
    println!("   ✅ Generated schema_sqlite_example.sql (SQLite syntax)");

    println!();

    Ok(())
}

// ============================================================================
// UTILITY: Display Schema File Helpers
// ============================================================================

/// Demonstrates accessing schema configuration
fn _demo_schema_config() {
    // Check configured schema file path
    if let Some(path) = TideConfig::schema_file_path() {
        println!("Schema file configured at: {}", path);
    }

    // Check database type for schema generation
    if let Some(db_type) = TideConfig::get_database_type() {
        println!("Database type: {}", db_type);
        println!("  - Supports JSON: {}", db_type.supports_json());
        println!("  - Supports Arrays: {}", db_type.supports_arrays());
        println!("  - Quote character: {}", db_type.quote_char());
    }
}
