# Databases

TideORM runs the same models and queries on PostgreSQL, MySQL, MariaDB and SQLite, and writes each statement in the dialect of the database it runs on. This page covers how the backend is chosen, using several databases at once, how Rust types map to columns, and where the backends differ.

## Supported Databases

| Database | Feature | URL |
|---|---|---|
| PostgreSQL | `postgres` | `postgres://user:pass@localhost:5432/app` (or `postgresql://`) |
| MySQL | `mysql` | `mysql://user:pass@localhost:3306/app` |
| MariaDB | `mysql` (or its alias `mariadb`) | `mysql://user:pass@localhost:3306/app` (or `mariadb://`) |
| SQLite | `sqlite` | `sqlite://./app.db?mode=rwc`, or `sqlite::memory:` |

CockroachDB, YugabyteDB, TiDB and Vitess run through the PostgreSQL and MySQL drivers; see [Compatible Servers](compatible-servers.md).

```rust
// PostgreSQL
TideConfig::init().database("postgres://user:pass@localhost/mydb").connect().await?;

// MySQL or MariaDB
TideConfig::init().database("mysql://user:pass@localhost/mydb").connect().await?;

// SQLite: a file, created if missing because of mode=rwc
TideConfig::init().database("sqlite://./data.db?mode=rwc").connect().await?;
```

`sqlite::memory:` gives each pool its own in-memory database, shared by the pool's connections and gone when the pool closes.

## How the Backend Is Chosen

The URL's scheme picks the backend, and with it the SQL dialect: placeholders (`$1` on PostgreSQL, `?` elsewhere), identifier quoting (`"name"` on PostgreSQL and SQLite, `` `name` `` on MySQL and MariaDB), and every clause that differs. `TideConfig::database_type(..)` names it only when the scheme cannot (see [Configuration](configuration.md#database-types)).

MySQL and MariaDB share a protocol, so a `mysql://` URL can reach either. The connection reads which one answered from the server's handshake as it opens, and from then on TideORM writes MariaDB's dialect where the two differ.

A handle answers for its own connection:

```rust
let db = require_db()?;

match db.backend() {
    DatabaseType::Postgres => println!("PostgreSQL"),
    DatabaseType::MySQL => println!("MySQL"),
    DatabaseType::MariaDB => println!("MariaDB"),
    DatabaseType::SQLite => println!("SQLite"),
}

if let Some(server) = db.server() {
    println!("connected to {server}"); // "CockroachDB", "TiDB", ..
}
```

| Call | Answers |
|---|---|
| `db.backend()` | The `DatabaseType` of the handle's connection: `Postgres`, `MySQL`, `MariaDB` or `SQLite`. A disconnected handle falls back to the configured type, and then to `Postgres` with a warning |
| `db.try_backend()` | The same, but a disconnected handle is an error instead of a guess |
| `db.server()` | The `DatabaseServer` that answered: `Postgres`, `CockroachDb`, `YugabyteDb`, `MySql`, `MariaDb`, `TiDb`, `Vitess` or `Sqlite`. `None` for a disconnected handle, or a lazy pool that has not opened a connection yet. `server.backend()` gives its dialect |
| `TideConfig::get_database_type()` | The type `connect()` settled on, or the one `apply()` declared; a fallback for code with no handle |

`DatabaseType` also answers a few questions about a backend:

| Method | Returns |
|---|---|
| `supports_arrays()` | `true` on PostgreSQL only: native array columns |
| `supports_returning()` | `true` on PostgreSQL and SQLite: `UPDATE .. RETURNING`, which `BatchUpdateBuilder::execute_returning()` needs |
| `optimal_batch_size()` | A rule of thumb for rows per batch: 1000 on PostgreSQL, 500 on MySQL and MariaDB, 100 on SQLite |
| `as_str()` | `"PostgreSQL"`, `"MySQL"`, `"MariaDB"` or `"SQLite"` |
| `DatabaseType::from_url(url)` | The backend a URL's scheme names, or `None` |

