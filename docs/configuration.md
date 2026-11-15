# Configuration

`TideConfig` connects TideORM at startup: it opens the connection pool, installs it as the global database every model call uses, and can run migrations, schema sync and seeders on the way. `Database::builder()` opens further pools, with driver options a URL cannot express.

```rust
use tideorm::prelude::*;

async fn start() -> tideorm::Result<()> {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL is set");
    TideConfig::init()
        .database(url)
        .max_connections(20)
        .connect()
        .await?;
    Ok(())
}
```

## Basic Connection

```rust
// One call: connect and install the global database
Database::init("postgres://localhost/mydb").await?;

// The same through TideConfig, which takes the other settings on this page
TideConfig::init()
    .database("postgres://localhost/mydb")
    .connect()
    .await?;
```

The URL's scheme picks the backend: `postgres://` or `postgresql://`, `mysql://` or `mariadb://`, and `sqlite:`. A URL carries credentials, so read it from the environment (such as `DATABASE_URL`) rather than writing it into code; TideORM does not read a URL from the environment itself.

`connect()` returns the global `&'static Database`. `Database::init(url)` does the same with the driver's pool defaults (see [Driver Options](#driver-options)); `Database::connect(url)` opens a pool without making it global, and `Database::set_global(db)` installs one you built. [Databases](databases.md) covers running several databases side by side.

## What `connect()` Does

`connect()` runs these steps in order, each only when enabled:

1. Installs the configuration: languages, hidden attributes, the encryption key and token codecs, the attachment URL generator.
2. Opens the pool and installs it as the global database. A `mysql://` URL that reaches a MariaDB server is detected here, from the server's handshake.
3. Runs the registered migrations, with `run_migrations(true)`.
4. Syncs the schema from the registered models, with `sync(true)`.
5. Runs the registered seeders, with `run_seeds(true)`.
6. Writes the schema file, with `schema_file(path)`.

The steps are not one atomic unit. A failure part-way leaves the earlier steps applied: a seeder error returns `Err` with the migrations and sync already done.

`connect()` fails when no URL was set, when the backend cannot be told from the URL and `database_type(..)` does not name it, when a MySQL server cannot report its version, or when a step fails.

```rust
TideConfig::init()
    .database(url)
    .migration(CreateUsers)          // register; runs only with run_migrations(true)
    .migration(CreatePosts)
    .run_migrations(true)
    .seed(DemoUsers)                 // register; runs only with run_seeds(true)
    .run_seeds(true)
    .connect()
    .await?;
```

| Setting | Effect |
|---|---|
| `migration(m)` | Registers a migration. Migrations run in version order, whatever order they were registered in. See [Migrations](migrations.md) |
| `run_migrations(bool)` | Runs the registered migrations on connect. Off by default |
| `seed(s)` | Registers a seeder, after any already registered. See [Seeding](seeding.md) |
| `run_seeds(bool)` | Runs the registered seeders on connect, after migrations and sync. Off by default; a failed seeder does not undo what ran before it |
| `models::<T>()` | Registers a model, or a tuple of models (`models::<(User, Post)>()`), for schema sync and schema export |
| `auto_models()` | Registers every model compiled into the binary |
| `models_matching(pattern)` | Registers the compiled models whose source file matches a glob: `*` matches one path segment, `**` any number. The file must still be reachable through `mod` declarations, and a pattern matching no model logs a warning |
| `sync(bool)` | Creates missing tables and columns for the registered models. For development; use migrations in production. See [Schema Sync and Export](schema.md) |
| `force_sync(bool)` | Drops and recreates every registered table on each connect, deleting its rows. Needs `sync(true)`. Refused when `TIDEORM_ENV` is `production`, unless `TIDEORM_ALLOW_FORCE_SYNC` is `1` or `true` |
| `schema_file(path)` | Writes the schema SQL to `path` once per connect, after any sync, for review in diffs. `TideConfig::schema_file_path()` reports the path |

`connect_only()` connects with the same configuration but skips migrations, sync, seeders and the schema file, whatever the builder enabled. The migration and seed runners the CLI scaffolds connect this way, so a `force_sync` in the application's configuration cannot drop the tables they are about to work on.

`apply()` installs the configuration without connecting at all: no pool is opened, so the URL and pool settings go unused. Use it in tests and tools that need the settings but not a database.

`TideConfig::is_connected()` says whether a global database is installed.

## Pool Configuration

```rust
use std::time::Duration;

TideConfig::init()
    .database("postgres://localhost/mydb")
    .max_connections(20)
    .min_connections(5)
    .connect_timeout(Duration::from_secs(10))
    .idle_timeout(Duration::from_secs(300))
    .max_lifetime(Duration::from_secs(3600))
    .acquire_timeout(Duration::from_secs(5))
    .connect()
    .await?;
```

