# Query Builder

`Model::query()` starts a `QueryBuilder` for a model. Chain filters, ordering and paging onto it, then run it with a terminal such as `get()` or `count()`. Every value is bound as a parameter, every column name is checked before the statement runs, and the SQL is written for the backend the query runs on.

```rust
use tideorm::prelude::*;

async fn adult_users() -> tideorm::Result<Vec<User>> {
    User::query()
        .where_eq(User::columns.active, true)
        .where_gte(User::columns.age, 18)
        .order_desc(User::columns.created_at)
        .limit(20)
        .get()
        .await
}
```

Joins, aggregates, grouping, unions, window functions, CTEs, subqueries, row locks and bulk updates are on [Joins, Aggregates and Advanced SQL](advanced-queries.md). JSON and array filters are on [JSON and Arrays](json-and-arrays.md).

## Filters

Every filter takes its column as a string, either the database column name or the Rust field name, or as a typed column: `User::columns.active`, or its constant form `User::ACTIVE`. The typed form is checked by the compiler and is the better default; see [Typed Columns](typed-columns.md).

```rust,ignore
User::query().where_eq("active", true)             // checked when the query runs
User::query().where_eq(User::columns.active, true) // checked at compile time
```

| Method | Keeps the rows where |
|---|---|
| `where_eq(col, value)` | `col = value` |
| `where_not(col, value)`, `where_ne(col, value)` | `col <> value`; a NULL column never matches |
| `where_gt`, `where_gte`, `where_lt`, `where_lte` | `col >`, `>=`, `<`, `<=` the value |
| `where_in(col, values)` | `col` is one of `values`: a `Vec`, an array, `&ids`, a set |
| `where_not_in(col, values)` | `col` is none of `values`; a NULL column never matches |
| `where_between(col, min, max)` | `col` lies between `min` and `max`, inclusive |
| `where_not_between(col, min, max)` | `col` lies outside them; a NULL column never matches |
| `where_like(col, pattern)`, `where_not_like` | `col` matches a `LIKE` pattern, used as written: `%` and `_` stay wildcards |
| `where_contains(col, text)` | `col` contains `text` literally; its `%` and `_` are escaped |
| `where_starts_with`, `where_ends_with` | `col` starts or ends with the text literally |
| `where_null(col)`, `where_not_null(col)` | `col IS NULL`, `col IS NOT NULL` |
| `where_column_eq(col, other)` | `col` equals another column of the row; also `_ne`, `_gt`, `_gte`, `_lt`, `_lte` |
| `where_col(condition)` | a typed column's condition holds, such as `User::columns.age.gte(18)` |
| `where_json_*`, `where_array_*` | see [JSON and Arrays](json-and-arrays.md) |
| `where_raw(sql)`, `where_raw_with(sql, values)` | a trusted SQL fragment holds; see [Raw SQL](raw-sql.md) |

Each call is ANDed with the query's other filters:

```rust,ignore
User::query()
    .where_eq(User::columns.status, "active")
    .where_not(User::columns.role, "admin")
    .where_in(User::columns.role, ["editor", "author"])
    .where_in("id", &user_ids) // any list: a Vec, an array, a slice, a set
    .where_between(User::columns.age, 18, 65)
    .where_not_null(User::columns.email_verified_at)
    .where_like(User::columns.name, "John%")
    .where_contains(User::columns.bio, &params.search)

// Two columns of the same row, or of a joined table
Order::query().where_column_gt("shipped_at", "ordered_at");
Post::query()
    .inner_join("users", "posts.user_id", "users.id")
    .where_column_lt("posts.created_at", "users.banned_at");
```

### Conditional Filters

`when(condition, f)` applies `f` only when `condition` holds, and `when_some(option, f)` applies it with the value when the `Option` holds one, so a filter that depends on a request parameter does not break the chain:

```rust,ignore
User::query()
    .when(params.only_active, |q| q.where_eq(User::columns.active, true))
    .when_some(params.role, |q, role| q.where_eq(User::columns.role, role))
```

### Typed Conditions

A typed column also builds a condition of its own, which `where_col` (and `or_where_col`, `and_where_col`) applies. The value must have the column's type, so a mismatch is a compile error. The conditions are `eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `like`, `not_like`, `contains`, `starts_with`, `ends_with`, `is_in`, `not_in`, `between`, `is_null` and `is_not_null`:

