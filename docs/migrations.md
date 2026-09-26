# Migrations

Use migrations for schema changes in deployed environments. The schema builder reference below covers the column helpers TideORM provides.

### Schema Builder Column Types

The migration schema builder provides convenience methods for all common column types:

#### Numeric Types

```rust
t.small_integer("count");              // SMALLINT
t.integer("quantity");                 // INTEGER  
t.big_integer("total");                // BIGINT
t.float("rate");                       // REAL/FLOAT
t.double("precise_rate");              // DOUBLE PRECISION
t.decimal("price");                    // DECIMAL(10,2)
t.decimal_with("amount", 16, 4);       // DECIMAL(16,4)
```

#### String Types

```rust
t.string("name");                      // VARCHAR(255)
t.text("description");                 // TEXT (LONGTEXT on MySQL)
```

#### Boolean

```rust
t.boolean("active");                   // BOOLEAN
```

#### Date/Time Types

```rust
t.date("birth_date");                  // DATE
t.time("start_time");                  // TIME (TIME(6) on MySQL)
t.datetime("logged_at");               // TIMESTAMP (Postgres) / DATETIME(6) (MySQL)
t.timestamp("created_at");             // TIMESTAMP (without timezone) / DATETIME(6) (MySQL)
t.timestamptz("expires_at");           // TIMESTAMPTZ (with timezone) / DATETIME(6) (MySQL)
```

#### Special Types

```rust
t.uuid("external_id");                 // UUID (Postgres) / BINARY(16) (MySQL)
t.json("metadata");                    // JSON
t.jsonb("data");                       // JSONB (PostgreSQL only)
t.binary("file_data");                 // BYTEA / LONGBLOB (MySQL) / BLOB (SQLite)
t.integer_array("tag_ids");            // INTEGER[] (PostgreSQL only)
t.text_array("tags");                  // TEXT[] (PostgreSQL only)
```

#### Convenience Methods

```rust
t.id();                                // BIGSERIAL PRIMARY KEY (auto-increment)
t.big_increments("id");                // Same as id()
t.increments("id");                    // INTEGER PRIMARY KEY (auto-increment)
t.foreign_id("user_id");               // BIGINT (for foreign keys)
t.timestamps();                        // created_at + updated_at (TIMESTAMPTZ)
t.timestamps_naive();                  // created_at + updated_at (TIMESTAMP, no tz)
t.soft_deletes();                      // deleted_at (nullable TIMESTAMPTZ)
```

#### MySQL and MariaDB

Every timestamp helper renders `DATETIME(6)` there: `TIMESTAMP` only spans 1970–2038 and a column without fractional digits drops microseconds, and a `CURRENT_TIMESTAMP` default is raised to `CURRENT_TIMESTAMP(6)` to match. `text()` and `binary()` render `LONGTEXT`/`LONGBLOB`, because `TEXT` and `BLOB` stop at 64 KB; a default on one of them, or on a JSON column, is written as the parenthesized expression MySQL requires. MySQL cannot index a `TEXT` column without a prefix length, so give an indexed or unique string column `string()` (`VARCHAR(255)`). Tables are created `DEFAULT CHARSET=utf8mb4`, whatever the database's own default is.

### Complete Migration Example

```rust
use tideorm::prelude::*;
use tideorm::migration::*;

struct CreateUsersTable;

#[async_trait]
impl Migration for CreateUsersTable {
    fn version(&self) -> &str { "20260115_001" }
    fn name(&self) -> &str { "create_users_table" }

    async fn up(&self, schema: &mut Schema) -> Result<()> {
        schema.create_table("users", |t| {
            t.id();                                    // id BIGSERIAL PRIMARY KEY
            t.string("email").unique().not_null();    // email VARCHAR(255) UNIQUE NOT NULL
            t.string("name").not_null();              // name VARCHAR(255) NOT NULL
            t.text("bio").nullable();                 // bio TEXT NULL
            t.boolean("active").default(true);        // active BOOLEAN DEFAULT true
            t.date("birth_date").nullable();          // birth_date DATE NULL
            t.decimal_with("balance", 12, 2)          // balance DECIMAL(12,2) DEFAULT 0.00
                .default("0.00");
            t.jsonb("preferences").nullable();        // preferences JSONB NULL
            t.timestamptz("email_verified_at")        // email_verified_at TIMESTAMPTZ NULL
                .nullable();
            t.timestamps();                           // created_at, updated_at TIMESTAMPTZ
            t.soft_deletes();                         // deleted_at TIMESTAMPTZ NULL
        }).await?;

        // Add custom index
        schema.create_index("users", "idx_users_email_active", &["email", "active"], false).await?;

        Ok(())
    }

    async fn down(&self, schema: &mut Schema) -> Result<()> {
        schema.drop_table("users").await
    }
}
```

### Matching Model Definition

```rust
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;

#[tideorm::model(table = "users", soft_delete)]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub bio: Option<String>,
    pub active: bool,
    pub birth_date: Option<NaiveDate>,
    pub balance: Decimal,
    pub preferences: Option<serde_json::Value>,
    pub email_verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}
```

### Renaming a Table

```rust
async fn up(&self, schema: &mut Schema) -> Result<()> {
    schema.rename_table("posts", "articles").await
}

async fn down(&self, schema: &mut Schema) -> Result<()> {
    schema.rename_table("articles", "posts").await
}
```

MySQL and MariaDB run `RENAME TABLE`, PostgreSQL and SQLite `ALTER TABLE ... RENAME TO`. The rows, columns and indexes move with the table, and foreign keys in other tables follow it.

---

## Schema Synchronization (Development Only)

TideORM can automatically sync your database schema with your models during development:

```rust
TideConfig::init()
    .database("postgres://localhost/mydb")
    .models_matching("src/models/*")
    .sync(true)  // Enable auto-sync (development only!)
    .connect()
    .await?;
```

`models_matching(...)` uses glob-style source path matching against compiled TideORM models. It is useful when your project keeps models under folders like `src/models/`, but it does not replace normal Rust module wiring.

Or export schema to a file:

```rust
TideConfig::init()
    .database("postgres://localhost/mydb")
    .schema_file("schema.sql")  // Generate SQL file
    .connect()
    .await?;
```

`SchemaWriter::write_schema("schema.sql").await?` writes the same file at any time: it reads every table of the connected database, with its columns, primary key and indexes.

> ⚠️ **Warning**: Do NOT use `sync(true)` in production! Use proper migrations instead.

---