```rust
let db_type = require_db()?.backend();
if db_type.supports_returning() {
    // execute_returning() is available
}
```

## Several Databases at Once

The global database `TideConfig` installs is a default, not a limit. `Database::connect(url)` (or `Database::builder()`) opens another pool without replacing it, and there are three ways to send work there:

```rust
let replica = Database::connect(&replica_url).await?;

// One query
let recent = User::query_with(&replica).order_desc(User::ID).limit(10).get().await?;

// One lookup by key
let user = User::find_with(user_id, &replica).await?;

// Every model call inside a block, with no handle passed around
let tenant = Database::connect(&tenant_url).await?;
let user = tenant
    .scope(async {
        let user = User::create(user).await?;
        Post::create(Post { user_id: user.id, ..Default::default() }).await?;
        Ok(user)
    })
    .await?;
```

Inside `scope`, a call that names no database (`User::create`, `User::query()`, `user.update()`) runs on the scope's database. `Database::current()` returns whichever database such a call would use here: the enclosing transaction's, else the scope's, else the global one.

A call that names a database joins the innermost enclosing transaction opened on that database's pool; inside another database's transaction it keeps to its own connection, so a replica read inside a primary's transaction still goes to the replica. [Transactions](transactions.md) covers how transactions and scopes combine.

Work started with `tokio::spawn` inside a scope runs outside it, on the global database: await it inside the scope, or open the scope inside the spawned task.

## Data Type Mappings

A field's Rust type decides its column type. Schema sync and migrations create these columns, and reads and writes convert through them:

| Rust type | PostgreSQL | MySQL and MariaDB | SQLite | Notes |
|---|---|---|---|---|
| `i8` | `SMALLINT` | `TINYINT` | `INTEGER` | |
| `i16` | `SMALLINT` | `SMALLINT` | `INTEGER` | |
| `i32` | `INT4` | `INT` | `INTEGER` | |
| `i64` | `BIGINT` | `BIGINT` | `INTEGER` | Recommended for primary keys |
| `u8` | `SMALLINT` | `TINYINT UNSIGNED` | `INTEGER` | |
| `u16` | `INT4` | `SMALLINT UNSIGNED` | `INTEGER` | |
| `u32` | `BIGINT` | `INT UNSIGNED` | `INTEGER` | |
| `u64` | `BIGINT` | `BIGINT UNSIGNED` | `INTEGER` | PostgreSQL and SQLite store up to `i64::MAX`; a larger value is refused when it is written |
| `f32` | `REAL` | `FLOAT` | `REAL` | |
| `f64` | `DOUBLE PRECISION` | `DOUBLE` | `REAL` | |
| `bool` | `BOOLEAN` | `TINYINT(1)` | `INTEGER` | |
| `String` | `TEXT` | `LONGTEXT` | `TEXT` | Schema sync creates a MySQL key or indexed column as `VARCHAR(255)`, because MySQL cannot index `TEXT` without a prefix length |
| `Uuid` | `UUID` | `BINARY(16)` | `TEXT` | MySQL binds a UUID as 16 raw bytes, so the column must be `BINARY(16)` |
| `Decimal` | `DECIMAL` | `DECIMAL(65,30)` | `REAL` | See [Decimals on SQLite](#decimals-on-sqlite) |
| `serde_json::Value` | `JSONB` | `JSON` | `TEXT` | MariaDB stores `JSON` as `LONGTEXT` |
| `Vec<u8>` | `BYTEA` | `LONGBLOB` | `BLOB` | Binary data |
| `NaiveDate` | `DATE` | `DATE` | `TEXT` | Date only |
| `NaiveTime` | `TIME` | `TIME(6)` | `TEXT` | Time only |
| `NaiveDateTime` | `TIMESTAMP` | `DATETIME(6)` | `TEXT` | No time zone |
| `DateTime<Utc>` | `TIMESTAMPTZ` | `DATETIME(6)` | `TEXT` | With time zone; `DateTime<Local>` and `DateTime<FixedOffset>` map the same way |
| `Option<T>` | nullable | nullable | nullable | Wraps any type above to make the column nullable |

PostgreSQL's `INT4` is its `INTEGER`, written out so CockroachDB, which reads a bare `INT` as 64-bit, creates the same column.

`tideorm::prelude` has short names for some of these: `Json` and `Jsonb` for `serde_json::Value`, `Text` for `String`, and `IntArray`, `BigIntArray`, `TextArray`, `BoolArray`, `FloatArray` and `JsonArray` for the array types below.

### Arrays

On PostgreSQL a field can be an array: `Vec<i32>`, `Vec<i64>`, `Vec<f64>`, `Vec<bool>`, `Vec<String>` or `Vec<serde_json::Value>`, stored as the matching `T[]` column. MySQL, MariaDB and SQLite have no array columns: keep a list in a JSON column (`serde_json::Value`) there, and the array filters and updates work on it as a JSON array. See [JSON and Arrays](json-and-arrays.md).

### Enums, Newtypes and Other Types

A field of a type with no column mapping fails to compile, with an error at the field that says the type "is not a type a column can hold". Give the type a mapping, or keep it out of the table:

- An enum stored as text derives `DeriveActiveEnum`, with the value each variant stores:

  ```rust
  use tideorm::prelude::*;

  #[derive(
      Debug, Clone, Default, PartialEq, Eq,
      tideorm::orm::EnumIter,
      tideorm::orm::DeriveActiveEnum,
      serde::Serialize,
      serde::Deserialize,
  )]
  #[orm(rs_type = "String", db_type = "String(StringLen::None)")]
  pub enum OrderStatus {
      #[default]
      #[orm(string_value = "pending")]
      Pending,
      #[orm(string_value = "shipped")]
      Shipped,
  }

  #[tideorm::model(table = "orders")]
  pub struct Order {
      #[tideorm(primary_key, auto_increment)]
      pub id: i64,
      pub status: OrderStatus,
  }
  ```

- A newtype over a mapped type derives `tideorm::orm::DeriveValueType` and takes its inner type's column (`struct Cents(i64)` is a `BIGINT`).
- A field that is not a column at all is marked `#[tideorm(skip)]`.

`UnixTimestamp` and `UnixTimestampMillis` in the prelude convert between epoch seconds or milliseconds and `DateTime<Utc>`, and have no column type of their own: store the `i64` (`as_seconds()`) and convert at the edges.

### Dates and Times

Use `DateTime<Utc>` for moments in time, such as when a row was created or a session expires; on PostgreSQL it is a `TIMESTAMPTZ`. Use `NaiveDateTime` for a wall-clock time with no zone, `NaiveDate` for a date and `NaiveTime` for a time of day.

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "sessions")]
pub struct Session {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub expires_at: DateTime<Utc>,   // TIMESTAMPTZ
    pub last_seen: NaiveDateTime,    // TIMESTAMP
    pub birthday: Option<NaiveDate>, // DATE
    pub opens_at: NaiveTime,         // TIME
}
```

Each has a migration column method of its own:

```rust
schema.create_table("sessions", |t| {
    t.id();
    t.big_integer("user_id").not_null();
    t.timestamptz("expires_at").not_null();
    t.timestamp("last_seen").default_now();
    t.date("birthday");
    t.time("opens_at").not_null();
    t.timestamps(); // created_at and updated_at, TIMESTAMPTZ
}).await?;
```

`timestamps()` creates `TIMESTAMPTZ` columns that default to now; `timestamps_naive()` creates `TIMESTAMP` ones. See [Migrations](migrations.md).

PostgreSQL's `infinity` and `-infinity` dates and `TIME '24:00:00'` have no Rust value, so a typed read of such a row fails with an error naming the column; `get_json()` returns them as text.

### MySQL and MariaDB Storage

- Every timestamp is a `DATETIME(6)`: MySQL's `TIMESTAMP` only spans 1970 to 2038, and a column without fractional digits drops microseconds. Each connection runs in UTC, so a `DateTime<Utc>` is stored as its UTC wall time.
- Text and binary columns are `LONGTEXT` and `LONGBLOB`, because `TEXT` and `BLOB` stop at 64 KB, and tables are created `DEFAULT CHARSET=utf8mb4` whatever the database's default, so any Rust `String` fits.
- Each connection takes `NO_BACKSLASH_ESCAPES` out of its session's SQL mode, since SQL written for MySQL escapes with backslashes.

### Decimals on SQLite

SQLite has no exact decimal type, and a `REAL` keeps about 15 significant digits. A decimal is read as SQLite stores it, so a column declared `TEXT` keeps every digit but compares as text. Store minor units in an integer (cents in an `i64`) to keep both.

## Backend Differences

The query builder, the model API and migrations work on every backend. These are the places where what runs differs:

| Feature | PostgreSQL | MySQL | MariaDB | SQLite |
|---|---|---|---|---|
| JSON columns and JSON filters | Yes (`JSONB`) | Yes | Yes | Yes (JSON1 functions) |
| Array columns (`Vec<T>` fields) | Yes | No: JSON arrays | No: JSON arrays | No: JSON arrays |
| A single-row insert returns the row in one statement | Yes | No: read back after | Yes (10.5+) | Yes |
| `execute_returning()` (`UPDATE .. RETURNING`) | Yes | No | No | Yes |
| Upsert | `ON CONFLICT` | Locked read, then update or insert | As MySQL | `ON CONFLICT` |
| Window functions and CTEs | Yes | Yes | Yes | Yes |
| `GROUPS` window frames | Yes | No | No | Yes |
| Full-text search | Yes | Yes | Yes | Yes (FTS5) |
| Schema-qualified tables (`schema.table`) | Schemas | Databases | Databases | No |
| `lock_for_update()` | Yes | Yes | Yes | Renders nothing: SQLite locks the whole database |

A clause a backend lacks fails the query with an error when it runs; TideORM never drops it or runs the query without it. `debug_for(DatabaseType::MySQL)` and `build_sql_preview_for(..)` show a query in another backend's dialect (see [Logging and Profiling](profiling.md)).

### JSON Operations

JSON filters are written once and translated to each backend:

```rust
Product::query()
    .where_json_contains("metadata", json!({"featured": true}))
    .get()
    .await?;
