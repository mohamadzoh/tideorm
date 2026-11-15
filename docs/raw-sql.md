# Raw SQL

When the query builder cannot express a statement, run your own SQL. Its values are bound as parameters, never written into the text, and its rows can be read as models, as a struct of your own or as JSON.

```rust
use tideorm::prelude::*;

#[derive(FromQueryResult)]
struct CountryTotal {
    country: String,
    users: i64,
}

async fn busy_countries(min: i64) -> tideorm::Result<Vec<CountryTotal>> {
    // PostgreSQL markers; MySQL, MariaDB and SQLite take `?`
    Database::raw_as_with_params(
        "SELECT country, COUNT(*) AS users FROM users GROUP BY country HAVING COUNT(*) >= $1",
        [min.into()],
    )
    .await
}
```

## Running Statements

These are associated functions on `Database`: they run inside the enclosing [transaction](transactions.md), or on the global connection when there is none.

| Function | Returns |
|---|---|
| `Database::raw::<M>(sql)`, `raw_with_params::<M>(sql, params)` | `Vec<M>`, each row read as the model `M` |
| `Database::raw_as::<T>(sql)`, `raw_as_with_params::<T>(sql, params)` | `Vec<T>`, each row read as a `FromQueryResult` type |
| `Database::raw_json(sql)`, `raw_json_with_params(sql, params)` | `Vec<serde_json::Value>`, each row as a JSON object |
| `Database::execute(sql)`, `execute_with_params(sql, params)` | `u64`, the rows affected |

```rust,ignore
let adults: Vec<User> = Database::raw::<User>("SELECT * FROM users WHERE age > 18").await?;

let active: Vec<User> = Database::raw_with_params::<User>(
    "SELECT * FROM users WHERE age > $1 AND status = $2",
    vec![18.into(), "active".into()],
)
.await?;

let affected = Database::execute(
    "UPDATE users SET active = false WHERE last_login < NOW() - INTERVAL '1 year'",
)
.await?;

let removed = Database::execute_with_params(
    "DELETE FROM users WHERE status = $1",
    vec!["banned".into()],
)
.await?;
```

`execute(sql)` sends its text unprepared, so it may hold several statements separated by `;`, such as a script. The `_with_params` forms prepare one statement.

## Parameters

Parameters are `DbValue`s (in the prelude), usually built with `.into()` from a Rust value. `params` takes a `Vec`, an array or any iterator of them. Bind SQL `NULL` as a typed `None`:

```rust,ignore
let params: Vec<DbValue> = vec![true.into(), "alice".into(), None::<String>.into()];
```

The SQL is sent as you wrote it, so write the backend's own markers:

| Backend | Markers |
|---|---|
| PostgreSQL | `$1`, `$2`, .. |
| MySQL, MariaDB | `?` |
| SQLite | `?`, `?NNN` or `$NNN`, numbered from 1 |

SQLite refuses named parameters (`:name`, `@name`), `?0` and `$0`, and a statement whose markers take more values than you passed or leave some unused.

Never format a value into the SQL text yourself; bind it. Identifiers cannot be bound, so a table or column name in raw SQL must be one you wrote, not one taken from a request.

## Reading Rows

### As Models

`raw::<M>` reads each row the way a query does: through the model's own columns, with encrypted fields decrypted, relations attached to the database the row came from, and dirty tracking started. The statement must return the model's columns. Nothing else is added: no soft-delete scope, no default projection, no limit.

### As Your Own Type

`#[derive(FromQueryResult)]` (in the prelude) reads a row of any shape, such as a report or a join:

```rust,ignore
#[derive(FromQueryResult)]
struct Totals {
    country: String,
    users: i64,
    last_signup: Option<chrono::DateTime<chrono::Utc>>,
}

let totals: Vec<Totals> = Database::raw_as(
    "SELECT country, COUNT(*) AS users, MAX(created_at) AS last_signup FROM users GROUP BY country",
)
.await?;
```

- Each field is read from the column of its name, as the field's type, the way a model reads its columns, so alias a computed column to its field (`AS users`). A raw identifier field (`r#type`) reads the column `type`.
- An `Option` field takes SQL `NULL`; any other field fails on one.
- The derive takes structs with named fields and no attributes.
- A row that does not read fails the call with a conversion error naming the type: `raw_as() could not read a row as ..`.

A query builder's `get_as::<T>()` uses serde instead, so a `Deserialize` type works there; `raw_as` needs `FromQueryResult`.

### As JSON

