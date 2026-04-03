# TideORM

A Rust ORM with field-declared relations and a fluent query builder.

[![CI](https://github.com/mohamadzoh/tideorm/actions/workflows/ci.yml/badge.svg)](https://github.com/mohamadzoh/tideorm/actions/workflows/ci.yml)
[![Website](https://img.shields.io/badge/website-tideorm.com-blue.svg)](https://tideorm.com)
[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Features

- **Clean Model Definitions** - Simple `#[tideorm::model(table = "...")]` attribute macro
- **Field-Declared Relations** - `HasOne`, `HasMany`, `BelongsTo`, and `HasManyThrough` relations are defined directly on the model
- **Async-First** - Built for modern async/await workflows
- **Auto Schema Sync** - Automatic table management during development
- **Multi-Database** - PostgreSQL, MySQL, and SQLite support
- **Query Builder** - Fluent filtering, OR groups, joins, unions, CTEs, and window functions
- **Profiling & Logging** - Built-in query logging plus execution counters and slow-query stats
- **Data Lifecycle Tools** - Migrations, seeding, validation, callbacks, soft deletes, and transactions
- **Entity Manager** - Optional persistence context for aggregate workflows and managed entity lifecycles
- **Optional Modules** - Attachments, translations, and full-text search are available behind feature flags
- **Tokenization** - Secure record ID encoding/decoding helpers

## Quick Start

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub active: bool,
    
    // Relations defined as struct fields
    #[tideorm(has_one = "Profile", foreign_key = "user_id")]
    pub profile: HasOne<Profile>,
    
    #[tideorm(has_many = "Post", foreign_key = "user_id")]
    pub posts: HasMany<Post>,
}

#[tideorm::model(table = "posts")]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    
    #[tideorm(belongs_to = "User", foreign_key = "user_id")]
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
    let matching_users = User::query()
        .where_eq("active", true)
        .begin_or()
            .or_where_like("name", "%Jane%")
            .or_where_like("email", "%@example.com")
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

Relation helper fields like `HasOne<T>` and `HasMany<T>` are runtime-only wrappers, not persisted columns. TideORM's generated serde support serializes their cached payloads when present and rebuilds the runtime wrappers on deserialize, so round-tripped JSON can preserve loaded relations without turning the wrappers themselves into stored schema fields.

Composite primary keys are supported by marking multiple fields with `#[tideorm(primary_key)]`. Composite keys are used as tuples in CRUD APIs, for example `UserRole::find((user_id, role_id))`. `auto_increment` and tokenization remain single-primary-key features.

For batch inserts, use `Model::insert_all(...)`. It is TideORM's single bulk-insert API and returns the inserted models with database-generated values populated when the active backend supports or emulates that behavior.

For tests and reconfiguration-heavy workflows, TideORM's global state is resettable. Use `Database::reset_global()`, `TideConfig::reset()`, and `TokenConfig::reset()` before applying a fresh setup.

For aggregate workflows with an explicit persistence context, enable the `entity-manager` feature and see [docs/entity-manager.md](docs/entity-manager.md).

## Installation

```toml
[dependencies]
# PostgreSQL (default)
tideorm = { version = "0.9.10", features = ["postgres"] }

# MySQL
tideorm = { version = "0.9.10", features = ["mysql"] }

# SQLite
tideorm = { version = "0.9.10", features = ["sqlite"] }

# Enable attachments support explicitly
tideorm = { version = "0.9.10", features = ["postgres", "attachments"] }

# Enable translations support explicitly
tideorm = { version = "0.9.10", features = ["postgres", "translations"] }

# Enable full-text search support explicitly
tideorm = { version = "0.9.10", features = ["postgres", "fulltext"] }

# Enable the entity manager explicitly
tideorm = { version = "0.9.10", features = ["postgres", "entity-manager"] }
```

### Feature Flags

| Feature | Description |
|---------|-------------|
| `postgres` | PostgreSQL support (default) |
| `mysql` | MySQL/MariaDB support |
| `sqlite` | SQLite support |
| `runtime-tokio` | Tokio runtime (default) |
| `runtime-async-std` | async-std runtime |
| `attachments` | Compile-time-only feature gate for the attachments API and attachment-specific benchmarks/tests; adds no extra dependencies |
| `translations` | Compile-time-only feature gate for the translations API and translation-specific benchmarks/tests; adds no extra dependencies |
| `fulltext` | Compile-time-only feature gate for the full-text search API and fulltext-specific benchmarks/tests; adds no extra dependencies |
| `entity-manager` | Enables the `EntityManager` facade (`find`, `find_managed`, `load`, `save`, `persist`, `merge`, `remove`, `detach`, `flush`), plus `save_with_entity_manager`, `find_in_entity_manager`, entity-manager-aware relation loads, and aggregate synchronization for loaded `HasOne`, `HasMany`, and `HasManyThrough` relations |

Attachments are opt-in. Enable the `attachments` feature when you want to use `tideorm::attachments`, `HasAttachments`, or attachment URL generation helpers. This is a compile-time API gate only; it does not pull in additional crates.

Translations are opt-in. Enable the `translations` feature when you want to use `tideorm::translations`, `HasTranslations`, or `ApplyTranslations`. This is a compile-time API gate only; it does not pull in additional crates.

Full-text search is opt-in. Enable the `fulltext` feature when you want to use `tideorm::fulltext`, `FullTextSearch`, or the highlighting helpers. This is a compile-time API gate only; it does not pull in additional crates.

The entity manager is opt-in. Enable the `entity-manager` feature when you want an explicit persistence context for aggregate workflows: `entity_manager.find::<Model>(...)`, `entity_manager.find_managed::<Model>(...)`, `entity_manager.load(&mut relation)`, `entity_manager.save(&model)`, and managed lifecycle operations such as `persist`, `merge`, `remove`, `detach`, and `flush`. The compatibility entry points `find_in_entity_manager`, `load_in_entity_manager`, and `save_with_entity_manager()` remain available too. See [docs/entity-manager.md](docs/entity-manager.md) for the full workflow.

## Documentation

The docs now ship as an mdBook.

- Read the book locally in the repo: [docs/introduction.md](docs/introduction.md)
- Published site: **[tideorm.com](https://tideorm.com)**
- Lightweight markdown index: [DOCUMENTATION.md](DOCUMENTATION.md)
- Rebuild the generated site locally with `mdbook build` or preview it with `mdbook serve --open`

Core chapters:
- [Getting Started](docs/getting-started.md) - Configuration, type mappings, examples, and testing
- [Models](docs/models.md) - Model definition, CRUD behavior, lifecycle hooks, validation, tokenization, and advanced TideORM helpers
- [Queries](docs/queries.md) - Query builder, full-text search, multi-database behavior, raw SQL, logging, and errors
- [Profiling](docs/profiling.md) - Global query timing, slow-query stats, manual reports, and query analysis
- [Relations](docs/relations.md) - Field-declared relations, attachments, translations, and runtime relation wrappers
- [Entity Manager](docs/entity-manager.md) - Persistence-context identity map, aggregate synchronization helpers, and managed entity lifecycle operations
- [Migrations](docs/migrations.md) - Schema builder column types, migration authoring, and schema sync guidance

## Ecosystem

This repository is the core TideORM library. Related projects live separately:

- **[TideORM CLI](https://github.com/mohamadzoh/tideorm-cli)** - External command-line tooling for TideORM
- **[TideORM Examples](https://github.com/mohamadzoh/tideorm-examples)** - End-to-end example applications and demos

## Examples

For runnable applications and broader demos, see **[tideorm-examples](https://github.com/mohamadzoh/tideorm-examples)**.

## Testing

Start with the smallest command that covers your change. CI also runs formatting, linting, library tests, and backend-specific checks.

Common local commands:

```bash
# Fast library validation
cargo test --lib

# Full suite for a backend feature set
cargo test --features postgres

# Cross-backend compile/test coverage
cargo test --all-features

# SQLite smoke test
cargo test --test sqlite_ci_smoke_test --features "sqlite runtime-tokio" --no-default-features
```

See [docs/getting-started.md](docs/getting-started.md#testing) for more.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

Please review the [Contributing Guidelines](CONTRIBUTING.md) before opening a pull request.

Please review the [Code of Conduct](CODE_OF_CONDUCT.md) before participating in project discussions or reviews.

## Support

If you need usage help or troubleshooting guidance, please review [SUPPORT.md](SUPPORT.md) and open the appropriate issue type.

## Security

If you need to report a suspected vulnerability, please use the private reporting process described in the [Security Policy](SECURITY.md) instead of opening a public issue.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