```

| Operation | PostgreSQL | MySQL and MariaDB | SQLite |
|---|---|---|---|
| Contains | `(col)::jsonb @> value` | `JSON_CONTAINS(col, value)`, with type guards | Rebuilt from `json_each` |
| Key exists | `jsonb_typeof(col) = 'object' AND (col)::jsonb ? 'key'` | `JSON_CONTAINS_PATH(col, 'one', '$."key"')` | `json_type(col, '$."key"') IS NOT NULL` |
| Path exists | `jsonb_path_exists((col)::jsonb, 'strict $.path', '{}', TRUE)` | `JSON_CONTAINS_PATH(col, 'one', '$.path')` | `json_type(col, '$.path') IS NOT NULL` |

`where_json_key_not_exists` and `where_json_path_not_exists` negate the two tests. On every backend a member that holds JSON `null` exists, and a `NULL` column matches neither a test nor its negation. A path is `$` followed by `.key`, `."quoted key"` or `[index]` steps, or those steps alone, read from the root (`a.b[0]` is `$.a.b[0]`). Every backend reads it strictly: a key exists only in an object, never as a string element of an array, a step never looks inside an array it does not index, and an index never reads a value that is not an array as an array of one (MySQL's indexed steps check their parent is an array). A path with other steps fails the query, except on PostgreSQL (not CockroachDB), which reads it as the JSONPath written.

PostgreSQL's JSON operators exist only for `jsonb`, so the column is cast: they work on a `json` column (what `t.json(..)` creates) too, and on a `jsonb` column the cast is dropped at planning, so its GIN index still applies.

SQLite has no containment operators, so TideORM rebuilds PostgreSQL's `@>` and `<@` from `json_each`: an object matches key by key, an array element by element, and a bare scalar matches a top-level array that holds it. MySQL's `JSON_CONTAINS` lets a value match an array holding it at any depth, so on its own `{"tags": "a"}` would match `{"tags": ["a", "b"]}`; TideORM adds a type check for each path of the document you pass, so MySQL and MariaDB follow PostgreSQL there too. Below an array the paths are not known in advance, so MySQL's reading stays there: `[{"k": [1, 2]}]` contains `[{"k": 1}]` on MySQL and not on PostgreSQL.

Equality on a JSON column (`where_eq`) compares documents, not text: `=` on MySQL's JSON type, `JSON_EQUALS` on MariaDB, and a structural comparison built from `json_each` on SQLite, so key order and `1.0` against `1` do not matter.

### Array Operations

The array filters (`where_array_contains`, `where_array_contained_by`, `where_array_overlaps`) take a list of values:

```rust
Product::query()
    .where_array_contains("tags", vec!["sale", "featured"])
    .get()
    .await?;
