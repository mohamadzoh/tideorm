# TideORM Application Template

A comprehensive template demonstrating all TideORM features for building database-driven Rust applications.

## Features Demonstrated

### Models
- **User** - Basic model with relationships, hidden fields, indexes
- **Post** - Complex model with JSON columns, soft deletes, timestamps  
- **Comment** - Nested relationships (replies), multiple foreign keys
- **Tag** - Lookup table pattern with usage counting

### TideORM Features
- ✅ **CRUD Operations** - Create, Read, Update, Delete
- ✅ **Timestamps** - `created_at`, `updated_at` with chrono
- ✅ **Soft Deletes** - `deleted_at` field with automatic filtering
- ✅ **Relationships** - `has_many`, `belongs_to`
- ✅ **JSON Columns** - Store flexible data with `serde_json::Value`
- ✅ **Hidden Fields** - Exclude sensitive fields from serialization
- ✅ **Indexes** - Performance optimization with `#[index]`, `#[unique_index]`
- ✅ **Query Builder** - Fluent API for complex queries
- ✅ **Pagination** - `limit()`, `offset()`
- ✅ **Ordering** - `order_by()`
- ✅ **Filtering** - `where_eq()`, `where_gt()`, `where_like()`, `where_null()`
- ✅ **Counting** - `count()`
- ✅ **Schema Sync** - Auto-create tables in development

## Quick Start

```bash
# Clone and enter the template
cd template

# Create data directory (for SQLite)
mkdir data

# Run the demo
cargo run
```

## Project Structure

```
template/
├── Cargo.toml          # Dependencies
├── .env.example        # Environment template
├── src/
│   ├── main.rs         # Application entry point with demo
│   └── models/
│       ├── mod.rs      # Model exports
│       ├── user.rs     # User model with relationships
│       ├── post.rs     # Post model with JSON & soft delete
│       ├── comment.rs  # Comment model with nested replies
│       └── tag.rs      # Tag model for categorization
└── data/
    └── app.db          # SQLite database (auto-created)
```

## Model Examples

### User Model
```rust
#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "users", hidden = "password_hash")]
#[has_many(Post, foreign_key = "user_id")]
#[index("email")]
#[unique_index("email")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub email: String,
    pub status: String,
    #[tide(nullable)]
    pub bio: Option<String>,
    #[tide(nullable)]
    pub password_hash: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}
```

### Post Model with Soft Deletes & JSON
```rust
#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "posts", soft_delete)]
#[belongs_to(User, foreign_key = "user_id")]
pub struct Post {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub slug: String,
    pub title: String,
    pub content: String,
    pub published: bool,
    #[tide(nullable)]
    pub published_at: Option<chrono::DateTime<chrono::Utc>>,
    pub view_count: i64,
    #[tide(nullable)]
    pub tags: Option<serde_json::Value>,      // JSON column
    #[tide(nullable)]
    pub metadata: Option<serde_json::Value>,  // JSON column
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    #[tide(nullable)]
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}
```

## Query Examples

```rust
// Create
let user = User::new("Alice", "alice@example.com").save().await?;

// Find by ID
let user = User::find(1).await?;

// Query with conditions
let active = User::query()
    .where_eq("status", "active")
    .order_by("created_at", Order::Desc)
    .get()
    .await?;

// Pagination
let page = Post::query()
    .limit(10)
    .offset(0)
    .get()
    .await?;

// LIKE search
let matches = Post::query()
    .where_like("title", "%rust%")
    .get()
    .await?;

// Relationships
let posts = user.posts().await?;
let author = post.author().await?;

// Count
let total = Post::query().count().await?;
```

## Configuration

Copy `.env.example` to `.env` and configure:

```env
DATABASE_URL=sqlite://./data/app.db?mode=rwc
RUST_LOG=info
```

## Database Support

This template uses SQLite. TideORM also supports:
- PostgreSQL
- MySQL

Change the feature in `Cargo.toml`:
```toml
tideorm = { path = "..", features = ["postgres", "runtime-tokio"] }
```

## What the Demo Shows

When you run `cargo run`, the demo will:

1. **CREATE** - Insert users, posts, comments, and tags
2. **READ** - Query by ID, email, conditions, pagination
3. **UPDATE** - Modify user bio, publish posts, increment view counts
4. **QUERY BUILDER** - Complex queries with multiple conditions
5. **RELATIONSHIPS** - Navigate between related models
6. **SOFT DELETE** - Delete and verify filtering

## License

MIT
