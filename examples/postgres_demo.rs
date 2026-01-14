//! TideORM PostgreSQL Example
//!
//! **Category:** PostgreSQL Features
//!
//! This example demonstrates TideORM's features with a real PostgreSQL database.
//!
//! ## Run this example
//!
//! ```bash
//! cargo run --example postgres_demo
//! ```
//!
//! ## Features Demonstrated
//!
//! - Global configuration (languages, fallback_language)
//! - DB_SYNC mode - automatic table creation
//! - CRUD operations (Create, Read, Update, Delete)
//! - Soft deletes
//! - JSON serialization with hidden attributes
//! - Translations support (i18n)
//! - File attachments configuration
//! - Pagination
//!
//! ## Running this example
//!
//! 1. Configure database in `.env` file:
//!    ```
//!    POSTGRESQL_DATABASE_URL=postgres://postgres:postgres@localhost:5432/test_tide_orm
//!    ```
//!
//! 2. Run: DB_SYNC=true cargo run --example postgres_demo

use tideorm::prelude::*;
use std::collections::HashMap;

// ============================================================================
// USER MODEL
// ============================================================================

/// User model - demonstrates basic CRUD operations
/// 
/// Configuration attributes:
/// - hidden: password (if present) and deleted_at won't appear in JSON
/// - searchable: name and email fields for full-text search
/// 
/// Index macros (separate from #[tide]):
/// - #[index("email")] - for fast lookups
/// - #[unique_index("email")] - enforce uniqueness
#[tideorm::model]
#[tide(table = "users", hidden = "password,deleted_at", searchable = "name,email")]
#[index("email")]
#[index("status")]
#[unique_index("email")]
pub struct User {
    /// Primary key
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    
    /// User's email address
    pub email: String,
    
    /// User's display name
    pub name: String,
    
    /// User status: "active", "inactive", "banned"
    pub status: String,
    
    /// Record creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    
    /// Record last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl User {
    /// Create a new user instance (not yet saved)
    pub fn new(email: impl Into<String>, name: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: 0, // Will be set by database
            email: email.into(),
            name: name.into(),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        }
    }
    
    /// Check if user is active
    pub fn is_active(&self) -> bool {
        self.status == "active"
    }
    
    /// Deactivate the user
    pub fn deactivate(&mut self) {
        self.status = "inactive".to_string();
        self.updated_at = chrono::Utc::now();
    }
}

// ============================================================================
// POST MODEL
// ============================================================================

/// Post model - demonstrates translations, file attachments, and soft delete
///
/// Configuration attributes:
/// - soft_delete: enables soft delete support
/// - hidden: deleted_at won't appear in JSON output
/// - translatable: title and content can be translated
/// - has_one_files: single file attachment (thumbnail)
/// - has_many_files: multiple file attachments (images, documents)
///
/// Index macros (separate from #[tide]):
/// - #[index("author_id")] - FK lookup
/// - #[index(name = "...", columns = "...")] - named composite
/// - #[unique_index("slug")] - unique constraint
///
/// Note: languages and fallback_language are inherited from global TideConfig!
#[tideorm::model]
#[tide(
    table = "posts",
    soft_delete,
    hidden = "deleted_at",
    translatable = "title,content",
    has_one_files = "thumbnail",
    has_many_files = "images,documents"
)]
#[index("author_id")]
#[index(name = "idx_posts_status", columns = "status,published_at")]
#[unique_index("slug")]
pub struct Post {
    /// Primary key
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    
    /// Foreign key to users table
    pub author_id: i64,
    
    /// URL-friendly slug
    pub slug: String,
    
    /// Post status: "draft", "published", "archived"
    pub status: String,
    
    /// Number of views
    pub view_count: i32,
    
    /// Post title
    pub title: String,
    
    /// Post content
    pub content: String,
    
    /// Short excerpt
    pub excerpt: Option<String>,
    
    /// When published (null = not published)
    pub published_at: Option<chrono::DateTime<chrono::Utc>>,
    
    /// Record creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    
    /// Record last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
    
