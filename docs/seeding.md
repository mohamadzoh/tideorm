# Seeding

Seeds fill a database with the rows an application needs to start, such as roles, categories or a first admin user, or with sample data for development. TideORM records each seed it runs, so running the seeds again skips the ones already done.

```rust
use tideorm::prelude::*;

struct RoleSeeder;

#[async_trait]
impl Seed for RoleSeeder {
    fn name(&self) -> &str {
        "role_seeder"
    }

    async fn run(&self, _db: &Database) -> tideorm::Result<()> {
        for name in ["admin", "editor", "viewer"] {
            Role::create(Role { id: 0, name: name.to_string() }).await?;
        }
        Ok(())
    }

    async fn rollback(&self, _db: &Database) -> tideorm::Result<()> {
        Role::query().where_in("name", ["admin", "editor", "viewer"]).delete().await?;
        Ok(())
    }
}

async fn seed_database() -> tideorm::Result<()> {
    let result = Seeder::new().add(RoleSeeder).run().await?;
    println!("ran {} seeds", result.executed.len());
    Ok(())
}
```

## Writing a Seed

The `Seed` trait has two required methods and three optional ones:

| Method | Default | Meaning |
|---|---|---|
| `name()` | Required | A unique name, stored in the ledger; use a descriptive snake_case name such as `user_seeder` |
| `run(db)` | Required | Inserts the seed's rows |
| `rollback(db)` | Does nothing | Removes what `run` inserted |
| `priority()` | `100` | Lower runs first |
| `depends_on()` | None | Names of seeds that must run before this one |

Each seed runs in one transaction with its ledger entry, so a seed that fails part way leaves neither rows nor an entry behind, and can be fixed and run again without duplicating what it wrote. Model calls inside `run` that name no database join that transaction. A rollback reverts the seed and removes its entry in one transaction too.

## Running Seeds

At startup, register seeds on `TideConfig` and turn running them on:

```rust
TideConfig::init()
    .database("postgres://localhost/mydb")
    .seed(RoleSeeder)
    .seed(AdminUserSeeder)
    .run_seeds(true)
    .connect()
    .await?;
```

Seeds run only with `run_seeds(true)`, which is off by default. They run after migrations and schema sync, so they can fill the tables either creates, and a `force_sync` does not drop what they wrote. If seeding fails, what already ran is **not** rolled back: `connect()` returns the error with the database partially initialized. `connect_only()` skips seeds.

For more control, use a `Seeder`:

```rust
let seeder = Seeder::new()
    .add(RoleSeeder)
    .add(AdminUserSeeder);

seeder.run().await?;                     // every pending seed
seeder.run_seed("role_seeder").await?;   // one seed, even if it already ran
seeder.rollback().await?;                // undo the last seed
seeder.rollback_steps(2).await?;         // undo the last two
seeder.rollback_seed("role_seeder").await?; // undo one seed by name
seeder.reset().await?;                   // undo every seed
seeder.refresh().await?;                 // reset, then run

for status in seeder.status().await? {
    println!("{} (priority {}): {}", status.name, status.priority, status.executed);
}
let names: Vec<&str> = seeder.names().collect(); // needs no database

// Another database than the global one: a tenant's, or a test's own
Seeder::new().add(RoleSeeder).database(&tenant_db).run().await?;
```

With `database(&db)`, the seeds, their ledger and their lock all run on `db`, and so do the model calls a seed makes. `add_boxed(Box<dyn Seed>)` registers a boxed seed.

| Method | Returns | Notes |
|---|---|---|
| `run()` | `Result<SeedResult, SeedFailure>` | Skips seeds the ledger records |
| `run_seed(name)` | `Result<SeedResult>` | Runs a seed again even if it already ran, once every seed it depends on has run; a re-run keeps its original ledger entry |
| `rollback()`, `rollback_steps(n)` | `Result<SeedResult, SeedFailure>` | Newest first |
| `rollback_seed(name)` | `Result<SeedResult>` | Fails with `Seed '<name>' has not run` when the ledger does not record it |
| `reset()` | `Result<SeedResult, SeedFailure>` | Creates the ledger first, so on a fresh database it returns an empty result: "reset, then run" works as a bootstrap |
| `refresh()` | `Result<SeedResult, SeedFailure>` | `reset()`, then `run()` |
| `status()` | `Result<Vec<SeedStatus>>` | `name`, `executed` and `priority` of each registered seed |

A `SeedResult` lists the seeds it `executed`, `skipped` and `rolled_back` by name; `has_executed()` says whether anything ran.

A failing seed stops a batch command (`run`, `rollback`, `rollback_steps`, `reset`, `refresh`); the seeds it ran or rolled back before stay so. The error, a `SeedFailure`, holds them in its `result` beside the `error`, and `?` turns it into that `tideorm::Error`.

## Order and Dependencies

```rust
struct AdminUserSeeder;

#[async_trait]
impl Seed for AdminUserSeeder {
    fn name(&self) -> &str { "admin_user_seeder" }
    fn depends_on(&self) -> Vec<&str> { vec!["role_seeder"] }

    async fn run(&self, _db: &Database) -> tideorm::Result<()> {
        let admin = Role::query().where_eq("name", "admin").first_or_fail().await?;
        User::create(User { id: 0, role_id: admin.id, ..Default::default() }).await?;
        Ok(())
    }
}
```

Each seed runs after the seeds it depends on. Among the seeds whose dependencies have all run, the lowest `priority()` goes next, and equal priorities keep the order they were added in.

- A dependency must have run, in this run or an earlier one. `run()` fails on a seed whose dependency the ledger does not record and this seeder does not run first; `run_seed()` refuses one whose dependency has not run.
- A dependency on a seed this seeder does not hold orders nothing; it only has to be in the ledger already.
- A dependency cycle is a configuration error naming the seeds on it.

## The Ledger

Seeds that ran are recorded in the `_seeds` table, which TideORM creates when it first needs it and no migration owns. A rollback undoes the seed the ledger recorded last. When that seed is no longer registered (renamed or removed since it ran), the rollback fails rather than skip it, and the error says to register it again or delete its `_seeds` row.

## The Seed Lock

Complete seed operations take a database lock before reading the ledger, so concurrent runners, such as replicas booting together, never run one pending seed twice. The lock works as the [migration lock](migrations.md#the-migration-lock) does, under its own name: `pg_advisory_lock` with a key of its own on PostgreSQL, `GET_LOCK('tideorm_seeds:<database>', 300)` on MySQL and MariaDB, and a `<database file>.tideorm-seeds.lock` sidecar file on SQLite.

- A seed's own code must not start another mutating seed operation: it already holds the lock.
- Inside a transaction of yours, the lock stays held until that transaction ends, across savepoints and sequential seed operations.
- Inside a SQLite transaction whose read snapshot is out of date, a seed operation fails before any seed runs; restart that transaction to see the committed ledger.

## Logging

Set `TIDE_LOG_SEEDS=1` (or `TIDE_LOG_QUERIES=1`) to log each seed as it runs, completes and rolls back. Only `1` and `true` turn either on.

## From the CLI

`tideorm db seed` runs a project's seeders through the project's own runner, the `<package>-seed` binary the CLI scaffolds, which connects with `connect_only()` and runs the `Seeder` that `src/seeders/mod.rs` builds. Everything on this page holds there too.
