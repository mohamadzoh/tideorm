# TideORM Multi-Database Testing Guide

This guide explains how to run TideORM integration tests across all supported databases.

## Supported Databases

| Database | Feature Flag | Default Tests |
|----------|-------------|---------------|
| PostgreSQL | `postgres` (default) | ✅ Enabled |
| MySQL/MariaDB | `mysql` | ⏸️ Opt-in |
| SQLite | `sqlite` | ✅ Enabled |

## Quick Start

### 1. PostgreSQL Tests (Default)

```bash
# Using default PostgreSQL database
export POSTGRESQL_DATABASE_URL=postgres://postgres:postgres@localhost:5432/test_tide_orm
cargo test --test postgres_integration_tests
cargo test --test postgres_advanced_tests
```

### 2. MySQL Tests

```bash
# Requires MySQL server and MYSQL_DATABASE_URL
export MYSQL_DATABASE_URL=mysql://root:password@localhost:3306/tideorm_test
cargo test --test mysql_integration_tests --features mysql --no-default-features
```

### 3. SQLite Tests

```bash
# Uses in-memory database by default
cargo test --test sqlite_integration_tests --features sqlite --no-default-features

# Or with a file-based database
export SQLITE_DATABASE_URL=sqlite:./test.db
cargo test --test sqlite_integration_tests --features sqlite --no-default-features
```

## Environment Setup

### Copy the example env file:
```bash
cp .env.example .env
```

### Edit `.env` with your database credentials:
```env
# PostgreSQL (required for default tests)
POSTGRESQL_DATABASE_URL=postgres://user:password@localhost:5432/tideorm_test

# MySQL (optional - enables MySQL tests)
MYSQL_DATABASE_URL=mysql://user:password@localhost:3306/tideorm_test

# SQLite (optional - defaults to in-memory)
SQLITE_DATABASE_URL=sqlite::memory:
```

## Database Setup

### PostgreSQL

```sql
-- Create test database
CREATE DATABASE tideorm_test;

-- Grant permissions (if needed)
GRANT ALL PRIVILEGES ON DATABASE tideorm_test TO your_user;
```

### MySQL

```sql
-- Create test database
CREATE DATABASE tideorm_test CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

-- Grant permissions (if needed)
GRANT ALL PRIVILEGES ON tideorm_test.* TO 'your_user'@'localhost';
FLUSH PRIVILEGES;
```

### SQLite

No setup required! SQLite tests use an in-memory database by default.

## Test Files

| File | Database | Description |
|------|----------|-------------|
| `postgres_integration_tests.rs` | PostgreSQL | Basic CRUD, queries, soft deletes |
| `postgres_advanced_tests.rs` | PostgreSQL | JSON, transactions, advanced features |
| `mysql_integration_tests.rs` | MySQL | Full MySQL test suite |
| `sqlite_integration_tests.rs` | SQLite | Full SQLite test suite |
| `query_builder_tests.rs` | None | Unit tests (no DB required) |
| `unit_tests.rs` | None | Unit tests (no DB required) |

## Running All Tests

### All unit tests (no database required):
```bash
cargo test --lib
cargo test --test unit_tests
cargo test --test query_builder_tests
```

### All integration tests (with PostgreSQL):
```bash
cargo test --test postgres_integration_tests
cargo test --test postgres_advanced_tests
```

### All databases (requires all DBs configured):
```bash
# PostgreSQL
cargo test --test postgres_integration_tests
cargo test --test postgres_advanced_tests

# MySQL
cargo test --test mysql_integration_tests --features mysql --no-default-features

# SQLite
cargo test --test sqlite_integration_tests --features sqlite --no-default-features
```

## Skipping Tests

You can skip specific database tests using environment variables:

```bash
# Skip PostgreSQL tests
export SKIP_POSTGRES_TESTS=1

# Skip MySQL tests
export SKIP_MYSQL_TESTS=1

# Skip SQLite tests
export SKIP_SQLITE_TESTS=1
```

## CI/CD Integration

### GitHub Actions Example

```yaml
name: Tests

on: [push, pull_request]

jobs:
  unit-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --lib
      - run: cargo test --test unit_tests
      - run: cargo test --test query_builder_tests

  postgres-tests:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:15
        env:
          POSTGRES_USER: postgres
          POSTGRES_PASSWORD: postgres
          POSTGRES_DB: tideorm_test
        ports:
          - 5432:5432
        options: >-
          --health-cmd pg_isready
          --health-interval 10s
          --health-timeout 5s
          --health-retries 5
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --test postgres_integration_tests
        env:
          POSTGRESQL_DATABASE_URL: postgres://postgres:postgres@localhost:5432/tideorm_test

  mysql-tests:
    runs-on: ubuntu-latest
    services:
      mysql:
        image: mysql:8
        env:
          MYSQL_ROOT_PASSWORD: password
          MYSQL_DATABASE: tideorm_test
        ports:
          - 3306:3306
        options: >-
          --health-cmd "mysqladmin ping"
          --health-interval 10s
          --health-timeout 5s
          --health-retries 5
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --test mysql_integration_tests --features mysql --no-default-features
        env:
          MYSQL_DATABASE_URL: mysql://root:password@localhost:3306/tideorm_test

  sqlite-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --test sqlite_integration_tests --features sqlite --no-default-features
```

## Feature Flags

TideORM uses feature flags to control database support:

```toml
[features]
default = ["postgres"]
postgres = ["sqlx/postgres"]
mysql = ["sqlx/mysql"]
sqlite = ["sqlx/sqlite"]
all-databases = ["postgres", "mysql", "sqlite"]
```

### Compiling with multiple databases:
```bash
cargo build --features "postgres mysql sqlite"
# or
cargo build --features all-databases
```

## Test Coverage

All integration test suites cover:

- ✅ Database connection and ping
- ✅ CRUD operations (Create, Read, Update, Delete)
- ✅ Query builder (where, ordering, pagination, aggregations)
- ✅ Bulk operations (bulk delete, batch insert)
- ✅ Soft deletes (soft delete, restore, with_trashed, only_trashed)
- ✅ JSON operations (storage, retrieval, native queries)
- ✅ First and first_or_fail methods
- ✅ Count and exists methods
- ✅ Pattern matching (LIKE, IN, BETWEEN)

## Troubleshooting

### PostgreSQL connection refused
```
Error: connection refused
```
- Ensure PostgreSQL is running: `pg_isready`
- Check the connection string in `.env`

### MySQL access denied
```
Error: Access denied for user
```
- Verify username and password
- Ensure user has permissions on the test database

### SQLite file permission error
```
Error: unable to open database file
```
- Check file path permissions
- Try using in-memory database: `sqlite::memory:`

### Feature flag errors
```
Error: unresolved import `sqlx::mysql`
```
- Ensure you're using the correct feature flag:
  ```bash
  cargo test --features mysql --no-default-features
  ```