    /// Soft delete timestamp (null = not deleted)
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Post {
    /// Create a new post instance (not yet saved)
    pub fn new(author_id: i64, title: impl Into<String>, content: impl Into<String>) -> Self {
        let title = title.into();
        let now = chrono::Utc::now();
        Self {
            id: 0,
            author_id,
            slug: slugify(&title),
            status: "draft".to_string(),
            view_count: 0,
            title,
            content: content.into(),
            excerpt: None,
            published_at: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        }
    }
    
    /// Publish the post
    pub fn publish(&mut self) {
        self.status = "published".to_string();
        self.published_at = Some(chrono::Utc::now());
        self.updated_at = chrono::Utc::now();
    }
    
    /// Unpublish (revert to draft)
    pub fn unpublish(&mut self) {
        self.status = "draft".to_string();
        self.published_at = None;
        self.updated_at = chrono::Utc::now();
    }
    
    /// Archive the post
    pub fn archive(&mut self) {
        self.status = "archived".to_string();
        self.updated_at = chrono::Utc::now();
    }
    
    /// Check if published
    pub fn is_published(&self) -> bool {
        self.status == "published" && self.published_at.is_some()
    }
    
    /// Increment view count
    pub fn increment_views(&mut self) {
        self.view_count += 1;
        self.updated_at = chrono::Utc::now();
    }
    
    /// Get reading time estimate (words per minute = 200)
    pub fn reading_time_minutes(&self) -> u32 {
        let word_count = self.content.split_whitespace().count();
        ((word_count as f64) / 200.0).ceil() as u32
    }
    
    /// Soft delete the post
    pub fn soft_delete(&mut self) {
        self.deleted_at = Some(chrono::Utc::now());
        self.updated_at = chrono::Utc::now();
    }
    
    /// Restore from soft delete
    pub fn restore(&mut self) {
        self.deleted_at = None;
        self.updated_at = chrono::Utc::now();
    }
    
    /// Check if soft deleted
    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }
}

/// Simple slugify function
fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

// ============================================================================
// DATABASE SETUP (SQL for creating tables)
// ============================================================================

const CREATE_TABLES_SQL: &str = r#"
-- Drop existing tables (for clean demo runs)
DROP TABLE IF EXISTS posts CASCADE;
DROP TABLE IF EXISTS users CASCADE;

