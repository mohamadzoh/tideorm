# Getting Started

## Configuration

### Basic Connection

```rust
// Simple connection
Database::init("postgres://localhost/mydb").await?;

// With TideConfig (recommended)
TideConfig::init()
    .database("postgres://localhost/mydb")
    .connect()
    .await?;
```

### Pool Configuration

```rust
TideConfig::init()
    .database("postgres://localhost/mydb")
    .max_connections(20)        // Maximum pool size
    .min_connections(5)         // Minimum idle connections
    .connect_timeout(Duration::from_secs(10))
    .idle_timeout(Duration::from_secs(300))
    .max_lifetime(Duration::from_secs(3600))
    .connect()
    .await?;
```

The two timeouts are independent. `connect_timeout` bounds each attempt to open a connection, including socket setup, TLS, handshake, authentication, and driver initialization hooks, both on demand and for `min_connections`. `acquire_timeout` bounds a whole checkout: waiting for a free slot, pinging, hooks, and any connecting. Whichever deadline expires first ends the wait; increasing `connect_timeout` does not extend the wait for a slot.

`pool(..)` replaces every pool setting at once, such as a `PoolConfig` shared with `Database::builder`; `PoolConfig` has a setter for each setting:

```rust
let pool = PoolConfig::default()
    .max_connections(20)
    .acquire_timeout(Duration::from_secs(2));

TideConfig::init()
    .database("postgres://localhost/mydb")
    .pool(pool)
    .connect()
    .await?;
```

To shut down cleanly, `db.close().await?` waits for connections in use to come back and closes the pool (SQLite then releases its file); every clone of the handle shares the pool, so statements on them fail afterwards.

### Driver Options

A URL names the common settings. For the rest, such as PostgreSQL's `application_name` or SQLite's journal mode, give `Database::builder` the driver's own options instead of a URL:

```rust
let db = Database::builder()
    .postgres_options(
        PgConnectOptions::new()
            .host("localhost")
            .database("mydb")
            .application_name("billing")
            .options([("statement_timeout", "5s")]),
    )
    .max_connections(20)
    .build()
    .await?;
```

`mysql_options` (MySQL or MariaDB) and `sqlite_options` take `MySqlConnectOptions` and `SqliteConnectOptions` the same way; each needs its backend's feature. The builder's `{:?}` leaves the options out, since they hold the password.

Each connection keeps the statements it prepares, up to 100, dropping the least recently used. `Database::builder().statement_cache_capacity(n)` changes that on every backend, URL or driver options alike; with `0` no statement is kept and each is prepared for its own run, which a transaction-pooling proxy such as PgBouncer needs.

### Pinned Sessions