| Setting | Default | Meaning |
|---|---|---|
| `max_connections` | 10 | The ceiling on open connections, and so on concurrent database work, for the whole process. Keep it below what the server allows across all your instances |
| `min_connections` | 1 | Connections kept open even while idle; more trades idle server connections for fewer cold starts |
| `connect_timeout` | 8 s | The bound on each attempt to open a connection: socket, TLS, handshake, authentication and connection hooks |
| `acquire_timeout` | 8 s | The bound on a whole checkout: waiting for a free connection, its ping and hooks, and opening a new one |
| `idle_timeout` | 10 min | Closes a connection left unused this long |
| `max_lifetime` | 30 min | Closes a connection this old, however busy. Keep it under any idle or lifetime limit the server, or a proxy in front of it, enforces |

The two timeouts are independent. Whichever deadline passes first ends the wait, so raising `connect_timeout` does not extend the wait for a free slot.

`pool(..)` replaces every pool setting at once with a `PoolConfig`, which starts from the defaults above and has a setter of the same name for each one. Its fields are public, so the same values can feed `Database::builder()`:

```rust
let pool = PoolConfig::default()
    .max_connections(20)
    .acquire_timeout(Duration::from_secs(2));

TideConfig::init()
    .database("postgres://localhost/mydb")
    .pool(pool.clone())
    .connect()
    .await?;

let replica = Database::builder()
    .url(replica_url)
    .max_connections(pool.max_connections)
    .acquire_timeout(pool.acquire_timeout)
    .build()
    .await?;
```

## Driver Options

A URL names the common settings. For the rest, such as PostgreSQL's `application_name` or SQLite's journal mode, give `Database::builder()` the driver's own options instead of a URL:

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
Database::set_global(db)?;
```

| Builder method | Takes | Feature |
|---|---|---|
| `postgres_options` | `PgConnectOptions` (`host`, `port`, `username`, `password`, `database`, `ssl_mode(PgSslMode)`, `application_name`, `options`, ..) | `postgres` |
| `mysql_options` | `MySqlConnectOptions` (`host`, `port`, `username`, `password`, `database`, `ssl_mode(MySqlSslMode)`, `charset`, `timezone`, ..), for MySQL or MariaDB | `mysql` |
| `sqlite_options` | `SqliteConnectOptions` (`filename`, `create_if_missing`, `journal_mode(SqliteJournalMode)`, `synchronous`, `foreign_keys`, `busy_timeout`, `read_only`, ..) | `sqlite` |

All of these types are in `tideorm::prelude`. A SQLite example:

```rust
use std::time::Duration;

let db = Database::builder()
    .sqlite_options(
        SqliteConnectOptions::new()
            .filename("app.db")
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(10)),
    )
    .build()
    .await?;
```

SQLite connections enforce foreign keys, wait up to 5 s for a locked database, and do not create a missing file unless asked (`create_if_missing(true)`, or `?mode=rwc` in a URL).

The builder also takes a URL (`url(..)`) and the pool settings of the previous section. A setting left unset keeps the driver's default, which is not always `TideConfig`'s: the driver keeps no idle connection and waits 30 s for a free one, where `TideConfig` keeps one and waits 8. `build()` fails without a URL or driver options, with `max_connections(0)`, and with `min_connections` above `max_connections`. The builder's `{:?}` masks a URL's credentials and leaves driver options out, since they hold the password.

An in-memory SQLite database lives only while a connection to it is open, so its pool keeps one connection open and closes none for being idle or old, whatever the settings say. With `cache=private` each connection gets a database of its own, so such a pool must hold one connection.

### Statement Cache

Each connection prepares a parameterized statement once and reuses it while it lives, keeping up to 100 and dropping the least recently used. `statement_cache_capacity(n)` changes that on every backend, for URLs and driver options alike. With `0` no statement is kept and each is prepared for its own run, which a transaction-pooling proxy such as PgBouncer needs. `db.cached_statements().await?` reports how many statements one of the handle's connections holds.

## Pinned Sessions

A pool hands each statement whichever connection is free, so what one statement sets for its session (`SET`, `PRAGMA`, MySQL's `GET_LOCK`) may not hold for the next. `pin_session()` opens a connection of its own, with the pool's settings but outside its slots, and every statement through the returned handle runs on it, transactions included:

```rust
let session = db.pin_session().await?;
session.exec_raw("SET FOREIGN_KEY_CHECKS = 0").await?;
// ... statements that rely on that setting ...
session.close().await?;
```

The connection closes with `close()` or once the last clone of the handle drops, and is never replaced: once it is lost, the handle's statements fail. A second transaction begun on the session waits until the first ends. `pin_session()` fails on a transaction's handle and on a private SQLite database (in memory without a shared cache, or temporary), where a new connection would open an empty database.

## Health Checks and Shutdown

`db.ping().await?` checks that the handle's own connection answers, even inside a transaction opened on another database; any failure is an `Error::Connection`.

`db.close().await?` waits for the connections in use to come back and closes the pool (SQLite then releases its file). Every clone of the handle shares the pool, so statements on any of them fail afterwards. It fails on a transaction's handle, which belongs to its scope.

## Database Types

TideORM reads the backend from the URL. Name it with `database_type(..)` only when the scheme does not say:

```rust
TideConfig::init()
    .database_type(DatabaseType::Postgres) // Postgres, MySQL, MariaDB or SQLite
    .database("postgres://localhost/mydb")
    .connect()
    .await?;
