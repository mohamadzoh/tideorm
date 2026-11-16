# Testing

This page shows how to test an application built on TideORM: a database per test, fixtures, rolling a test back, running one body on several backends, and code that uses the global database. [Testing TideORM Itself](#testing-tideorm-itself) at the end lists the commands for working on TideORM.

## A Database for Each Test

Give each test its own database and run the code under test inside `scope`. A call that names no database (`User::create`, `User::query()`, `user.update()`) then uses that database instead of the global one, so tests that run side by side never see each other's rows and need no global setup:

```rust
use tideorm::prelude::*;

/// A fresh in-memory SQLite database with the application's migrations applied.
async fn test_db() -> tideorm::Result<Database> {
    let db = Database::builder().url("sqlite::memory:").build().await?;
    migrations::all().database(&db).run().await?; // the project's `Migrator` registry
    Ok(db)
}

#[tokio::test]
async fn renames_a_user() -> tideorm::Result<()> {
    let db = test_db().await?;
    db.scope(async {
        let user = User::create(User {
            email: "ada@example.com".into(),
            ..Default::default()
        })
        .await?;
        let renamed = rename(user.id, "Ada".into()).await?;
        assert_eq!(renamed.name, "Ada");
        Ok(())
    })
    .await
}
```

Every connection of a `sqlite::memory:` pool shares one in-memory database, and each `Database` built from that URL gets a database of its own, gone when the pool closes. Running the migrations gives the test the same schema as production; [Schema Sync and Export](schema.md) is an alternative for a quick test without migrations.

Work the test starts with `tokio::spawn` runs outside the scope, on the global database: await it inside the scope instead, or give the spawned task the database (`db.scope(..)` inside it). [Several Databases at Once](databases.md#several-databases-at-once) has the rules for which database a call uses.

## Seeding Fixtures

Create rows inside the scope, with the model's own API or in one statement with `insert_all`:

```rust
db.scope(async {
    User::insert_all(vec![
        User { email: "a@example.com".into(), ..Default::default() },
        User { email: "b@example.com".into(), ..Default::default() },
    ])
    .await?;
    // ...
    Ok(())
})
.await?;
```

`insert_all` validates every model but runs no callbacks. A project scaffolded by `tideorm init` can run its seeders the same way: `seeders::all().database(&db).run().await?` (see [Seeding](seeding.md)).

## Rolling a Test Back

On a server database every test shares, run the test in `test_transaction`, which always rolls back everything it wrote and returns what the test returned. Every TideORM call inside joins it, and code under test that opens its own transaction runs as a savepoint inside it, whose commit the rollback undoes too:

```rust
use tideorm::prelude::*;

#[tokio::test]
async fn ships_an_order() -> tideorm::Result<()> {
    let db = Database::builder()
        .url(std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL"))
        .build()
        .await?;
    db.test_transaction(async {
        let order = Order::create(Order { quantity: 2, ..Default::default() }).await?;
        assert!(ship(order.id).await?);
        Ok(())
    })
    .await
}
```

A failed assertion panics, which rolls the transaction back too. A task the test spawns does not join the transaction. See [Transactions](transactions.md) for savepoints and how calls join the transaction.

## The Same Test on Several Backends

Write the body once, taking the database, and call it from one test per backend. Skip a server backend when its URL is not set:

```rust
use tideorm::prelude::*;

async fn creates_users(db: &Database) -> tideorm::Result<()> {
    db.scope(async {
        User::create(User { email: "ada@example.com".into(), ..Default::default() }).await?;
        assert_eq!(User::query().count().await?, 1);
        Ok(())
    })
    .await
}

#[tokio::test]
async fn creates_users_on_sqlite() -> tideorm::Result<()> {
    creates_users(&test_db().await?).await
}

#[tokio::test]
async fn creates_users_on_postgres() -> tideorm::Result<()> {
    let Ok(url) = std::env::var("TEST_POSTGRES_URL") else {
        return Ok(());
    };
    let db = Database::builder().url(url).build().await?;
    migrations::all().database(&db).run().await?;
    creates_users(&db).await
}
```

Each backend's feature must be enabled for its URL to connect (see [Installation](installation.md)). [Databases](databases.md#backend-differences) lists what differs between them.

## Code That Uses the Global Database

Code that calls `db()` or `require_db()` itself, or startup code built on `TideConfig`, needs the global database. Tests of it share one global slot, so run them one at a time (a shared lock, or `--test-threads=1`) and reset it between tests:

```rust
Database::reset_global();
TideConfig::reset(); // the tokenization settings too; TokenConfig::reset() clears only those

TideConfig::init()
    .database("sqlite::memory:")
    .connect()
    .await?;
```

The query logger and the global profiler are process-wide too: call `QueryLogger::reset()` and `GlobalProfiler::reset()` in tests that assert on them (see [Logging and Profiling](profiling.md)).

## Checking Migrations in CI

`tideorm migrate status --check` exits with an error while a migration is pending, so a deploy step can refuse to start an application whose database is behind its code.

## Testing TideORM Itself

The commands below test TideORM, not your application. Run them from the `tideorm` repository and use the smallest one that covers your change:

```bash
# Unit tests, default features (PostgreSQL driver, Tokio)
cargo test --lib

# Default features, every target
cargo test --features postgres

# Broad compatibility pass
cargo test --all-features

# SQLite integration suite and smoke test
cargo test --test sqlite_integration_tests --features "sqlite runtime-tokio" --no-default-features
cargo test --test sqlite_ci_smoke_test --features "sqlite runtime-tokio" --no-default-features

# Server integration suites
cargo test --test postgres_integration_tests
cargo test --test mysql_integration_tests --features mysql
cargo test --test mariadb_integration_tests --features mysql

# Compile-fail tests for the derive
cargo test --test relation_compile_fail

# Rebuild the book after documentation changes
mdbook build
```

The backend suites decide whether to run from the environment, and each backend decides differently:

| Backend | Runs when | Skipped by |
|---|---|---|
| SQLite | Always (in memory) | `SKIP_SQLITE_TESTS` |
| PostgreSQL | `TEST_DATABASE_URL`, `POSTGRESQL_DATABASE_URL` (read in that order) or `RUN_POSTGRES_TESTS` is set | `SKIP_POSTGRES_TESTS` |
| MySQL | `MYSQL_DATABASE_URL` or `RUN_MYSQL_TESTS` is set | `SKIP_MYSQL_TESTS` |
| MariaDB | `MARIADB_DATABASE_URL` or `RUN_MARIADB_TESTS` is set | `SKIP_MARIADB_TESTS` |

A backend that is enabled but cannot connect fails its suite instead of passing quietly. The suites create and drop fixed-name tables, so run them one at a time against a shared server. [CONTRIBUTING.md](https://github.com/mohamadzoh/tideorm/blob/master/CONTRIBUTING.md) has the full list of variables and their default URLs.