A pool hands each statement whichever connection is free, so what one statement sets for its session (`SET`, `PRAGMA`, MySQL's `GET_LOCK`) may not hold for the next. `pin_session()` opens a connection of its own, with the pool's settings but outside its slots, and every statement through the returned handle runs on it, transactions included:

```rust
let session = db.pin_session().await?;
session.exec_raw("SET FOREIGN_KEY_CHECKS = 0").await?;
// ... statements that rely on that setting ...
session.close().await?;
```

The connection closes with `close()` or once the last clone of the handle drops, and is never replaced: once it is lost, the handle's statements fail.

### Database Types

```rust
TideConfig::init()
    .database_type(DatabaseType::Postgres)  // or MySQL, SQLite
    .database("postgres://localhost/mydb")
    .connect()
    .await?;
```

### Resetting Global State

TideORM keeps its active database handle, global configuration, and tokenization settings in process-global state so models can run without threading a connection through every call. That state is now intentionally resettable, which is useful for tests, benchmarks, and applications that need to reconfigure TideORM during the same process lifetime.

```rust
use tideorm::prelude::*;

Database::reset_global();
TideConfig::reset(); // the tokenization key and codecs too

TideConfig::init()
    .database("sqlite::memory:")
    .database_type(DatabaseType::SQLite)
    .connect()
    .await?;
```

`TideConfig::apply()`, `TideConfig::connect()` and `TideConfig::connect_only()` overwrite previously applied TideORM configuration. If a new configuration omits the database type, the stored type is cleared instead of leaving the old backend classification behind. `connect_only()` connects without the startup work the configuration enables (migrations, schema sync, seeds, the schema file), which is how the migration and seed runners `tideorm init` scaffolds connect with the application's own configuration.

### Environment Variables

TideORM reads these from the process environment; a boolean flag is on only when set to `1` or `true`. The connection URL is not one of them: pass it to `.database(..)` yourself, from `DATABASE_URL` or wherever your configuration keeps it.

| Variable | Effect |
|---|---|
| `TIDE_LOG_QUERIES` | Print every statement to stderr (see [Query Logging](queries.md#query-logging)). Read once, at the first statement. |
| `TIDE_LOG_LEVEL` | The `QueryLogger` level: `error`, `warn`, `info`, `debug` or `trace`. |
| `TIDE_SLOW_QUERY_MS` | The slow-query threshold, in milliseconds. |
| `TIDE_LOG_SEEDS` | Log each seeder as it runs and rolls back. |
| `TIDEORM_ENV` | `production` (any case) makes `force_sync(true)` fail before it drops anything. |
| `TIDEORM_ALLOW_FORCE_SYNC` | Lets `force_sync(true)` run even when `TIDEORM_ENV` is `production`. |

---

## Data Type Mappings

### Rust to SQL Type Reference

| Rust Type | PostgreSQL | MySQL | SQLite | Notes |
|-----------|------------|-------|--------|-------|
| `i8` | SMALLINT | SMALLINT | INTEGER | |
| `i16` | SMALLINT | SMALLINT | INTEGER | |
| `i32` | INTEGER | INT | INTEGER | |
| `i64` | BIGINT | BIGINT | INTEGER | Recommended for primary keys |
| `u8` | SMALLINT | TINYINT UNSIGNED | INTEGER | |
| `u16` | INTEGER | SMALLINT UNSIGNED | INTEGER | |
| `u32` | BIGINT | INT UNSIGNED | INTEGER | |
| `u64` | BIGINT | BIGINT UNSIGNED | INTEGER | PostgreSQL and SQLite store up to `i64::MAX`; a larger value is refused when it is written |
| `f32` | REAL | FLOAT | REAL | |
| `f64` | DOUBLE PRECISION | DOUBLE | REAL | |
| `bool` | BOOLEAN | TINYINT(1) | INTEGER | |
| `String` | TEXT | LONGTEXT | TEXT | Schema sync creates a MySQL key or indexed column as `VARCHAR(255)`, because MySQL cannot index `TEXT` without a prefix length |
| `Option<T>` | (nullable) | (nullable) | (nullable) | Wraps any type to make it nullable |
| `uuid::Uuid` | UUID | BINARY(16) | TEXT | MySQL binds a UUID as 16 raw bytes, so the column must be `BINARY(16)` |
| `rust_decimal::Decimal` | DECIMAL | DECIMAL(65,30) | REAL | SQLite has no exact decimal type, and a REAL keeps about 15 significant digits. A decimal is read as SQLite stores it, so a column declared `TEXT` keeps every digit but compares as text; store minor units to keep both |
| `serde_json::Value` | JSONB | JSON | TEXT | |
| `Vec<u8>` | BYTEA | LONGBLOB | BLOB | Binary data |
| `chrono::NaiveDate` | DATE | DATE | TEXT | Date only |
| `chrono::NaiveTime` | TIME | TIME(6) | TEXT | Time only |
| `chrono::NaiveDateTime` | TIMESTAMP | DATETIME(6) | TEXT | No timezone |
| `chrono::DateTime<Utc>` | **TIMESTAMPTZ** | DATETIME(6) | TEXT | **With timezone** |

On PostgreSQL a field can also be an array: `Vec<i32>`, `Vec<i64>`, `Vec<f64>`, `Vec<bool>`, `Vec<String>` or `Vec<serde_json::Value>`. A field of any other type is a compile error on that field: store an enum as a `String` or an integer, and mark a field that is not a column `#[tideorm(skip)]`.

On MySQL and MariaDB every timestamp is a `DATETIME(6)`: MySQL's `TIMESTAMP` only spans 1970–2038 and a column without fractional digits drops microseconds. The connection runs in UTC, so a `DateTime<Utc>` is stored as its UTC wall time. Each connection also takes `NO_BACKSLASH_ESCAPES` out of its session's SQL mode, since SQL written for MySQL escapes with backslashes. Text and binary columns are `LONGTEXT`/`LONGBLOB` because `TEXT` and `BLOB` stop at 64 KB, and tables are created `DEFAULT CHARSET=utf8mb4` whatever the database default is, so any Rust `String` fits.

### Date and Time Types

TideORM provides proper support for all common date/time scenarios:

#### DateTime with Timezone (Recommended for most cases)

Use `chrono::DateTime<Utc>` for timestamps that should include timezone information:

```rust
use chrono::{DateTime, Utc};

#[tideorm::model(table = "sessions")]
pub struct Session {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub token: String,
    pub expires_at: DateTime<Utc>,        // Maps to TIMESTAMPTZ in PostgreSQL
    pub created_at: DateTime<Utc>,        // Maps to TIMESTAMPTZ in PostgreSQL
    pub updated_at: DateTime<Utc>,        // Maps to TIMESTAMPTZ in PostgreSQL
}
```

In migrations, use `timestamptz()`:

```rust
schema.create_table("sessions", |t| {
    t.id();
    t.big_integer("user_id").not_null();
    t.string("token").not_null();
    t.timestamptz("expires_at").not_null();
    t.timestamps();  // Uses TIMESTAMPTZ by default
}).await?;
```

#### DateTime without Timezone

Use `chrono::NaiveDateTime` when you don't need timezone information:

```rust
use chrono::NaiveDateTime;

#[tideorm::model(table = "logs")]
pub struct Log {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub message: String,
    pub logged_at: NaiveDateTime,         // Maps to TIMESTAMP (no timezone)
}
```

In migrations, use `timestamp()`:

```rust
schema.create_table("logs", |t| {
    t.id();
    t.text("message").not_null();
    t.timestamp("logged_at").default_now();
}).await?;
```

#### Date Only

Use `chrono::NaiveDate` for date-only fields:

```rust
use chrono::NaiveDate;

#[tideorm::model(table = "events")]
pub struct Event {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub event_date: NaiveDate,            // Maps to DATE
}
```

In migrations, use `date()`:

```rust
schema.create_table("events", |t| {
    t.id();
    t.string("name").not_null();
    t.date("event_date").not_null();
}).await?;
```

#### Time Only

Use `chrono::NaiveTime` for time-only fields:

```rust
use chrono::NaiveTime;

#[tideorm::model(table = "schedules")]
pub struct Schedule {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub start_time: NaiveTime,            // Maps to TIME
    pub end_time: NaiveTime,
}
```

In migrations, use `time()`:

```rust
schema.create_table("schedules", |t| {
    t.id();
    t.string("name").not_null();
    t.time("start_time").not_null();
    t.time("end_time").not_null();
}).await?;
```


---

## Examples

See the [TideORM Examples repository](https://github.com/mohamadzoh/tideorm-examples) for complete working examples.

---

## Testing

To test an application built on TideORM, see [Testing](testing.md). The commands below test TideORM itself; use the smallest one that covers your change.

```bash
# Fast library validation
cargo test --lib

# Broad compatibility pass
cargo test --all-features

# Default backend suite
cargo test --features postgres

# SQLite integration suite
cargo test --test sqlite_integration_tests --features "sqlite runtime-tokio" --no-default-features

# PostgreSQL integration suite
cargo test --test postgres_integration_tests

# MySQL integration suite
cargo test --test mysql_integration_tests --features mysql

# MariaDB integration suite
cargo test --test mariadb_integration_tests --features mysql

# SQLite smoke test
cargo test --test sqlite_ci_smoke_test --features "sqlite runtime-tokio" --no-default-features

# Rebuild the book after documentation changes
mdbook build
```

Integration suites may require local database servers and environment variables. When a test needs a fresh TideORM setup, clear global state first with `Database::reset_global()` and `TideConfig::reset()`, which also clears the tokenization settings (`TokenConfig::reset()` clears only those).
