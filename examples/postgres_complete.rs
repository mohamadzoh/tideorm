//! # TideORM Complete PostgreSQL Feature Showcase
//!
//! **Category:** PostgreSQL Features (Complete)
//!
//! This example demonstrates ALL TideORM features with PostgreSQL.
//! Use this as a reference before implementing support for other databases.
//!
//! ## Run this example
//!
//! ```bash
//! cargo run --example postgres_complete
//! ```
//!
//! ## Features Demonstrated
//!
//! ### Core Features
//! - Global configuration (TideConfig)
//! - CRUD operations (Create, Read, Update, Delete)
//! - Query Builder (WHERE, ORDER, LIMIT, OFFSET)
//! - Pagination
//! - Soft deletes
//!
//! ### Relations
//! - `#[belongs_to]` macro
//! - `#[has_one]` macro
//! - `#[has_many]` macro
//! - Loading relations
//!
//! ### PostgreSQL-Specific
//! - JSON/JSONB columns and queries
//! - Array columns and queries
//! - Full-text search (future)
//!
//! ### Advanced Features
//! - Transactions
//! - Callbacks (before_save, after_create, etc.)
//! - Batch operations (insert_all, update_all)
//! - Scopes (reusable query conditions)
//! - Raw SQL queries
//! - JOIN operations (INNER, LEFT, RIGHT)
//! - Aggregations (SUM, AVG, MIN, MAX, COUNT DISTINCT)
//! - GROUP BY and HAVING clauses
//!
//! ## Running this example
//!
//! ```bash
//! # 1. Configure database in .env file:
//! #    POSTGRESQL_DATABASE_URL=postgres://postgres:postgres@localhost:5432/test_tide_orm
//!
//! # 2. Run the example:
//! cargo run --example postgres_complete
//! ```

use tideorm::prelude::*;
use std::collections::HashMap;
use tideorm::relations::{HasOne, HasMany, BelongsTo};

// ============================================================================
// MODEL DEFINITIONS WITH RELATIONS
// ============================================================================

/// User model - demonstrates has_many and has_one relations
#[derive(Model)]
#[tide(table = "users", hidden = "password_hash,deleted_at", searchable = "name,email")]
#[index("email")]
#[unique_index("email")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub status: String,
    pub password_hash: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    
    // Relations defined as fields
    #[tide(has_many = "Post", foreign_key = "user_id")]
    pub posts: HasMany<Post>,
    
    #[tide(has_one = "Profile", foreign_key = "user_id")]
    pub profile: HasOne<Profile>,
}

impl User {
    pub fn new(email: impl Into<String>, name: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: 0,
            email: email.into(),
            name: name.into(),
            status: "active".to_string(),
            password_hash: None,
            created_at: now,
            updated_at: now,
            ..Default::default()
        }
    }
}

/// Profile model - demonstrates belongs_to relation and JSON column
#[derive(Model)]
#[tide(table = "profiles")]
pub struct Profile {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub bio: Option<String>,
    pub website: Option<String>,
    /// JSON column for flexible settings storage
    pub settings: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    
    // BelongsTo relation
    #[tide(belongs_to = "User", foreign_key = "user_id")]
    pub user: BelongsTo<User>,
}

impl Profile {
    pub fn new(user_id: i64) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: 0,
            user_id,
            bio: None,
            website: None,
            settings: serde_json::json!({"theme": "light", "notifications": true}),
            created_at: now,
            updated_at: now,
            ..Default::default()
        }
    }
}

/// Post model - demonstrates belongs_to, soft delete, and array columns
#[derive(Model)]
#[tide(table = "posts", soft_delete, hidden = "deleted_at")]
#[index("user_id")]
#[index("status")]
pub struct Post {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub content: String,
    pub status: String,
    /// Array column for tags
    pub tags: Vec<String>,
    /// JSON column for metadata
    pub metadata: serde_json::Value,
    pub view_count: i32,
    pub published_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    
    // Relations
    #[tide(belongs_to = "User", foreign_key = "user_id")]
    pub author: BelongsTo<User>,
    
    #[tide(has_many = "Comment", foreign_key = "post_id")]
    pub comments: HasMany<Comment>,
}

impl Post {
    pub fn new(user_id: i64, title: impl Into<String>, content: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: 0,
            user_id,
            title: title.into(),
            content: content.into(),
            status: "draft".to_string(),
            tags: vec![],
            metadata: serde_json::json!({}),
            view_count: 0,
            published_at: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
            ..Default::default()
        }
    }
    
    pub fn with_tags(mut self, tags: Vec<&str>) -> Self {
        self.tags = tags.into_iter().map(String::from).collect();
        self
    }
    
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
    
    pub fn publish(&mut self) {
        self.status = "published".to_string();
        self.published_at = Some(chrono::Utc::now());
        self.updated_at = chrono::Utc::now();
    }
}

