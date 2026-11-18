# Transactions

A transaction runs several statements so that they all take effect or none do. TideORM's transactions are ambient: every TideORM call made inside one joins it, so you pass no handle around.

```rust
use tideorm::prelude::*;

async fn sign_up(db: &Database, user: User, profile: Profile) -> tideorm::Result<User> {
    db.transaction(async move |_tx| {
        let user = User::create(user).await?;
        Profile::create(Profile { user_id: user.id, ..profile }).await?;

        // Ok commits, Err rolls back
        Ok(user)
    })
    .await
}
```

When the closure returns `Ok`, the transaction commits. When it returns `Err` or panics, the transaction rolls back. It has always ended, one way or the other, when `transaction` returns.

## Opening a Transaction

| Method | Takes | Use it for |
|---|---|---|
| `transaction` | An async closure handed the transaction | The usual case |
| `transaction_with` | `TransactionOptions`, then an async closure | An isolation level, an access mode or a SQLite `BEGIN` mode |
| `atomically` | A plain future | Work that needs no handle; the future may borrow local variables |

`atomically` is the same transaction for a future instead of a closure. Every TideORM call inside it joins the transaction anyway:

```rust
let order = require_db()?.atomically(async {
    let order = Order::create(order).await?;
    Item::update_all().decrement("stock", 1).where_eq("id", item_id).execute().await?;
    Ok(order)
}).await?;
```

Use `require_db()?` for the global database. `Database::current()?` is the database the surrounding code runs on: the enclosing transaction's, else a `scope`'s, else the global one. `Database::current()?.transaction(..)` therefore opens a savepoint inside an enclosing transaction. Inside a transaction, `Database::current()` returns the transaction's handle: drop it before the transaction ends, or the commit fails (see [Leaked Handles](#leaked-handles)).

## Scopes

`scope` runs a future with a database as the ambient connection without opening a transaction, so model calls that name no handle (`create`, `save`, `update`, `delete`, `find`, `insert_all`, `Model::query()`, `Database::raw` and the rest) use a replica or a tenant's database instead of the global one:

```rust
let user = tenant_db.scope(async {
    let user = User::create(user).await?;
    Profile::create(Profile { user_id: user.id, ..Default::default() }).await?;
    Ok(user)
}).await?;
```

Inside a transaction opened on the same database's pool, the calls in a scope join that transaction.

## Choosing the Database a Call Uses

| You want | Use |
|---|---|
| The application's database | Nothing: `User::find(id)`, `User::query()`, `user.save()` |
| One read from another database, such as a replica | `User::query_with(&replica)`, `User::find_with(id, &replica)`; eager loads of that query read there too |
| Every call in a piece of code on another database, such as a tenant's | `tenant_db.scope(async { .. })` |
| Several writes that succeed or fail together | `db.transaction(..)` or `db.atomically(..)` |

A call that names no database uses the innermost scope around it (a transaction is one), else the global database. A call that names one (`query_with`, `find_with`) joins the innermost enclosing transaction opened on that database's pool, and otherwise runs on that database's own connection, even inside another database's transaction: a replica read inside a primary's transaction goes to the replica, outside the transaction. Model writes (`create`, `save`, `update`, `delete`) name no database, so run them inside a `scope` or a transaction of the database they belong to.

That lets one service function serve every database, the caller choosing where it runs:

```rust
async fn rename(id: i64, name: String) -> tideorm::Result<User> {
    let mut user = User::find_or_fail(id).await?;
    user.name = name;
    user.update().await
}

let user = tenant_db.scope(rename(7, "Ada".into())).await?;    // the tenant's database
let user = primary.atomically(rename(7, "Ada".into())).await?; // in a transaction
let total = Order::query_with(&replica).count().await?;         // a replica read
```

## Nested Transactions

A transaction opened inside another one is a savepoint of it. When the inner closure returns `Err`, only the inner work rolls back; the outer transaction can catch the error and go on, or return it and roll back everything:

```rust
db.transaction(async move |_tx| {
    let order = Order::create(order).await?;

    // Optional work: if it fails, the order is still placed
    let gift = require_db()?.transaction(async move |_tx| {
        let card = GiftCard::create(card).await?;
        Ledger::record(&card).await?;
        Ok(card)
    }).await;
    if let Err(error) = gift {
        eprintln!("gift card skipped: {error}");
    }

    Ok(order)
}).await?;
```

