# Changelog

All notable changes to TideORM will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.8.5] - 2026-03-20

### Fixed

- Reworked `Database::transaction()` to restore the transaction-scoped database override on every future poll, so model and query helpers keep using the active transaction even if the async runtime resumes the future on a different thread.

### Changed

- Moved the remaining inline unit tests out of implementation modules and into dedicated owner test files under `src/testing/`, keeping private-module coverage while making the source files easier to maintain.
- Refreshed dependency examples and macro-crate docs to use the 0.8.5 release version.

### Internal

- Added a regression test that manually polls a transaction-scoped future on two different threads to verify the per-poll override behavior.
- Verified the release prep with `cargo test --lib --features sqlite` and `cargo clippy --workspace --all-targets -- -D warnings`.

## [0.8.4] - 2026-03-20

### Fixed

- Replaced silent translation and file-attachment serialization stubs so `Model::load_language_translations()`, `Model::get_files_attribute()`, and `Model::set_files_attribute()` now operate on the model state instead of succeeding without effect.
- Made `Model::load_all_translations()` fail loudly with a clear unsupported error instead of silently pretending to load all translations into scalar model fields.
- Hardened `Database::transaction()` so leaked transaction handles now fail consistently on both commit and rollback paths instead of silently relying on drop-based rollback in the error path.
- Ensured `NestedSaveBuilder::save()` persists related models with the parent foreign key instead of returning only FK-patched JSON payloads.

### Changed

- Reduced database-access overhead on model hot paths by resolving the current connection/backend directly from the active scope instead of repeatedly cloning the outer `Database` wrapper.
- Changed transaction-scoped thread-local overrides to store `DatabaseHandle` directly and updated `ConnectionRef::Database` to carry the shared internal connection handle instead of cloning `DatabaseConnection` per lookup.
- Refreshed public docs and examples to use the current global-database initialization API and the 0.8.4 crate version.

### Internal

- Kept the workspace warning-free after the connection-handle refactor by updating generated macro code, full-text execution paths, eager loading, nested bulk upserts, and query helpers to use shared internal connections correctly.
- Verified the release with `cargo test --lib` and `cargo clippy --workspace --all-targets -- -D warnings`.

## [0.8.1] - 2026-03-18

### Fixed

- Restored transaction scoping for TideORM model and query helpers so `save()`, `update()`, `delete()`, eager loading, nested operations, full-text reads, and aggregate helpers now honor the active transaction instead of bypassing it through the global pooled connection.
- Hardened raw SQL builder escape hatches by rejecting obvious injection markers in `where_raw`, raw column expressions, and nested subqueries before execution.
- Restored the original `Model::to_hash_map()` behavior that hides structured presenter `params` payloads entirely; `params` remains a reserved presenter key and is now documented as such.
- Removed disabled-path profiling overhead in `__profile_future()` by skipping `Instant::now()` when global profiling is off.
- Replaced leaked schema-path storage in `TideConfig` so repeated `apply()` and `connect()` calls no longer leak each configured schema file path.
- Reworked `SelfRefMany::load_tree()` to use a single recursive CTE query, eliminating per-node descendant lookups and honoring the configured `local_key` when walking self-referential trees.
- Repaired all-features build breakage after the reconfigurable global-database refactor by updating direct SeaORM call sites to borrow owned internal connections correctly.
- Restored consistent full-text SQL parameterization coverage across the query and full-text test suites, including PostgreSQL ranked search placeholders and SQLite FTS pagination bindings.
- Tightened encrypted-field missing-key coverage so integration tests now assert the actionable startup-configuration error message returned by `Encrypted<T>`.

### Changed

- `require_db()`, `try_db()`, `TideConfig::db()`, `TideConfig::try_db()`, `Model::db()`, and `Model::database()` now return owned `Database` handles for consistency with transaction-aware current-connection access.
- `TideConfig::schema_file_path()` now returns `Option<String>` instead of `Option<&'static str>` so schema path state can be replaced safely without leaking memory across reconfiguration.
- Documented resettable global configuration, tokenization override reset behavior, and the batched nested many-model save/update/delete paths in the README and mdBook chapters.

### Internal