```rust,ignore
User::query().where_col(User::columns.age.gte(18))
User::query().where_col(User::columns.role.is_in(["admin", "moderator"]))
User::query().where_col(User::columns.email.ends_with("@example.com"))
User::query().where_col(User::columns.deleted_at.is_null())
```

A typed column names its model's table, so in another model's query it means that model's column: `Post::query().inner_join("users", ..).where_eq(User::columns.active, true)` filters `users.active`.

### Values

Values are any type a model field can hold (`impl Into<DbValue>`): numbers, text, `bool`, `Uuid`, `chrono` dates and timestamps, `Decimal`, bytes, `serde_json::Value`, an `Option` of one, or a reference to one. Each is bound as its own type, so pass the native value: `where_eq("id", user_id)`, `where_gt("created_at", since)`. A value of another type than the column's, such as a UUID or a timestamp written as text, or a `serde_json::Value`, is read as the column's type, so `where_eq("id", "42")` still compares an integer.

- **NULL.** `where_eq(col, None::<T>)` is `IS NULL`. `where_not` and `where_not_in` follow SQL and never match a NULL column. A `None` inside `where_in` also matches NULL rows, and one inside `where_not_in` keeps the non-NULL rows outside the list.
- **JSON `null`.** On a JSON column, `where_eq(col, serde_json::Value::Null)` (or `Some(Value::Null)`) matches the rows holding the JSON `null` document, and `None` the SQL NULL ones; inside `where_in` and `where_not_in` the two read the same way.
- **NaN and infinity.** A filter value holding a NaN or infinite float fails the query, as does a `set()` or `having()` given one: no SQL comparison can take it. Check a float parsed from a request with `is_finite()`.
- **Columns are identifiers.** The column argument is a column, `table.column`, or a typed column; anything else is rejected when the query runs. SQL expressions go through `where_raw()`, whose SQL you vouch for; never pass user input to it. When the expression needs a value, write `?` for it and pass the value to `where_raw_with()`, which binds it: `where_raw_with("LOWER(email) = LOWER(?)", [email.into()])`. The `?` works on every backend.
- **Long lists.** A `where_in`/`where_not_in` list of more than 1,000 values that share a type is bound as one array on PostgreSQL (`= ANY($1)`), and a list of more than 1,000 strings as one JSON value on SQLite (`IN (SELECT value FROM json_each(?))`); anywhere else a list of more than 1,000 integers is written inline instead of bound. So a list of ids of any length works everywhere, and one of text on PostgreSQL and SQLite. Other long lists, such as text on MySQL and MariaDB or UUIDs and timestamps on SQLite and MySQL, are bound one parameter each, which caps them at the backend's limit: 32,766 on SQLite, 65,535 on PostgreSQL and MySQL.
- **`LIKE` and case.** `where_contains`, `where_starts_with` and `where_ends_with` escape `%` and `_`, so they are safe for user input; `where_like` uses its pattern as written. Whether a match is case-sensitive is the backend's: PostgreSQL's is, MySQL's (with its default collations) and SQLite's (for ASCII) are not.
- **Text equality follows the collation.** MySQL's default collation ignores case and accents, so there `where_eq("email", "ada@example.com")` finds `Ada@Example.com`, `"cafe"` finds `café`, and a unique index refuses both spellings; MariaDB's default collations also ignore trailing spaces, so `"a "` finds `"a"`. PostgreSQL and SQLite compare text exactly. Normalize the values you look up, such as lowercasing emails before saving, to get the same answer everywhere.

### Where Each Filter Family Works

Every operator in the table above, the JSON, array, column-to-column, `where_col` and raw forms included, comes in several families. Which ones a builder offers:

| Builder | Families |
|---|---|
| `QueryBuilder` (`Model::query()`) | `where_*`, `or_where_*`, both with the raw forms |
| `OrGroup` (inside `where_any`/`where_all`) | `where_*`, with the raw forms |
| `OrBranchBuilder` (after `begin_or()`) | `or_where_*`, `and_where_*`, both with the raw forms |
| `BatchUpdateBuilder` (`update_all()`) | `where_*`, `or_where_*`, without the raw forms |

`when` and `when_some` work on all four.

## OR Conditions

