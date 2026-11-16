# Quick Start

This page builds a complete program: two models with a relation, an in-memory SQLite database, and every basic operation from creating rows to deleting them. It needs no database server.

## Set Up the Project

```sh
cargo new tide-quickstart
cd tide-quickstart
```

```toml
[dependencies]
tideorm = { version = "0.17.9", default-features = false, features = ["sqlite", "runtime-tokio"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

## The Program

Replace `src/main.rs` with:

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub active: bool,

    // A relation is a field, not a column
    #[tideorm(has_many = "Post", foreign_key = "user_id")]
    pub posts: HasMany<Post>,
}

#[tideorm::model(table = "posts")]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub published: bool,

    #[tideorm(belongs_to = "User", foreign_key = "user_id")]
    pub author: BelongsTo<User>,
}

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    // Connect, and create the tables from the models (development only)
    TideConfig::init()
        .database("sqlite::memory:")
        .max_connections(1)
        .models::<(User, Post)>()
        .sync(true)
        .connect()
        .await?;

    // Create: an auto-increment key left at 0 is numbered by the database
    let user = User {
        email: "ada@example.com".into(),
        name: "Ada".into(),
        active: true,
        ..Default::default()
    }
    .save()
    .await?;

    for (title, published) in [("Hello", true), ("Draft", false)] {
        Post::create(Post {
            user_id: user.id,
            title: title.into(),
            published,
            ..Default::default()
        })
        .await?;
    }

    // Query with the generated typed columns
    let published = Post::query()
        .where_eq(Post::USER_ID, user.id)
        .where_eq(Post::PUBLISHED, true)
        .order_asc(Post::ID)
        .get()
        .await?;
    println!("published: {}", published.len());

    // Load a relation lazily...
    let posts = user.posts.load().await?;
    println!("{} wrote {} posts", user.name, posts.len());

    // ...or together with the rows that own it
    let users = User::query().with(User::POSTS).get().await?;
    let loaded = users[0].posts.get_cached().map_or(0, |posts| posts.len());
    println!("eager-loaded posts: {loaded}");

    // Update: find returns None when no row has the key
    let mut user = User::find(user.id).await?.expect("Ada exists");
    user.name = "Ada Lovelace".into();
    let user = user.update().await?;
    println!("renamed to {}", user.name);

    // Read one column, as its field's type (Vec<String>)
    let titles = Post::query().order_asc(Post::ID).pluck(Post::TITLE).await?;
    println!("titles: {titles:?}");

    // Delete
    Post::query().where_eq(Post::USER_ID, user.id).delete().await?;
    user.delete().await?;
    println!("users left: {}", User::query().count().await?);
    Ok(())
}
```

`cargo run` prints:

```text
published: 1
Ada wrote 2 posts
eager-loaded posts: 2
renamed to Ada Lovelace
titles: ["Hello", "Draft"]
users left: 0
```

## What the Program Does

- `#[tideorm::model(table = "..")]` turns a struct into a model. It generates `Default`, the serde impls, the CRUD methods, and a typed constant for each column (`Post::USER_ID`) and each relation (`User::POSTS`). A misspelt constant, or a value of the wrong type for its column, fails to compile. [Defining Models](models.md) covers every attribute.
- `TideConfig::init()..connect()` opens the connection pool and makes it the global database every model call uses. `sync(true)` creates missing tables and columns from the registered models; it is for development, and [Migrations](migrations.md) are for production. [Configuration](configuration.md) lists every setting.
- `save()` and `create()` insert a row and return the model as stored, with its key filled in. `find()`, `update()` and `delete()` work by primary key. [Reading and Writing](crud.md) covers them and the batch writes.
- `query()` starts a [query builder](queries.md); `get()`, `count()`, `pluck()` and `delete()` are some of its terminals.
- `load()` queries a relation; `with()` loads it for every row of a query in one extra query, read back with `get_cached()`. See [Relations](relations.md).

## Using Migrations Instead of Sync

In an application with real data, create tables with migrations: they are versioned, reversible, and each runs once. Replace `.models::<..>().sync(true)` with a migration:

```rust
use tideorm::migration::{Migration, Schema};
use tideorm::prelude::*;

struct CreateUsers;

#[tideorm::async_trait::async_trait]
impl Migration for CreateUsers {
    fn version(&self) -> &str {
        "20261115_001"
    }
    fn name(&self) -> &str {
        "create_users"
    }
    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema
            .create_table("users", |table| {
                table.id();
                table.string("email").not_null();
                table.string("name").not_null();
                table.boolean("active").not_null();
            })
            .await
    }
    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.drop_table("users").await
    }
}

async fn connect() -> tideorm::Result<()> {
    TideConfig::init()
        .database("sqlite::memory:")
        .max_connections(1)
        .migration(CreateUsers)
        .run_migrations(true)
        .connect()
        .await?;
    Ok(())
}
```

[Migrations](migrations.md) explains the schema builder and the `Migrator`, which runs, rolls back and reports migrations outside startup.

## Running on Another Backend

Swap the backend feature and the URL; the models and queries stay the same:

| Backend | Feature | URL |
|---|---|---|
| SQLite file | `sqlite` | `sqlite://./app.db?mode=rwc` (`mode=rwc` creates the file) |
| PostgreSQL | `postgres` | `postgres://user:pass@localhost/app` |
| MySQL or MariaDB | `mysql` | `mysql://user:pass@localhost/app` |

Read the URL from the environment, such as `DATABASE_URL`, rather than writing credentials into code. [Databases](databases.md) covers what differs between backends.

## Examples

The TideORM repository ships runnable programs in `examples/`. Each is a complete SQLite program, except `postgres`:

| Example | Shows |
|---|---|
| `crud` | Insert, typed filters, update and delete |
| `relations` | Declaring and eager-loading a has-many relation by its constant |
| `transactions` | Committing a closure, and returning `Err` to roll back |
| `migrations` | Registering and running a reversible migration |
| `scopes` | Declaring a reusable typed query scope |
| `postgres` | Connecting with `DATABASE_URL` and running a read-only query |

Run one from a checkout of the repository:

```sh
cargo +1.94.0 run --no-default-features --features sqlite,runtime-tokio --example crud
DATABASE_URL=postgres://user:password@localhost/database cargo +1.94.0 run --example postgres
```

The [tideorm-examples](https://github.com/mohamadzoh/tideorm-examples) repository holds larger end-to-end applications, and `tideorm init` from the [TideORM CLI](https://github.com/mohamadzoh/tideorm-cli) scaffolds a project with models, migrations and seeders already wired up.

## Where Next

- [Defining Models](models.md): attributes, keys, hidden fields and custom column names.
- [Reading and Writing](crud.md): every way to create, read, update and delete rows.
- [Query Builder](queries.md): filters, ordering, pagination and chunked reads.
- [Relations](relations.md): every relation type, eager loading and nested saves.
- [Transactions](transactions.md): grouping writes, savepoints, and choosing the database a call uses.
- [Errors](errors.md): telling a duplicate key from a deadlock, and what to retry.