- Removed stale imports left behind by the runtime-global refactor.
- Verified the release with `cargo test --all-features`.

## [0.8.0] - 2026-03-16

### Changed

- Moved the documentation to an mdBook with dedicated Getting Started, Models, Queries, Relations, and Migrations chapters, and wired the site for GitHub Pages deployment at `tideorm.com`.
- Standardized model and field metadata on the `#[tideorm(...)]` attribute form across docs, tests, benchmarks, and generated macro output.
- Split the `tideorm-macros` implementation into focused modules so entity generation, model trait generation, serde, relations, tokenization, validation, and parsing logic are maintained independently.
- Initialized supported relation wrappers through generated `with_relations()` setup for loaded models, so `HasOne`, `HasMany`, `BelongsTo`, and `HasManyThrough` fields are wired with the correct runtime context.

### Changed — Breaking

- Gated attachments, translations, and full-text search behind explicit Cargo features. Consumers now need to opt into `attachments`, `translations`, and `fulltext` when using those modules or APIs.
- Removed the legacy `#[tide(...)]` compatibility form and related prelude re-exports. Use `#[tideorm(...)]` attributes consistently.

### Fixed

- Excluded runtime-only relation helper fields from TideORM's generated serde output and restored them with defaults during deserialization.
- Restored valid PostgreSQL JSON and array operator SQL generation after the SeaQuery condition rewrite, keeping advanced query coverage green for `@>`, `<@`, `?`, `@?`, and array overlap/containment operators.
- Cleaned up the top-level documentation entry points so `DOCUMENTATION.md`, README links, and the split mdBook chapters no longer contain stale anchors or partially copied monolith content.

### Removed

- Dropped the explicit `sea-schema` dependency and removed unused development dependencies `lazy_static`, `pretty_assertions`, and `serial_test`.

### Internal

- Added docs verification to CI and a dedicated Pages workflow for publishing the mdBook.
- Verified the release with `cargo test`, `cargo test postgres_advanced_tests`, and `mdbook build`.

## [0.7.3] - 2026-03-16

### Fixed

- Relation helper fields generated by `#[derive(Model)]` are now excluded from TideORM's auto-generated serde output and restored with defaults on deserialize, preventing `HasOne`, `HasMany`, `BelongsTo`, and similar runtime-only relation wrappers from leaking into JSON payloads.

### Changed

- Refreshed the README and documentation examples to stop recommending manual `id: 0` initialization for common auto-increment create flows.
- Documented that relation helper fields are skipped by generated serde and restored with defaults during deserialization.

### Internal

- Aligned the tokenization test suite with the current XChaCha20-Poly1305 implementation, including randomized tokens per encode and the authenticated token length now produced by the runtime.
- Corrected the SQLite FTS5 SQL test expectation to match the current identifier-quoting behavior in generated statements.
- Verified the release with a full `cargo test` pass.

## [0.7.2] - 2026-03-15

### Changed

- Refactored the query subsystem so the oversized query builder implementation is now split into focused modules, reducing maintenance risk without changing the public builder API.
- Standardized read execution paths on parameterized SQL generation for get, first, count, exists, and JSON reads to keep backend-specific quoting and placeholder handling consistent.
- Replaced the default record tokenization internals with XChaCha20-Poly1305 authenticated encryption using randomized nonces while keeping the public token APIs intact.

### Fixed

- Repaired the extracted SQL query module after a bad split left overlapping implementations in place.
- Restored JOIN clause validation in the advanced query builder so invalid table, alias, and column identifiers invalidate the query instead of being accepted.
- Hardened mutation queries so delete, restore, soft-delete, and force-delete fail fast when combined with incompatible SELECT, JOIN, ORDER BY, GROUP BY, HAVING, UNION, CTE, or window-function modifiers.
- Improved cache key generation so more query-shaping fields are included, preventing collisions between distinct advanced query configurations.
- Corrected `Model::is_new()` so auto-increment primary keys with value `0` are treated as unsaved records.

### Added

- Added a GitHub Actions CI workflow that runs `cargo check` and `cargo test --lib` across PostgreSQL, MySQL, and SQLite feature sets.

### Internal

