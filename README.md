# TideORM

A developer-friendly ORM for Rust with clean, expressive syntax.

[![Website](https://img.shields.io/badge/website-tideorm.com-blue.svg)](https://tideorm.com)
[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## TideORM CLI & Studio

**NEW!** TideORM now includes a powerful CLI with a beautiful web-based interface!

```bash
# Install the CLI
cargo install tideorm-cli

# Initialize a project
tideorm init my_project

# Generate models with a single command
tideorm make model User --fields="name:string,email:string:unique" --relations="posts:has_many:Post" --migration

# Launch TideORM Studio (Web UI)
tideorm ui
```

TideORM Studio provides a visual interface for:
- 🏗️ Model generation with all options
- 📦 Migration management
- 🌱 Database seeding
- ⚡ Interactive SQL query playground

See the [TideORM CLI README](../tideorm-cli/README.md) for full documentation.

---

## Features

- **Clean Model Definitions** - Simple `#[tideorm::model]` attribute macro
- **SeaORM-Style Relations** - Define relations as struct fields
- **Async-First** - Built for modern async/await workflows
- **Auto Schema Sync** - Automatic table management during development
- **Type Safe** - Full Rust type safety with zero compromises
- **Multi-Database** - PostgreSQL, MySQL, and SQLite support
- **Batteries Included**:
  - Fluent Query Builder with Window Functions & CTEs
  - Database Migrations & Seeding
  - Model Validation System
  - Record Tokenization (secure ID encryption)
  - Translations (i18n) for multilingual content
  - File Attachments with metadata
  - Full-Text Search with highlighting
  - Soft Deletes & Callbacks
  - Transaction Support

## Quick Start

```rust
use tideorm::prelude::*;

#[tideorm::model]
#[tide(table = "users")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub active: bool,
    
    // Relations defined as struct fields
    #[tide(has_one = "Profile", foreign_key = "user_id")]
    pub profile: HasOne<Profile>,
    
    #[tide(has_many = "Post", foreign_key = "user_id")]
    pub posts: HasMany<Post>,
}

#[tideorm::model]
#[tide(table = "posts")]
pub struct Post {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    
    #[tide(belongs_to = "User", foreign_key = "user_id")]
    pub author: BelongsTo<User>,
}

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    // Connect with auto schema sync (development only!)
    TideConfig::init()
        .database("postgres://localhost/mydb")
        .sync(true)
        .connect()
        .await?;

    // Create
    let user = User {
        id: 0,
        email: "john@example.com".into(),
        name: "John Doe".into(),
        active: true,
        ..Default::default()
    };
    let user = user.save().await?;

    // Query
    let users = User::query()
        .where_eq("active", true)
        .order_desc("created_at")
        .limit(10)
        .get()
        .await?;

    // Complex queries with OR conditions
    let premium_or_featured = Product::query()
        .where_eq("active", true)
        .begin_or()
            .or_where_gt("price", 500.0).and_where_gte("rating", 4.5)
            .or_where_eq("featured", true)
        .end_or()
        .get()
        .await?;

    // Load relations
    let posts = user.posts.load().await?;
    let profile = user.profile.load().await?;

    // Update
    let mut user = User::find(1).await?.unwrap();
    user.name = "Jane Doe".into();
    user.update().await?;

    // Delete
    User::destroy(1).await?;

    Ok(())
}
```

## Model Relations

TideORM supports SeaORM-style relations defined as struct fields:

```rust
#[tideorm::model]
#[tide(table = "users")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    
    // One-to-one: User has one Profile
    #[tide(has_one = "Profile", foreign_key = "user_id")]
    pub profile: HasOne<Profile>,
    
    // One-to-many: User has many Posts
    #[tide(has_many = "Post", foreign_key = "user_id")]
    pub posts: HasMany<Post>,
    
    // Many-to-many through pivot table
    #[tide(has_many_through = "Role", pivot = "user_roles", foreign_key = "user_id", related_key = "role_id")]
    pub roles: HasManyThrough<Role, UserRole>,
}

#[tideorm::model]
#[tide(table = "posts")]
pub struct Post {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    
    // Inverse relation: Post belongs to User
    #[tide(belongs_to = "User", foreign_key = "user_id")]
    pub author: BelongsTo<User>,
}

// Loading relations
let user = User::find(1).await?.unwrap();
let posts = user.posts.load().await?;              // Vec<Post>
let profile = user.profile.load().await?;          // Option<Profile>

let post = Post::find(1).await?.unwrap();
let author = post.author.load().await?;            // Option<User>

// Check if relation exists
let has_posts = user.posts.exists().await?;        // bool
let post_count = user.posts.count().await?;        // u64

// Load with constraints
let recent_posts = user.posts.load_with(|q| {
    q.where_eq("published", true)
     .order_desc("created_at")
     .limit(5)
}).await?;
```

## Installation

```toml
[dependencies]
# PostgreSQL (default)
tideorm = { version = "*", features = ["postgres"] }

# MySQL
tideorm = { version = "*", features = ["mysql"] }

# SQLite
tideorm = { version = "*", features = ["sqlite"] }
```

### Feature Flags

| Feature | Description |
|---------|-------------|
| `postgres` | PostgreSQL support (default) |
| `mysql` | MySQL/MariaDB support |
| `sqlite` | SQLite support |
| `runtime-tokio` | Tokio runtime (default) |
| `runtime-async-std` | async-std runtime |

## Relation Types

| Type | Description | Example |
|------|-------------|---------|
| `HasOne<T>` | One-to-one relationship | User has one Profile |
| `HasMany<T>` | One-to-many relationship | User has many Posts |
| `BelongsTo<T>` | Inverse of HasOne/HasMany | Post belongs to User |
| `HasManyThrough<T, P>` | Many-to-many via pivot | User has many Roles through UserRoles |
| `MorphOne<T>` | Polymorphic one-to-one | Post/Video has one Image |
| `MorphMany<T>` | Polymorphic one-to-many | Post/Video has many Comments |

## Documentation

For detailed documentation on all features, see **[DOCUMENTATION.md](DOCUMENTATION.md)**.

Key sections:
- [Configuration](DOCUMENTATION.md#configuration) - Database connections and pool settings
- [Model Definition](DOCUMENTATION.md#model-definition) - Defining your models
- [Query Builder](DOCUMENTATION.md#query-builder) - Building complex queries
- [OR Conditions](DOCUMENTATION.md#or-conditions) - Fluent OR API with begin_or/end_or
- [CRUD Operations](DOCUMENTATION.md#crud-operations) - Create, Read, Update, Delete
- [Soft Deletes](DOCUMENTATION.md#soft-deletes) - Soft delete support
- [Transactions](DOCUMENTATION.md#transactions) - Transaction handling
- [Callbacks](DOCUMENTATION.md#callbacks--hooks) - Lifecycle hooks
- [File Attachments](DOCUMENTATION.md#file-attachments) - Manage file relationships
- [Translations (i18n)](DOCUMENTATION.md#translations-i18n) - Multilingual content
- [Validation](DOCUMENTATION.md#model-validation) - Data validation rules
- [Full-Text Search](DOCUMENTATION.md#full-text-search) - Search with highlighting
- [Multi-Database](DOCUMENTATION.md#multi-database-support) - Cross-database compatibility

### TideORM CLI

For the command-line interface and TideORM Studio, see the **[TideORM CLI README](../tideorm-cli/README.md)**.

CLI features:
- **Model Generator** - Create models with fields, relations, attachments, translations
- **Migration System** - Create, run, rollback database migrations
- **Seeders & Factories** - Database seeding and test data generation
- **TideORM Studio** - Web-based UI for all CLI operations

## Examples

For comprehensive examples demonstrating TideORM features, see the **[tideorm-examples](https://github.com/mohamadzoh/tideorm-examples)** repository.

```bash
# Clone the examples repository
git clone https://github.com/mohamadzoh/tideorm-examples.git
cd tideorm-examples

# Run a basic example
cargo run --bin basic

# Run with different databases
cargo run --bin postgres_demo
cargo run --bin mysql_demo --features "mysql runtime-tokio" --no-default-features
cargo run --bin sqlite_demo --features "sqlite runtime-tokio" --no-default-features
```

### Available Examples

| Example | Description |
|---------|-------------|
| `basic` | Core CRUD operations |
| `query_builder` | Advanced querying (WHERE, ORDER BY, LIMIT) |
| `where_and_or_demo` | Comprehensive WHERE & OR conditions |
| `upsert_demo` | Insert-or-update with conflict handling |
| `postgres_complete` | Complete PostgreSQL feature showcase |
| `migrations` | Database schema migrations |
| `validation_demo` | Model validation system |
| `fulltext_demo` | Full-text search with highlighting |
| `tokenization_demo` | Secure record ID tokenization |

See the [tideorm-examples README](https://github.com/mohamadzoh/tideorm-examples#readme) for the complete list.

## Testing

Run tests:

```bash
cargo test --features postgres
```

See [DOCUMENTATION.md](DOCUMENTATION.md#examples) for more.

## Rusty Rails Project

TideORM is part of the larger **Rusty Rails** project, which aims to bridge the gap between Rust and Ruby/Ruby on Rails ecosystems. We're actively working on recreating Ruby libraries in Rust to make working with Rust more easy and fun for new developers.

### Related Projects

- **[TideORM CLI](https://github.com/mohamadzoh/tideorm-cli)** - Command-line interface with TideORM Studio web UI
- **[TideORM Examples](https://github.com/mohamadzoh/tideorm-examples)** - Comprehensive examples
- More Rust libraries coming soon!

---

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

---

**Made with love by the Rusty Rails team**
