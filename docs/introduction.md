# Introduction

TideORM is an async ORM for Rust applications. You declare a model as a plain struct with one attribute, its relations as fields of that struct, and read and write rows through a fluent query builder. It runs on PostgreSQL, MySQL, MariaDB and SQLite, and on CockroachDB, YugabyteDB, TiDB and Vitess through the PostgreSQL and MySQL drivers.

This book is for Rust developers building an application on a SQL database: a web service, a worker, a command-line tool. It assumes you know Rust and async/await; it does not assume you know any other ORM.

## A First Look

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub active: bool,

    #[tideorm(has_many = "Post", foreign_key = "user_id")]
    pub posts: HasMany<Post>,
}

async fn recent_active_users() -> tideorm::Result<Vec<User>> {
    User::query()
        .where_eq(User::ACTIVE, true)
        .with(User::POSTS)
        .order_desc(User::ID)
        .limit(10)
        .get()
        .await
}
```

`#[tideorm::model]` generates everything else: the table mapping, `User::create`, `User::find`, `user.update()`, the typed column constants (`User::ACTIVE`) and the relation names (`User::POSTS`). A misspelt column or a value of the wrong type is a compile error.

## What TideORM Gives You

- **Models** with validation, callbacks, automatic timestamps, soft deletes, query scopes, encrypted fields and tokenized IDs.
- **Queries** that cover filters, OR groups, joins, aggregates, unions, window functions, CTEs, JSON and array operators, full-text search and pagination, plus raw SQL when you need it.
- **Relations** declared as struct fields (`HasOne`, `HasMany`, `BelongsTo`, `HasManyThrough` and polymorphic ones), loaded lazily or in batches.
- **Data tools**: transactions with savepoints, migrations, seeders, schema sync for development, and schema export.
- **Operations**: classified errors you can retry on, query logging, profiling and opt-in project rules checked by `cargo check`.

Optional modules (file attachments, translations, full-text search, the entity manager, dirty tracking and encrypted fields) sit behind Cargo features that add no dependencies.

## Reading Map

| If you want to | Read |
|---|---|
| Add TideORM to a project | [Installation](installation.md), then [Quick Start](quick-start.md) |
| Connect, tune the pool or set options | [Configuration](configuration.md) |
| Know how a backend differs, or how Rust types map to columns | [Databases](databases.md) and [Compatible Servers](compatible-servers.md) |
| Define models and write rows | [Defining Models](models.md), [Reading and Writing](crud.md) |
| Build queries | [Query Builder](queries.md) and the pages under it |
| Load related rows | [Relations](relations.md) |
| Group writes, change the schema, fill tables | [Transactions](transactions.md), [Migrations](migrations.md), [Seeding](seeding.md) |
| Handle failures and see what runs | [Errors](errors.md), [Logging and Profiling](profiling.md) |
| Test your application | [Testing](testing.md) |
| Fix something that does not work | [Troubleshooting](troubleshooting.md) |

The [API cheat sheet](https://github.com/mohamadzoh/tideorm/blob/master/API.md) lists signatures at a glance, and the [Rust API reference](https://docs.rs/tideorm) documents every public item, optional modules included. The [TideORM CLI](https://github.com/mohamadzoh/tideorm-cli) scaffolds projects and runs migrations and seeders, and [tideorm-examples](https://github.com/mohamadzoh/tideorm-examples) holds complete applications.
