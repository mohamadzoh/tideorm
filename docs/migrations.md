# Migrations

Migrations change a database's schema in versioned, reversible steps. Use them for every deployed environment; [schema sync](schema.md) is for local development only.

```rust
use tideorm::prelude::*;
use tideorm::migration::*;

struct CreateUsersTable;

#[async_trait]
impl Migration for CreateUsersTable {
    fn version(&self) -> &str { "20260115_001" }
    fn name(&self) -> &str { "create_users_table" }

    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.create_table("users", |t| {
            t.id();                                    // id BIGSERIAL PRIMARY KEY
            t.string("email").unique().not_null();    // email VARCHAR(255) UNIQUE NOT NULL
            t.string("name").not_null();              // name VARCHAR(255) NOT NULL
            t.text("bio").nullable();                 // bio TEXT NULL
            t.boolean("active").default(true);        // active BOOLEAN DEFAULT TRUE
            t.date("birth_date").nullable();          // birth_date DATE NULL
            t.decimal_with("balance", 12, 2)          // balance DECIMAL(12, 2) DEFAULT '0.00'
                .default("0.00");
            t.jsonb("preferences").nullable();        // preferences JSONB NULL
            t.timestamptz("email_verified_at")        // email_verified_at TIMESTAMPTZ NULL
                .nullable();
            t.timestamps();                           // created_at, updated_at TIMESTAMPTZ
            t.soft_deletes();                         // deleted_at TIMESTAMPTZ NULL
            t.index(&["name"]);                       // idx_users_name
            t.unique_index_named("users_email_key", &["email", "deleted_at"]);
        }).await?;

        schema.create_index("users", "idx_users_email_active", &["email", "active"]).await?;
        Ok(())
    }

    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.drop_table("users").await
    }
}
```

The comments show PostgreSQL's SQL; see [Column Types](#column-types) for the other backends. The model that matches this table:

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

A migration's `version` orders it: migrations run in the lexicographic order of their versions, so use a sortable form such as `YYYYMMDD_NNN`. Its `name` is for people, and for picking one migration out by name.

## Running Migrations

At startup, register the migrations on `TideConfig` and turn running them on:

```rust
TideConfig::init()
    .database("postgres://localhost/mydb")
    .migration(CreateUsersTable)
    .migration(CreatePostsTable)
    .run_migrations(true)
    .connect()
    .await?;
```

Registering is not running: migrations run only with `run_migrations(true)`, which is off by default, and only from `connect()`. They run in version order, whatever order they were registered in. `connect_only()` skips them.

For more control, use a `Migrator`:

```rust
let migrator = Migrator::new()
    .add(CreateUsersTable)
    .add(CreatePostsTable);

let result = migrator.run().await?;                 // apply pending migrations
migrator.run_steps(2).await?;                       // apply the next two
migrator.rollback().await?;                         // undo the last one
migrator.rollback_steps(3).await?;                  // undo the last three
migrator.reset().await?;                            // undo every one
migrator.refresh().await?;                          // reset, then run

migrator.run_one("20240101_create_posts").await?;   // one migration, by version or name,
migrator.rollback_one("create_posts").await?;       //   out of turn
migrator.mark("20240101_create_posts", true).await?; // record it without running it
let sql = migrator.pretend("create_posts", true).await?; // the statements its up() would run

for status in migrator.status().await? { /* registered migrations, applied or not */ }
for applied in migrator.applied().await? { /* the ledger: version, name, applied_at */ }
let strays = migrator.unregistered().await?;        // applied, but registered nowhere here

// Another database than the global one: a tenant's, or a test's own
Migrator::new().add(CreateUsersTable).database(&tenant_db).run().await?;
```

`add_boxed(Box<dyn Migration>)` registers a boxed migration. A version registered twice is refused before anything runs. With `database(&db)` the migrations, their ledger and their lock all run on `db`.

### Results and Failures

The batch commands (`run`, `run_steps`, `rollback`, `rollback_steps`, `reset`, `refresh`) return a `MigrationResult`, whose `applied`, `skipped` and `rolled_back` list each migration's version and name (`has_applied()` says whether anything was applied). It displays as a readable report.

