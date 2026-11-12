# Troubleshooting

Common mistakes, what they look like, and the fix.

## "Global database connection not initialized"

A model call ran before `Database::init`, `Database::set_global` or `TideConfig::init()..connect()`, or after `Database::reset_global()`. Connect at startup, before serving requests. `db()` panics in this case; `require_db()` returns this error instead, so prefer it outside `main`. A test can run its code on a database of its own with `db.scope(..)` and skip the global one (see [Testing](testing.md)).

## A `before_save` (or other hook) never runs

Callbacks are found through the `Callbacks` trait. A method with the same name in a plain `impl User { .. }` block compiles and is never called. Move it into `impl Callbacks for User { .. }`. Hooks also run only for single-model writes: `insert_all`, `update_all`, upserts and query writes (`query().delete()`) skip them.

## `conflicting implementations of trait Serialize` (E0119)

A `#[derive(Serialize, Deserialize)]` written above `#[tideorm::model]`, or in the same `#[derive(..)]` list as `Model`, conflicts with the serde impls TideORM generates. Put your own derive *below* the model attribute; TideORM then reads its `#[serde(..)]` renames and writes JSON keys the way serde would.

## A model is missing from `sync`, `auto_models()` or `models_matching(..)`

Models register themselves when their file is compiled. A file no `mod` declaration reaches is not compiled, so its models register nothing, and a `models_matching` pattern that matches no compiled model only logs a warning. Add the `mod` line.

## "requires at least one explicit filter" / "matches every row"

`update_all()`, `query().delete()` and the other bulk writes refuse to run without a filter, so a forgotten `where_*` cannot touch the whole table. A filter that cannot exclude a row (an empty `where_not_in`, a `%` pattern, a raw `1 = 1`) is refused the same way. Add the filter you meant. To delete every row on purpose, call `delete_all()`.

## `get()` refuses a `select(..)`

A query that loads models must select every model column: an unselected `Option` field would read as `None` and look like data. Select every column, or read the columns you want with `pluck`, `get_json()` or a `#[derive(FromQueryResult)]` struct through `raw_as`.

## A spawned task does not see the transaction's rows

A transaction follows the future it was given. Work started with `tokio::spawn` (or `async_std::task::spawn`) inside it runs on the global database, outside the transaction: it cannot see uncommitted rows and can wait on the transaction's own locks. Await the work inside the transaction, or start it after the transaction ends.

## "transaction handle leaked outside the transaction scope"

A clone of the transaction handle outlived the closure, usually by being moved into a spawned task or stored somewhere. The transaction is rolled back. Do not keep the handle: calls inside the closure join the transaction without it.

## A `DATE`/`TIMESTAMP` column fails to decode on PostgreSQL

PostgreSQL's `infinity` and `-infinity` dates and `TIME '24:00:00'` have no Rust date or time, so a typed read of such a row fails with an error naming the column. Read it with `get_json()`, which returns them as text, or keep them out of the column.

## `force_sync` refuses to run

With `TIDEORM_ENV=production`, `force_sync(true)` is refused, because it drops and recreates every registered table. Use migrations in production. To run it anyway, for example against a staging database you mean to empty, set `TIDEORM_ALLOW_FORCE_SYNC=true`.

## The CLI connects to the wrong database

The CLI reads `DATABASE_URL` from the process environment before the project's env file, so a `DATABASE_URL` exported in your shell wins over `.env`. `tideorm db status` shows where the URL it uses came from. Unset the shell variable, or set the one you mean.

## A query fails with a backend's syntax error

Check the SQL TideORM will run, in the backend's own dialect, before running it:

```rust
let query = User::query().where_eq("active", true).order_desc("created_at");
println!("{}", query.debug());
```

Some clauses exist only on some backends, such as `DISTINCT ON` (PostgreSQL only): a query that uses one on another backend fails when it runs, rather than running without it. The [feature matrix](queries.md#feature-compatibility-matrix) lists the main differences.
