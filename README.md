# TideORM 🌊

A developer-friendly ORM for Rust with clean, expressive syntax.

[![Website](https://img.shields.io/badge/website-tideorm.com-blue.svg)](https://tideorm.com)
[![Rust](https://img.shields.io/badge/rust-1.82+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## ✨ Features

- **🎯 Clean Model Definitions** - Simple `#[derive(Model)]` macro
- **⚡ Async-First** - Built for modern async/await workflows
- **🔄 Auto Schema Sync** - Automatic table management during development
- **🛡️ Type Safe** - Full Rust type safety with zero compromises
- **🗃️ Multi-Database** - PostgreSQL, MySQL, and SQLite support
- **📦 Batteries Included**:
  - Fluent Query Builder with Window Functions & CTEs
  - Database Migrations & Seeding
  - Model Validation System
  - Translations (i18n) for multilingual content
  - File Attachments with metadata
  - Full-Text Search with highlighting
  - Soft Deletes & Callbacks
  - Transaction Support

## 🚀 Quick Start

```rust
use tideorm::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "users")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub active: bool,
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
    };
    let user = user.save().await?;

    // Query
    let users = User::query()
        .where_eq("active", true)
        .order_desc("created_at")
        .limit(10)
        .get()
        .await?;

    // Update
    let mut user = User::find(1).await?.unwrap();
    user.name = "Jane Doe".into();
    user.update().await?;

    // Delete
    User::destroy(1).await?;

    Ok(())
}
```

## 📦 Installation

```toml
[dependencies]
# PostgreSQL (default)
tideorm = { version = "0.3", features = ["postgres"] }

# MySQL
tideorm = { version = "0.3", features = ["mysql"] }

# SQLite
tideorm = { version = "0.3", features = ["sqlite"] }
```

### Feature Flags

| Feature | Description |
|---------|-------------|
| `postgres` | PostgreSQL support (default) |
| `mysql` | MySQL/MariaDB support |
| `sqlite` | SQLite support |
| `runtime-tokio` | Tokio runtime (default) |
| `runtime-async-std` | async-std runtime |

## 📖 Documentation

For detailed documentation on all features, see **[DOCUMENTATION.md](DOCUMENTATION.md)**.

Key sections:
- [Configuration](DOCUMENTATION.md#configuration) - Database connections and pool settings
- [Model Definition](DOCUMENTATION.md#model-definition) - Defining your models
- [Query Builder](DOCUMENTATION.md#query-builder) - Building complex queries
- [CRUD Operations](DOCUMENTATION.md#crud-operations) - Create, Read, Update, Delete
- [Soft Deletes](DOCUMENTATION.md#soft-deletes) - Soft delete support
- [Transactions](DOCUMENTATION.md#transactions) - Transaction handling
- [Callbacks](DOCUMENTATION.md#callbacks--hooks) - Lifecycle hooks
- [File Attachments](DOCUMENTATION.md#file-attachments) - Manage file relationships
- [Translations (i18n)](DOCUMENTATION.md#translations-i18n) - Multilingual content
- [Validation](DOCUMENTATION.md#model-validation) - Data validation rules
- [Full-Text Search](DOCUMENTATION.md#full-text-search) - Search with highlighting
- [Multi-Database](DOCUMENTATION.md#multi-database-support) - Cross-database compatibility

## 📚 Examples

See the [examples](examples/) directory for complete working examples:

| Example | Description |
|---------|-------------|
| [basic.rs](examples/basic.rs) | Basic CRUD operations |
| [query_builder.rs](examples/query_builder.rs) | Advanced query building |
| [validation_demo.rs](examples/validation_demo.rs) | Model validation |
| [caching_demo.rs](examples/caching_demo.rs) | Query caching |
| [fulltext_demo.rs](examples/fulltext_demo.rs) | Full-text search |
| [attachments_translations_demo.rs](examples/attachments_translations_demo.rs) | Files & i18n |
| [schema_file_demo.rs](examples/schema_file_demo.rs) | Schema generation |
| [migrations.rs](examples/migrations.rs) | Database migrations |

Run an example:

```bash
cargo run --example basic --features postgres
```

## 🧪 Testing

```bash
# Run all tests
cargo test --features postgres

# Run specific test
cargo test query_builder --features postgres

# Run with all features
cargo test --all-features
```

## � Rusty Rails Project

TideORM is part of the larger **Rusty Rails** project, which aims to bridge the gap between Rust and Ruby/Ruby on Rails ecosystems. We're actively working on recreating Ruby libraries in Rust to make working with Rust more easy and fun for new developers.

### Related Projects

- 🔗 More Rust libraries coming soon!
- 🚀 Performance-focused Ruby alternatives
- 📦 Easy-to-use APIs familiar to Ruby developers

---

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

---

**Made with ❤️ by the Rusty Rails team**