`raw_json` returns each row as a JSON object, with each column decoded by the type the database declares for it. [Rows as JSON](queries.md#rows-as-json) describes how each type reads, including SQLite's expressions.

```rust,ignore
let rows = Database::raw_json("SELECT id, email, created_at FROM users LIMIT 10").await?;
```

## On a Specific Database

Every function has an instance form on a `Database` handle, for an application with several databases (see [Databases](databases.md)):

| Associated function | On a handle |
|---|---|
| `Database::raw::<M>` | `db.query_raw::<M>(sql)`, `db.query_raw_with_params::<M>(sql, params)` |
| `Database::raw_as::<T>` | `db.query_raw_as::<T>(sql)`, `db.query_raw_as_with_params::<T>(sql, params)` |
| `Database::raw_json` | `db.query_raw_json(sql)`, `db.query_raw_json_with_params(sql, params)` |
| `Database::execute` | `db.exec_raw(sql)`, `db.exec_raw_with_params(sql, params)` |

```rust,ignore
let reporting = Database::connect(&reporting_url).await?;
let rows = reporting.query_raw_json("SELECT * FROM daily_totals").await?;
```

A transaction opened on the handle's own pool still wins: inside `Database::transaction`, `db.exec_raw(..)` joins that transaction rather than running beside it. A transaction on another database does not take it: the statement runs on the handle's own database.

## Raw Fragments in a Query

The query builder takes raw SQL for the parts it cannot express, and checks each fragment before the query runs:

| Method | Fragment |
|---|---|
| `where_raw(sql)`, `or_where_raw`, `and_where_raw` | a condition |
| `where_raw_with(sql, params)`, `or_where_raw_with`, `and_where_raw_with` | a condition with bound values |
| `having_raw(sql)`, `having_raw_with(sql, params)` | a `HAVING` condition |
| `select_raw(sql)` | a projection |
| `order_by_raw(sql, Order::Asc)` | an ordering expression |
| `union_raw(sql)`, `union_all_raw(sql)`, `union_raw_with(sql, params)`, `union_all_raw_with(sql, params)` | a union operand |
| `CTE::new(name, sql)`, `with_recursive_cte(..)` | a CTE body |

```rust,ignore
let users = User::query()
    .where_raw_with("LOWER(email) = LOWER(?)", [email.into()])
    .order_by_raw("COALESCE(nickname, name)", Order::Asc)
    .get()
    .await?;

let busy = Post::query()
    .select_raw("user_id, COUNT(*) AS posts")
    .group_by("user_id")
    .having_raw_with("COUNT(*) > ?", [10_i64.into()])
    .get_json()
    .await?;
```

The `_with` forms take `?` as the marker on every backend, whatever the backend's own: TideORM writes the backend's markers when the statement renders. A `?` inside a quoted literal is text, and a count of `?` other than the number of values fails the query. The values may come from a request. A value no backend can bind as given (a `u64` past `i64::MAX`) is bound as an exact decimal.

Which filter families take the `*_raw` forms is listed under [Where Each Filter Family Works](queries.md#where-each-filter-family-works); unions, CTEs and window functions under [Joins, Aggregates and Advanced SQL](advanced-queries.md).

### What Is Checked

A fragment is trusted SQL: it is checked for constructs that would change the statement around it, never escaped. Every fragment is refused when it is empty or holds:

- a statement separator (`;`);
- a comment (`--`, `/* */`, or `#`, which MySQL and MariaDB read as one);
- a NUL byte;
- an unterminated string literal or quoted identifier, or a dollar-quoted string;
- a backslash right before a quote inside a literal, which MySQL reads as an escape and PostgreSQL and SQLite do not;
- a quote inside square brackets, which SQLite reads as a quoted identifier.

Some fragments are held to more:

- **`having_raw`** refuses subqueries, joins, set operators, data-changing and DDL keywords, `EXISTS`, `INTO`, `LIMIT`, `OFFSET`, `WINDOW` and `OVER`, and any character other than an operator, `.`, `,`, `?`, `:` or balanced parentheses. `LEFT(..)` and `RIGHT(..)` are functions there, not joins.
- **`union_raw` and a CTE body** must begin with `SELECT` or `WITH`, a `WITH` must end in a `SELECT`, and they refuse `INSERT`, `UPDATE`, `DELETE`, `DROP`, `ALTER`, `CREATE`, `TRUNCATE`, `RETURNING`, `MERGE`, `REPLACE`, `UPSERT`, `GRANT`, `REVOKE`, `CALL`, `EXECUTE` and `VALUES` at their top level (a function of that name, such as `REPLACE(..)`, is fine). A union operand may not hold a `UNION`, `INTERSECT` or `EXCEPT` of its own; use `union()` with a query for that. A `CTE::new` body may; each half of `with_recursive_cte` may not.

A refused fragment does not fail the builder call: the query fails when it runs, and `validate()` reports it without running anything.

The checks catch mistakes, not every injection. Never format a request's text into a fragment; bind it with a `_with` form.

A batch update's `set_trusted_raw(col, expression)` is the one fragment that is not checked at all; reach for the computed setters on [Reading and Writing](crud.md) first.

## Forbidding Raw SQL

The `forbid_raw_sql` [project rule](project-rules.md) refuses raw SQL: the fragments above, `set_trusted_raw`, and every statement on this page. `Database::raw::<M>` is held to `M`'s rules; `execute`, `raw_as` and `raw_json` name no model, so they are refused whenever the project forbids raw SQL. Migrations may still run raw SQL through `Schema::raw`.

## Prepared Statements

Each connection prepares a parameterized statement once and reuses it, keeping up to 100 by default (see [Configuration](configuration.md) for `statement_cache_capacity`). `db.cached_statements()` reports how many statements one of the handle's connections holds.

Name the columns in raw SQL rather than `SELECT *` when the table can change under a running application. After a column is added, PostgreSQL rejects a cached `SELECT *` (`cached plan must not change result type`) and SQLite can report the old column list, until the connection is replaced. The query builder is not affected: it always names the model's columns.

## Caching and Logging

TideORM cannot tell which tables a raw statement writes: a `SELECT` can write through a function, a view or a trigger. So every raw statement drops the [query cache](caching.md)'s cached reads of its database once it has run, reads included, and so does one that fails, since a statement before the failing one may already have committed. Query builder reads keep their targeted invalidation.

Raw statements reach the query logger and the profiler like any other; see [Logging and Profiling](profiling.md).