```

A `mysql://` URL that reaches MariaDB is detected when the pool opens, and `TideConfig::get_database_type()` then answers `MariaDB`. A handle's own `Database::backend()` is the authority for the statements it runs; see [Databases](databases.md).

## Application-Wide Settings

`TideConfig` also holds settings every model shares. `connect()` and `apply()` install them.

```rust
TideConfig::init()
    .database(url)
    .encryption_key(std::env::var("TIDEORM_KEY").expect("TIDEORM_KEY is set"))
    .hidden_attributes(["password_digest"])
    .languages(["en", "fr", "ar"])
    .fallback_language("en")
    .file_base_url("https://cdn.example.com/")
    .connect()
    .await?;
```

| Setting | Effect |
|---|---|
| `encryption_key(key)` | The master key for [tokenization](tokenization.md) and [encrypted fields](encrypted-fields.md). Per-column keys are derived from it, so changing it makes every earlier ciphertext and token unreadable. Load it from the environment or a secret manager |
| `token_encoder(f)`, `token_decoder(f)` | Replace how tokens are encoded and decoded, given the serialized key (or token) and the model name. Set both: a token the decoder rejects reads as `Ok(None)`, so a mismatched pair fails silently |
| `hidden_attributes([..])` | Fields left out of every model's `to_json()`, on top of each model's own hidden fields. A direct `serde_json::to_value` still sees them |
| `languages([..])`, `fallback_language(..)` | The languages translated fields accept, `["en"]` by default, and the one read when a translation is missing, `"en"` by default. See [Translations](translations.md) |
| `file_base_url(url)`, `file_base_url_for(field, url)` | The prefix of attachment URLs, for every field or for one field; with none, the stored key is the URL. See [File Attachments](attachments.md) |
| `file_url_generator(f)` | Replaces attachment URL generation entirely, for signed or expiring URLs (`attachments` feature) |

`Config::global()` returns the installed settings, and `Config::get_languages()`, `get_fallback_language()`, `get_hidden_attributes()` and `get_file_base_url_for(field)` read single ones.

## Resetting Global State

TideORM keeps the global database, the configuration and the tokenization settings in process-wide state, so models run without a connection threaded through every call. All of it can be reset, for tests, benchmarks and applications that reconfigure TideORM while running:

```rust
use tideorm::prelude::*;

Database::reset_global();
TideConfig::reset(); // the tokenization key and codecs too

TideConfig::init()
    .database("sqlite::memory:")
    .connect()
    .await?;
```

`TideConfig::reset()` clears the configuration only, `Database::reset_global()` the global connection, and `TokenConfig::reset()` only the tokenization settings. `apply()`, `connect()` and `connect_only()` overwrite the configuration applied before them; one that names no database type clears the stored type instead of keeping the old one. [Testing](testing.md) shows how to avoid global state in tests altogether.

## Environment Variables

TideORM reads these from the process environment. A boolean flag is on only when set to `1` or `true`. The connection URL is not one of them: pass it to `.database(..)` yourself.

| Variable | Effect |
|---|---|
| `TIDE_LOG_QUERIES` | Prints every statement to stderr and records it in the query logger's history; see [Logging and Profiling](profiling.md). Read once, at the first statement |
| `TIDE_LOG_LEVEL` | The `QueryLogger` level: `error`, `warn`, `info`, `debug` or `trace` |
| `TIDE_SLOW_QUERY_MS` | The slow-query threshold, in milliseconds, of the logger and the profiler |
| `TIDE_LOG_SEEDS` | Logs each seeder as it runs and rolls back |
| `TIDEORM_ENV` | `production` (any case) makes `force_sync(true)` fail before it drops anything |
| `TIDEORM_ALLOW_FORCE_SYNC` | Lets `force_sync(true)` run even when `TIDEORM_ENV` is `production` |