Write an OR as a group: `where_any(|g| ..)` ORs the conditions it adds, and `where_all(|g| ..)` ANDs them. Every group is written in parentheses and ANDed with the rest of the query, so the SQL reads the way the chain does. Each call adds a group of its own, and inside a group `nested_or` and `nested_and` add a group in turn:

```rust,ignore
// WHERE active = true AND (role = 'admin' OR role = 'moderator')
User::query()
    .where_eq(User::columns.active, true)
    .where_any(|g| g.where_eq(User::columns.role, "admin").where_eq(User::columns.role, "moderator"))

// WHERE (plan = 'pro' OR (plan = 'trial' AND verified = true))
User::query().where_any(|g| {
    g.where_eq(User::columns.plan, "pro")
        .nested_and(|t| t.where_eq(User::columns.plan, "trial").where_eq(User::columns.verified, true))
})

// WHERE (active = true AND (name = 'Ada' OR name = 'Grace'))
User::query().where_all(|g| {
    g.where_eq(User::columns.active, true)
        .nested_or(|o| o.where_eq(User::columns.name, "Ada").where_eq(User::columns.name, "Grace"))
})
```

An OR across what would otherwise be two filters goes in one group: `where_any(|g| g.where_eq(User::columns.active, true).where_eq(User::columns.role, "admin"))`. A typed column inside a group is always written with its table.

### The `or_where_*` Shorthand

All `or_where_*` calls on one query join **one** OR group, ANDed with the rest of the query. They never OR with a plain `where_*` filter:

```rust,ignore
// WHERE active = true AND (role = 'admin' OR role = 'moderator')
// not: active = true OR role = 'admin' OR role = 'moderator'
User::query()
    .where_eq(User::columns.active, true)
    .or_where_eq(User::columns.role, "admin")
    .or_where_eq(User::columns.role, "moderator")

// WHERE (price > 1000 OR price < 50)
Product::query()
    .or_where_gt(Product::columns.price, 1000.0)
    .or_where_lt(Product::columns.price, 50.0)
```

Any operator works there: `or_where_in`, `or_where_null`, `or_where_between`, `or_where_like` and the rest. Use `where_any` or `begin_or()` for a second, separate OR group.

### OR Branches With `begin_or`

`begin_or()` opens an OR group whose branches are ANDs: `or_where_*` starts a branch and `and_where_*` adds to the current one. `end_or()` closes the group and returns to the query. A group with no condition adds nothing.

```rust,ignore
// WHERE active = true AND rating >= 4.0
//   AND ((category = 'Electronics' AND price < 1000) OR (category = 'Home' AND featured = true))
Product::query()
    .where_eq("active", true)
    .where_gte("rating", 4.0)
    .begin_or()
        .or_where_eq("category", "Electronics").and_where_lt("price", 1000.0)
        .or_where_eq("category", "Home").and_where_eq("featured", true)
    .end_or()

// Two groups: WHERE (category = 'Electronics' OR category = 'Home') AND (brand = 'Apple' OR brand = 'Samsung')
Product::query()
    .begin_or()
        .or_where_eq("category", "Electronics")
        .or_where_eq("category", "Home")
    .end_or()
    .begin_or()
        .or_where_eq("brand", "Apple")
        .or_where_eq("brand", "Samsung")
    .end_or()
```

Every operator has an `and_where_*` form there: `and_where_not`, `and_where_in`, `and_where_null`, `and_where_between`, `and_where_contains`, `and_where_column_gt`, `and_where_json_contains`, `and_where_raw_with` and so on.

To see the WHERE clause a chain builds, print the query's `debug()` (see [Inspecting a Query](#inspecting-a-query)).

## Reusing Filters

`consolidate()` takes a snapshot of a query's clauses as a `QueryFragment`, and `apply(&fragment)` replays it onto another query:

```rust,ignore
let active = User::query().where_eq(User::columns.status, "active").consolidate();
let admins = User::query().apply(&active).where_eq(User::columns.role, "admin").get().await?;
```

The fragment holds clauses, not SQL, so its values stay bound and it renders for whichever backend the query runs on. Applying merges the way calling the setters again would: WHERE conditions, OR conditions, ordering, joins, grouping, HAVING, raw selects, unions, window functions and CTEs are appended; `limit`, `offset`, `select` and the cache settings are last-wins, a slot the fragment left unset keeping the query's own; `distinct()` turns on and stays on; a fragment that set neither `with_trashed()` nor `only_trashed()` leaves the soft-delete scope alone; and when both hold an error, the query's own is the one reported. `QueryFragment::new()` makes an empty fragment, and `is_empty()` and `condition_count()` describe one.

