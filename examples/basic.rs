//! # TideORM Basic Example
//!
//! **Category:** CRUD Operations
//!
//! This example demonstrates the core functionality of TideORM.
//! Notice how SeaORM is never exposed to the user.
//!
//! ## Run this example
//!
//! ```bash
//! cargo run --example basic
//! ```

use tideorm::prelude::*;

// =============================================================================
// MODEL DEFINITION
// =============================================================================
// 
// Define your models using the #[derive(Model)] macro.
// No SeaORM types, traits, or concepts are visible here.

/// User model - represents a user in the database
/// 
/// Demonstrates index definitions with separate macros:
/// - #[index("column")] for regular indexes
/// - #[unique_index("column")] for unique constraints
/// - Composite indexes: #[index("col1,col2")]
/// - Named indexes: #[index(name = "idx_custom", columns = "col1,col2")]
#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "users")]
#[index("email")]
#[index("active")]
#[unique_index("email")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    #[tide(nullable)]
    pub bio: Option<String>,
    pub active: bool,

    // Timestamps are auto-filled by TideORM
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Post model - represents a blog post
/// 
/// Demonstrates composite indexes with custom names
#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "posts")]
#[index("user_id")]
#[index(name = "idx_user_published", columns = "user_id,published")]
pub struct Post {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub body: String,
    pub published: bool,
    // Timestamps are auto-filled by TideORM
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// =============================================================================
// MAIN EXAMPLE
// =============================================================================

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    // =========================================================================
    // DATABASE CONNECTION
    // =========================================================================
    //
    // Initialize TideORM once at startup with all configuration.
    // After this, ALL model operations use this connection automatically.
    
    // Load database URL from .env file
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("POSTGRESQL_DATABASE_URL").unwrap();
    
    // Note: This will fail without a running database, but demonstrates the API
    match TideConfig::init()
        .database_type(DatabaseType::Postgres)
        .database(&db_url)
        .max_connections(10)
        .min_connections(2)
        .sync(true)  // Auto-sync tables (development only!)
        // .force_sync(true)  // ⚠️ DANGER: Drops columns not in model! Uncomment for strict sync.
        .models::<(User, Post)>()  // Register models for sync
        .languages(&["en", "fr"])
        .connect()
        .await
    {
        Ok(_) => {
            println!("✓ TideORM initialized!");
        }
        Err(e) => {
            println!("Could not connect to database: {}", e);
            println!("\nThis example shows the TideORM API - no database needed to compile!");
            println!("\nExample API usage:\n");
            print_api_examples();
            return Ok(());
        }
    };
    
    // =========================================================================
    // CREATE (INSERT)
    // =========================================================================
    
    // Generate unique email to avoid duplicate key errors
    let unique_email = format!("john{}@example.com", chrono::Utc::now().timestamp_millis());
    
    let user = User {
        id: 0, // Will be auto-generated
        email: unique_email,
        name: "John Doe".to_string(),
        bio: Some("Hello, I'm John!".to_string()),
        active: true,
        // These will be auto-set by TideORM, but we need to provide initial values
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    
    let user = user.save().await?;
    println!("Created user: {:?}", user);
    
    // =========================================================================
    // READ (SELECT)
    // =========================================================================
    
    // Get all records
    let all_users = User::all().await?;
    println!("Total users: {}", all_users.len());
    
    // Query builder
    let active_users = User::query()
        .where_eq("active", true)
        .order_by("name", Order::Asc)
        .limit(10)
        .get()
        .await?;
    
    println!("Active users: {}", active_users.len());
    
    // =========================================================================
    // UPDATE
    // =========================================================================
    
    if let Some(mut user) = User::query().first().await? {
        user.name = "John Smith".to_string();
        let _user = user.update().await?;
        println!("Updated user!");
    }
    
    // =========================================================================
    // DELETE
    // =========================================================================
    
    // Delete with query
    let deleted = User::query()
        .where_eq("active", false)
        .delete()
        .await?;
    
    println!("Deleted {} inactive users", deleted);
    
    // Or delete instance directly
    // let user = User::find(1).await?.unwrap();
    // user.delete().await?;

    println!("\n✓ Example completed successfully!");
    Ok(())
}

fn print_api_examples() {
    println!(r#"
// Model Definition:
#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "users")]
pub struct User {{
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
}}

// Initialize database (once at startup):
Database::init("postgres://localhost/mydb").await?;

// Create:
let user = User {{ id: 0, email: "test@example.com".into(), name: "Test".into() }};
let user = user.save().await?;

// Read:
let users = User::all().await?;
let user = User::find(1).await?;
let users = User::query().where_eq("active", true).get().await?;

// Update:
user.name = "New Name".into();
let user = user.update().await?;

// Delete:
user.delete().await?;
User::destroy(1).await?;
"#);
}

// =============================================================================
// KEY POINTS
// =============================================================================
//
// 1. NO SeaORM types are visible in user code:
//    - No `Entity`, `Model`, `ActiveModel`
//    - No `DbConn`, `DatabaseConnection`
//    - No `sea_orm::*` imports
//
// 2. Clean, Rails/Laravel-like API:
//    - `Database::init()` - initialize once
//    - `User::all()` - get all records
//    - `User::find(1)` - find by id
//    - `user.save()` - insert record
//    - `user.update()` - update record
//    - `user.delete()` - delete record
//
// 3. Fluent query builder:
//    - `User::query().where_eq().order_by().get()`
//
// 4. Type-safe without verbosity
//
// 5. Async-first design
//
// 6. User-friendly errors