Each migration commits on its own, so a failing one stops a batch command with the earlier ones still applied or rolled back. The error, a `MigrationFailure`, holds those in its `result` beside the `error`, and `?` turns it into that `tideorm::Error`:

```rust
match migrator.run().await {
    Ok(result) => println!("{result}"),
    Err(failure) => {
        eprintln!("applied before the failure: {:?}", failure.result.applied);
        return Err(failure.error);
    }
}
```

On backends with transactional DDL (PostgreSQL, SQLite, CockroachDB), a migration and its ledger row commit together, so a statement failing halfway cannot leave an applied change with no ledger row. A rollback reverts a migration and removes its row in one transaction where the backend allows it.

### Rollback Order and the Ledger

The ledger, `_migrations` by default, records which migrations ran and when. A rollback undoes the migration applied most recently, by its ledger entry, not the one with the highest version; after a long-lived branch merges, those are often different migrations. Every entry counts, registered or not: an entry this migrator has no migration for stops the rollback rather than being skipped over.

- `status()` returns one `MigrationStatus` per registered migration: `version`, `name`, `applied` and `applied_at`.
- `applied()` returns every `AppliedMigration` the ledger records, in the order they were applied, with `applied_at` as the database stamped it (in the session's time zone on PostgreSQL, MySQL and MariaDB, in UTC on SQLite).
- `unregistered()` lists the ledger's entries that no registered migration matches. `reset()` refuses to run while it lists anything, since it could not undo those migrations.

`migrations_table("schema_versions")` records the ledger in another table. Its name may only hold ASCII letters, digits and underscores; an invalid one is reported the first time the ledger is touched. The CLI passes the `[migration] table` setting of `tideorm.toml` here, and an application migrating at startup must use the same table, or each keeps its own ledger and the migrations the CLI applied run a second time. `TideConfig` always uses `_migrations`, so with another table run a `Migrator` yourself.

### One Migration at a Time

- `run_one(name)` runs a pending migration, by version or else by name, even when pending migrations come before it. `rollback_one(name)` reverts an applied one wherever it is in the ledger. Both hold the same lock as `run`.
- `mark(name, applied)` records a migration as applied without running it, or with `false` removes its record. The MySQL family and YugabyteDB commit DDL as it runs, so a migration whose ledger write failed there leaves its change unrecorded and every later run fails on it; marking it applied reconciles the ledger, and unmarking covers a rollback that dropped the change but kept the row.
- `pretend(name, apply)` returns the statements the migration's `up()` (or with `false` its `down()`) would run, without running them. The migration runs with a `Schema` that collects its statements, inside a transaction that is rolled back, so what it runs past the schema (a data backfill) changes nothing, except DDL on the MySQL family and YugabyteDB, which commits as it runs. The schema still reads the database where it must (`has_table`, MySQL's `alter_table`) and sees it before the migrations it pretends. It takes no lock and writes no ledger row.

`run_one`, `rollback_one` and `mark` refuse a migration already in the state they would put it in, and a name two migrations share is ambiguous.

### The Migration Lock

Concurrent migrators are serialized with a lock, so replicas booting together apply each migration once instead of racing. The lock is held on a connection of its own, while the migrations run on the pool, so even a one-connection pool works, and a cancelled `run()` closes that connection and releases the lock.

| Backend | Lock |
|---|---|
| PostgreSQL | A session advisory lock (`pg_advisory_lock`), its session's idle and transaction timeouts turned off |
| CockroachDB | A transaction-scoped advisory lock (`pg_advisory_xact_lock`), released by a rollback |
| MySQL, MariaDB | `GET_LOCK('tideorm_migrations:<database>', 300)`; a name past 64 characters uses a hash of the database name. Waiting longer than 300 seconds fails with an `Error::Transaction` naming the lock. Vitess holds it on a reserved connection |
| SQLite | An OS lock on a sidecar file, `<database file>.tideorm-migrations.lock`, which must not be deleted; an in-memory database uses an in-process lock |

Seeds take a lock of their own the same way; see [Seeding](seeding.md).