## Ordering

```rust,ignore
User::query()
    .order_by("created_at", Order::Desc)
    .order_by(User::columns.name, Order::Asc)

User::query().order_asc(User::columns.name)         // ORDER BY name ASC
User::query().order_desc(User::columns.created_at)  // ORDER BY created_at DESC
User::query().latest()                               // ORDER BY created_at DESC
User::query().oldest()                               // ORDER BY created_at ASC
Post::query().latest_by(Post::columns.published_at)  // ORDER BY published_at DESC
Post::query().oldest_by("published_at")              // ORDER BY published_at ASC
```

Each call adds a sort key after the earlier ones. `reorder(column, direction)` replaces every order the query has so far, such as one a scope added.

`order_by` checks its column as a filter does, and takes one direction: `order_by("name desc", Order::Asc)` names two and fails the query. Take a request's sort string with `sort()`: terms separated by commas, each a column with an optional direction, as `name`, `name desc`, `+name` or `-created_at` for descending. Every column is checked as `order_by` checks one, so the string cannot smuggle an expression in. An empty string orders nothing; an empty term (`"name,,id"`) fails the query.

```rust,ignore
// ?sort=-created_at,name
let posts = Post::query().sort(&params.sort).get().await?;
```

`Order` is `Asc` (the default) or `Desc`. It parses from `"asc"`, `"desc"`, `"ascending"` or `"descending"` in any case, through `FromStr` and serde, so a request can deserialize one; it serializes as `"asc"`/`"desc"`, and `reverse()` gives the other direction. A model named `Order` shadows the prelude's `Order` enum; write `SortOrder::Desc` instead, the same enum under a second name.

`order_by_raw(expression, direction)` orders by a trusted SQL expression, such as `COALESCE(nickname, name)`; it is checked for statement separators, comments and NUL characters, never escaped.

Where NULLs sort is the backend's: PostgreSQL puts them last in ascending order and first in descending order, MySQL and SQLite the other way round. Sort on `IS NULL` first to get one order everywhere:

```rust,ignore
User::query()
    .order_by_raw("last_login_at IS NULL", Order::Asc) // NULLs last on every backend
    .order_asc(User::columns.last_login_at)
```

## Limit, Offset and Pages

```rust,ignore
User::query().order_asc(User::columns.id).limit(10).offset(20)
User::query().order_asc(User::columns.id).page(3, 25) // page 3, 25 per page
```

`page(page, per_page)` counts pages from 1. A page or page size of 0, or one whose offset passes `i64::MAX`, fails the query with a validation error naming `page` or `per_page`. `offset()` without `limit()` works on every backend: MySQL, MariaDB and SQLite get an open-ended `LIMIT` written for them.

Give a paged query a unique order, such as one ending in the primary key. Without one, PostgreSQL returns rows in storage order, which an `UPDATE` changes, so consecutive pages can repeat one row and skip another. `Model::paginate(page, per_page)` orders by the primary key for you (see [Reading and Writing](crud.md)).

### Pages With a Total

`paginate(page, per_page)` runs the query for one page and counts every matching row, and returns a `Paginated<M>`: the page's models in `items`, the count in `total`, and the `page` and `per_page` asked for. `last_page()` (1 when nothing matches), `has_next_page()` and `has_previous_page()` are worked out from them, and `map(f)` turns the items into another type, such as a view model. It serializes as `{"items": .., "total": .., "page": .., "per_page": .., "last_page": ..}`, ready to return from an API:

```rust,ignore
let page = Post::query()
    .where_eq(Post::columns.published, true)
    .order_desc(Post::columns.id)
    .paginate(2, 20)
    .await?;
println!("page {} of {} ({} posts)", page.page, page.last_page(), page.total);
```

### Cursor Pagination

`cursor_paginate(per_page, cursor)` pages by the primary key instead, the way `chunks()` does: pass `None` for the first page and each page's `next_cursor` for the one after it. It returns a `CursorPage<M>`, serialized as `{"items": .., "next_cursor": ..}`, whose `next_cursor` is `None` once a page comes back short (a page that ends exactly at the last row is followed by an empty one). It counts nothing and skips no rows by `OFFSET`, so a deep page costs what the first does, and rows written between requests are neither repeated nor skipped.

