//! TideORM Upsert Example
//!
//! **Category:** Upsert Operations
//!
//! Demonstrates the new upsert/on-conflict functionality in TideORM.
//!
//! ## Features Demonstrated
//! - `insert_or_update` - Simple upsert by conflict column
//! - `on_conflict` builder - Advanced upsert with column control
//!
//! ## Run this example
//!
//! ```bash
//! cargo run --example upsert_demo
//! ```
//!
//! ## Prerequisites
//!
//! PostgreSQL running and configured in `.env` file:
//! ```
//! POSTGRESQL_DATABASE_URL=postgres://postgres:postgres@localhost:5432/test_tide_orm
//! ```

use tideorm::prelude::*;

#[derive(Debug, Clone, Model, Serialize, Deserialize)]
#[tide(table = "users")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub login_count: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Model, Serialize, Deserialize)]
#[tide(table = "settings")]
pub struct Setting {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub tenant_id: i64,
    pub key: String,
    pub value: String,
}

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    println!("🚀 TideORM Upsert Demo\n");
    
    // =========================================================================
    // SETUP
    // =========================================================================
    
    // Load database URL from .env file
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("POSTGRESQL_DATABASE_URL")
        .unwrap();
    
    TideConfig::init()
        .database(&db_url)
        .max_connections(5)
        .connect()
        .await?;
    
    // Create tables
    Database::execute("DROP TABLE IF EXISTS settings CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS users CASCADE").await?;
    
    Database::execute(r#"
        CREATE TABLE users (
            id BIGSERIAL PRIMARY KEY,
            email VARCHAR(255) NOT NULL UNIQUE,
            name VARCHAR(255) NOT NULL,
            login_count INTEGER NOT NULL DEFAULT 0,
            created_at TIMESTAMP NOT NULL,
            updated_at TIMESTAMP NOT NULL
        )
    "#).await?;
    
    Database::execute(r#"
        CREATE TABLE settings (
            id BIGSERIAL PRIMARY KEY,
            tenant_id BIGINT NOT NULL,
            key VARCHAR(100) NOT NULL,
            value TEXT NOT NULL,
            UNIQUE(tenant_id, key)
        )
    "#).await?;
    
    println!("✓ Tables created\n");
    
    // =========================================================================
    // SIMPLE UPSERT - insert_or_update
    // =========================================================================
    
    println!("📦 Simple Upsert with insert_or_update()\n");
    
    // First insert
    let user = User {
        id: 0,
        email: "john@example.com".to_string(),
        name: "John Doe".to_string(),
        login_count: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    
    let user = User::insert_or_update(user, vec!["email"]).await?;
    println!("✓ First insert: {} - {} (login_count: {})", user.email, user.name, user.login_count);
    
    // Update on conflict (same email)
    let user_update = User {
        id: 0,  // ID doesn't matter for upsert
        email: "john@example.com".to_string(),
        name: "John Smith".to_string(),  // Changed name
        login_count: 2,  // Incremented
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    
    let user = User::insert_or_update(user_update, vec!["email"]).await?;
    println!("✓ Update on conflict: {} - {} (login_count: {})", user.email, user.name, user.login_count);
    
    println!();
    
    // =========================================================================
    // COMPOSITE KEY UPSERT
    // =========================================================================
    
    println!("🔑 Composite Key Upsert\n");
    
    // Insert setting for tenant 1
    let setting = Setting {
        id: 0,
        tenant_id: 1,
        key: "theme".to_string(),
        value: "dark".to_string(),
    };
    
    let setting = Setting::insert_or_update(setting, vec!["tenant_id", "key"]).await?;
    println!("✓ Inserted: tenant={}, key={}, value={}", setting.tenant_id, setting.key, setting.value);
    
    // Update the same setting
    let setting_update = Setting {
        id: 0,
        tenant_id: 1,
        key: "theme".to_string(),
        value: "light".to_string(),  // Changed value
    };
    
    let setting = Setting::insert_or_update(setting_update, vec!["tenant_id", "key"]).await?;
    println!("✓ Updated: tenant={}, key={}, value={}", setting.tenant_id, setting.key, setting.value);
    
    println!();
    
    // =========================================================================
    // ADVANCED UPSERT - on_conflict builder
    // =========================================================================
    
    println!("🎯 Advanced Upsert with on_conflict()\n");
    
    // Update only specific columns on conflict
    let user = User {
        id: 0,
        email: "john@example.com".to_string(),
        name: "John Doe Jr.".to_string(),
        login_count: 5,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    
    let user = User::on_conflict(vec!["email"])
        .update_columns(vec!["login_count", "updated_at"])  // Only update these columns
        .insert(user)
        .await?;
    
    println!("✓ Updated only login_count: {} (name unchanged)", user.login_count);
    
    // Update all except certain columns
    let user2 = User {
        id: 0,
        email: "jane@example.com".to_string(),
        name: "Jane Doe".to_string(),
        login_count: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    
    let _user2 = User::on_conflict(vec!["email"])
        .update_all_except(vec!["id", "email", "created_at"])  // Update everything except these
        .insert(user2)
        .await?;
    
    println!("✓ Inserted new user with update_all_except strategy");
    
    println!();
    
    // =========================================================================
    // CLEANUP
    // =========================================================================
    
    println!("🧹 Cleaning up...");
    Database::execute("DROP TABLE IF EXISTS settings CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS users CASCADE").await?;
    
    println!("\n✅ Upsert demo complete!\n");
    
    Ok(())
}
