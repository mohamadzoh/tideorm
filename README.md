# TideORM

A Rust ORM with field-declared relations and a fluent query builder.

[![CI](https://github.com/mohamadzoh/tideorm/actions/workflows/ci.yml/badge.svg)](https://github.com/mohamadzoh/tideorm/actions/workflows/ci.yml)
[![Website](https://img.shields.io/badge/website-tideorm.com-blue.svg)](https://tideorm.com)
[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Features

- **Clean Model Definitions** - Simple `#[tideorm::model(table = "...")]` attribute macro
- **Field-Declared Relations** - `HasOne`, `HasMany`, `BelongsTo`, and `HasManyThrough` relations are defined directly on the model
- **Eager Loading** - Batch relation loading with `query().with(...)` and nested eager paths to avoid N+1 lookups
- **Chunked Reads** - Process large result sets with `query().chunk(...)` instead of loading every row at once
- **Named Query Scopes** - Declare model-local `#[tideorm::scopes]` methods for chains like `User::query().active().verified()`
- **Optional Dirty Tracking** - Enable `dirty-tracking` only when you want `changed_fields()` and `original_value()` model inspection helpers
- **Async-First** - Built for modern async/await workflows
- **Auto Schema Sync** - Automatic table management during development
- **Multi-Database** - PostgreSQL, MySQL, and SQLite support
- **Query Builder** - Fluent filtering, OR groups, joins, unions, CTEs, and window functions
- **Profiling & Logging** - Built-in query logging plus execution counters and slow-query stats
- **Data Lifecycle Tools** - Migrations, seeding, validation, callbacks, soft deletes, and transactions
- **Entity Manager** - Optional persistence context for aggregate workflows and managed entity lifecycles
- **Optional Modules** - Attachments, translations, and full-text search are available behind feature flags
- **Field Encryption** - Optional `encrypted-fields` feature for model-level encrypted columns that automatically encrypt on write and decrypt on load using TideORM's configured encryption key
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
        .models_matching("src/models/*.model.rs")
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

    // Load relations lazily
    let posts = user.posts.load().await?;
    let profile = user.profile.load().await?;

    // Or batch-load them eagerly from the query itself
    let users = User::query()
        .where_eq("active", true)
        .with("profile")
        .with("posts")
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

Relation helper fields like `HasOne<T>` and `HasMany<T>` are runtime-only wrappers, not persisted columns. TideORM's generated serde support serializes their cached payloads when present and rebuilds the runtime wrappers on deserialize, so round-tripped JSON can preserve loaded relations without turning the wrappers themselves into stored schema fields.

Composite primary keys are supported by marking multiple fields with `#[tideorm(primary_key)]`. Composite keys are used as tuples in CRUD APIs, for example `UserRole::find((user_id, role_id))`. `auto_increment` and tokenization remain single-primary-key features.

For batch inserts, use `Model::insert_all(...)`. It is TideORM's single bulk-insert API and returns the inserted models with database-generated values populated when the active backend supports or emulates that behavior.

For tests and reconfiguration-heavy workflows, TideORM's global state is resettable. Use `Database::reset_global()`, `TideConfig::reset()`, and `TokenConfig::reset()` before applying a fresh setup.

For aggregate workflows with an explicit persistence context, enable the `entity-manager` feature and see [docs/entity-manager.md](docs/entity-manager.md).

## Encrypted Fields

Enable the `encrypted-fields` Cargo feature before using model-level auto-encryption.

Use model-level encrypted fields when you want selected persisted columns, such as phone numbers or other sensitive strings, to be stored encrypted in the database while remaining plain `String` or `Option<String>` values in Rust.

```toml
[dependencies]
tideorm = { version = "0.10.0", default-features = false, features = ["postgres", "runtime-tokio", "encrypted-fields"] }
```

```rust
use tideorm::prelude::*;

#[tideorm::model(
    table = "customers",
    encrypted = "customer_phone_number, backup_phone"
)]
pub struct Customer {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    #[tideorm(column = "customer_phone_number")]
    pub phone_number: String,
    pub backup_phone: Option<String>,
}

TideConfig::init()
    .database("sqlite://./app.db")
    .encryption_key("replace-with-a-long-random-secret")
    .connect()
    .await?;
```

Encrypted field notes:

- `encrypted = "..."` is separate from `#[tideorm(tokenize)]`; it uses the same configured encryption key and crypto primitives, but it protects normal model columns instead of record IDs.
- Each encrypted field uses a scoped key derived from the configured encryption secret plus the model table and column name, so ciphertext from one attribute does not decrypt under another attribute's context.
- TideORM encrypts these fields before `create()`, `save()`, `update()`, `insert_all()`, nested saves, and batch `update_all().set(...)` writes.
- TideORM decrypts these fields automatically when loading models through normal queries, eager loads, and raw model hydration.
- Encrypted columns must contain TideORM encrypted payloads or `NULL`. Plaintext legacy rows and older global-scope payloads are rejected on load; migrate that data explicitly before enabling the feature.
- Query predicates are not rewritten yet. Filters such as `where_eq("customer_phone_number", "...")` still compare against the stored database value, so plaintext lookups on encrypted columns are not currently transparent.
- Supported encrypted field types are `String`, `Text`, `Option<String>`, and `Option<Text>`.

## Installation

Every snippet below sets `default-features = false` and names its backend and runtime
explicitly. The default feature set is `["postgres", "runtime-tokio"]`, so a MySQL- or
SQLite-only project that leaves the defaults on still compiles the whole PostgreSQL
driver stack.

```toml
[dependencies]
# PostgreSQL
tideorm = { version = "0.10.0", default-features = false, features = ["postgres", "runtime-tokio"] }

# MySQL / MariaDB
tideorm = { version = "0.10.0", default-features = false, features = ["mysql", "runtime-tokio"] }

# SQLite
tideorm = { version = "0.10.0", default-features = false, features = ["sqlite", "runtime-tokio"] }

# Enable attachments support explicitly
tideorm = { version = "0.10.0", default-features = false, features = ["postgres", "runtime-tokio", "attachments"] }

# Enable translations support explicitly
tideorm = { version = "0.10.0", default-features = false, features = ["postgres", "runtime-tokio", "translations"] }

# Enable full-text search support explicitly
tideorm = { version = "0.10.0", default-features = false, features = ["postgres", "runtime-tokio", "fulltext"] }

# Enable the entity manager explicitly
tideorm = { version = "0.10.0", default-features = false, features = ["postgres", "runtime-tokio", "entity-manager"] }

# Enable model dirty tracking explicitly
tideorm = { version = "0.10.0", default-features = false, features = ["postgres", "runtime-tokio", "dirty-tracking"] }

# Enable model encrypted fields explicitly
tideorm = { version = "0.10.0", default-features = false, features = ["postgres", "runtime-tokio", "encrypted-fields"] }
```

Swap `runtime-tokio` for `runtime-async-std` if you run on async-std.

### What else your crate needs

`#[tideorm::model]` expands into your crate, so a few things have to be reachable
from *there* rather than from TideORM. `tideorm init` writes all of them into a
scaffolded project; add them by hand if you are wiring TideORM into an existing one.

```toml
[dependencies]
tideorm = { version = "0.10.0", default-features = false, features = ["postgres", "runtime-tokio"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = "0.4"          # only if you use date/time columns

# Declare locally every TideORM feature you enable. A derive's `#[cfg(feature = ..)]`
# is evaluated against YOUR crate's features, not TideORM's, so without this the
# generated code silently takes the feature-off branch and rustc warns
# `unexpected cfg condition value`.
[features]
entity-manager = ["tideorm/entity-manager"]
```

`serde` and `serde_json` are not optional: the derive emits `::serde` and
`::serde_json` paths, so a model will not compile without both as direct
dependencies. `chrono` is only needed if you spell a column type with an explicit
`chrono::` path — the `DateTime<Utc>` re-exported from `tideorm::prelude` works
without it.

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
| `dirty-tracking` | Enables the model-level `changed_fields()` and `original_value()` helpers and their persisted-state tracking hooks |
| `encrypted-fields` | Enables `#[tideorm(encrypted = "...")]` model auto-encrypt/decrypt hooks for persisted string columns |

Attachments are opt-in. Enable the `attachments` feature when you want to use `tideorm::attachments`, `HasAttachments`, or attachment URL generation helpers. This is a compile-time API gate only; it does not pull in additional crates.

Translations are opt-in. Enable the `translations` feature when you want to use `tideorm::translations`, `HasTranslations`, or `ApplyTranslations`. This is a compile-time API gate only; it does not pull in additional crates.

Full-text search is opt-in. Enable the `fulltext` feature when you want to use `tideorm::fulltext`, `FullTextSearch`, or the highlighting helpers. This is a compile-time API gate only; it does not pull in additional crates.

The entity manager is opt-in. Enable the `entity-manager` feature when you want an explicit persistence context for aggregate workflows: `entity_manager.find::<Model>(...)`, `entity_manager.find_managed::<Model>(...)`, `entity_manager.load(&mut relation)`, `entity_manager.save(&model)`, and managed lifecycle operations such as `persist`, `merge`, `remove`, `detach`, and `flush`. The compatibility entry points `find_in_entity_manager`, `load_in_entity_manager`, and `save_with_entity_manager()` remain available too. See [docs/entity-manager.md](docs/entity-manager.md) for the full workflow.

Dirty tracking is opt-in. Enable the `dirty-tracking` feature when you want model instances to expose `changed_fields()` and `original_value()` based on the latest persisted snapshot TideORM loaded or saved.

Model encrypted fields are opt-in. Enable the `encrypted-fields` feature when you want `#[tideorm(encrypted = "...")]` to automatically encrypt configured string columns on writes and decrypt them on model loads.

Schema sync can also register compiled models by source path with glob-style patterns such as `models_matching("src/models/*")` or `models_matching("src/models/*.model.rs")`. The matching files still need to be part of the crate through normal Rust `mod` declarations because TideORM filters compiled model metadata rather than loading source files dynamically.

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
- [Benchmarking](docs/benchmarking.md) - Benchmark target matrix, PostgreSQL setup, feature-gated bench commands, and Criterion baseline workflow
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

The backends do not share gate semantics: SQLite is opt-out (`SKIP_SQLITE_TESTS`), MySQL is opt-in (`RUN_MYSQL_TESTS` or `MYSQL_DATABASE_URL`, with no skip flag), and the PostgreSQL suites always connect. See [CONTRIBUTING.md](CONTRIBUTING.md#test-database-environment-variables) for the full variable table.

See [docs/getting-started.md](docs/getting-started.md#testing) for more.

## Benchmarking

Always run a single Criterion target; a bare `cargo bench` is not usable because several targets need a database. `query_benchmarks`, `crud_benchmarks`, and `or_clause_benchmarks` require a live PostgreSQL server and abort the run without one; they default to `postgres://postgres:postgres@localhost:5432/test_tide_orm` and respect `POSTGRESQL_DATABASE_URL`.

Common local commands:

```bash
# No database required
cargo bench --bench validation_benchmarks
cargo bench --bench attachments_translations_benchmarks --features "attachments translations"
cargo bench --bench fulltext_benchmarks --features fulltext

# SQLite driver required
cargo bench --bench cache_benchmarks --features sqlite

# Live PostgreSQL server required
cargo bench --bench query_benchmarks
cargo bench --bench crud_benchmarks
cargo bench --bench or_clause_benchmarks

# Compile every bench without running any
cargo bench --no-run --features "sqlite attachments translations fulltext"
```

See [docs/benchmarking.md](docs/benchmarking.md) for the full benchmark matrix and Criterion baseline workflow.

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