An inner commit only ends the savepoint: its rows are written for good when the outermost transaction commits, and are rolled back with it. Code that opens its own transaction can therefore be called both on its own and inside a caller's transaction.

A transaction ends only its own level. Rolling one back first rolls back the levels nested in it; committing one while a level nested in it is still open is refused and rolls both back; and a nested transaction whose enclosing one has already ended refuses its statements. A rolled-back savepoint is also released, so retries in a loop do not stack savepoints.

Two nested transactions run side by side with `join!` take turns, one savepoint at a time.

## Transactions and Concurrent Tasks

A transaction, like a `scope`, follows the future it was given: it is installed each time that future is polled. So:

- calls awaited in the closure join it;
- futures run together inside it with `join!`, `try_join!` or `select!` join it too, since they are polled as part of its future. They share its one connection, so their statements run one after another;
- a task started with `tokio::spawn` (or `async_std::task::spawn`) does **not**. It is polled on its own, so its calls run on the global database, outside the transaction: they commit on their own and stay if the transaction rolls back, they cannot see its uncommitted rows, and they can wait on locks it holds.

```rust
let user = db.transaction(async move |_tx| {
    let (user, settings) = tokio::try_join!(
        User::create(user),         // both in the transaction
        Settings::create(settings),
    )?;
    Ok(user)
}).await?;

// Work that must not be part of the transaction starts after it, on purpose,
// and names its database when it is not the global one
let db = tenant_db.clone();
tokio::spawn(async move { db.scope(send_welcome(&user)).await });
```

## Isolation Level and Access Mode

`transaction_with` begins the transaction with `TransactionOptions`:

```rust
let report = db.transaction_with(
    TransactionOptions {
        isolation_level: Some(IsolationLevel::Serializable),
        access_mode: Some(AccessMode::ReadOnly),
        ..TransactionOptions::default()
    },
    async move |_tx| Report::build().await,
).await?;
```

