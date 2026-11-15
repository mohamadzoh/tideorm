# Troubleshooting

Common mistakes, what they look like, and the fix.

## "Global database connection not initialized"

A model call ran before `Database::init`, `Database::set_global` or `TideConfig::init()..connect()`, or after `Database::reset_global()`. Connect at startup, before serving requests. `db()` panics in this case; `require_db()` returns this error instead, so prefer it outside `main`. A test can run its code on a database of its own with `db.scope(..)` and skip the global one (see [Testing](testing.md)).

## A `before_save` (or Other Hook) Never Runs

Callbacks are found through the `Callbacks` trait. A method with the same name in a plain `impl User { .. }` block compiles and is never called. Move it into `impl Callbacks for User { .. }` (see [Callbacks and Timestamps](callbacks.md)). Hooks also run only for single-model writes: `insert_all`, `update_all`, upserts and query writes (`query().delete()`) skip them.

## `conflicting implementations of trait Serialize` (E0119)

A `#[derive(Serialize, Deserialize)]` written above `#[tideorm::model]`, or in the same `#[derive(..)]` list as `Model`, conflicts with the serde impls TideORM generates. Put your own derive *below* the model attribute; TideORM then reads its `#[serde(..)]` renames and writes JSON keys the way serde would (see [Defining Models](models.md)).

## A Model Is Missing from `sync`, `auto_models()` or `models_matching(..)`

Models register themselves when their file is compiled. A file no `mod` declaration reaches is not compiled, so its models register nothing, and a `models_matching` pattern that matches no compiled model only logs a warning. Add the `mod` line.

## "requires at least one explicit filter" / "matches every row"

`update_all()`, `query().delete()` and the other bulk writes refuse to run without a filter, so a forgotten `where_*` cannot touch the whole table. A filter that cannot exclude a row (an empty `where_not_in`, a `%` pattern, a raw `1 = 1`) is refused the same way. Add the filter you meant. To delete every row on purpose, call `delete_all()`.

## `get()` Refuses a `select(..)`

A query that loads models must select every model column: an unselected `Option` field would read as `None` and look like data. Select every column, or read the columns you want with `pluck`, `get_json()` or a `#[derive(FromQueryResult)]` struct through `raw_as`.

## A Spawned Task Does Not See the Transaction's Rows

A transaction follows the future it was given. Work started with `tokio::spawn` (or `async_std::task::spawn`) inside it runs on the global database, outside the transaction: it cannot see uncommitted rows and can wait on the transaction's own locks. Await the work inside the transaction, or start it after the transaction ends. See [Transactions](transactions.md).

## "transaction handle leaked outside the transaction scope"

A clone of the transaction handle outlived the closure, usually by being moved into a spawned task or stored somewhere. The transaction is rolled back. Do not keep the handle: calls inside the closure join the transaction without it.

## A `DATE` or `TIMESTAMP` Column Fails to Decode on PostgreSQL

PostgreSQL's `infinity` and `-infinity` dates and `TIME '24:00:00'` have no Rust date or time, so a typed read of such a row fails with an error naming the column. Read it with `get_json()`, which returns them as text, or keep them out of the column.

## `force_sync` Refuses to Run

With `TIDEORM_ENV=production`, `force_sync(true)` is refused, because it drops and recreates every registered table. Use [migrations](migrations.md) in production. To run it anyway, for example against a staging database you mean to empty, set `TIDEORM_ALLOW_FORCE_SYNC=true`.

## The CLI Connects to the Wrong Database

The CLI reads `DATABASE_URL` from the process environment before the project's env file, so a `DATABASE_URL` exported in your shell wins over `.env`. `tideorm db status` shows where the URL it uses came from. Unset the shell variable, or set the one you mean.

## A Query Fails on One Backend Only

Check the SQL TideORM will run, in the backend's own dialect, before running it:

```rust
let query = User::query().where_eq(User::ACTIVE, true).order_desc(User::ID);
println!("{}", query.debug());
println!("{}", query.build_sql_preview_for(DatabaseType::MySQL));
```

Some features exist only on some backends. A query that uses one where it is missing fails when it runs, rather than running without it: a window frame of `GROUPS` on MySQL or MariaDB is an `Error::Query` naming the frame, and `execute_returning()` on MySQL or MariaDB is an `Error::BackendNotSupported`. [Backend Differences](databases.md#backend-differences) lists them. [Errors](errors.md) explains how to tell a backend's refusal from a failure worth retrying.

## A Query Is Refused by a Project Rule

A `tideorm.toml` with a `[rules]` table can refuse raw SQL, unbounded reads, long `IN` lists and deep eager loads, and fail the build on models that break its model rules. The error names the rule. Exempt the model in `[rules.allow]` or change the query (see [Project Rules](project-rules.md)).

## Statements Are Slow or There Are Too Many

Set `TIDE_LOG_QUERIES=true` to print every statement, or enable `QueryLogger` to collect slow ones, and look for one read per row of a list, the sign of a relation loaded in a loop: load it with `with(..)` instead. See [Logging and Profiling](profiling.md).
