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

#### Any Column Type

`t.column(name, column_type)` takes any `ColumnType`, the type a model field's Rust type maps to (`<T as ValueType>::column_type()`, what schema sync creates), so a migration can declare the column a field gets:

```rust
t.column("visits", ColumnType::Unsigned);                        // BIGINT / INT UNSIGNED / INTEGER
t.column("code", ColumnType::String(StringLen::N(12)));          // VARCHAR(12)
t.column("ratios", ColumnType::Array(Arc::new(ColumnType::Double))); // DOUBLE PRECISION[] / JSON / TEXT
t.column("amount", ColumnType::custom("NUMERIC(20,0)"));         // written as given
```

`tideorm::migration::column_type_sql(&column_type, db_type)` returns the SQL a type renders to.

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
            t.index(&["name"]);                       // idx_users_name
            t.unique_index_named("users_email_key", &["email", "deleted_at"]);
        }).await?;

        // Add custom index
        schema.create_index("users", "idx_users_email_active", &["email", "active"]).await?;

        Ok(())
    }

    async fn down(&self, schema: &mut Schema) -> Result<()> {
        schema.drop_table("users").await
    }
}
```

`index` and `unique_index` name an index after its table and columns (`idx_<table>_<columns>`, with `_unique` for a unique one); `index_named` and `unique_index_named` take the name.

A string default is escaped the way the backend reads a literal, so `default(r"C:\temp\")` stores those backslashes on MySQL and MariaDB too, and a current-time default may be spelled `now()`, `CURRENT_TIMESTAMP` or `LOCALTIMESTAMP` on any backend. A generated index name (`idx_<table>_<columns>`) past 63 bytes is shortened with a hash, as PostgreSQL would otherwise cut two names that share a prefix to one and MySQL refuses them; a model's `#[index]` names are shortened the same way. `create_table_if_not_exists` can run again after a half-applied migration: on MySQL, which has no `CREATE INDEX IF NOT EXISTS`, it skips the indexes the table already has.

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

### Tables in Another Schema

Every table name the schema builder takes may be written `schema.table`: a PostgreSQL schema, or a MySQL or MariaDB database. SQLite keeps the tables in its main database, as schema sync does, and ignores the schema part.

```rust
schema.create_table("billing.invoices", |t| {
    t.id();
    t.foreign_id("account_id").references("billing.accounts", "id");
    t.index(&["account_id"]);
}).await?;
```

Index and foreign key names derive from the table alone (`idx_invoices_account_id`, `fk_invoices_account_id`), and `has_table`/`has_column` look in the schema the name gives. MySQL and MariaDB move a table to another database when `rename_table`'s new name names one; PostgreSQL renames a table within its schema only, and moving one is `ALTER TABLE .. SET SCHEMA` through `raw`.

### Foreign Keys

```rust
schema.create_table("posts", |t| {
    t.id();
    t.foreign_id("user_id")
        .not_null()
        .references("users", "id")
        .on_delete(ForeignKeyAction::Cascade);
    t.big_integer("tenant_id").not_null();
    t.big_integer("editor_id");
    // A key over several columns
    t.foreign(&["tenant_id", "editor_id"], "tenant_users", &["tenant_id", "id"])
        .name("fk_posts_editor")
        .on_delete(ForeignKeyAction::SetNull);
}).await?;
```

A key is written as a table constraint, because MySQL and MariaDB ignore a `REFERENCES` written on a column. Unnamed, it is `fk_<table>_<columns>`, shortened with a hash past 63 bytes like an index name, and that name is what `drop_foreign_key` takes. `on_delete` and `on_update` apply to the key `references` declares, so they follow it. The actions are `Cascade`, `Restrict`, `SetNull`, `SetDefault` (which MySQL's InnoDB refuses) and `NoAction`, the default.

### Altering a Table

```rust
schema.alter_table("users", |t| {
    t.string("nickname");                          // every typed column helper adds a column
    t.foreign_id("team_id").references("teams", "id");
    t.rename_column("name", "full_name");
    t.alter_column("email").not_null();
    t.alter_column("score").set_type(ColumnType::BigInteger).default(0);
    t.change_column("age", ColumnType::BigInteger); // alter_column("age").set_type(..)
    t.alter_column("bio").drop_default();
    t.add_index(&["full_name"]);
    t.add_unique_index(&["nickname"]);              // add_index_named / add_unique_index_named take a name
    t.drop_index("idx_users_legacy");
    t.drop_column("legacy");
}).await?;
```

The changes run in the order they were made. PostgreSQL changes a column's type, nullability or default in place. MySQL and MariaDB restate a column whole with `MODIFY COLUMN`, so `alter_table` first reads the table's `SHOW CREATE TABLE` and keeps what the change leaves alone: its nullability, default, `AUTO_INCREMENT`, comment and, for a column that still holds text, its character set and collation. A change of default alone runs `ALTER COLUMN .. SET DEFAULT` there.

SQLite cannot change a column, or add or drop a foreign key on a table it has, so such an alteration fails before any of its statements runs (see [Rebuilding a SQLite Table](#rebuilding-a-sqlite-table)). A column added with `references` takes its key along on SQLite too, which then wants the column to default to NULL.

### Indexes and Introspection

```rust
schema.create_index("posts", "idx_posts_published", &["published_at"]).await?;
schema.create_unique_index("posts", "idx_posts_slug", &["slug"]).await?;
schema.create_index_if_not_exists("posts", "idx_posts_author", &["author_id"]).await?;
schema.drop_index_if_exists("posts", "idx_posts_old").await?;

if !schema.has_column("posts", "summary").await? {
    schema.alter_table("posts", |t| { t.text("summary"); }).await?;
}
if schema.database_type() == DatabaseType::Postgres {
    schema.raw("CREATE EXTENSION IF NOT EXISTS pg_trgm").await?;
}
```

MySQL has no `CREATE INDEX IF NOT EXISTS`, and neither MySQL nor MariaDB takes `DROP INDEX IF EXISTS` on a table, so there the `if` forms ask the catalog first. `has_table` and `has_column` look in the connection's current schema on PostgreSQL, its database on MySQL and MariaDB, and the main database on SQLite; a migration running in a transaction sees what it has created.

### Running Migrations

```rust
let migrator = Migrator::new()
    .add(CreateUsersTable)
    .add(CreatePostsTable);

let result = migrator.run().await?;            // apply pending migrations
migrator.run_steps(2).await?;                  // apply the next two
migrator.rollback().await?;                    // undo the last one
migrator.run_one("20240101_create_posts").await?;   // one migration, by version or name,
migrator.rollback_one("create_posts").await?;       //   out of turn
migrator.mark("20240101_create_posts", true).await?; // record it without running it
let sql = migrator.pretend("create_posts", true).await?; // the statements its up() would run
for status in migrator.status().await? { /* registered migrations, applied or not */ }
for applied in migrator.applied().await? { /* the ledger: version, name, applied_at */ }
let strays = migrator.unregistered().await?;   // applied, but registered nowhere here

// Another database than the global one: a tenant's, or a test's own
Migrator::new().add(CreateUsersTable).database(&tenant_db).run().await?;
```

Each migration commits on its own, so a failing one stops `run`, `run_steps`, `rollback`, `rollback_steps`, `reset` or `refresh` with the earlier ones still applied or rolled back; the error they return, a `MigrationFailure`, lists those in its `result` beside the `error`, and `?` turns it into that `tideorm::Error`.

`reset` refuses to run while `unregistered` lists anything, since it could not undo those migrations. `run_one`, `rollback_one` and `mark` refuse a migration already in the state they would put it in. `pretend` runs the migration with a schema that collects its statements, inside a transaction it rolls back, so what the migration writes past the schema is undone too, except DDL on MySQL and MariaDB, which commits as it runs; it takes no lock and writes no ledger row.

### Rebuilding a SQLite Table

SQLite cannot change a column's type, so the change is a rebuild: create the new table, copy the rows, drop the old one and rename the new one. With foreign keys enforced, which is the driver's default, the `DROP TABLE` deletes the rows of other tables that point at the table with `ON DELETE CASCADE`, or fails on them. SQLite only turns enforcement off outside a transaction, and every migration runs in one, so `PRAGMA foreign_keys = OFF` inside it is refused. Ask for it instead:

```rust
fn foreign_keys_off(&self) -> bool {
    true
}

async fn up(&self, schema: &mut Schema) -> Result<()> {
    schema.raw("CREATE TABLE users_new (id INTEGER PRIMARY KEY, age INTEGER NOT NULL)").await?;
    schema.raw("INSERT INTO users_new SELECT id, CAST(age AS INTEGER) FROM users").await?;
    schema.raw("DROP TABLE users").await?;
    schema.raw("ALTER TABLE users_new RENAME TO users").await
}
```

The migration then runs on a connection of its own with enforcement off, and before it commits TideORM runs `PRAGMA foreign_key_check`: a row whose foreign key matches no row fails the migration, which rolls back. The check runs before the migration as well, so a database that already has such rows is refused with the first of them named, rather than the migration being blamed for them. A migration that cascades on purpose keeps the default. `tideorm migrate` runs a project's migrations through this same `Migrator`, so the same holds there.

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

`SchemaWriter::write_schema("schema.sql").await?` writes the same file for the global database at any time, `SchemaWriter::write_schema_for(&db, "schema.sql")` for another one, and `SchemaWriter::schema_sql(&db)` returns the script instead of writing it. On PostgreSQL, MySQL and MariaDB it reads every table of the connected database, with its columns, primary key and indexes. What those cannot describe comes after the tables as the catalog declares it: a full-text, expression, partial or prefix index, or one with `INCLUDE` columns or `NULLS NOT DISTINCT`. A generated column is written with its expression, and the tables' foreign keys and `CHECK` constraints (and PostgreSQL's `EXCLUDE` ones) come last, added with `ALTER TABLE` once every table they name exists. An index MySQL builds on an expression is named in a comment instead. SQLite keeps the statement that created each table, view, index and trigger, so its file is those statements, generated columns, collations, constraints and `AUTOINCREMENT` included; an FTS5 table's shadow tables are left out, since the FTS5 table recreates them.

> ⚠️ **Warning**: Do NOT use `sync(true)` in production! Use proper migrations instead.

---