```rust,ignore
let page = Post::query()
    .where_eq(Post::columns.published, true)
    .order_desc(Post::columns.id)
    .cursor_paginate(20, params.cursor.as_deref())
    .await?;
```

The query follows the rules of `chunk()` below, and refuses `limit()`. A cursor is opaque but not secret: it names the key to continue from, the query's filters still apply, and one that does not decode fails with `Error::Validation` on the field `cursor`.

## Processing Large Results in Chunks

```rust
use tideorm::prelude::*;

async fn send_digest(_user: &User) -> tideorm::Result<()> {
    Ok(())
}

async fn digest_everyone() -> tideorm::Result<()> {
    User::query()
        .where_eq(User::columns.active, true)
        .chunk(500, |batch| async move {
            for user in &batch {
                send_digest(user).await?;
            }
            Ok(())
        })
        .await?;

    // Or pull the batches yourself, holding any state across `.await`s
    let mut emails = Vec::new();
    let mut batches = User::query().chunks(500)?;
    while let Some(batch) = batches.next().await? {
        emails.extend(batch.into_iter().map(|user| user.email));
    }
    Ok(())
}
```

`chunk()` and `chunks()` walk the query by a primary-key cursor instead of loading the whole result at once, so a callback may update or delete rows it has processed without a later batch skipping any. Each batch is one query, only one batch is held at a time, and the next is read when you ask for it; there is no row-by-row stream, so `chunks()` is how to read a result larger than memory. Filters, `limit()` and the cache settings stay in effect.

The rules, which `cursor_paginate()` shares:

- The model needs a single-column primary key, and the projection must keep it.
- The query may be ordered only by the primary key; order it descending with `order_desc` on the key to walk backwards. Any other order, `offset()`, `GROUP BY`/`HAVING`, a union or a window function fails, and so does a chunk size of 0.
- On a join that repeats a key, the rows sharing it are handed over in one batch, so only a key repeated more often than the chunk size fails.

## Running a Query

