# TideORM Examples

This directory contains example applications demonstrating various TideORM features organized by category.

## Prerequisites

Before running any examples, ensure you have:

1. **Database Configuration**: Create a `.env` file in the project root:
   ```
   POSTGRESQL_DATABASE_URL=postgres://postgres:postgres@localhost:5432/test_tide_orm
   ```
   
2. PostgreSQL running with a database named `test_tide_orm`

> **Note**: All examples now load database configuration from the `.env` file, just like the tests.
> See [.env.example](.env.example) for a template or [TEST_CONFIG.md](../TEST_CONFIG.md) for details.

### Quick PostgreSQL Setup

```bash
# Create database
psql -U postgres -c "CREATE DATABASE test_tide_orm;"

# Or use Docker
docker run --name postgres-tideorm \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=test_tide_orm \
  -p 5432:5432 \
  -d postgres:latest
```

## Examples by Category

### 🌊 Basic Operations (CRUD)

Learn the fundamentals of TideORM with simple create, read, update, and delete operations.

**[basic.rs](basic.rs)** - Core CRUD operations
- Model definition with indexes
- Creating and saving records
- Finding and querying records
- Updating records
- Deleting records

```bash
cargo run --example basic
```

---

### 🔍 Query Building

Master the fluent query builder API for complex queries.

**[query_builder.rs](query_builder.rs)** - Advanced querying
- WHERE conditions (eq, not, gt, lt, like, in, null)
- Ordering and pagination
- Limiting and offsetting results
- Counting records
- Complex condition chaining

```bash
cargo run --example query_builder
```

---

### 🔄 Upsert Operations

Handle insert-or-update scenarios with conflict resolution.

**[upsert_demo.rs](upsert_demo.rs)** - Upsert/on-conflict handling
- Simple upsert with `insert_or_update()`
- Composite key conflicts
- Advanced control with `on_conflict()` builder
- Selective column updates

```bash
cargo run --example upsert_demo
```

---

### 🐘 PostgreSQL Features

Explore PostgreSQL-specific functionality.

**[postgres_demo.rs](postgres_demo.rs)** - PostgreSQL basics
- Global configuration
- Connection pool settings
- PostgreSQL data types
- Basic relationships

```bash
cargo run --example postgres_demo
```

**[postgres_complete.rs](postgres_complete.rs)** - Complete PostgreSQL showcase
- All TideORM features in one place
- JSON/JSONB operations
- Array operations
- Relations (BelongsTo, HasOne, HasMany)
- Transactions
- Callbacks
- Scopes
- Soft deletes
- JOIN operations
- Aggregations

```bash
cargo run --example postgres_complete
```

---

### 🐬 MySQL/MariaDB Features

Explore MySQL and MariaDB-specific functionality.

**[mysql_demo.rs](mysql_demo.rs)** - MySQL database operations
- MySQL connection setup
- JSON column operations (MySQL 5.7+)
- Database feature detection
- CRUD operations
- Aggregations

```bash
cargo run --example mysql_demo --features "mysql runtime-tokio" --no-default-features
```

> **Note**: Requires `MYSQL_DATABASE_URL` environment variable

---

### 🪶 SQLite Features

Use TideORM with SQLite for embedded databases.

**[sqlite_demo.rs](sqlite_demo.rs)** - SQLite database operations
- SQLite connection (file-based)
- JSON1 extension support
- Lightweight database operations
- Development and testing use cases

```bash
cargo run --example sqlite_demo --features sqlite --no-default-features
```

---

## Running Examples by Category

### CRUD Operations
```bash
# Basic CRUD
cargo run --example basic

# With upsert
cargo run --example upsert_demo
```

### Query Building
```bash
# Query builder
cargo run --example query_builder

# Complex queries (in postgres_complete)
cargo run --example postgres_complete
```

### PostgreSQL Specific
```bash
# Basic PostgreSQL features
cargo run --example postgres_demo

# All PostgreSQL features
cargo run --example postgres_complete
```

### MySQL/MariaDB
```bash
# Set MySQL connection URL
export MYSQL_DATABASE_URL=mysql://root:@localhost/tideorm_demo

# Run MySQL demo
cargo run --example mysql_demo --features "mysql runtime-tokio" --no-default-features
```

### SQLite
```bash
# Run SQLite demo (creates demo.db file)
cargo run --example sqlite_demo --features "sqlite runtime-tokio" --no-default-features
```

### Advanced Features
```bash
# Relations, transactions, callbacks (in postgres_complete)
cargo run --example postgres_complete
```

---

## Quick Reference

| Example | Category | Features | Run Command |
|---------|----------|----------|-------------|
| `basic.rs` | CRUD | Create, Read, Update, Delete | `cargo run --example basic` |
| `query_builder.rs` | Queries | WHERE, ORDER BY, LIMIT, COUNT | `cargo run --example query_builder` |
| `upsert_demo.rs` | Upsert | insert_or_update, on_conflict | `cargo run --example upsert_demo` |
| `postgres_demo.rs` | PostgreSQL | Config, pool, types, relations | `cargo run --example postgres_demo` |
| `postgres_complete.rs` | All Features | JSON, arrays, JOINs, aggregations | `cargo run --example postgres_complete` |
| `mysql_demo.rs` | MySQL | JSON, aggregations, CRUD | `cargo run --example mysql_demo --features "mysql runtime-tokio" --no-default-features` |
| `sqlite_demo.rs` | SQLite | JSON1, CRUD, embedded DB | `cargo run --example sqlite_demo --features "sqlite runtime-tokio" --no-default-features` |

---

## Learning Path

If you're new to TideORM, we recommend following this order:

1. **Start with basics**: `basic.rs` - Learn CRUD operations
2. **Query building**: `query_builder.rs` - Master the query API
3. **Upsert operations**: `upsert_demo.rs` - Handle conflicts
4. **PostgreSQL features**: `postgres_demo.rs` - Database-specific features
5. **Complete reference**: `postgres_complete.rs` - See everything together

---

## Troubleshooting

### Connection Errors

If you get connection errors:

1. Check PostgreSQL is running:
   ```bash
   psql -U postgres -c "SELECT version();"
   ```

2. Verify database exists:
   ```bash
   psql -U postgres -c "\l" | grep test_tide_orm
   ```

3. Check your `.env` file has the correct `POSTGRESQL_DATABASE_URL`

### Compilation Errors

Make sure you're in the workspace root:
```bash
cd /path/to/tideorm
cargo run --example basic
```

---

## Contributing Examples

To add a new example:

1. Create `examples/your_example.rs`
2. Add documentation header with:
   - Description
   - Category
   - Features demonstrated
   - Run command
3. Update this README with your example
4. Ensure it runs with `cargo run --example your_example`

---

## Additional Resources

- [Main README](../README.md) - Full TideORM documentation
- [Test Configuration](../TEST_CONFIG.md) - Database setup guide
- [API Documentation](https://docs.rs/tideorm) - Complete API reference