- Verified the release with a full cargo test pass after the query-module recovery and cleanup.
- Moved larger internal unit suites into dedicated files under `src/testing/` to keep implementation modules cleaner without losing private-module test coverage.

## [0.7.0] - 2025-03-09

### Added — MariaDB Support

- **`DatabaseType::MariaDB` variant**: Full first-class MariaDB support with auto-detection. Connecting via `mysql://` to a MariaDB server automatically detects the variant using `SELECT VERSION()`
- **`mariadb://` URL scheme**: `from_url()` and `TideConfig` now accept `mariadb://` URLs (rewritten to `mysql://` for the sqlx driver)
- **`#[non_exhaustive]` on `DatabaseType`**: Future-proofing the enum for new backends without breaking downstream matches
- **`is_mysql_compatible()` / `is_mariadb()` helpers**: Static methods on `TideConfig` for runtime backend detection
- **MariaDB RETURNING support**: MariaDB 10.5+ supports `INSERT ... RETURNING`, so the RETURNING-rejection check now only applies to MySQL (not MariaDB)

### Added — Error Ergonomics

- **`From<sea_orm::DbErr>` for `Error`**: Use `?` directly on SeaORM operations instead of `.map_err(translate_error)`
- **`From<std::io::Error>` for `Error`**: Converts to `Error::Internal`
- **`From<serde_json::Error>` for `Error`**: Converts to `Error::Conversion`

### Added — Transaction API

- **`Transaction::connection()`**: Public method to get the underlying `&DatabaseTransaction` for use with SeaORM operations inside transactions

### Changed — Breaking

- **`Database::transaction()` signature changed**: The closure now receives `&Transaction` (reference) instead of `Transaction` (owned), and must return `Pin<Box<dyn Future>>`. This fixes a critical bug where transactions were never committed. Usage: `db.transaction(|tx| Box::pin(async move { ... })).await?`
- **`Model::transaction()` signature changed**: Matches the new `Database::transaction()` signature
- **`DatabaseType` is now `#[non_exhaustive]`**: `match` on `DatabaseType` must include a wildcard arm

### Changed — API

- **`Model::db()` / `Model::database()` now return `Result`**: Changed from `&'static Database` to `Result<&'static Database>` — use `?` or `.unwrap()` at callsite
- **`TideConfig::db()` now returns `Result`**: Changed from `&'static Database` to `Result<&'static Database>`
- **`require_db()` re-exported in prelude**: Added to `prelude::*` alongside `db` and `try_db`

### Fixed — Critical

- **Transaction commit bug**: `Database::transaction()` now properly commits on success instead of silently rolling back. The previous implementation moved the transaction into the closure, causing it to auto-rollback on drop regardless of outcome
- **MySQL batch insert error swallowing**: `Model::insert_all()` no longer silently falls back to individual inserts when batch insert fails — errors are now propagated. The underlying `QueryExecutor::insert_many()` properly handles MySQL/SQLite by falling back to individual inserts internally

### Fixed — MySQL/MariaDB

- **MySQL array type mapping**: `Vec<i32>`, `IntArray`, `BigIntArray`, `TextArray`, `BoolArray`, `FloatArray`, `JsonArray` now correctly map to `JSON` on MySQL/MariaDB (previously fell through to `TEXT`)
- **All database-dispatch match arms updated**: 50+ match arms across `query.rs`, `schema.rs`, `migration.rs`, `fulltext.rs`, `model.rs`, `seeding.rs` now include `DatabaseType::MariaDB` alongside `DatabaseType::MySQL`

### Internal

- **`QueryExecutor::insert_many()` rewritten**: Now checks backend support for `INSERT ... RETURNING` — uses batch RETURNING on PostgreSQL, falls back to individual inserts on MySQL/SQLite
- **`database::backend()` improved**: Prefers `TideConfig::get_database_type()` for MariaDB-awareness before falling back to SeaORM backend detection

## [0.6.0] - 2026-02-21

### Added

