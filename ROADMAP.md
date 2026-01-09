# TideORM Roadmap

This document outlines the planned features and improvements for TideORM.

## Current Version: 0.2.0

### ✅ Completed Features

#### Core ORM
- [x] Model derive macro with `#[derive(Model)]`
- [x] CRUD operations (Create, Read, Update, Delete)
- [x] Global database configuration (`TideConfig`)
- [x] Connection pooling with configurable limits
- [x] Type-safe query builder
- [x] Pagination support
- [x] Soft deletes

#### Relations
- [x] `#[belongs_to]` macro
- [x] `#[has_one]` macro
- [x] `#[has_many]` macro
- [x] Eager loading relations
- [x] Many-to-many relationships (`#[has_many_through]`)
- [x] Polymorphic relations
- [x] Nested eager loading
- [x] Relation constraints

#### Query Builder
- [x] WHERE conditions (eq, not, like, in, null, gt, lt, gte, lte)
- [x] ORDER BY (asc, desc)
- [x] LIMIT and OFFSET
- [x] JOIN operations (INNER, LEFT, RIGHT)
- [x] Aggregations (SUM, AVG, MIN, MAX, COUNT, COUNT DISTINCT)
- [x] GROUP BY and HAVING
- [x] Scopes for reusable query conditions
- [x] Conditional query building (`when`, `when_some`)
- [x] Subqueries
- [x] UNION queries
- [x] Window functions
- [x] CTEs (Common Table Expressions)
- [x] Raw expressions in queries

#### PostgreSQL Features
- [x] JSON/JSONB column support
- [x] Array column support
- [x] JSON queries (`where_json_contains`, `where_json_key_exists`)
- [x] Array queries (`where_array_contains`, `where_array_overlaps`)

#### Database Support
- [x] PostgreSQL full support
- [x] MySQL/MariaDB full support
- [x] SQLite full support
- [x] Database-specific optimizations

#### Migrations
- [x] Migration trait and system
- [x] Create/Alter/Drop tables
- [x] Add/Modify/Remove columns
- [x] Index management
- [x] Migration tracking (`_migrations` table)
- [x] Rollback support (single, multiple, reset)
- [x] Refresh command (reset + run)

#### Schema
- [x] Schema generation from models
- [x] Index definitions (`#[index]`, `#[unique_index]`)
- [x] Database introspection

#### Performance
- [x] Query caching (LRU, FIFO, TTL strategies)
- [x] Prepared statement caching
- [x] Batch update operations
- [x] Bulk delete with conditions

#### Other Features
- [x] Transactions
- [x] Callbacks (before/after save, create, update, delete)
- [x] Batch operations (insert_all)
- [x] Raw SQL queries
- [x] JSON serialization with hidden attributes
- [x] Upsert (insert_or_update, on_conflict)
- [x] Attribute casting
- [x] Better error messages
- [x] Query logging and debugging
- [x] Performance profiling
- [x] Documentation improvements

---

## Version 0.3.0 (Planned)

### Advanced Features
- [ ] Database seeding system
- [ ] Model factories for testing
- [ ] Database events/observers
- [ ] Model validation

### CLI Tool
- [ ] `tideorm migrate` command
- [ ] `tideorm rollback` command
- [ ] `tideorm generate:model` command
- [ ] `tideorm generate:migration` command
- [ ] `tideorm db:seed` command

### Developer Experience
- [x] Better error messages
- [x] Query logging and debugging
- [x] Performance profiling
- [x] Documentation improvements

---

## Version 0.4.0 (Planned)

### Enterprise Features
- [ ] Read replicas support
- [ ] Connection sharding
- [ ] Multi-tenancy support
- [ ] Audit logging
- [ ] Soft delete scopes (global)

### Full-Text Search
- [ ] PostgreSQL full-text search
- [ ] MySQL full-text search
- [ ] Search indexing

---

## Version 1.0.0 (Future)

### Stability
- [ ] API stabilization
- [ ] Comprehensive test coverage
- [ ] Performance benchmarks
- [ ] Production-ready documentation

### Ecosystem
- [ ] Actix-web integration
- [ ] Axum integration
- [ ] Rocket integration
- [ ] Tower middleware

---

## Contributing

We welcome contributions! If you'd like to work on any of these features, please:

1. Check the [issues](https://github.com/mohamadzoh/tideorm/issues) for existing discussions
2. Open a new issue to discuss your proposed changes
3. Submit a pull request

## Feedback

Have suggestions for the roadmap? Open an issue or reach out at [tideorm.com](https://tideorm.com).