### From the CLI

`tideorm migrate` runs a project's migrations through the project's own `Migrator`, registered in `src/migrations/mod.rs`, so everything on this page holds there too. A migration not registered there is invisible to the CLI.

## Schema Builder

`Schema` is what `up()` and `down()` receive. Every method is async and returns `tideorm::Result`:

| Method | Does |
|---|---|
| `create_table(name, closure)` | Creates a table; the closure declares its columns on a `TableBuilder` |
| `create_table_if_not_exists(name, closure)` | The same, doing nothing for a table that exists |
| `alter_table(name, closure)` | Changes a table through an `AlterTableBuilder` |
| `drop_table(name)`, `drop_table_if_exists(name)` | Drops a table |
| `rename_table(from, to)` | Renames a table |
| `has_table(name)`, `has_column(table, column)` | Reads the catalog |
| `create_index(table, name, &columns)`, `create_unique_index(..)` | Creates an index |
| `create_index_if_not_exists(..)`, `create_unique_index_if_not_exists(..)` | The same, doing nothing for an index that exists |
| `drop_index(table, name)`, `drop_index_if_exists(table, name)` | Drops an index |
| `raw(sql)` | Runs a statement as written |

`database_type()` returns the backend the migration runs on, for a step only one backend takes.

`create_table_if_not_exists` can run again after a half-applied migration: on MySQL, which has no `CREATE INDEX IF NOT EXISTS`, it skips the indexes the table already has.

### Column Types

| Method | PostgreSQL | MySQL, MariaDB | SQLite |
|---|---|---|---|
| `small_integer` | `SMALLINT` | `SMALLINT` | `INTEGER` |
| `integer` | `INT4` | `INT` | `INTEGER` |
| `big_integer`, `foreign_id` | `BIGINT` | `BIGINT` | `INTEGER` |
| `float` | `REAL` | `FLOAT` | `REAL` |
| `double` | `DOUBLE PRECISION` | `DOUBLE` | `REAL` |
| `decimal` | `DECIMAL(10, 2)` | `DECIMAL(10, 2)` | `REAL` |
| `decimal_with(name, p, s)` | `DECIMAL(p, s)` | `DECIMAL(p, s)` | `REAL` |
| `string` | `VARCHAR(255)` | `VARCHAR(255)` | `TEXT` |
| `string_with(name, n)` | `VARCHAR(n)` | `VARCHAR(n)` | `TEXT` |
| `text` | `TEXT` | `LONGTEXT` | `TEXT` |
| `boolean` | `BOOLEAN` | `TINYINT(1)` | `INTEGER` |
| `date` | `DATE` | `DATE` | `TEXT` |
| `time` | `TIME` | `TIME(6)` | `TEXT` |
| `datetime`, `timestamp` | `TIMESTAMP` | `DATETIME(6)` | `TEXT` |
| `timestamptz` | `TIMESTAMPTZ` | `DATETIME(6)` | `TEXT` |
| `uuid` | `UUID` | `BINARY(16)` | `TEXT` |
| `json` | `JSON` | `JSON` | `TEXT` |
| `jsonb` | `JSONB` | `JSON` | `TEXT` |
| `binary` | `BYTEA` | `LONGBLOB` | `BLOB` |
| `integer_array` | `INT4[]` | `JSON` | `TEXT` |
| `text_array` | `TEXT[]` | `JSON` | `TEXT` |

`string_with` takes a length of at least 1: up to 10,485,760 on PostgreSQL, and up to 16,383 on MySQL and MariaDB (utf8mb4), or 768 for a column that is indexed or unique. SQLite ignores the length.

Convenience methods:

```rust
t.id();                    // big_increments("id")
t.big_increments("id");    // BIGINT auto-increment primary key (BIGSERIAL on PostgreSQL)
t.increments("id");        // integer auto-increment primary key (SERIAL4 on PostgreSQL)
t.timestamps();            // created_at, updated_at: TIMESTAMPTZ NOT NULL DEFAULT now
t.timestamps_naive();      // the same without a time zone (TIMESTAMP)
t.soft_deletes();          // deleted_at: nullable TIMESTAMPTZ
```

