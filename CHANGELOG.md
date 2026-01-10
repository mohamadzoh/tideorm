# Changelog

All notable changes to TideORM will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-01-08

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
- `#[tide(soft_delete)]` attribute
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
- `#[tide(hidden = "field1,field2")]` - Hide fields from JSON output

#### Configuration
- `#[tide(table = "name")]` - Custom table name
- `#[tide(primary_key)]` - Mark primary key
- `#[tide(auto_increment)]` - Auto-increment field
- `#[tide(searchable = "fields")]` - Searchable fields
- `#[tide(translatable = "fields")]` - Translatable fields
- `#[tide(has_one_files = "field")]` - Single file attachment config
- `#[tide(has_many_files = "fields")]` - Multiple file attachments config

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

[Unreleased]: https://github.com/mohamadzoh/tideorm/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/mohamadzoh/tideorm/releases/tag/v0.1.0