-- Users table
CREATE TABLE users (
    id BIGSERIAL PRIMARY KEY,
    email VARCHAR(255) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Posts table  
CREATE TABLE posts (
    id BIGSERIAL PRIMARY KEY,
    author_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    slug VARCHAR(255) NOT NULL UNIQUE,
    status VARCHAR(50) NOT NULL DEFAULT 'draft',
    view_count INTEGER NOT NULL DEFAULT 0,
    title VARCHAR(255) NOT NULL,
    content TEXT NOT NULL,
    excerpt TEXT,
    published_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ
);

-- Create indexes
CREATE INDEX IF NOT EXISTS idx_posts_author_id ON posts(author_id);
CREATE INDEX IF NOT EXISTS idx_posts_status ON posts(status);
CREATE INDEX IF NOT EXISTS idx_posts_slug ON posts(slug);
CREATE INDEX IF NOT EXISTS idx_posts_deleted_at ON posts(deleted_at);
"#;

// ============================================================================
// MAIN EXAMPLE
// ============================================================================

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    println!("🌊 TideORM PostgreSQL Demo\n");
    println!("==========================================\n");
    
    // ========================================================================
    // GLOBAL CONFIGURATION
    // ========================================================================
    // Configure TideORM globally - all settings in one place!
    // Database connection, pool settings, languages, hidden attributes...
    println!("⚙️  Setting up TideORM...");
    
    // Load database URL from .env file
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("POSTGRESQL_DATABASE_URL")
        .unwrap();
    
    TideConfig::init()
        // Database type (explicit for clarity)
        .database_type(DatabaseType::Postgres)
        // Database connection
        .database(&db_url)
        // Connection pool settings (production-ready defaults)
        .max_connections(20)           // Maximum concurrent connections
        .min_connections(5)            // Keep at least 5 ready
        .connect_timeout(std::time::Duration::from_secs(10))
        .idle_timeout(std::time::Duration::from_secs(300))  // 5 minutes
        .max_lifetime(std::time::Duration::from_secs(1800)) // 30 minutes
        // Schema sync (development only!)
        .sync(false)  // Set to true to auto-create tables from models
        // Application settings
        .languages(&["en", "fr", "ar", "es"])  // Supported languages for all models
        .fallback_language("en")                // Default fallback language
        .hidden_attributes(&["deleted_at"])     // Globally hidden from JSON
        .connect()
        .await?;
    
    println!(" Connected to database!");
    println!("   Languages: {:?}", Config::get_languages());
    println!("   Fallback: {}", Config::get_fallback_language());
    println!();
    
    // For this demo, we create tables manually using SQL.
    // In development, you can use .sync(true) to auto-create tables from models.
    println!("📋 Setting up database schema...");
    
    // Create tables using raw SQL (for demo purposes)
    use tideorm::internal::ConnectionTrait;
    let conn = db().__internal_connection();
    conn.execute_unprepared(CREATE_TABLES_SQL).await.map_err(|e| tideorm::Error::query(e.to_string()))?;
    println!(" Tables ready!\n");
    
    // ========================================================================
    // CREATE USERS
    // ========================================================================
    println!("👤 Creating users...");
    
    let user1 = User::new("alice@example.com", "Alice Johnson");
    let user1 = User::create(user1).await?;
    println!("   Created: {} (id: {})", user1.name, user1.id);
    
    let user2 = User::new("bob@example.com", "Bob Smith");
    let user2 = User::create(user2).await?;
    println!("   Created: {} (id: {})", user2.name, user2.id);
    
    let user3 = User::new("charlie@example.com", "Charlie Brown");
    let user3 = User::create(user3).await?;
    println!("   Created: {} (id: {})\n", user3.name, user3.id);
    
    // ========================================================================
    // QUERY USERS
    // ========================================================================
    println!("🔍 Querying users...");
    
    // Get all users
    let all_users = User::all().await?;
    println!("   Total users: {}", all_users.len());
    
    // Count users
    let count = User::count().await?;
    println!("   User count: {}", count);
    
    // Find by ID
    let found_user = User::find(user1.id).await?;
    println!("   Found user by ID {}: {:?}", user1.id, found_user.map(|u| u.name));
    
    // Find or fail
    let user = User::find_or_fail(user2.id).await?;
    println!("   Find or fail: {} ({})", user.name, user.email);
    
    // Check existence
    let exists = User::exists(user3.id).await?;
    println!("   User {} exists: {}", user3.id, exists);
    
    // First and last
    if let Some(first) = User::first().await? {
        println!("   First user: {}", first.name);
    }
    
    println!();
    
    // ========================================================================
    // CREATE POSTS
    // ========================================================================
    println!("📝 Creating posts...");
    
    let mut post1 = Post::new(user1.id, "Getting Started with Rust", "Rust is a systems programming language...");
    post1.excerpt = Some("An introduction to Rust programming".to_string());
    post1.publish();
    let post1 = Post::create(post1).await?;
    println!("   Created: '{}' by user {} (status: {})", post1.title, post1.author_id, post1.status);
    
    let mut post2 = Post::new(user1.id, "Advanced Rust Patterns", "In this post we explore advanced patterns...");
    post2.publish();
    let post2 = Post::create(post2).await?;
    println!("   Created: '{}' (status: {})", post2.title, post2.status);
    
    let post3 = Post::new(user2.id, "Draft Post", "This is a draft that hasn't been published yet.");
    let post3 = Post::create(post3).await?;
    println!("   Created: '{}' (status: {})", post3.title, post3.status);
    
    let mut post4 = Post::new(user2.id, "Archived Content", "Old content that's been archived.");
    post4.archive();
    let post4 = Post::create(post4).await?;
    println!("   Created: '{}' (status: {})\n", post4.title, post4.status);
    
    // ========================================================================
    // QUERY POSTS
    // ========================================================================
    println!("🔍 Querying posts...");
    
    let all_posts = Post::all().await?;
    println!("   Total posts: {}", all_posts.len());
    
    // First post
    if let Some(first_post) = Post::first().await? {
        println!("   First post: '{}'", first_post.title);
        println!("   Reading time: {} min", first_post.reading_time_minutes());
    }
    
    println!();
    
    // ========================================================================
    // UPDATE RECORDS
    // ========================================================================
    println!("✏️  Updating records...");
    
    // Update a post's view count
    let mut post1 = Post::find_or_fail(post1.id).await?;
    post1.increment_views();
    post1.increment_views();
    post1.increment_views();
    let post1 = post1.update().await?;
    println!("   Post '{}' view count: {}", post1.title, post1.view_count);
    
    // Update user status
    let mut user3 = User::find_or_fail(user3.id).await?;
    user3.deactivate();
    let user3 = user3.update().await?;
    println!("   User '{}' status: {}", user3.name, user3.status);
    
    println!();
    
    // ========================================================================
    // SOFT DELETE
    // ========================================================================
    println!("🗑️  Soft delete demo...");
    
    // Soft delete a post
    let mut post4 = Post::find_or_fail(post4.id).await?;
    println!("   Before soft delete: is_deleted = {}", post4.is_deleted());
    post4.soft_delete();
    let post4 = post4.update().await?;
    println!("   After soft delete: is_deleted = {}", post4.is_deleted());
    
    // Restore the post
    let mut post4 = Post::find_or_fail(post4.id).await?;
    post4.restore();
    let post4 = post4.update().await?;
    println!("   After restore: is_deleted = {}", post4.is_deleted());
    
    println!();
    
    // ========================================================================
    // JSON SERIALIZATION
    // ========================================================================
    println!("📄 JSON serialization...");
    
    let user = User::find_or_fail(user1.id).await?;
    let json = user.to_json(None);
    println!("   User as JSON: {}", serde_json::to_string_pretty(&json).unwrap());
    
    // JSON with language option (for translated models)
    let mut opts = HashMap::new();
    opts.insert("language".to_string(), "fr".to_string());
    let post = Post::find_or_fail(post1.id).await?;
    let post_json = post.to_json(Some(opts));
    println!("   Post as JSON (with language=fr option): {}", 
        serde_json::to_string(&post_json).unwrap().chars().take(100).collect::<String>() + "...");
    
    // Collection to JSON
    let users = User::all().await?;
    let users_json = User::collection_to_json(users, None);
    println!("   Users collection has {} items", users_json.as_array().map(|a| a.len()).unwrap_or(0));
    
    // to_hash_map (legacy support)
    let user = User::find_or_fail(user1.id).await?;
    let hash_map = user.to_hash_map();
    println!("   User as HashMap: {} keys", hash_map.len());
    
    println!();
    
    // ========================================================================
    // MODEL CONFIGURATION DEMO
    // ========================================================================
    println!("⚙️  Model configuration demo...");
    
    // Show hidden attributes
    println!("   User hidden attributes: {:?}", User::hidden_attributes());
    println!("   Post hidden attributes: {:?}", Post::hidden_attributes());
    
    // Show searchable fields
    println!("   User searchable fields: {:?}", User::searchable_fields());
    
    // Show translation config - Note: These come from GLOBAL TideConfig!
    println!("   Post translatable fields: {:?}", Post::translatable_fields());
    println!("   Post allowed languages (from global config): {:?}", Post::allowed_languages());
    println!("   Post fallback language (from global config): {:?}", Post::fallback_language());
    
    // Show file attachment config
    println!("   Post hasOne files: {:?}", Post::has_one_attached_file());
    println!("   Post hasMany files: {:?}", Post::has_many_attached_files());
    println!("   Post all file relations: {:?}", Post::files_relations());
    
    // Soft delete enabled
    println!("   Post soft_delete enabled: {}", Post::soft_delete_enabled());
    
    println!();
    
    // ========================================================================
    // DELETE RECORDS
    // ========================================================================
    println!("❌ Deleting records...");
    
    // Delete by ID
    let affected = Post::destroy(post3.id).await?;
    println!("   Deleted post {} (rows affected: {})", post3.id, affected);
    
    // Delete instance
    let post4 = Post::find_or_fail(post4.id).await?;
    let affected = post4.delete().await?;
    println!("   Deleted post instance (rows affected: {})", affected);
    
    // Final count
    let final_count = Post::count().await?;
    println!("   Remaining posts: {}", final_count);
    
    println!();
    
    // ========================================================================
    // RELOAD DEMO
    // ========================================================================
    println!("🔄 Reload demo...");
    
    let post = Post::find_or_fail(post1.id).await?;
    println!("   Post title before reload: '{}'", post.title);
    let reloaded = post.reload().await?;
    println!("   Post title after reload: '{}'", reloaded.title);
    // ========================================================================
    // PAGINATION
    // ========================================================================
    println!("📖 Pagination demo...");
    
    // Create more posts for pagination demo
    for i in 1..=5 {
        let post = Post::new(user1.id, format!("Pagination Test Post {}", i), "Content for pagination test");
        Post::create(post).await?;
    }
    
    let page1 = Post::paginate(1, 3).await?;
    println!("   Page 1 (3 per page): {} posts", page1.len());
    
    let page2 = Post::paginate(2, 3).await?;
    println!("   Page 2 (3 per page): {} posts", page2.len());
    
    println!();
    
    // ========================================================================
    // SUMMARY
    // ========================================================================
    println!("==========================================");
    println!("✨ Demo completed successfully!");
    println!();
    println!("📊 Final Summary:");
    println!("   - Users: {}", User::count().await?);
    println!("   - Posts: {}", Post::count().await?);
    println!();
    println!("🌊 TideORM Features Demonstrated:");
    println!("    Global config (TideConfig) - languages, fallback set ONCE");
    println!("    DB_SYNC mode");
    println!("    CRUD operations (Create, Read, Update, Delete)");
    println!("    find, find_or_fail, exists, first, last");
    println!("    Pagination (paginate)");
    println!("    Soft deletes (soft_delete attribute)");
    println!("    JSON serialization with options");
    println!("    Hidden attributes (hidden = \"...\")");
    println!("    Translations config (translatable - uses global languages)");
    println!("    File attachments config (has_one_files, has_many_files)");
    println!("    Searchable fields (searchable = \"...\")");
    println!();
    println!("🌊 TideORM - SeaORM completely hidden!");
    println!("   Notice: No SeaORM imports required in user code!");
    
    Ok(())
}