### Any Column Type

`t.column(name, column_type)` takes any `ColumnType`, the type a model field's Rust type maps to, which schema sync creates, so a migration can declare the column a field gets:

```rust
use std::sync::Arc;

t.column("visits", ColumnType::Unsigned);                            // BIGINT / INT UNSIGNED / INTEGER
t.column("code", ColumnType::String(StringLen::N(12)));              // VARCHAR(12)
t.column("ratios", ColumnType::Array(Arc::new(ColumnType::Double))); // DOUBLE PRECISION[] / JSON / TEXT
t.column("amount", ColumnType::custom("NUMERIC(20,0)"));             // written as given
```

`tideorm::migration::column_type_sql(&column_type, db_type)` returns the SQL a type renders to. A few mappings to know: an unsigned or one-byte integer has no unsigned type on PostgreSQL and takes a wider signed one; `ColumnType::Enum` is the quoted type name on PostgreSQL (create the type yourself) and `ENUM(..)` on MySQL and MariaDB; a type with no equivalent on a backend falls back to `TEXT` on SQLite and `VARCHAR(255)` on MySQL and MariaDB for network and interval types.

### Column Modifiers

Every typed method returns a column builder, which adds the column when it is dropped:

| Modifier | Does |
|---|---|
| `not_null()`, `nullable()` | Sets nullability |
| `default(value)` | Sets a default |
| `default_now()` | Defaults to `CURRENT_TIMESTAMP` |
| `unique()` | Adds a `UNIQUE` constraint on the column |
| `check(expr)` | Adds a `CHECK` constraint, as `.check("email LIKE '%@%'")` |
| `references(table, column)` | Adds a foreign key; see [Foreign Keys](#foreign-keys) |
| `on_delete(action)`, `on_update(action)` | Sets the actions of the key `references` declared |
| `primary_key()` | Makes the column the primary key |
| `extra(sql)` | Appends SQL to the column definition as written |

`default` takes a `DefaultValue`, or anything that converts to one: `&str` and `String` (a string literal), the integer types, `f32`/`f64`, `bool` (`TRUE`/`FALSE`), `Decimal` (its exact digits) and `Uuid` (text on PostgreSQL, bytes on MySQL, MariaDB and SQLite). `DefaultValue::Raw("..")` writes an expression as given, and `DefaultValue::Null` a NULL.

A string default is escaped the way the backend reads a literal, so `default(r"C:\temp\")` stores those backslashes on MySQL and MariaDB too. A current-time default may be spelled `now()`, `CURRENT_TIMESTAMP` or `LOCALTIMESTAMP` on any backend.

### Indexes and Table Constraints

```rust
schema.create_table("user_roles", |t| {
    t.foreign_id("user_id").not_null();
    t.foreign_id("role_id").not_null();
    t.big_integer("tenant_id").not_null();
    t.string("email");
    t.primary_key(&["user_id", "role_id"]);           // a composite primary key
    t.unique(&["user_id", "tenant_id"]);              // a UNIQUE constraint, named by the database
    t.unique_named("uq_email_tenant", &["email", "tenant_id"]);
    t.index(&["tenant_id"]);                          // idx_user_roles_tenant_id
    t.unique_index(&["email"]);                       // idx_user_roles_email_unique
    t.index_named("idx_roles_by_tenant", &["tenant_id", "role_id"]);
}).await?;
```

`index` and `unique_index` name an index after its table and columns (`idx_<table>_<columns>`, with `_unique` for a unique one); `index_named` and `unique_index_named` take the name. A generated name past 63 bytes is shortened with a hash, since PostgreSQL would otherwise cut two names that share a prefix to one and MySQL refuses them; a model's `#[index]` names are shortened the same way. `primary_key(&columns)` wins over a column declared as the primary key.

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
    t.string("nickname");                          // every typed column method adds a column
    t.add_column("rank", ColumnType::Integer);     // or any type
    t.foreign_id("team_id").references("teams", "id");
    t.rename_column("name", "full_name");
    t.alter_column("email").not_null();
    t.alter_column("score").set_type(ColumnType::BigInteger).default(0);
    t.change_column("age", ColumnType::BigInteger); // alter_column("age").set_type(..)
    t.alter_column("bio").drop_default();
    t.add_index(&["full_name"]);
    t.add_unique_index(&["nickname"]);              // add_index_named / add_unique_index_named take a name
    t.drop_index("idx_users_legacy");
    t.add_foreign_key(&["manager_id"], "users", &["id"]).on_delete(ForeignKeyAction::SetNull);
    t.drop_foreign_key("fk_users_legacy_id");
    t.drop_column("legacy");
}).await?;
```

`alter_column(name)` changes a column with `set_type`, `nullable`, `not_null`, `default` and `drop_default`. The changes run in the order they were made.

| Backend | How a column changes |
|---|---|
| PostgreSQL | In place: its type, nullability or default |
| MySQL, MariaDB | `MODIFY COLUMN` restates the column whole, so `alter_table` first reads the table's `SHOW CREATE TABLE` and keeps what the change leaves alone: its nullability, default, `AUTO_INCREMENT`, comment and, for a column that still holds text, its character set and collation. A change of default alone runs `ALTER COLUMN .. SET DEFAULT` |
| SQLite | It cannot change a column, or add or drop a foreign key on a table it has, so such an alteration fails before any of its statements runs (see [Rebuilding a SQLite Table](#rebuilding-a-sqlite-table)). A column added with `references` takes its key along, which then wants the column to default to NULL |

### Renaming a Table

```rust
async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
    schema.rename_table("posts", "articles").await
}

