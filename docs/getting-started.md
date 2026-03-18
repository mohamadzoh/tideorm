# Getting Started

This chapter covers initial configuration, type mappings, and the quick reference material that helps new users get productive quickly.

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
TideConfig::reset();
TokenConfig::reset();

TideConfig::init()
    .database("sqlite::memory:")
    .database_type(DatabaseType::SQLite)
    .connect()
    .await?;
```

`TideConfig::apply()` and `TideConfig::connect()` overwrite previously applied TideORM configuration. If a new configuration omits the database type, the stored type is cleared instead of leaving the old backend classification behind.

---

## Data Type Mappings

This section explains how Rust types map to SQL types across different databases, and how to properly configure your models and migrations.

### Rust to SQL Type Reference

| Rust Type | PostgreSQL | MySQL | SQLite | Notes |
|-----------|------------|-------|--------|-------|
| `i8`, `i16` | SMALLINT | SMALLINT | INTEGER | |
| `i32` | INTEGER | INT | INTEGER | |
| `i64` | BIGINT | BIGINT | INTEGER | Recommended for primary keys |
| `u8`, `u16` | SMALLINT | SMALLINT UNSIGNED | INTEGER | |
| `u32` | INTEGER | INT UNSIGNED | INTEGER | |
| `u64` | BIGINT | BIGINT UNSIGNED | INTEGER | |
| `f32` | REAL | FLOAT | REAL | |
| `f64` | DOUBLE PRECISION | DOUBLE | REAL | |
| `bool` | BOOLEAN | TINYINT(1) | INTEGER | |
| `String` | TEXT | TEXT | TEXT | |
| `Option<T>` | (nullable) | (nullable) | (nullable) | Wraps any type to make it nullable |
| `uuid::Uuid` | UUID | CHAR(36) | TEXT | |
| `rust_decimal::Decimal` | DECIMAL | DECIMAL(65,30) | TEXT | |
| `serde_json::Value` | JSONB | JSON | TEXT | |
| `Vec<u8>` | BYTEA | BLOB | BLOB | Binary data |
| `chrono::NaiveDate` | DATE | DATE | TEXT | Date only |
| `chrono::NaiveTime` | TIME | TIME | TEXT | Time only |
| `chrono::NaiveDateTime` | TIMESTAMP | DATETIME | TEXT | No timezone |
| `chrono::DateTime<Utc>` | **TIMESTAMPTZ** | TIMESTAMP | TEXT | **With timezone** |

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

```bash
# Fast library validation
cargo test --lib

# Run the full suite for one backend feature set
cargo test --features postgres

# Run specific test
cargo test query_builder --features postgres

# Cross-backend compile/test coverage
cargo test --all-features

# Rebuild the book after documentation changes
mdbook build
```

The CI matrix covers PostgreSQL, MySQL, and SQLite feature sets. For local work, `cargo test --lib` is the fastest smoke test and `cargo test --all-features` is the broadest compatibility check. When a test needs a fresh TideORM setup, clear global state first with `Database::reset_global()`, `TideConfig::reset()`, and `TokenConfig::reset()`.