- **`require_db()` function**: Non-panicking alternative to `db()` — returns `Result<&Database>` instead of panicking when the global connection is not initialized. Exported from `tideorm::require_db`
- **Batch insert via `QueryExecutor::insert_many`**: New internal method using a single multi-row INSERT statement, reducing database round trips from O(n) to O(1)
- **Structured logging macros**: `tide_info!`, `tide_warn!`, `tide_debug!` for consistent `[TideORM]`-prefixed log output across all modules
- **Primary key column in derive macro**: `primary_key_column()` now returns the actual primary key column, enabling proper `last()` ordering by PK descending

### Improved

- **Tuple registration expanded to 200**: `RegisterMigrations`, `RegisterSeeds`, and `RegisterModels` now support tuples of up to 200 types (previously limited to 12–16). Refactored from hand-written impls to recursive macros
- **`insert_all()` uses batch insert**: Single multi-row INSERT with automatic fallback to individual inserts if the backend doesn't support `INSERT ... RETURNING`
- **`last()` orders by primary key DESC**: Previously returned an arbitrary first record; now correctly returns the last record by primary key
- **`raw_json()` column extraction**: Improved type priority chain (bool before int, nullable-first) for more accurate JSON output
- **Replaced `lazy_static!` with `parking_lot::RwLock` const init**: In `logging` and `profiling` modules for simpler, zero-overhead static initialization
- **Better panic message for `db()`**: Now mentions `Database::set_global()` and suggests `try_db()` as alternative

### Changed

- **All `db()` calls replaced with `require_db()?`**: Throughout `model.rs`, `query.rs`, `migration.rs`, `seeding.rs`, `schema.rs`, `database.rs`, and macro-generated code — these now return descriptive errors instead of panicking
- **All `eprintln!` replaced with structured logging**: Consistent `[TideORM]`/`[TideORM WARN]`/`[TideORM DEBUG]` prefixed output across sync, migration, seeding, config, and query modules
- **Derive macro lint suppression narrowed**: From blanket `clippy::all` to specific `clippy::derivable_impls`, `clippy::enum_variant_names`, `clippy::redundant_closure`
- **Removed blanket `#![allow(dead_code, unused_imports)]`** from `internal/mod.rs` — now uses targeted `#[allow(unused_imports)]` on the specific import block

### Fixed

- **Attachment detach safety**: Uses `if let Some(first)` instead of `unwrap()` in `detach()` logic
- **OrBranch single-condition safety**: Uses `if let Some(condition)` instead of `unwrap()` in `OrBranchBuilder`
- **Migration rollback safety**: `match applied.last()` with early return instead of `unwrap()` when no migrations are applied
- **Seed rollback safety**: `match executed.last()` with early return instead of `unwrap()` when no seeds are executed
- **Changelog date typos**: Corrected years from 2026 to 2025 for historical entries (0.1.0, 0.4.3, 0.4.4, 0.4.5)

### Dependencies

- `sea-orm`: 2.0.0-rc.30 → 2.0.0-rc.32
- `sea-query`: 1.0.0-rc.30 → 1.0.0-rc.31
- `uuid`: 1.19.0 → 1.21.0
- `getrandom`: 0.3.4 → 0.4.1
- Plus transitive dependency updates

## [0.5.0] - 2025-07-22

### Improved

- **Zero clippy warnings**: Resolved all clippy warnings across lib, macros, tests, and benchmarks
- **Macro code quality**: `Model` derive macro no longer emits `needless_update` (`..Default::default()`) when structs have no relation fields — generates cleaner, more idiomatic output
- **Regex performance**: `highlight_text()` in fulltext module pre-compiles regex patterns outside the loop instead of re-creating them per word
- **Config access optimization**: Config accessor methods (`get_languages`, `get_fallback_language`, etc.) now read directly from the `RwLock` without cloning the entire `Config` struct
- **Macro lint fixes**: Converted `match` single-arm patterns to `if let`, removed useless `.into()` conversion, replaced `i.to_string() == "created_at"` comparisons with direct ident comparison

### Changed

- **`LogLevel::from_str()` → `LogLevel::parse_str()`**: Renamed to avoid confusion with `std::str::FromStr` trait (clippy `should_implement_trait`)
- **`CastType::from_str()` → `CastType::parse_str()`**: Same rename for consistency
- **`CommaSeparated::to_string()`** inherent method removed — `Display` trait implementation provides this automatically
- **`sort_seeds_by_priority_and_deps()`** return type changed from `Vec<&Box<dyn Seed>>` to `Vec<&dyn Seed>` (clippy `borrowed_box`)