| Field | Values | Backends |
|---|---|---|
| `isolation_level` | `IsolationLevel::ReadUncommitted`, `ReadCommitted`, `RepeatableRead`, `Serializable` | PostgreSQL, MySQL, MariaDB |
| `access_mode` | `AccessMode::ReadOnly`, `ReadWrite` | PostgreSQL, MySQL, MariaDB; SQLite refuses `ReadOnly` with an error as the transaction begins, since it cannot keep a transaction from writing |
| `sqlite_transaction_mode` | `SqliteTransactionMode::Deferred` (SQLite's default), `Immediate`, `Exclusive` | SQLite |

A read-only transaction refuses writes. On SQLite, `Immediate` takes the write lock as the transaction begins, so two competing writers queue at `BEGIN` (up to the busy timeout) instead of the second failing at its first write; `Exclusive` also keeps other connections from reading, outside WAL mode.

Options set a transaction up as it begins, so a `transaction_with` inside another transaction, which opens a savepoint, returns an error when given any.

## Concurrent Updates

A transaction does not stop two requests from reading the same row at the same time. `update()` writes every column of the model it was given, so in a read-check-write the last writer wins and silently undoes the other: two orders of 3 against a stock of 4 both pass the check, and both ship.

Lock the rows you are about to change with `lock_for_update()` (`SELECT .. FOR UPDATE`). A second transaction that locks the same row waits until the first commits, then reads what it wrote:

```rust
let shipped = Database::current()?.atomically(async {
    let mut item = Item::query()
        .where_eq("id", id)
        .lock_for_update()
        .first_or_fail()
        .await?;
    if item.stock < quantity {
        return Ok(false);
    }
    item.stock -= quantity;
    item.update().await?;
    Ok(true)
}).await?;
```

Outside a transaction the lock ends with the statement. `count()`, `exists()` and the aggregates lock the rows they read too, and a locked query never reads from the query cache. PostgreSQL rejects the lock on a query with `DISTINCT`, `GROUP BY` or a `UNION`.

For a plain counter, a single conditional statement needs no lock: `Item::update_all().decrement("stock", quantity).where_eq("id", id).where_gte("stock", quantity).execute()` returns `0` when the stock ran out.

SQLite has no row locks, so `lock_for_update()` adds nothing there: the first write of a transaction locks the whole database, and the second of two competing transactions fails with a retryable `LockNotAvailable` error instead.

## Failed Statements and Automatic Rollback

What happens after a statement fails inside a transaction depends on the failure:

- **PostgreSQL** aborts the whole transaction on any failed statement: the server refuses the statements after it, and would answer the `COMMIT` with a silent rollback. TideORM refuses that commit with an `Error::Transaction` instead. To catch a failure and carry on, run the statement in a nested transaction: only its savepoint rolls back.
- **MySQL, MariaDB and SQLite** keep a transaction going after an ordinary failed statement, such as a constraint violation, so the closure can catch the error and continue.
- **SQLite** rolls a transaction back by itself after some failed statements (a constraint's `ON CONFLICT ROLLBACK`, a trigger's `RAISE(ROLLBACK)`), and **every database** rolls back the whole transaction a deadlock hits, savepoints included.

In the last case TideORM refuses the transaction's later statements, nested transactions and its commit at every savepoint depth, even when the closure caught the error, so nothing runs outside it; rolling back stays safe. Retry the whole transaction: its earlier writes were rolled back too.

A connection goes back to the pool with no transaction open: one left open is rolled back, and a connection that cannot be brought back to that state is closed.

## Retrying

`Error::is_retryable()` says whether running the work again can plausibly succeed. It reads the failure's classification, `Error::failure_kind()`, whatever the backend's message said:

| `DbFailureKind` | Typically |
|---|---|
| `Deadlock` | SQLSTATE `40P01`, MySQL 1213 |
| `SerializationFailure` | SQLSTATE `40001`, under `SERIALIZABLE`, and on CockroachDB, which runs every transaction `SERIALIZABLE`; TiDB's write conflict (9007) |
| `LockNotAvailable` | SQLSTATE `55P03`, a MySQL lock wait timeout, SQLite's busy or locked database |
| `StatementTimeout` | SQLSTATE `57014` |
| `ConnectionTimeout`, `ConnectionClosed`, `ConnectionRefused` | No connection, or a lost one |

Retry the whole transaction, never the one statement that failed. `transaction_retrying` runs the closure again in a new transaction after a deadlock, a serialization failure or a lock it could not take, up to the given number of runs in all, with a short random wait between them:

```rust
async fn transfer_with_retry(db: &Database, from: i64, to: i64, amount: i64) -> tideorm::Result<()> {
    db.transaction_retrying(TransactionOptions::default(), 4, async |_tx| {
        transfer(from, to, amount).await
    })
    .await
}
```

The closure can run more than once, so send emails and other effects outside the database after it returns. Inside another transaction on the same pool it runs once, as a savepoint that ignores `options` (the enclosing transaction began with its own): a deadlock or a serialization failure aborts the enclosing transaction, so retry that one. It never retries a lost connection: one lost while the `COMMIT` is on the wire leaves the outcome unknown, and the database may have committed. Make retried work safe to repeat, or check for its effect first. [Errors](errors.md#retrying-a-transaction) has the details. See [Compatible Servers](compatible-servers.md) for the transaction behaviour of CockroachDB, TiDB and Vitess.

## Leaked Handles

Do not move the transaction handle, or a `Database` taken from `Database::current()` inside it, into a spawned task or anywhere it outlives the closure. A clone still alive when the closure returns fails the commit with "transaction handle leaked outside the transaction scope". TideORM then rolls the transaction (or the savepoint) back through the shared handle, which ends its locks at once and refuses the stray handle's statements afterwards. The error says whether that rollback succeeded, and carries the closure's own error when it had already failed.

## Pinned Sessions

A pool hands each statement whichever connection is free, so what one statement sets for its session (`SET`, `PRAGMA`, MySQL's `GET_LOCK`) may not hold for the next, and stays on that connection for whatever statement takes it later. `pin_session()` returns a handle on one connection of its own:

```rust
let session = db.pin_session().await?;
session.exec_raw("SET statement_timeout = '5s'").await?;

// Every statement through `session` runs on that connection, with that setting
let rows = User::query_with(&session).get().await?;
session.transaction(async move |_tx| { /* runs there too */ Ok(()) }).await?;

session.close().await?;
```

- The connection is opened with the pool's settings but outside the pool, so it takes none of the pool's connections.
- Transactions begun on the session run on its connection. A second one begun there waits until the first ends; nest through the transaction instead.
- It closes with `close()` or when the last clone of the handle drops, and is never replaced: once it is lost, the handle's statements fail.
- `pin_session()` on a session opens another one.
- It fails on a transaction's handle, which belongs to its scope, and on a private SQLite database (in memory without a shared cache, or a temporary one), where a new connection would open an empty database.