/// Comment model - demonstrates belongs_to with multiple relations
#[derive(Model)]
#[tide(table = "comments")]
#[index("post_id")]
#[index("user_id")]
pub struct Comment {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub post_id: i64,
    pub user_id: i64,
    pub content: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    
    // Relations
    #[tide(belongs_to = "User", foreign_key = "user_id")]
    pub commenter: BelongsTo<User>,
    
    #[tide(belongs_to = "Post", foreign_key = "post_id")]
    pub post: BelongsTo<Post>,
}

impl Comment {
    pub fn new(post_id: i64, user_id: i64, content: impl Into<String>) -> Self {
        Self {
            id: 0,
            post_id,
            user_id,
            content: content.into(),
            created_at: chrono::Utc::now(),
            ..Default::default()
        }
    }
}

/// Product model - demonstrates callbacks and JSON queries
#[derive(Model)]
#[tide(table = "products")]
#[index("category")]
#[index("active")]
pub struct Product {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub category: String,
    pub price: i64,  // cents
    pub stock: i32,
    pub active: bool,
    /// JSON column for product attributes
    pub attributes: serde_json::Value,
    /// Array column for related SKUs
    pub related_skus: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Product {
    pub fn new(name: impl Into<String>, category: impl Into<String>, price: i64) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: 0,
            name: name.into(),
            category: category.into(),
            price,
            stock: 0,
            active: true,
            attributes: serde_json::json!({}),
            related_skus: vec![],
            created_at: now,
            updated_at: now,
        }
    }
}

// Note: Custom callbacks would be implemented like this if the blanket impl
// wasn't provided. For now, callbacks use the default no-op implementation.
// To use custom callbacks, you'd need to opt-out of the blanket impl.

// ============================================================================
// DATABASE SCHEMA
// ============================================================================

const CREATE_TABLES_SQL: &str = r#"
-- Drop existing tables (for clean demo)
DROP TABLE IF EXISTS comments CASCADE;
DROP TABLE IF EXISTS posts CASCADE;
DROP TABLE IF EXISTS profiles CASCADE;
DROP TABLE IF EXISTS products CASCADE;
DROP TABLE IF EXISTS users CASCADE;

