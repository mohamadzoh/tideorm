# Testing Your Application

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

Every connection of a `sqlite::memory:` pool shares one in-memory database, and each `Database` built from that URL gets a database of its own, gone when the pool closes.

Work the test starts with `tokio::spawn` runs outside the scope, on the global database: await it inside the scope instead, or give the spawned task the database (`db.scope(..)` inside it).

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

A project scaffolded by `tideorm init` can run its seeders the same way: `seeders::all().database(&db).run().await?`.

## Rolling a Test Back

On a server database every test shares, run the test in a transaction that ends with an error, which rolls back everything it wrote. Code under test that opens its own transaction runs as a savepoint inside it:

```rust
#[tokio::test]
async fn ships_an_order() -> tideorm::Result<()> {
    let db = Database::builder()
        .url(std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL"))
        .build()
        .await?;
    let outcome = db
        .transaction(async move |_tx| {
            let order = Order::create(Order { quantity: 2, ..Default::default() }).await?;
            assert!(ship(order.id).await?);
            Err::<(), _>(tideorm::Error::query("rolled back after the test"))
        })
        .await;
    assert!(outcome.is_err_and(|error| error.to_string().contains("rolled back after the test")));
    Ok(())
}
```

A failed assertion panics, which rolls the transaction back too.

## The Same Test on Several Backends

Write the body once, taking the database, and call it from one test per backend. Skip a server backend when its URL is not set:

```rust
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

## Code That Uses the Global Database

Code that calls `db()` or `require_db()` itself, or startup code built on `TideConfig`, needs the global database. Tests of it share one global slot, so run them one at a time (a shared lock, or `--test-threads=1`) and reset it between tests:

```rust
Database::reset_global();
TideConfig::reset(); // the tokenization key and codecs too

TideConfig::init()
    .database("sqlite::memory:")
    .connect()
    .await?;
```

## Checking Migrations in CI

`tideorm migrate status --check` exits with an error while a migration is pending, so a deploy step can refuse to start an application whose database is behind its code.