### Fixed

- **`identity_map` in relations**: Removed `.map(|c| c)` identity mapping
- **Profiling scoring**: Combined identical UPDATE/DELETE score branches
- **Test assertions**: Replaced `assert!(true)` placeholders with proper empty test bodies
- **Benchmark code quality**: Fixed `iter().count()` → `.len()`, range loops → iterators, redundant closures, unnecessary borrows, redundant match guards

### Dependencies

- Updated all transitive dependencies to latest Rust 1.85-compatible versions
- `uuid`: 1.19.0 → 1.20.0
- `proc-macro2`: 1.0.105 → 1.0.106
- `quote`: 1.0.43 → 1.0.44
- Plus 40+ transitive dependency updates

## [0.4.5] - 2025-01-17

### Added

#### Record Tokenization

Note: The original tokenization implementation described below was replaced in 0.7.2 by XChaCha20-Poly1305 authenticated encryption with randomized nonces. These notes remain here as historical release context.

- **New `#[tideorm(tokenize)]` attribute**: Enable tokenization on any model with a single attribute
- **Secure ID encryption**: Convert record IDs to encrypted, URL-safe tokens via `Tokenizable` trait
- **Model-specific tokens**: Tokens include model name in HMAC, preventing cross-model token reuse
- **Tamper detection**: HMAC verification ensures tokens haven't been modified
- **Instance methods**: `user.tokenize()`, `user.to_token()`, `user.regenerate_token()`
- **Static methods**: `User::tokenize_id(42)`, `User::detokenize(&token)`, `User::decode_token(&token)`
- **Async fetch**: `User::from_token(&token).await` - decode and fetch in one call
- **Configuration hierarchy**: Default → TideConfig → Model (most specific wins)
- **Global encryption key**: Configure via `TokenConfig::set_encryption_key("your-secret-key")`
- **Custom encoders/decoders**: Use `TokenConfig::set_encoder()` and `set_decoder()` for custom strategies
- **Model-level overrides**: Implement `Tokenizable` trait manually for custom logic
- **URL-safe output**: Base64-URL encoding (A-Za-z0-9-_) safe for URLs without escaping
- **New error types**: `TideError::Tokenization` and `TideError::InvalidToken` for clear error handling
- New types exported: `TokenConfig`, `TokenEncoder`, `TokenDecoder`, `Tokenizable`
- New comprehensive example: `examples/tokenization_demo.rs`
- New test file: `tests/tokenization_tests.rs` (48 tests total)
- New benchmarks: `benches/tokenization_benchmarks.rs`

### Example

```rust
use tideorm::prelude::*;

#[derive(Model)]
#[tideorm(table = "users", tokenize)]  // Just add `tokenize` here!
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
}

// Configure encryption key once
TokenConfig::set_encryption_key("my-super-secret-key-at-least-32-chars");

// Tokenize a record
let user = User::find(1).await?.unwrap();
let token = user.tokenize()?;  // "iIBmdKYhJh4_vSKFlBTP..."

// Decode token to ID
let id = User::detokenize(&token)?;  // 1

// Or fetch directly from token
let same_user = User::from_token(&token).await?;

// Tokens are model-specific
let user_token = User::tokenize_id(1)?;
let product_token = Product::tokenize_id(1)?;
assert_ne!(user_token, product_token);  // Different!

// Cross-model decoding fails
assert!(User::detokenize(&product_token).is_err());
```

#### Strongly-Typed Column Support