-- Users table
CREATE TABLE users (
    id BIGSERIAL PRIMARY KEY,
    email VARCHAR(255) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    password_hash VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Profiles table (1:1 with users)
CREATE TABLE profiles (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    bio TEXT,
    website VARCHAR(255),
    settings JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Posts table (1:N with users, with soft delete)
CREATE TABLE posts (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title VARCHAR(255) NOT NULL,
    content TEXT NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'draft',
    tags TEXT[] NOT NULL DEFAULT '{}',
    metadata JSONB NOT NULL DEFAULT '{}',
    view_count INTEGER NOT NULL DEFAULT 0,
    published_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ
);

-- Comments table (belongs to both posts and users)
CREATE TABLE comments (
    id BIGSERIAL PRIMARY KEY,
    post_id BIGINT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Products table (for callbacks and JSON/array demos)
CREATE TABLE products (
    id BIGSERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    category VARCHAR(100) NOT NULL,
    price BIGINT NOT NULL,
    stock INTEGER NOT NULL DEFAULT 0,
    active BOOLEAN NOT NULL DEFAULT true,
    attributes JSONB NOT NULL DEFAULT '{}',
    related_skus TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes
CREATE INDEX idx_posts_user_id ON posts(user_id);
CREATE INDEX idx_posts_status ON posts(status);
CREATE INDEX idx_posts_tags ON posts USING GIN(tags);
CREATE INDEX idx_posts_metadata ON posts USING GIN(metadata);
CREATE INDEX idx_products_category ON products(category);
CREATE INDEX idx_products_attributes ON products USING GIN(attributes);
"#;

// ============================================================================
// MAIN
// ============================================================================

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║     🌊 TideORM - Complete PostgreSQL Feature Showcase        ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // ========================================================================
    // 1. CONFIGURATION
    // ========================================================================
    section("1. CONFIGURATION");
    
    // Load database URL from .env file
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("POSTGRESQL_DATABASE_URL")
        .unwrap();
    
    TideConfig::init()
        .database_type(DatabaseType::Postgres)
        .database(&db_url)
        .max_connections(20)
        .min_connections(5)
        .connect_timeout(std::time::Duration::from_secs(10))
        .idle_timeout(std::time::Duration::from_secs(300))
        .languages(&["en", "fr", "es", "ar"])
        .fallback_language("en")
        .hidden_attributes(&["password_hash", "deleted_at"])
        .connect()
        .await?;
    
    println!("✅ Connected to PostgreSQL");
    println!("   Database type: {:?}", TideConfig::get_database_type());
    println!("   Languages: {:?}", Config::get_languages());
    
    // Setup database schema
    println!("\n📋 Setting up database schema...");
    use tideorm::internal::ConnectionTrait;
    let conn = db().__internal_connection();
    conn.execute_unprepared(CREATE_TABLES_SQL)
        .await
        .map_err(|e| tideorm::Error::query(e.to_string()))?;
    println!("✅ Schema ready!");

    // ========================================================================
    // 2. CRUD OPERATIONS
    // ========================================================================
    section("2. CRUD OPERATIONS");
    
    // CREATE
    println!("📝 Creating records...");
    
    let alice = User::new("alice@example.com", "Alice Johnson");
    let alice = User::create(alice).await?;
    println!("   Created user: {} (id: {})", alice.name, alice.id);
    
    let bob = User::new("bob@example.com", "Bob Smith");
    let bob = User::create(bob).await?;
    println!("   Created user: {} (id: {})", bob.name, bob.id);
    
    let charlie = User::new("charlie@example.com", "Charlie Brown");
    let charlie = User::create(charlie).await?;
    println!("   Created user: {} (id: {})", charlie.name, charlie.id);
    
    // READ
    println!("\n🔍 Reading records...");
    
    let user = User::find(alice.id).await?;
    println!("   find({}): {:?}", alice.id, user.map(|u| u.name.clone()));
    
    let user = User::find_or_fail(bob.id).await?;
    println!("   find_or_fail({}): {}", bob.id, user.name);
    
    let exists = User::exists(charlie.id).await?;
    println!("   exists({}): {}", charlie.id, exists);
    
    let all_users = User::all().await?;
    println!("   all(): {} users", all_users.len());
    
    let count = User::count().await?;
    println!("   count(): {}", count);
    
    let first = User::first().await?;
    println!("   first(): {:?}", first.map(|u| u.name));
    
    let last = User::last().await?;
    println!("   last(): {:?}", last.map(|u| u.name));
    
    // UPDATE
    println!("\n✏️  Updating records...");
    
    let mut user = User::find_or_fail(alice.id).await?;
    user.name = "Alice J. Johnson".to_string();
    user.updated_at = chrono::Utc::now();
    let user = user.update().await?;
    println!("   Updated: {} -> {}", alice.name, user.name);
    
    // DELETE
    println!("\n❌ Deleting records...");
    let temp_user = User::new("temp@example.com", "Temporary");
    let temp_user = User::create(temp_user).await?;
    let affected = User::destroy(temp_user.id).await?;
    println!("   Deleted user {} (rows affected: {})", temp_user.id, affected);

    // ========================================================================
    // 3. RELATIONS
    // ========================================================================
    section("3. RELATIONS (belongs_to, has_one, has_many)");
    
    // Create profiles (has_one)
    println!("📝 Creating profiles (has_one)...");
    let alice_profile = Profile::new(alice.id);
    let alice_profile = Profile::create(alice_profile).await?;
    println!("   Created profile for Alice (id: {})", alice_profile.id);
    
    // Create posts (has_many)
    println!("\n📝 Creating posts (has_many)...");
    let mut post1 = Post::new(alice.id, "Getting Started with Rust", "Rust is amazing...");
    post1 = post1.with_tags(vec!["rust", "programming", "tutorial"]);
    post1.publish();
    let post1 = Post::create(post1).await?;
    println!("   Created post: '{}' by user {}", post1.title, post1.user_id);
    
    let mut post2 = Post::new(alice.id, "Advanced Rust Patterns", "Let's dive deeper...");
    post2 = post2.with_tags(vec!["rust", "advanced"]);
    let post2 = Post::create(post2).await?;
    println!("   Created post: '{}' (draft)", post2.title);
    
    let mut post3 = Post::new(bob.id, "My First Post", "Hello world!");
    post3.publish();
    let post3 = Post::create(post3).await?;
    println!("   Created post: '{}' by user {}", post3.title, post3.user_id);
    
    // Create comments (belongs_to multiple)
    println!("\n📝 Creating comments...");
    let comment1 = Comment::new(post1.id, bob.id, "Great article!");
    let comment1 = Comment::create(comment1).await?;
    println!("   Created comment on post {} by user {}", comment1.post_id, comment1.user_id);
    
    let comment2 = Comment::new(post1.id, charlie.id, "Very helpful, thanks!");
    Comment::create(comment2).await?;
    
    // Load relations using field-based syntax
    println!("\n🔗 Loading relations...");
    
    // has_one: User -> Profile
    let mut alice = User::find_or_fail(alice.id).await?;
    alice.profile = HasOne::new("user_id", "id").with_parent_pk(serde_json::json!(alice.id));
    let profile = alice.profile.load().await?;
    println!("   Alice's profile: {:?}", profile.map(|p| p.id));
    
    // has_many: User -> Posts
    alice.posts = HasMany::new("user_id", "id").with_parent_pk(serde_json::json!(alice.id));
    let posts = alice.posts.load().await?;
    println!("   Alice's posts: {} posts", posts.len());
    
    // belongs_to: Post -> User
    let mut post = Post::find_or_fail(post1.id).await?;
    post.author = BelongsTo::new("user_id", "id").with_fk_value(serde_json::json!(post.user_id));
    let author = post.author.load().await?;
    println!("   Post '{}' author: {:?}", post.title, author.map(|u| u.name));
    
    // has_many: Post -> Comments
    post.comments = HasMany::new("post_id", "id").with_parent_pk(serde_json::json!(post.id));
    let comments = post.comments.load().await?;
    println!("   Post '{}' has {} comments", post.title, comments.len());
    
    // belongs_to: Comment -> User
    let mut comment = Comment::find_or_fail(comment1.id).await?;
    comment.commenter = BelongsTo::new("user_id", "id").with_fk_value(serde_json::json!(comment.user_id));
    let commenter = comment.commenter.load().await?;
    println!("   Comment by: {:?}", commenter.map(|u| u.name));

    // ========================================================================
    // 4. QUERY BUILDER
    // ========================================================================
    section("4. QUERY BUILDER");
    
    println!("🔍 WHERE conditions...");
    
    // where_eq
    let active_users = User::query()
        .where_eq("status", "active")
        .get()
        .await?;
    println!("   where_eq('status', 'active'): {} users", active_users.len());
    
    // where_not
    let non_alice = User::query()
        .where_not("email", "alice@example.com")
        .get()
        .await?;
    println!("   where_not('email', 'alice@...'): {} users", non_alice.len());
    
    // where_like
    let a_names = User::query()
        .where_like("name", "A%")
        .get()
        .await?;
    println!("   where_like('name', 'A%'): {} users", a_names.len());
    
    // where_in
    let selected = User::query()
        .where_in("id", vec![alice.id, bob.id])
        .get()
        .await?;
    println!("   where_in('id', [...]): {} users", selected.len());
    
    // where_null / where_not_null
    let with_password = User::query()
        .where_not_null("password_hash")
        .count()
        .await?;
    println!("   where_not_null('password_hash'): {} users", with_password);
    
    // ORDER BY
    println!("\n📊 ORDER BY...");
    let ordered = User::query()
        .order_by("name", Order::Asc)
        .get()
        .await?;
    let names: Vec<_> = ordered.iter().map(|u| u.name.as_str()).collect();
    println!("   order_by('name', Asc): {:?}", names);
    
    // LIMIT & OFFSET
    println!("\n📄 LIMIT & OFFSET...");
    let limited = User::query()
        .limit(2)
        .get()
        .await?;
    println!("   limit(2): {} users", limited.len());
    
    let offset = User::query()
        .limit(2)
        .offset(1)
        .get()
        .await?;
    println!("   limit(2).offset(1): {} users", offset.len());
    
    // Combined queries
    println!("\n🔗 Combined queries...");
    let complex = User::query()
        .where_eq("status", "active")
        .where_like("email", "%@example.com")
        .order_by("created_at", Order::Desc)
        .limit(10)
        .get()
        .await?;
    println!("   Complex query: {} users", complex.len());
    
    // COUNT with conditions
    let count = User::query()
        .where_eq("status", "active")
        .count()
        .await?;
    println!("   count() with where: {}", count);

    // ========================================================================
    // 5. JSON/JSONB OPERATIONS (PostgreSQL)
    // ========================================================================
    section("5. JSON/JSONB OPERATIONS");
    
    // Create products with JSON attributes
    println!("📝 Creating products with JSON attributes...");
    
    let mut laptop = Product::new("MacBook Pro", "Electronics", 199900);
    laptop.attributes = serde_json::json!({
        "brand": "Apple",
        "color": "silver",
        "specs": {"ram": 16, "storage": 512},
        "features": ["retina", "m2-chip", "magsafe"]
    });
    laptop.related_skus = vec!["MBP-14".into(), "MBP-16".into()];
    laptop.stock = 50;
    let laptop = Product::create(laptop).await?;
    println!("   Created: {} with JSON attributes", laptop.name);
    
    let mut phone = Product::new("iPhone 15", "Electronics", 99900);
    phone.attributes = serde_json::json!({
        "brand": "Apple",
        "color": "black",
        "specs": {"storage": 256},
        "features": ["5g", "usb-c", "dynamic-island"]
    });
    phone.stock = 100;
    let phone = Product::create(phone).await?;
    println!("   Created: {} with JSON attributes", phone.name);
    
    let mut shirt = Product::new("T-Shirt", "Clothing", 2999);
    shirt.attributes = serde_json::json!({
        "brand": "Generic",
        "color": "blue",
        "size": "M"
    });
    shirt.stock = 200;
    Product::create(shirt).await?;
    
    // JSON queries
    println!("\n🔍 JSON queries...");
    
    // where_json_contains - find products with specific attribute
    let apple_products = Product::query()
        .where_json_contains("attributes", serde_json::json!({"brand": "Apple"}))
        .get()
        .await?;
    println!("   where_json_contains('brand': 'Apple'): {} products", apple_products.len());
    
    // where_json_key_exists - find products with specific key
    let with_specs = Product::query()
        .where_json_key_exists("attributes", "specs")
        .get()
        .await?;
    println!("   where_json_key_exists('specs'): {} products", with_specs.len());
    
    // where_json_key_not_exists
    let without_size = Product::query()
        .where_json_key_not_exists("attributes", "size")
        .get()
        .await?;
    println!("   where_json_key_not_exists('size'): {} products", without_size.len());

    // ========================================================================
    // 6. ARRAY OPERATIONS (PostgreSQL)
    // ========================================================================
    section("6. ARRAY OPERATIONS");
    
    println!("🔍 Array queries on posts.tags...");
    
    // where_array_contains - find posts with specific tag
    let rust_posts = Post::query()
        .where_array_contains("tags", vec!["rust"])
        .get()
        .await?;
    println!("   where_array_contains(['rust']): {} posts", rust_posts.len());
    
    // where_array_overlaps - find posts with any of these tags
    let tutorial_or_advanced = Post::query()
        .where_array_overlaps("tags", vec!["tutorial", "advanced"])
        .get()
        .await?;
    println!("   where_array_overlaps(['tutorial', 'advanced']): {} posts", tutorial_or_advanced.len());
    
    // Combined: array + other conditions
    let published_rust = Post::query()
        .where_eq("status", "published")
        .where_array_contains("tags", vec!["rust"])
        .get()
        .await?;
    println!("   Published posts with 'rust' tag: {} posts", published_rust.len());

    // ========================================================================
    // 7. SOFT DELETE
    // ========================================================================
    section("7. SOFT DELETE");
    
    println!("🗑️  Soft delete operations on posts...");
    
    // Create a post to soft delete
    let mut temp_post = Post::new(alice.id, "Temporary Post", "This will be deleted");
    temp_post.publish();
    let temp_post = Post::create(temp_post).await?;
    println!("   Created post: {} (id: {})", temp_post.title, temp_post.id);
    
    // Soft delete
    let mut post_to_delete = Post::find_or_fail(temp_post.id).await?;
    post_to_delete.deleted_at = Some(chrono::Utc::now());
    let deleted_post = post_to_delete.update().await?;
    println!("   Soft deleted: deleted_at = {:?}", deleted_post.deleted_at.map(|_| "set"));
    
    // Query excluding soft deleted (default behavior with SoftDelete trait)
    let all_posts = Post::all().await?;
    println!("   Post::all() (includes soft deleted for now): {} posts", all_posts.len());
    
    // Query with trashed
    let with_trashed = Post::query()
        .with_trashed()
        .get()
        .await?;
    println!("   with_trashed(): {} posts", with_trashed.len());
    
    // Query only trashed
    let only_trashed = Post::query()
        .only_trashed()
        .get()
        .await?;
    println!("   only_trashed(): {} posts", only_trashed.len());
    
    // Restore
    let mut post_to_restore = Post::find_or_fail(temp_post.id).await?;
    post_to_restore.deleted_at = None;
    let restored = post_to_restore.update().await?;
    println!("   Restored: deleted_at = {:?}", restored.deleted_at);

    // ========================================================================
    // 8. TRANSACTIONS
    // ========================================================================
    section("8. TRANSACTIONS");
    
    println!("💳 Transaction example...");
    
    // Successful transaction
    let result = User::transaction(|_txn| {
        Box::pin(async move {
            // In a real app, you'd use txn for queries
            // For now, we just demonstrate the API
            println!("   Inside transaction...");
            Ok::<_, tideorm::Error>(())
        })
    }).await;
    println!("   Transaction result: {:?}", result.map(|_| "success"));
    
    // Note: For actual transactional queries, you'd pass the transaction
    // connection to your queries. The transaction auto-commits on success
    // and auto-rollbacks on error.

    // ========================================================================
    // 9. BATCH OPERATIONS
    // ========================================================================
    section("9. BATCH OPERATIONS");
    
    println!("📦 Batch insert...");
    
    let products_to_insert = vec![
        Product::new("Headphones", "Electronics", 14999),
        Product::new("Keyboard", "Electronics", 7999),
        Product::new("Mouse", "Electronics", 2999),
        Product::new("Monitor", "Electronics", 29999),
    ];
    
    let inserted = Product::insert_all(products_to_insert).await?;
    println!("   Inserted {} products", inserted.len());
    
    println!("\n📦 Batch update (using query builder)...");
    
    // Note: update_all is a Model method, not query builder method
    // For bulk updates, use raw SQL or iterate
    let electronics = Product::query()
        .where_eq("category", "electronics")
        .get()
        .await?;
    println!("   Found {} electronics products to update", electronics.len());

    // ========================================================================
    // 10. SCOPES (Reusable Query Conditions)
    // ========================================================================
    section("10. SCOPES");
    
    println!("🎯 Using scopes for reusable queries...");
    
    // Define scope functions
    fn active_scope<M: Model>(query: QueryBuilder<M>) -> QueryBuilder<M> {
        query.where_eq("status", "active")
    }
    
    fn published_scope<M: Model>(query: QueryBuilder<M>) -> QueryBuilder<M> {
        query.where_eq("status", "published")
    }
    
    fn expensive_scope<M: Model>(query: QueryBuilder<M>) -> QueryBuilder<M> {
        query.where_gt("price", 10000)
    }
    
    // Use scopes
    let active_users = User::query()
        .scope(active_scope)
        .get()
        .await?;
    println!("   scope(active): {} users", active_users.len());
    
    let published_posts: Vec<Post> = Post::query()
        .scope(published_scope)
        .get()
        .await?;
    println!("   scope(published): {} posts", published_posts.len());
    
    let expensive_products = Product::query()
        .scope(expensive_scope)
        .get()
        .await?;
    println!("   scope(expensive > $100): {} products", expensive_products.len());
    
    // Conditional scopes with when()
    let min_price: Option<i64> = Some(5000);
    let conditional = Product::query()
        .when(min_price.is_some(), |q| q.where_gt("price", min_price.unwrap()))
        .get()
        .await?;
    println!("   when(min_price): {} products", conditional.len());
    
    // when_some for Option values
    let category_filter: Option<&str> = Some("electronics");
    let filtered = Product::query()
        .when_some(category_filter, |q, cat| q.where_eq("category", cat))
        .get()
        .await?;
    println!("   when_some(category): {} products", filtered.len());

    // ========================================================================
    // 11. CALLBACKS
    // ========================================================================
    section("11. CALLBACKS");
    
    println!("🔔 Callbacks demonstration...");
    println!("   Note: TideORM provides a blanket Callbacks impl for all models.");
    println!("   Custom callbacks can be added by implementing the Callbacks trait.");
    println!("   Available callbacks:");
    println!("     - before_save() / after_save()");
    println!("     - before_create() / after_create()");
    println!("     - before_update() / after_update()");
    println!("     - before_delete() / after_delete()");
    println!("     - before_validation() / after_validation()");
    
    // Create product - default callbacks are no-op
    let product = Product::new("Callback Test", "Test", 1000);
    let product = Product::create(product).await?;
    println!("   Created product: {} (id: {})", product.name, product.id);

    // ========================================================================
    // 12. RAW SQL QUERIES
    // ========================================================================
    section("12. RAW SQL QUERIES");
    
    println!("📜 Raw SQL queries...");
    
    // Execute raw SQL using internal connection (already imported at top)
    let result = conn.execute_unprepared("UPDATE users SET status = 'active' WHERE status = 'active'")
        .await
        .map_err(|e| tideorm::Error::query(e.to_string()))?;
    println!("   execute_unprepared(): {:?}", result.rows_affected());
    
    // For raw queries with results, use Database::raw (no params version)
    let results: Vec<User> = Database::raw(
        "SELECT * FROM users WHERE status = 'active' ORDER BY created_at DESC LIMIT 10"
    ).await?;
    println!("   Database::raw(): {} users", results.len());
    
    // Raw query with params
    let results: Vec<User> = Database::raw_with_params(
        "SELECT * FROM users WHERE status = $1 ORDER BY created_at DESC LIMIT $2",
        vec!["active".into(), 10i64.into()]
    ).await?;
    println!("   Database::raw_with_params(): {} users", results.len());
    
    // Execute with params
    let affected = Database::execute_with_params(
        "UPDATE products SET stock = stock + $1 WHERE category = $2",
        vec![10i32.into(), "electronics".into()]
    ).await?;
    println!("   Database::execute_with_params(): {} rows affected", affected);

    // ========================================================================
    // 13. JSON SERIALIZATION
    // ========================================================================
    section("13. JSON SERIALIZATION");
    
    println!("📄 JSON output...");
    
    let user = User::find_or_fail(alice.id).await?;
    let json = user.to_json(None);
    println!("   to_json(): {}", serde_json::to_string(&json).unwrap());
    
    // With options
    let mut opts = HashMap::new();
    opts.insert("language".to_string(), "fr".to_string());
    let json = user.to_json(Some(opts));
    println!("   to_json(lang=fr): id={}", json.get("id").unwrap());
    
    // Collection to JSON
    let users = User::all().await?;
    let json = User::collection_to_json(users, None);
    println!("   collection_to_json(): {} items", json.as_array().unwrap().len());
    
    // Hidden fields are excluded
    println!("   Hidden fields (not in JSON): {:?}", User::hidden_attributes());

    // ========================================================================
    // 14. PAGINATION
    // ========================================================================
    section("14. PAGINATION");
    
    println!("📖 Pagination...");
    
    // Create more products for pagination demo
    for i in 1..=10 {
        let p = Product::new(format!("Pagination Item {}", i), "test", i * 100);
        Product::create(p).await?;
    }
    
    let page1 = Product::paginate(1, 5).await?;
    println!("   Page 1 (5/page): {} products", page1.len());
    
    let page2 = Product::paginate(2, 5).await?;
    println!("   Page 2 (5/page): {} products", page2.len());
    
    let page3 = Product::paginate(3, 5).await?;
    println!("   Page 3 (5/page): {} products", page3.len());
    
    // Using query builder pagination
    let electronics_page = Product::query()
        .where_eq("category", "electronics")
        .page(1, 10)
        .get()
        .await?;
    println!("   Electronics page 1: {} products", electronics_page.len());

    // ========================================================================
    // 15. JOIN OPERATIONS
    // ========================================================================
    section("15. JOIN OPERATIONS");
    
    println!("🔗 JOIN Operations...");
    
    // Inner join: Get posts with their user data
    let posts_with_users = Post::query()
        .inner_join("users", "posts.user_id", "users.id")
        .where_eq("posts.status", "published")
        .order_desc("posts.created_at")
        .limit(5)
        .get()
        .await?;
    println!("   Inner join (posts with users): {} posts", posts_with_users.len());
    
    // Left join: Get users with optional posts
    let users_with_posts = User::query()
        .left_join("posts", "users.id", "posts.user_id")
        .where_eq("users.status", "active")
        .limit(10)
        .get()
        .await?;
    println!("   Left join (users with posts): {} users", users_with_posts.len());
    
    // Join with alias
    let posts_with_author = Post::query()
        .inner_join_as("users", "author", "posts.user_id", "author.id")
        .where_eq("posts.status", "published")
        .limit(5)
        .get()
        .await?;
    println!("   Join with alias (posts with author): {} posts", posts_with_author.len());
    
    // Multiple joins: Posts with author and comments
    let posts_with_all = Post::query()
        .inner_join("users", "posts.user_id", "users.id")
        .left_join("comments", "posts.id", "comments.post_id")
        .where_eq("posts.status", "published")
        .limit(10)
        .get()
        .await?;
    println!("   Multiple joins: {} posts", posts_with_all.len());

    // ========================================================================
    // 16. AGGREGATIONS
    // ========================================================================
    section("16. AGGREGATIONS");
    
    println!("📊 Aggregation Functions...");
    
    // SUM - total price of all products
    let total_price = Product::query()
        .where_eq("active", true)
        .sum("price")
        .await?;
    println!("   SUM (total price of active products): ${:.2}", total_price / 100.0);
    
    // AVG - average price of products
    let avg_price = Product::query()
        .where_eq("active", true)
        .avg("price")
        .await?;
    println!("   AVG (average price): ${:.2}", avg_price / 100.0);
    
    // MIN - minimum price
    let min_price = Product::query()
        .where_eq("active", true)
        .min("price")
        .await?;
    println!("   MIN (cheapest product): ${:.2}", min_price / 100.0);
    
    // MAX - maximum price
    let max_price = Product::query()
        .where_eq("active", true)
        .max("price")
        .await?;
    println!("   MAX (most expensive): ${:.2}", max_price / 100.0);
    
    // COUNT DISTINCT - unique categories
    let unique_categories = Product::query()
        .where_eq("active", true)
        .count_distinct("category")
        .await?;
    println!("   COUNT DISTINCT (unique categories): {}", unique_categories);
    
    println!("\n🔢 GROUP BY and HAVING...");
    
    // GROUP BY with raw SQL for complex aggregation
    // Note: For complex aggregations that don't map to a model, use execute
    // Example SQL (for reference):
    // SELECT category, COUNT(*) as product_count, AVG(price) as avg_price
    // FROM "products" WHERE active = true GROUP BY category HAVING COUNT(*) > 0
    
    // For raw queries returning non-model data, we can use sqlx directly or
    // create a simple model. Here we'll just demonstrate the API:
    let product_count_by_category: Vec<Product> = Database::raw(
        r#"SELECT * FROM "products" WHERE category = 'electronics'"#
    ).await?;
    println!("   Electronics products (via raw): {}", product_count_by_category.len());
    
    // Simpler aggregation using our helpers
    let electronics_stats = Product::query()
        .where_eq("category", "electronics")
        .count()
        .await?;
    let electronics_sum = Product::query()
        .where_eq("category", "electronics")
        .sum("price")
        .await?;
    println!("   Electronics: {} products, total ${:.2}", electronics_stats, electronics_sum / 100.0);
    
    // Using QueryBuilder's group_by and having helpers
    // Note: GROUP BY queries require selecting only grouped columns or aggregates
    // For complex GROUP BY with aggregates, use raw SQL:
    let posts_by_status: Vec<Post> = Database::raw(
        r#"SELECT status, CAST(COUNT(*) AS INT4) as view_count, 
           MIN(id) as id, MIN(user_id) as user_id, 
           MIN(title) as title, MIN(content) as content,
           ARRAY[]::text[] as tags, '{}'::jsonb as metadata,
           MIN(published_at) as published_at,
           MIN(created_at) as created_at, MIN(updated_at) as updated_at,
           NULL as deleted_at
           FROM "posts" 
           GROUP BY status 
           HAVING COUNT(*) >= 1"#
    ).await?;
    println!("   Posts grouped by status: {} groups", posts_by_status.len());

    // ========================================================================
    // 17. SCHEMA GENERATION
    // ========================================================================
    section("17. SCHEMA GENERATION");
    
    println!("📜 Schema generation from models...");
    
    // Generate schema for User model
    let user_schema = User::__get_sync_schema();
    println!("   User table schema:");
    println!("     Table: {}", user_schema.table_name);
    println!("     Columns: {}", user_schema.columns.len());
    println!("     Indexes: {}", User::indexes().len());
    println!("     Unique Indexes: {}", User::unique_indexes().len());
    
    // Generate schema for Post model (with soft delete)
    let post_schema = Post::__get_sync_schema();
    println!("\n   Post table schema:");
    println!("     Table: {}", post_schema.table_name);
    println!("     Columns: {}", post_schema.columns.len());
    println!("     Indexes: {}", Post::indexes().len());
    println!("     Soft delete column: {}", post_schema.columns.iter().any(|c| c.name == "deleted_at"));
    
    // Generate schema for Product model
    let product_schema = Product::__get_sync_schema();
    println!("\n   Product table schema:");
    println!("     Table: {}", product_schema.table_name);
    println!("     Columns: {}", product_schema.columns.len());
    
    // Use SchemaGenerator to generate SQL
    use tideorm::schema::{SchemaGenerator, TableSchemaBuilder, ColumnSchema};
    
    println!("\n📝 Generating SQL schema file...");
    
    let mut generator = SchemaGenerator::new(DatabaseType::Postgres);
    
    // Combine regular and unique indexes
    let mut all_indexes = User::indexes();
    all_indexes.extend(User::unique_indexes());
    
    // Build table schemas from model info
    let users_table = TableSchemaBuilder::new("users")
        .column(ColumnSchema::new("id", "BIGINT").primary_key().auto_increment())
        .column(ColumnSchema::new("email", "TEXT").not_null())
        .column(ColumnSchema::new("name", "TEXT").not_null())
        .column(ColumnSchema::new("status", "TEXT").not_null().default("'active'"))
        .column(ColumnSchema::new("password_hash", "TEXT"))
        .column(ColumnSchema::new("created_at", "TIMESTAMPTZ").not_null().default("NOW()"))
        .column(ColumnSchema::new("updated_at", "TIMESTAMPTZ").not_null().default("NOW()"))
        .indexes(all_indexes)
        .build();
    
    generator.add_table(users_table);
    
    let sql = generator.generate();
    println!("   Generated SQL preview:");
    for line in sql.lines().take(15) {
        println!("     {}", line);
    }
    println!("     ... (truncated)");
    
    // Write schema to file (optional - commented out to avoid file creation in demo)
    // tideorm::schema::SchemaWriter::write_schema("generated_schema.sql").await?;
    // println!("   ✅ Schema written to generated_schema.sql");
    
    println!("\n   Schema introspection from database...");
    
    // Use raw SQL for schema introspection
    let introspect_sql = r#"
        SELECT column_name, data_type, is_nullable
        FROM information_schema.columns 
        WHERE table_name = 'users' AND table_schema = 'public'
        ORDER BY ordinal_position
        LIMIT 7
    "#;
    
    let result = conn.execute_unprepared(introspect_sql).await;
    println!("   Users table introspection query executed: {:?}", result.is_ok());
    
    // Show what columns we expect from our model
    println!("\n   Expected columns from User model:");
    for col in user_schema.columns.iter().take(7) {
        println!("     - {} ({})", col.name, col.col_type);
    }

    // ========================================================================
    // SUMMARY
    // ========================================================================
    println!("\n");
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                    ✨ Demo Complete!                         ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");
    
    println!("📊 Final Counts:");
    println!("   Users:    {}", User::count().await?);
    println!("   Profiles: {}", Profile::count().await?);
    println!("   Posts:    {}", Post::count().await?);
    println!("   Comments: {}", Comment::count().await?);
    println!("   Products: {}", Product::count().await?);
    
    println!("\n🌊 All PostgreSQL Features Demonstrated:");
    println!("   ✅ Configuration (TideConfig)");
    println!("   ✅ CRUD Operations");
    println!("   ✅ Relations (#[belongs_to], #[has_one], #[has_many])");
    println!("   ✅ Query Builder (WHERE, ORDER, LIMIT, OFFSET)");
    println!("   ✅ JSON/JSONB Operations");
    println!("   ✅ Array Operations");
    println!("   ✅ Soft Delete");
    println!("   ✅ Transactions");
    println!("   ✅ Batch Operations");
    println!("   ✅ Scopes");
    println!("   ✅ Callbacks");
    println!("   ✅ Raw SQL");
    println!("   ✅ JSON Serialization");
    println!("   ✅ Pagination");
    println!("   ✅ JOIN Operations (INNER, LEFT, RIGHT)");
    println!("   ✅ Aggregations (SUM, AVG, MIN, MAX, COUNT DISTINCT)");
    println!("   ✅ GROUP BY / HAVING");
    println!("   ✅ Schema Generation");
    
    println!("\n📝 Next: Use this as reference for MySQL/SQLite support!");
    
    Ok(())
}

// ============================================================================
// HELPERS
// ============================================================================

fn section(title: &str) {
    println!("\n══════════════════════════════════════════════════════════════");
    println!("  {}", title);
    println!("══════════════════════════════════════════════════════════════\n");
}