| Terminal | Returns |
|---|---|
| `get()` | `Vec<M>`, every matching row |
| `first()` | `Option<M>`, the first row; without an order the database picks it |
| `first_or_fail()` | `M`, or a not-found error |
| `find(id)`, `find_or_fail(id)` | the row with that primary key among the rows the query matches |
| `count()` | `u64`; ignores ordering, `limit()` and `offset()` |
| `exists()` | `bool`; ignores ordering, `limit()` and `offset()` |
| `pluck(col)` | `Vec<T>`, one column of every row |
| `value(col)` | `Option<T>`, one column of the first row |
| `get_as::<T>()` | `Vec<T>`, each row as a type of your own |
| `get_json()` | `Vec<serde_json::Value>`, each row as a JSON object |
| `paginate(page, per_page)` | `Paginated<M>` |
| `cursor_paginate(per_page, cursor)` | `CursorPage<M>` |
| `chunk(size, f)`, `chunks(size)` | batches of `M` |
| `sum`, `avg`, `min`, `max`, `count_distinct`, `aggregates` | see [Aggregates](advanced-queries.md#aggregates) |
| `delete()`, `update_all()` and the other writes | see [Bulk Updates and Deletes](advanced-queries.md#bulk-updates-and-deletes) |

```rust,ignore
let users = User::query().where_eq(User::columns.active, true).get().await?;
let admin = User::query().where_eq(User::columns.email, "admin@example.com").first().await?;
let user = User::query().where_eq(User::columns.id, 1).first_or_fail().await?;
let active = User::query().where_eq(User::columns.active, true).count().await?;
let taken = User::query().where_eq(User::columns.email, email).exists().await?;
let mine = Post::query().where_eq(Post::columns.user_id, me).find(post_id).await?;
```

`find(id)` on a query looks the key up among the rows the query matches: its filters and soft-delete scope apply, so `Post::query().where_eq("user_id", me).find(post_id)` finds only my post. A composite key is passed as a tuple. It refuses `limit()`/`offset()` and a union, since the key would filter only the union's first query.

A query reads the model's columns by name rather than `SELECT *`, so a column added to the table while the application runs does not disturb it. `get()` builds complete models, so it refuses a `select()` that leaves model columns out: the missing ones would read as `None` or a default, and saving such a model would write those back over the stored values. Read a partial row with `get_json()`, `get_as()` or `pluck()`; selecting columns is covered under [Selecting Columns](advanced-queries.md#selecting-columns).

### Single Columns and Your Own Types

`pluck` reads one column of every row, `value` the column of the first row, and `get_as` each row as a type of your own (any `Deserialize` type) whose fields are the columns or their aliases: the shape of a join or a grouped `select_raw()` that no model has.

```rust,ignore
let emails: Vec<String> = User::query().where_eq(User::columns.active, true).pluck(User::columns.email).await?;
let newest: Option<String> = Post::query().latest().value("title").await?;

#[derive(serde::Deserialize)]
struct AuthorPosts { author: String, posts: i64 }

let rows: Vec<AuthorPosts> = Post::query()
    .inner_join("users", "posts.user_id", "users.id")
    .select_raw("users.name AS author, COUNT(*) AS posts")
    .group_by("users.name")
    .get_as()
    .await?;
```

Given a typed column, `pluck` and `value` read the field's own type; given a name, they read whatever type you ask for. `pluck` replaces any `select()`, though `distinct()` still applies to it. On a `union()` the column is read off the union's rows, and an encrypted column comes back decrypted, as `get()` returns it. A model's enum column reads as its variants, not as the strings it stores. A JSON `null` read into an `Option<serde_json::Value>` comes back as `None`.

### Rows as JSON

`get_json()` returns each row as a JSON object, which suits projections that are not a whole model:

```rust,ignore
let rows = User::query().select(["id", "email"]).get_json().await?;
```

The model's column types decode its columns on every backend: SQLite stores booleans as integers and JSON and dates as text, and the model's types are what put them back. Other columns (aliases, aggregates, window values) are decoded by the type the database declares: text comes back verbatim, binary as an array of bytes, `NUMERIC`/`DECIMAL` as a decimal string. A MySQL `TIME` is a signed duration, such as `-01:30:00` or `100:00:00`. PostgreSQL types with no Rust counterpart come back as the server prints them: an `INTERVAL` as its ISO 8601 duration, `TIMETZ`, `INET`, `CIDR` and the built-in ranges as their text, a `MONEY` as a decimal string; an array keeps its `NULL`s and dimensions. Floating-point NaN and infinities come back as the strings `"NaN"`, `"inf"` and `"-inf"`, inside PostgreSQL arrays too, apart from SQL NULL, which is JSON `null`. A value TideORM cannot read fails the call rather than reading as `null`.

SQLite declares no type for an expression, so one is read by what SQLite stores: a float sum or a running total is a number, and text is a string however it looks, unless a document-returning SQLite JSON function or `->` computed an object or array.

- A single-path `json_extract` returns a string member as plain SQL text, so its text cannot tell `"[2]"` from `[2]` and stays a string. Read a document with `->` instead (`doc -> '$.tags'`), which always returns JSON, in any form: chained, after another call, or with a parameter as its path. With several paths, `json_extract` returns a JSON array and is decoded as one.
- Any other expression over a JSON value, such as `COALESCE`, `CASE`, `MAX` or a scalar subquery reading a JSON column, reads as text.
- SQLite JSON expressions are followed through subqueries, CTEs and the views a statement reads, renamed columns included. An inner alias does not change an unrelated outer text expression's type.
- If a `UNION` combines a JSON expression with ordinary text in one output column, that column keeps SQLite's stored text rather than guessing which rows are documents. SQLite describes a compound's columns by its first arm, though, so a `JSON` table column there makes the column a document for every row.
- A table column is a document only when it is declared `JSON` or `JSONB`: an untyped column, or one of a type SQLite's driver does not name, reads its text as a string however it looks.

`Database::raw_json` reads rows with no model behind them; see [Raw SQL](raw-sql.md).

## Soft-Deleted Rows

A query on a soft-delete model leaves trashed rows out. `with_trashed()` reads them too and `only_trashed()` reads nothing else; see [Soft Deletes and Scopes](soft-deletes.md).

## Caching

`cache(ttl)` keeps the models a query reads in the process-wide query cache, which starts disabled; see [Query Cache](caching.md).

## Inspecting a Query

`debug()` shows the statement a query will run, with its values written in, and `validate()` returns the error a terminal would fail with, without running anything; see [Logging and Profiling](profiling.md).