- **Auto-generated typed columns**: `#[tideorm::model]` now generates a `{Model}Columns` struct with typed column accessors
- **Access columns via model attribute**: `User::columns.name`, `User::columns.age`, etc.
- **Unified query methods**: All query methods now accept both strings AND typed columns:
  - WHERE clauses: `where_eq`, `where_not`, `where_gt`, `where_gte`, `where_lt`, `where_lte`, `where_like`, `where_not_like`, `where_in`, `where_not_in`, `where_null`, `where_not_null`, `where_between`
  - OR conditions: `or_where_eq`, `or_where_not`, `or_where_gt`, `or_where_gte`, `or_where_lt`, `or_where_lte`, `or_where_like`, `or_where_in`, `or_where_not_in`, `or_where_null`, `or_where_not_null`, `or_where_between`
  - AND within OR branches: `and_where_eq`, `and_where_not`, `and_where_gt`, `and_where_gte`, `and_where_lt`, `and_where_lte`, `and_where_like`, `and_where_in`, `and_where_not_in`, `and_where_null`, `and_where_not_null`, `and_where_between`
  - ORDER BY: `order_by`, `order_asc`, `order_desc`
  - GROUP BY: `group_by`
  - Aggregations: `sum`, `avg`, `min`, `max`, `count_distinct`
  - HAVING: `having_sum_gt`, `having_avg_gt`
  - Window functions: `partition_by`, `order_by` (in `WindowFunctionBuilder`)
- **IDE autocomplete support**: Type `User::columns.` to see all available columns with their types
- **Compile-time type safety**: Wrong column names caught at compile time when using typed columns
- **`IntoColumnName` trait**: New trait allows any type implementing it to be used as a column name

### Example

```rust
// All of these work with the SAME methods:
User::query().where_eq("active", true)                      // String-based (runtime checked)
User::query().where_eq(User::columns.active, true)          // Typed column (compile-time checked)

// Works for all query methods:
User::query()
    .where_eq(User::columns.status, "active")
    .where_gt(User::columns.age, 18)
    .order_by(User::columns.created_at, Order::Desc)
    .group_by(User::columns.role)
    .get()
    .await?;
```

## [0.4.4] - 2025-01-16

### Added

#### File Attachment URL Generation

- **Field name context**: URL generators now receive the field name (e.g., "thumbnail", "avatar") for context-aware routing
- **Full metadata access**: URL generators also receive full `FileAttachment` struct with all metadata
- **Global base URL**: Configure via `TideConfig::file_base_url("https://cdn.example.com")`
- **Custom URL generators**: Use `TideConfig::file_url_generator(fn(field_name, file) -> String)` for smart URL routing
- **Model-specific overrides**: Override `file_url_generator()` in `ModelMeta` for per-model customization
- **Automatic URL in JSON**: `to_json()` now includes `url` field in file attachments
- **Manual URL generation**: `Config::generate_file_url()`, `Model::generate_file_url()`, `FileAttachment::url()`
- **FileUrlGenerator type**: Exported in prelude for custom generator functions
- New comprehensive example: `examples/attachment_url_demo.rs` with 24 test cases

### Changed

- **BREAKING**: `FileUrlGenerator` signature is now `fn(field_name: &str, file: &FileAttachment) -> String`
  - Migration: Change `|file| format!("...{}", file.key)` to `|_field_name, file| format!("...{}", file.key)`
  - Or use field_name: `|field_name, file| match field_name { "thumbnail" => ..., _ => ... }`
  - Benefit: Route URLs based on field type (thumbnails to image CDN, videos to streaming, etc.)

## [0.4.3] - 2025-01-14

### Added

#### Comprehensive OR Conditions Support

- **Simple OR methods**: `or_where_eq`, `or_where_not`, `or_where_gt`, `or_where_gte`, `or_where_lt`, `or_where_lte`, `or_where_like`, `or_where_not_like`, `or_where_in`, `or_where_not_in`, `or_where_null`, `or_where_not_null`, `or_where_between`
- **Fluent OR API**: `begin_or()` / `end_or()` for grouped OR conditions
- **AND chaining within OR groups**: `and_where_eq`, `and_where_not`, `and_where_gt`, `and_where_gte`, `and_where_lt`, `and_where_lte`, `and_where_like`, `and_where_in`, `and_where_not_in`, `and_where_null`, `and_where_not_null`, `and_where_between`
- Multiple sequential OR groups support for complex business logic
- New comprehensive example: `examples/where_and_or_demo.rs` with 50+ test cases

### Fixed

- Fixed critical bug where `or_groups` were not being applied to queries in `get()`, `first()`, `count()`, `delete()`, `count_distinct()`, and `aggregate_f64()` methods

