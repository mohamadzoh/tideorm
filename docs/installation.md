# Installation

Add `tideorm` to your `Cargo.toml` with one backend feature and one runtime feature, and add the async runtime itself:

```toml
[dependencies]
tideorm = { version = "0.18.2", default-features = false, features = ["postgres", "runtime-tokio"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

That is all a project needs to define models and run queries; serde, chrono and serde_json come through `tideorm` and its prelude. The [Quick Start](quick-start.md) builds a complete program on it.

The dependency may be renamed (`orm = { package = "tideorm", .. }`, then `#[orm::model]`): the macros read the name from the crate's `Cargo.toml`, `[workspace.dependencies]` included. A crate that defines models must depend on `tideorm` itself, under whatever name; a re-export through another crate is not enough.

## Choosing a Backend

Each backend is a Cargo feature that builds its driver:

```toml
# PostgreSQL
tideorm = { version = "0.18.2", default-features = false, features = ["postgres", "runtime-tokio"] }

# MySQL or MariaDB
tideorm = { version = "0.18.2", default-features = false, features = ["mysql", "runtime-tokio"] }

# SQLite
tideorm = { version = "0.18.2", default-features = false, features = ["sqlite", "runtime-tokio"] }
```

The default feature set is `["postgres", "runtime-tokio"]`. Set `default-features = false` and name your features explicitly: a MySQL- or SQLite-only project that leaves the defaults on still compiles the whole PostgreSQL driver.

Backend features add to each other. With several enabled, the connection URL picks the backend at runtime (see [Databases](databases.md)), so one binary can talk to PostgreSQL and SQLite at once. A URL for a backend whose feature is not compiled in is refused with a configuration error that names the feature to enable.

| Feature | Builds | Use it for |
|---|---|---|
| `postgres` | The PostgreSQL driver | PostgreSQL |
| `mysql` | The MySQL driver | MySQL and MariaDB |
| `sqlite` | The SQLite driver, with a bundled SQLite (3.46 or later) | SQLite; no system library is needed |
| `mariadb`, `tidb`, `vitess` | The same as `mysql` | Naming the server you run in your manifest |
| `cockroachdb`, `yugabytedb` | The same as `postgres` | Naming the server you run in your manifest |

The server-named features are aliases: `features = ["cockroachdb"]` and `features = ["postgres"]` build the same code. [Compatible Servers](compatible-servers.md) lists what differs on each server.

## Choosing a Runtime

| Feature | Runtime |
|---|---|
| `runtime-tokio` | Tokio (default) |
| `runtime-async-std` | async-std |

Enable one, and add that runtime to your own dependencies so you can start it (`#[tokio::main]`, `#[async_std::main]`).

## Optional Modules

Six features unlock optional APIs. They add no dependencies; they only make the API visible:

| Feature | Unlocks |
|---|---|
| `attachments` | File attachments on models (`HasAttachments`, `tideorm::attachments`); see [File Attachments](attachments.md) |
| `translations` | Translated fields (`HasTranslations`, `ApplyTranslations`, `tideorm::translations`); see [Translations](translations.md) |
| `fulltext` | Full-text search and highlighting (`tideorm::fulltext`); see [Full-Text Search](full-text-search.md) |
| `entity-manager` | The `EntityManager` persistence context: an identity map, `find`, `find_managed`, `load`, `save`, `persist`, `merge`, `remove`, `detach` and `flush`; see [Entity Manager](entity-manager.md) |
| `dirty-tracking` | `changed_fields()` and `original_value()` on models; see [Reading and Writing](crud.md) |
| `encrypted-fields` | `#[tideorm(encrypted = "..")]` columns, encrypted on write and decrypted on load; see [Encrypted Fields](encrypted-fields.md) |

```toml
tideorm = { version = "0.18.2", default-features = false, features = ["postgres", "runtime-tokio", "fulltext", "entity-manager"] }
```

Each page for a feature-gated API names its feature at the top.

## What Else Your Crate Needs

Nothing, for the models themselves. The code `#[tideorm::model]` generates reaches `serde`, `serde_json` and `chrono` through TideORM, and `tideorm::prelude` re-exports the types models usually hold: `DateTime`, `Utc`, `NaiveDate`, `NaiveDateTime`, `NaiveTime`, `Decimal`, `Uuid` and `JsonValue`.

Add a dependency only when your own code needs it:

- `serde`, if your own code derives `Serialize` or `Deserialize`.
- `chrono`, if you spell a type with an explicit `chrono::` path.

## Supported Rust Versions

TideORM needs Rust 1.94 or later. Every TideORM crate (the macros, the engine, the SQL builder and the drivers) declares `rust-version = "1.94"`, and CI checks the library with that toolchain. To build with it explicitly, run `cargo +1.94.0 build`.

## The CLI

The [TideORM CLI](https://github.com/mohamadzoh/tideorm-cli) is a separate crate whose binary is named `tideorm`. It scaffolds projects (`tideorm init`), generates models, migrations and seeders, and runs migrations and seeders through your project's own code. You do not need it to use the library.
