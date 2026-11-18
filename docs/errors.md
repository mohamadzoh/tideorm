# Errors

Every fallible TideORM call returns `tideorm::Result<T>`, whose error is `tideorm::Error`. The error says what kind of failure it was, what the database reported independent of backend and wording, whether retrying can succeed, and what status and stable code to answer a client with.

```rust
use tideorm::prelude::*;

async fn register(user: User) -> tideorm::Result<Option<User>> {
    match User::create(user).await {
        Ok(user) => Ok(Some(user)),
        // a unique index on users.email
        Err(e) if e.is_unique_violation() => Ok(None),
        Err(e) => Err(e),
    }
}
```

`tideorm::Result` is not in the prelude (it would shadow `std`'s `Result`), so write it out. `Error` and `DbFailureKind` are in the prelude; `ErrorContext` and `DbFailure` are in `tideorm::error`.

## The Error Type

`Error` is an enum grouped by where the failure came from. It is `#[non_exhaustive]`, so a `match` on it needs a wildcard arm.

| Variant | Raised when | `code()` | `http_status()` |
|---|---|---|---|
| `NotFound` | A record that must exist does not (`find_or_fail`, `first_or_fail`, ..) | `TIDE_NOT_FOUND` | 404 |
| `Connection` | A connection cannot be opened or used: refused, timed out, bad credentials, unknown database, no database configured | `TIDE_CONNECTION` | 503 |
| `Query` | A statement fails on the server, or the query builder refuses a query before sending it | `TIDE_QUERY` | 400 |
| `Validation` | A model fails its `#[validate]` rules; nothing was written | `TIDE_VALIDATION` | 422 |
| `Conversion` | A value cannot be converted, such as a column that does not decode into its field | `TIDE_CONVERSION` | 400 |
| `Transaction` | A transaction cannot continue or commit: it was aborted by an earlier failure, or its handle leaked out of its closure | `TIDE_TRANSACTION` | 409 |
| `Configuration` | The setup is wrong: no URL, a backend feature not compiled in, a bad pool setting | `TIDE_CONFIG` | 500 |
| `Internal` | Something unexpected, such as an I/O error | `TIDE_INTERNAL` | 500 |
| `BackendNotSupported` | The backend cannot do what was asked, such as `execute_returning()` on MySQL | `TIDE_BACKEND_NOT_SUPPORTED` | 501 |
| `PrimaryKeyNotSet` | An operation needs a primary key the model does not hold | `TIDE_PRIMARY_KEY_NOT_SET` | 400 |
| `Tokenization` | Tokens cannot be made, usually because no encryption key is configured | `TIDE_TOKENIZATION` | 400 |
| `InvalidToken` | `decode_token` was given a token that is invalid, tampered or for another model | `TIDE_INVALID_TOKEN` | 401 |
| `AccessDenied`, `Rbac` | Your own access checks, built with `Error::access_denied(permission, resource)` and `Error::rbac(..)` | `TIDE_ACCESS_DENIED`, `TIDE_RBAC` | 403, 500 |

`code()` does not change between releases, so it is what an API response or an alert should key on; the `Display` text may.

## Classifying Database Failures

A duplicate key and a deadlock are both `Error::Query`. `failure_kind()` tells them apart: it is the driver's classification of what the database reported (PostgreSQL's SQLSTATE, MySQL's error number, SQLite's result code), never a reading of the message. `is_retryable()` says whether running the operation again can succeed.

```rust
use tideorm::prelude::*;

match User::create(user).await {
    Ok(user) => { /* created */ }
    Err(e) if e.failure_kind() == DbFailureKind::UniqueViolation => { /* email taken */ }
    Err(e) if e.is_retryable() => { /* deadlock, lock timeout, dropped connection: retry */ }
    Err(e) => return Err(e),
}
```

`is_unique_violation()`, `is_foreign_key_violation()` and `is_not_null_violation()` ask the same about one kind.