## [0.1.0][0.1.0] - 2025-01-08

### 🎉 Initial Release

This is the first public release of TideORM, a developer-friendly ORM for Rust with clean, expressive syntax.

### Added

#### Core ORM

- `#[derive(Model)]` macro for defining models
- Global database configuration via `TideConfig`
- Connection pooling with configurable min/max connections
- Support for PostgreSQL (MySQL and SQLite planned)

#### CRUD Operations

- `Model::create()` - Create new records
- `Model::find()` / `Model::find_or_fail()` - Find by ID
- `Model::all()` - Get all records
- `Model::first()` / `Model::last()` - Get first/last record
- `Model::count()` - Count records
- `Model::exists()` - Check if record exists
- `model.update()` - Update existing records
- `model.delete()` / `Model::destroy()` - Delete records
- `model.reload()` - Refresh from database

#### Relations

- `#[belongs_to]` - Define belongs-to relationships
- `#[has_one]` - Define has-one relationships
- `#[has_many]` - Define has-many relationships
- `load_belongs_to()`, `load_has_one()`, `load_has_many()` - Eager loading

#### Query Builder

- Fluent query interface via `Model::query()`
- WHERE conditions: `where_eq`, `where_not`, `where_like`, `where_in`, `where_null`, `where_not_null`, `where_gt`, `where_lt`, `where_gte`, `where_lte`, `where_between`
- Ordering: `order_by`, `order_asc`, `order_desc`
- Pagination: `limit`, `offset`, `page`, `paginate`
- JOINs: `inner_join`, `left_join`, `right_join`, `inner_join_as`, `left_join_as`
- Aggregations: `sum`, `avg`, `min`, `max`, `count`, `count_distinct`
- Scopes: `scope`, `when`, `when_some`

#### PostgreSQL Features

- JSON/JSONB column support
- Array column support (`Vec<T>`)
- `where_json_contains` - Query JSON fields
- `where_json_key_exists` / `where_json_key_not_exists` - Check JSON keys
- `where_array_contains` - Query array fields
- `where_array_overlaps` - Array overlap queries

#### Migrations

- `Migration` trait for defining migrations
- `Schema` builder for creating/altering tables
- `TableBuilder` with column types (id, string, text, integer, bigint, boolean, timestamp, json, etc.)
- Index creation and management
- `Migrator` for running migrations
- Migration tracking in `_migrations` table
- `run()`, `rollback()`, `rollback_steps()`, `reset()`, `refresh()` operations
- `status()` for viewing migration status

#### Schema Generation

- `SchemaGenerator` for generating SQL from models
- `#[index("column")]` macro for defining indexes
- `#[unique_index("column")]` macro for unique indexes
- Database introspection support

#### Soft Deletes

- `#[tideorm(soft_delete)]` attribute
- `with_trashed()` - Include soft-deleted records
- `only_trashed()` - Only soft-deleted records
- Manual restore via setting `deleted_at = None`

#### Upsert Operations

- `Model::insert_or_update()` - Simple upsert
- `Model::on_conflict()` - Advanced upsert with column control
- `update_columns()` - Specify which columns to update
- `update_all_except()` - Update all except specified columns

#### Batch Operations

- `Model::insert_all()` - Bulk insert

#### Transactions

- `Model::transaction()` - Execute operations in a transaction

#### Callbacks

- `before_save` / `after_save`
- `before_create` / `after_create`
- `before_update` / `after_update`
- `before_delete` / `after_delete`
- `before_validation` / `after_validation`

#### JSON Serialization

- `to_json()` - Convert model to JSON
- `collection_to_json()` - Convert collection to JSON array
- `to_hash_map()` - Convert to HashMap
- `#[tideorm(hidden = "field1,field2")]` - Hide fields from JSON output

#### Configuration

- `#[tideorm(table = "name")]` - Custom table name
- `#[tideorm(primary_key)]` - Mark primary key
- `#[tideorm(auto_increment)]` - Auto-increment field
- `#[tideorm(searchable = "fields")]` - Searchable fields
- `#[tideorm(translatable = "fields")]` - Translatable fields
- `#[tideorm(has_one_files = "field")]` - Single file attachment config
- `#[tideorm(has_many_files = "fields")]` - Multiple file attachments config