```

| Backend | Column | How each filter runs |
|---|---|---|
| PostgreSQL | A native array | Each value is tested with `value = ANY(col)`: all of them for contains, any for overlaps; contained-by checks that no element lies outside the list |
| MySQL and MariaDB | A JSON array | Contains and contained-by are `JSON_CONTAINS` with the list as a JSON document; overlaps is the containment of any one value. A column holding something other than an array matches nothing |
| SQLite | A JSON array | Each value is looked up with `EXISTS (SELECT 1 FROM json_each(col) ..)`, compared as JSON, so `1` is not `true`. A column holding something other than an array matches nothing |

[JSON and Arrays](json-and-arrays.md) covers every JSON and array filter and update.

### Other Differences

- **Text length and types.** SQLite checks neither, so a string longer than its `VARCHAR(255)` is stored there and refused by PostgreSQL and MySQL. `#[validate(max_length = 255)]` keeps the backends in step; see [Validation](validation.md).
- **Trailing spaces.** MariaDB's default collations ignore trailing spaces in `=`; MySQL 8's do not.
- **Window defaults.** MariaDB's `LAG` and `LEAD` take no default value, so TideORM writes the default with a `CASE` around them.
- **Subquery limits.** MySQL and MariaDB refuse a `LIMIT` in an `IN` subquery.
- **Transactions.** SQLite refuses a read-only transaction (`AccessMode::ReadOnly`), and runs a migration inside a transaction, where `PRAGMA foreign_keys` has no effect. See [Transactions](transactions.md) and [Migrations](migrations.md).
- **Altering tables.** SQLite's `ALTER TABLE` cannot change or drop some columns; TideORM refuses those with an error that explains how to rebuild the table. MySQL and MariaDB keep a changed column's `NOT NULL`, default, auto-increment and comment, as PostgreSQL does. See [Migrations](migrations.md).
- **Batch inserts.** PostgreSQL, MariaDB and SQLite insert a batch with multi-row `INSERT .. RETURNING`; MySQL inserts multi-row statements and reads them back with one `SELECT` when the keys are known in advance, and otherwise inserts row by row. See [Reading and Writing](crud.md).
- **Upserts.** MySQL's `ON DUPLICATE KEY UPDATE` fires on any unique key, so on MySQL and MariaDB TideORM locks the row the conflict columns name and updates it or inserts, in one transaction. See [Reading and Writing](crud.md).
- **Large `where_in` lists.** Past 1,000 values PostgreSQL binds one array (`= ANY($1)`), SQLite binds a text list as one JSON value, and an all-integer list is written as literals elsewhere, which keeps a query under the backend's parameter limit.