async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
    schema.rename_table("articles", "posts").await
}
```

MySQL and MariaDB run `RENAME TABLE`, PostgreSQL and SQLite `ALTER TABLE .. RENAME TO`. The rows, columns and indexes move with the table, and foreign keys in other tables follow it.

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

## MySQL and MariaDB

Every timestamp method renders `DATETIME(6)` there: `TIMESTAMP` only spans 1970 to 2038, and a column without fractional digits drops microseconds; a `CURRENT_TIMESTAMP` default is raised to `CURRENT_TIMESTAMP(6)` to match. `text()` and `binary()` render `LONGTEXT`/`LONGBLOB`, because `TEXT` and `BLOB` stop at 64 KB, and a default on one of them, or on a JSON column, is written as the parenthesized expression MySQL requires. MySQL cannot index a `TEXT` column without a prefix length, so give an indexed or unique string column `string()` (`VARCHAR(255)`). Tables are created `DEFAULT CHARSET=utf8mb4`, whatever the database's own default is.

MySQL and MariaDB commit DDL as it runs, so a failed migration can leave part of its change applied; `mark` reconciles the ledger afterwards.

## Rebuilding a SQLite Table

SQLite cannot change a column's type, so the change is a rebuild: create the new table, copy the rows, drop the old one and rename the new one. With foreign keys enforced, which is the driver's default, the `DROP TABLE` deletes the rows of other tables that point at the table with `ON DELETE CASCADE`, or fails on them. SQLite only turns enforcement off outside a transaction, and every migration runs in one, so `PRAGMA foreign_keys = OFF` inside it is refused. Ask for it instead:

```rust
fn foreign_keys_off(&self) -> bool {
    true
}

async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
    schema.raw("CREATE TABLE users_new (id INTEGER PRIMARY KEY, age INTEGER NOT NULL)").await?;
    schema.raw("INSERT INTO users_new SELECT id, CAST(age AS INTEGER) FROM users").await?;
    schema.raw("DROP TABLE users").await?;
    schema.raw("ALTER TABLE users_new RENAME TO users").await
}
```

The migration then runs on a connection of its own with enforcement off, and before it commits TideORM runs `PRAGMA foreign_key_check`: a row whose foreign key matches no row fails the migration, which rolls back. The check runs before the migration as well, so a database that already has such rows is refused with the first of them named, rather than the migration being blamed for them. It cannot run inside a transaction of yours, nor on an in-memory database with a private cache. Other backends ignore `foreign_keys_off()`. A migration that cascades on purpose keeps the default.