#### Raw SQL

- `Database::raw()` - Execute raw SQL returning models
- `Database::raw_with_params()` - Raw SQL with parameters
- `Database::execute()` - Execute SQL without return
- `Database::execute_with_params()` - Execute with parameters

### Documentation

- Comprehensive README with quick start guide
- Example files for common use cases
- API documentation

### Examples

- `basic.rs` - Basic CRUD operations
- `postgres_demo.rs` - PostgreSQL features demo
- `postgres_complete.rs` - Complete feature showcase
- `query_builder.rs` - Query builder examples
- `upsert_demo.rs` - Upsert operations
- `migrations.rs` - Migration examples
- `migration_test_runner.rs` - Migration test suite
- `attachments_translations_demo.rs` - File attachments and translations demo

#### File Attachments System

- New `attachments` module with `HasAttachments` trait
- `attach(relation, file_key)` - Attach a single file to a relation
- `attach_many(relation, file_keys)` - Attach multiple files at once (hasMany only)
- `attach_with_metadata(relation, FileAttachment)` - Attach with custom metadata
- `detach(relation, file_key)` - Detach a specific file or all files
- `detach_many(relation, file_keys)` - Detach multiple files at once
- `sync(relation, file_keys)` - Replace all files in a relation
- `sync_with_metadata(relation, attachments)` - Sync with custom metadata
- `get_file(relation)` - Get single file attachment (hasOne)
- `get_files(relation)` - Get all file attachments (hasMany)
- `has_files(relation)` / `count_files(relation)` - Check/count attachments
- `FileAttachment` struct with key, filename, created_at, and optional metadata
- `FilesData` container for managing attachment data

#### Translations System

- New `translations` module with `HasTranslations` trait
- Translations stored in JSONB format: `{field: {lang: value}}`
- `set_translation(field, lang, value)` - Set a translation for a field
- `set_translations(field, translations)` - Set multiple translations at once
- `sync_translations(field, translations)` - Replace all translations for a field
- `get_translation(field, lang)` - Get translation for specific language
- `get_translated(field, lang)` - Get translation with fallback chain
- `get_all_translations(field)` - Get all translations for a field
- `get_translations_for_language(lang)` - Get all fields for a language
- `remove_translation(field, lang)` - Remove a specific translation
- `remove_field_translations(field)` - Remove all translations for a field
- `clear_translations()` - Clear all translations
- `has_translation(field, lang)` / `has_any_translation(field)` - Check translations
- `available_languages(field)` - Get languages available for a field
- `to_translated_json(options)` - Convert to JSON with translations applied
- `to_json_with_all_translations()` - Get JSON including all translations
- `TranslationInput` helper for processing API/form data
- `ApplyTranslations` trait for bulk applying translations
- Configurable fallback language chain

#### Testing

- 269 unit tests covering all modules
- Comprehensive test coverage for attachments and translations
- Extended trait implementation tests with mock models
- Edge case tests (Unicode, RTL languages, long text, special characters)
- `attachments_translations_benchmarks.rs` - Performance benchmarks

---

## Links

- **Website:** [https://tideorm.com](https://tideorm.com)
- **Repository:** [https://github.com/mohamadzoh/tideorm](https://github.com/mohamadzoh/tideorm)
- **Documentation:** See README.md and examples/

[Unreleased]: https://github.com/mohamadzoh/tideorm/compare/v0.8.5...HEAD
[0.8.5]: https://github.com/mohamadzoh/tideorm/compare/v0.8.4...v0.8.5
[0.8.4]: https://github.com/mohamadzoh/tideorm/compare/v0.8.1...v0.8.4
[0.8.1]: https://github.com/mohamadzoh/tideorm/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/mohamadzoh/tideorm/compare/v0.7.3...v0.8.0
[0.7.3]: https://github.com/mohamadzoh/tideorm/compare/v0.7.2...v0.7.3
[0.7.2]: https://github.com/mohamadzoh/tideorm/compare/v0.7.0...v0.7.2
[0.1.0]: https://github.com/mohamadzoh/tideorm/releases/tag/v0.1.0