| `DbFailureKind` | Database report | Retryable |
|---|---|---|
| `UniqueViolation` | SQLSTATE `23505` | No |
| `ForeignKeyViolation` | `23503` | No |
| `NotNullViolation` | `23502` | No |
| `CheckViolation` | `23514` | No |
| `ExclusionViolation` | `23P01` (PostgreSQL `EXCLUDE` constraints) | No |
| `InvalidValue` | Class `22`: a value too long, out of range or invalid for its type | No |
| `SyntaxError` | `42601`, MySQL `42000` | No |
| `UndefinedColumn` | `42703`, MySQL `42S22` | No |
| `UndefinedTable` | `42P01`, MySQL `42S02` | No |
| `InsufficientPrivilege` | `42501` | No |
| `Deadlock` | `40P01`, MySQL 1213 | Yes |
| `SerializationFailure` | `40001`, such as CockroachDB's "restart transaction" | Yes |
| `LockNotAvailable` | `55P03`, MySQL's lock wait timeout, a busy or locked SQLite database | Yes |
| `StatementTimeout` | `57014` | Yes |
| `ConnectionTimeout` | `53300`, or no pooled connection within `acquire_timeout` | Yes |
| `ConnectionClosed` | Class `08` but `08P01`, and the server's own session timeouts (`57P05`, `25P03`, `25P04`, MySQL 4031): the server closed the connection | Yes |
| `ConnectionRefused` | Nothing listens at the host and port, or the server is not up yet | Yes |
| `AuthenticationFailed` | `28P01`, `28000`, MySQL 1045 | No |
| `UnknownDatabase` | `3D000`, MySQL 1049 | No |
| `TlsFailed` | The TLS handshake failed | No |
| `InvalidConfiguration` | A bad URL or connection option | No |
| `NotConnected` | No connection is set up: a model used before connecting | No |
| `Unclassified` | Anything else, and every error that did not come from the database | No |

`DbFailureKind` is `#[non_exhaustive]`; `as_str()` gives a readable name, and `is_constraint_violation()` is true for the five constraint kinds, which the caller fixes by changing the data rather than retrying.

A few cases worth knowing:

- SQLite checks neither text length nor type, so a string longer than its `VARCHAR(255)` is stored there and refused (`InvalidValue`) by PostgreSQL and MySQL. `#[validate(max_length = 255)]` keeps the backends in step.
- A connection the server closes under a statement (a restart, a failover, MySQL's `KILL`) is `ConnectionClosed`. The pool replaces it, so the next call gets a fresh one.
- A statement refused on a transaction that a failure already aborted reports that failure's kind, so the refusals after a deadlock are retryable too.
- An error with no classified failure, such as one built with `Error::query(..)`, is not retryable, whatever its message says.

## Constraint and Driver Details

The `Connection`, `Query` and `Transaction` variants carry the driver's report when there was one:

```rust
if let Some(failure) = e.db_failure() {
    println!("{:?} {:?}", failure.kind(), failure.code()); // UniqueViolation Some("23505")
    println!("{:?} {:?}", failure.constraint(), failure.table());
}
```

| Method | Returns |
|---|---|
| `e.db_failure()` | The `DbFailure`, or `None` for failures that never reached a driver (query-builder misuse, validation, configuration) |
| `e.sqlstate()` | The SQLSTATE, or the driver's native code where the backend has none (SQLite) |
| `e.constraint()` | The violated constraint's name; only PostgreSQL reports it, MySQL and SQLite name it inside the message |
| `failure.table()` | The table the statement was writing, when the backend reports it |

An error that came from a driver keeps the driver's error as its `source()`, so `{:#}` chains and `anyhow` reach the backend's own error. `From<std::io::Error>` gives an `Internal` error and `From<serde_json::Error>` a `Conversion` error, each keeping its source, so `?` works on both inside a function returning `tideorm::Result`.

## Connection Errors

Every failure to open or use a connection is an `Error::Connection`, which `http_status()` answers with 503: a refused connection, a timeout waiting for a pooled connection, bad credentials, an unknown database, a failed TLS handshake, or a model used before any connection was set up (`NotConnected`). The kinds in the table above tell them apart.

Only `ConnectionRefused` and the timeouts are retryable. `AuthenticationFailed`, `UnknownDatabase`, `TlsFailed` and `InvalidConfiguration` are deployment faults that a retry will not fix, and belong in an alert as well as a 503. A URL for a backend whose Cargo feature is not compiled in is an `Error::Configuration` naming the feature to enable (see [Installation](installation.md)).

`db()` panics when no global connection is set; `require_db()` returns the same failure as an `Err`. See [Configuration](configuration.md) for connecting.

## Transaction Errors

`Error::Transaction` means the transaction cannot go on:

- A statement failed inside it and the transaction was aborted. Later statements on it are refused, and its commit rolls back and fails instead of committing. The refusal reports the original failure's kind, so `is_retryable()` is true after a deadlock or serialization failure.
- A clone of the transaction handle escaped the closure, so the commit fails with "transaction handle leaked outside the transaction scope" and the transaction is rolled back.
- A nested transaction whose enclosing one already ended refuses its statements.

See [Transactions](transactions.md) for how transactions and savepoints behave.

## Validation Errors

`Error::Validation` comes from a model's `#[validate]` rules before anything is written. Its `field` and `message` are the first failure; `validation_errors()` returns every failure, field by field:

```rust
use tideorm::prelude::*;

if let Err(e) = User::create(user).await {
    if let Some(errors) = e.validation_errors() {
        for (field, messages) in errors.all() {
            println!("{field}: {}", messages.join(", "));
        }
    }
}
```

`ValidationErrors` also has `get(field)`, `first()`, `messages()`, `iter()`, `len()` and `is_empty()`. Validation messages are written for users, so they can go out to a client as they are. See [Validation](validation.md) for the rules.

## Error Context

Errors from query-builder reads, batch inserts, catalog reads and migration schema reads carry an `ErrorContext`: the table, the column, the conditions, how they were combined, and the SQL. `context()` returns it for `NotFound` and `Query` errors:

```rust
if let Some(ctx) = e.context() {
    println!("table: {:?}", ctx.table);
    println!("conditions: {:?}", ctx.conditions); // values are shown as `?`
    println!("query: {:?}", ctx.query);
}
```

The conditions show `?` for the values you filtered by, so the context is safe to log. Call `debug()` on the query to see the values (see [Logging and Profiling](profiling.md)). A `NotFound` from `Model::find_or_fail(id)` carries no context; its message names the table and the key, such as `users with id = 7 not found`.

`with_context(ErrorContext)` attaches context to a `NotFound` or `Query` error you build or pass on, keeping the database failure it carries; other variants come back unchanged:

```rust
use tideorm::error::ErrorContext;

let err = Error::query("order total is negative")
    .with_context(ErrorContext::new().table("orders").column("total"));
```

## Presenting an Error

| Method | Gives |
|---|---|
| `code()` | The stable code, such as `TIDE_VALIDATION` |
| `http_status()` | A status for the variant (see [The Error Type](#the-error-type)) |
| `suggestion()` | A next step, per database failure kind where there is one (an `UndefinedTable` suggests running migrations), otherwise per variant |
| `log_format()` | `[CODE] message`, then indented `Table:`, `Conditions:`, `Query:`, `Database:` and `Suggestion:` lines |
| `Display` | One line starting with the variant, such as `Record not found: ..` or `Validation error: email - ..` |

### Answering a Request

`http_status()` answers 400 for every `Error::Query`, a duplicate key and a deadlock included, so an endpoint looks at `failure_kind()` first:

```rust
use tideorm::prelude::*;

/// The status and body an endpoint answers a TideORM error with. The body
/// carries the stable `code()`, never the error's text.
fn error_response(e: &Error) -> (u16, JsonValue) {
    if let Some(errors) = e.validation_errors() {
        // 422 with every failure, field by field: {"email": ["..."]}
        return (422, json!({ "code": e.code(), "errors": errors.all() }));
    }
    let status = match e.failure_kind() {
        DbFailureKind::UniqueViolation => 409,
        // deadlock, lock or statement timeout, dropped connection
        _ if e.is_retryable() => 503,
        // NotFound 404, Connection 503, Transaction 409, Configuration 500, ..
        _ => e.http_status(),
    };
    if status >= 500 {
        tracing::error!("{}", e.log_format());
    }
    (status, json!({ "code": e.code() }))
}
```

An error's `Display` text can hold the server's message, table, column and constraint names, and SQL; `log_format()` adds the query and its context. Send `code()` to the client and keep the rest in your logs.

## Retrying a Transaction

Retry the whole transaction, never one statement inside it: after a deadlock or a serialization failure the server has already rolled the whole transaction back. `transaction_retrying` does that:

```rust
use tideorm::prelude::*;

async fn place_order_with_retry(db: &Database, cart: &Cart) -> tideorm::Result<Order> {
    db.transaction_retrying(TransactionOptions::default(), 4, async |_tx| place_order(cart).await)
        .await
}
```

It runs the closure in a new transaction after a `Deadlock`, a `SerializationFailure` or a `LockNotAvailable`, up to the given number of runs in all, waiting 10 ms before the second, doubling up to 1 s, each wait shortened by a random part so competing transactions spread out. Any other failure, and the last attempt's, comes back as it is. It never retries a lost connection, whose commit may have gone through. Inside another transaction on the same pool it runs the closure once, as a savepoint that ignores the options the enclosing transaction began with its own: a deadlock or a serialization failure aborts the enclosing transaction, so that is the one to retry. The closure can run more than once, so do what happens outside the database, such as sending an email, after it returns.

CockroachDB runs every transaction at `SERIALIZABLE` and asks clients to retry with SQLSTATE `40001` ("restart transaction"), which is `SerializationFailure`, as is TiDB's write conflict (error 9007), so the same call serves them (see [Compatible Servers](compatible-servers.md)). PostgreSQL's `SERIALIZABLE` and `REPEATABLE READ` isolation levels report the same code.

To retry on other failures too, such as a statement timeout, write the loop yourself around `is_retryable()`.

Retryable means a new attempt can succeed, not that the failed one wrote nothing. A `ConnectionClosed` that cuts off the `COMMIT` leaves it unknown whether the transaction committed, and the retry then writes it a second time. The same holds for a single `create()` retried outside a transaction. Make a retried write idempotent: insert with a key the request carries (a client-chosen UUID, an idempotency key under a unique index) or use an upsert, so a second run finds the first one's row instead of adding another.

## Creating Your Own Errors

Each variant has a constructor, so your own code can return `tideorm::Error` alongside TideORM's:

| Constructor | Variant |
|---|---|
| `Error::not_found(msg)` | `NotFound` |
| `Error::connection(msg)` | `Connection` |
| `Error::query(msg)` | `Query` |
| `Error::validation(field, msg)` | `Validation` |
| `Error::conversion(msg)` | `Conversion` |
| `Error::transaction(msg)` | `Transaction` |
| `Error::configuration(msg)` | `Configuration` |
| `Error::internal(msg)` | `Internal` |
| `Error::access_denied(permission, resource)` | `AccessDenied` |
| `Error::rbac(msg)` | `Rbac` |
| `Error::backend_not_supported(msg, backend)` | `BackendNotSupported` |
| `Error::primary_key_not_set(msg, model)` | `PrimaryKeyNotSet` |
| `Error::tokenization(msg)` | `Tokenization` |
| `Error::invalid_token(msg)` | `InvalidToken` |

An error you build carries no database failure, so `failure_kind()` is `Unclassified` and `is_retryable()` is false whatever the message says. Returning an `Err` from a `Database::transaction` closure rolls the transaction back, which is how a check inside it cancels the work:

```rust
use tideorm::prelude::*;

db.transaction(async |_tx| {
    let order = Order::find_or_fail(order_id).await?;
    if order.total < 0 {
        return Err(Error::validation("total", "must not be negative"));
    }
    order.update().await
})
.await?;
```
