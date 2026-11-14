# Queries

For execution metrics and slow-query counters, see [Profiling](profiling.md).

## Query Builder

Query execution paths are aligned on parameterized SQL generation for reads, JOIN clauses are validated before execution, and destructive mutations reject incompatible SELECT, JOIN, ORDER BY, GROUP BY, UNION, CTE, and window-function modifiers instead of silently ignoring them.

### WHERE Conditions

Most `where_*` methods accept either string column names or typed columns:

```rust
// Every model gets a `columns` constant with typed column accessors
// User::columns.id, User::columns.name, User::columns.active, etc.

// The same method accepts either form:
User::query().where_eq("active", true)                    // String-based (runtime checked)
User::query().where_eq(User::columns.active, true)        // Typed column (compile-time checked)

// Typed columns are usually the better default
User::query()
    .where_eq(User::columns.status, "active")
    .where_gt(User::columns.age, 18)
    .where_not_null(User::columns.email)
    .get()
    .await?;
```

Other helpers follow the same pattern:

```rust
User::query().where_eq("status", "active")
User::query().where_eq(User::columns.status, "active")
User::query().where_not("role", "admin")
User::query().where_not(User::columns.role, "admin")

User::query().where_gt("age", 18)
User::query().where_gt(User::columns.age, 18)
User::query().where_gte("age", 18)
User::query().where_lt("age", 65)
User::query().where_lte("age", 65)

User::query().where_like("name", "%John%")
User::query().where_like(User::columns.name, "%John%")
User::query().where_not_like("email", "%spam%")

User::query().where_in("role", ["admin", "moderator"])
User::query().where_in(User::columns.role, vec!["admin", "moderator"])
User::query().where_in("id", &user_ids)   // any list: a Vec, an array, a slice, a set
User::query().where_not_in("status", ["banned", "suspended"])

User::query().where_null("deleted_at")
User::query().where_null(User::columns.deleted_at)
User::query().where_not_null("email_verified_at")

User::query().where_between("age", 18, 65)
User::query().where_between(User::columns.age, 18, 65)
User::query().where_not_between("age", 18, 65)

// Two columns of the same row, or of a joined table
Order::query().where_column_gt("shipped_at", "ordered_at")
Post::query().inner_join("users", "posts.user_id", "users.id")
    .where_column_lt("posts.created_at", "users.banned_at")

// Only when a request asks for it
User::query()
    .when(params.only_active, |q| q.where_eq("active", true))
    .when_some(params.role, |q, role| q.where_eq("role", role))
```

Every filter — the JSON and array filters included — has `or_where_*` and `and_where_*` forms, and works the same on a query, inside an OR group and on a batch update; `when`/`when_some` work on each of them too.

A typed column also builds a condition of its own, which `where_col` applies; the value must have the column's type, so a mismatch is a compile error:

```rust
User::query().where_col(User::columns.age.gte(18))
User::query().where_col(User::columns.role.is_in(["admin", "moderator"]))
User::query().where_col(User::columns.email.ends_with("@example.com"))
User::query().where_col(User::columns.deleted_at.is_null())
```

A typed column names its model's table, so in another model's query it means that model's column: `Post::query().inner_join("users", ..).where_eq(User::columns.active, true)` filters `users.active`.

Values are any type a model field can hold (`impl Into<DbValue>`: numbers, text, `bool`, `Uuid`, `chrono` dates and timestamps, `Decimal`, bytes, `serde_json::Value`, an `Option` of one, a reference to one), and each is bound as its own type, so pass the native value: `where_eq("id", user_id)`, `where_gt("created_at", since)`. A value of another type than the column's, such as a UUID or a timestamp as text, or a `serde_json::Value`, is read as the column's type, so `where_eq("id", "42")` still compares an integer. A few rules worth knowing:

- **NULL.** `where_eq(col, None::<T>)` is `IS NULL`. `where_not` and `where_not_in` follow SQL and never match a NULL column. A `None` inside `where_in` also matches NULL rows, and one inside `where_not_in` keeps the non-NULL rows outside the list.
- **JSON `null`.** On a JSON column, `where_eq(col, serde_json::Value::Null)` (or `Some(Value::Null)`) matches the rows holding the JSON `null` document, and `None` the SQL NULL ones; inside `where_in` and `where_not_in` the two read the same way.
- **NaN and infinity.** A filter value holding a NaN or infinite float fails the query, as does `set()` or `having()` given one: no SQL comparison can take it. Check a float parsed from a request with `is_finite()`.
- **Columns are identifiers.** The column argument is a column, `table.column`, or a typed column; anything else is rejected when the query runs. SQL expressions go through `where_raw()`, whose SQL you vouch for — never pass user input to it. When the expression needs a value, write a `?` for it and pass the value to `where_raw_with()`, which binds it: `where_raw_with("LOWER(email) = LOWER(?)", vec![email.into()])`. The `?` works on every backend, and `or_where_raw_with`/`and_where_raw_with` join an OR group the same way.
- **Long lists.** A `where_in`/`where_not_in` list of more than 1,000 values that share a type is bound as one array on PostgreSQL (`= ANY($1)`), and a list of more than 1,000 strings as one JSON value on SQLite (`IN (SELECT value FROM json_each(?))`); anywhere else a list of more than 1,000 integers is written inline instead of bound. So a list of ids or text of any length works. Other long lists, such as UUIDs or timestamps on SQLite and MySQL, are bound one parameter each, which caps them at the backend's limit: 32,766 on SQLite, 65,535 on PostgreSQL and MySQL.
- **`LIKE` and case.** `where_contains`/`where_starts_with`/`where_ends_with` escape `%` and `_`, so they are safe for user input. Whether they are case-sensitive is the backend's: PostgreSQL is, MySQL (with its default collations) and SQLite (for ASCII) are not.
- **Text equality follows the collation.** MySQL's default collation ignores case and accents, so there `where_eq("email", "ada@example.com")` finds `Ada@Example.com`, `"cafe"` finds `café`, and a unique index refuses both spellings; MariaDB's default collations also ignore trailing spaces, so `"a "` finds `"a"`. PostgreSQL and SQLite compare text exactly. Normalize values you look up, such as lowercasing emails before saving, to get the same answer everywhere.

### OR Conditions

Write an OR as a group: `where_any(|g| ..)` ORs the conditions it adds, and
`where_all(|g| ..)` ANDs them. Every group is written in parentheses and ANDed
with the rest of the query, so the SQL reads the way the chain does. The
`or_where_*` helpers and `begin_or()` / `end_or()` blocks further down build the
same groups in shorter forms. Everything here accepts string column names and
typed columns.

#### Grouped conditions (where_any / where_all)

Each call adds a group of its own. Inside a group, `nested_or` and
`nested_and` add a group in turn:

```rust
// WHERE active = true AND (role = 'admin' OR role = 'moderator')
User::query()
    .where_eq(User::columns.active, true)
    .where_any(|g| g.where_eq(User::columns.role, "admin").where_eq(User::columns.role, "moderator"))
    .get()
    .await?;

// WHERE (active = true OR role = 'admin')
// An OR across what would otherwise be two filters goes in one group.
User::query()
    .where_any(|g| g.where_eq(User::columns.active, true).where_eq(User::columns.role, "admin"))
    .get()
    .await?;

// WHERE (plan = 'pro' OR (plan = 'trial' AND verified = true))
User::query()
    .where_any(|g| {
        g.where_eq(User::columns.plan, "pro")
            .nested_and(|t| t.where_eq(User::columns.plan, "trial").where_eq(User::columns.verified, true))
    })
    .get()
    .await?;

// WHERE (active = true AND (name = 'Ada' OR name = 'Grace'))
User::query()
    .where_all(|g| {
        g.where_eq(User::columns.active, true)
            .nested_or(|o| o.where_eq(User::columns.name, "Ada").where_eq(User::columns.name, "Grace"))
    })
    .get()
    .await?;
```

Call `debug()` on a query to see the WHERE clause it builds before running it.

#### Simple OR Methods

All `or_where_*` calls on one query are combined into one OR group, ANDed with
the rest of the query. They never OR with a plain `where_*` filter:

```rust
// WHERE active = true AND (role = 'admin' OR role = 'moderator')
// not: active = true OR role = 'admin' OR role = 'moderator'
User::query()
    .where_eq(User::columns.active, true)
    .or_where_eq(User::columns.role, "admin")
    .or_where_eq(User::columns.role, "moderator")
    .get()
    .await?;
```

Use `where_any` (above) or `begin_or()` / `end_or()` for further, separate OR
groups.

```rust
// WHERE (role = 'admin' OR role = 'moderator')
// Works with both strings and typed columns:
User::query()
    .or_where_eq("role", "admin")                    // String-based
    .or_where_eq(User::columns.role, "moderator")   // Typed column
    .get()
    .await?;

// OR with comparison operators
Product::query()
    .or_where_gt(Product::columns.price, 1000.0)   // price > 1000
    .or_where_lt(Product::columns.price, 50.0)     // OR price < 50
    .get()
    .await?;  // Gets premium OR budget products

// OR with pattern matching
User::query()
    .or_where_like(User::columns.name, "John%")    // name LIKE 'John%'
    .or_where_like(User::columns.name, "Jane%")    // OR name LIKE 'Jane%'
    .get()
    .await?;

// OR with IN clause
Product::query()
    .or_where_in(Product::columns.category, vec!["Electronics", "Books"])
    .or_where_eq(Product::columns.featured, true)
    .get()
    .await?;

// OR with NULL checks
User::query()
    .or_where_null(User::columns.deleted_at)
    .or_where_gt(User::columns.reactivated_at, some_date)
    .get()
    .await?;

// OR with BETWEEN
Product::query()
    .or_where_between(Product::columns.price, 10.0, 50.0)    // Budget range
    .or_where_between(Product::columns.price, 500.0, 1000.0) // Premium range
    .get()
    .await?;
```

#### Fluent OR API (begin_or / end_or)

For complex queries with grouped OR conditions combined with AND, use the fluent OR API:

```rust
// Basic OR group: (category = 'Electronics' OR category = 'Home')
Product::query()
    .begin_or()
        .or_where_eq(Product::columns.category, "Electronics")
        .or_where_eq(Product::columns.category, "Home")
    .end_or()
    .get()
    .await?;

// OR with AND chains: (Apple AND active) OR (Samsung AND featured)
Product::query()
    .begin_or()
        .or_where_eq("brand", "Apple").and_where_eq("active", true)
        .or_where_eq("brand", "Samsung").and_where_eq("featured", true)
    .end_or()
    .get()
    .await?;

// Complex: active AND rating >= 4.0 AND ((Electronics AND price < 1000) OR (Home AND featured))
Product::query()
    .where_eq("active", true)
    .where_gte("rating", 4.0)
    .begin_or()
        .or_where_eq("category", "Electronics").and_where_lt("price", 1000.0)
        .or_where_eq("category", "Home").and_where_eq("featured", true)
    .end_or()
    .get()
    .await?;

// Multiple sequential OR groups
Product::query()
    .where_eq("active", true)
    .begin_or()
        .or_where_eq("category", "Electronics")
        .or_where_eq("category", "Home")
    .end_or()
    .begin_or()
        .or_where_eq("brand", "Apple")
        .or_where_eq("brand", "Samsung")
    .end_or()
    .get()
    .await?;
// SQL: WHERE active = true 
//      AND (category = 'Electronics' OR category = 'Home') 
//      AND (brand = 'Apple' OR brand = 'Samsung')
```

#### AND Methods within OR Groups

Use `and_where_*` methods to chain AND conditions within an OR branch:

```rust
Product::query()
    .begin_or()
        // First OR branch: Electronics with price > 500 and good rating
        .or_where_eq("category", "Electronics")
            .and_where_gt("price", 500.0)
            .and_where_gte("rating", 4.5)
        // Second OR branch: Home items that are featured
        .or_where_eq("category", "Home")
            .and_where_eq("featured", true)
        // Third OR branch: Any discounted item
        .or_where_not_null("discount_percent")
    .end_or()
    .get()
    .await?;
```

Available `and_where_*` methods:
- `and_where_eq(col, val)` - AND column = value
- `and_where_not(col, val)` - AND column != value  
- `and_where_gt(col, val)` - AND column > value
- `and_where_gte(col, val)` - AND column >= value
- `and_where_lt(col, val)` - AND column < value
- `and_where_lte(col, val)` - AND column <= value
- `and_where_like(col, pattern)` - AND column LIKE pattern
- `and_where_in(col, values)` - AND column IN (values)
- `and_where_not_in(col, values)` - AND column NOT IN (values)
- `and_where_null(col)` - AND column IS NULL
- `and_where_not_null(col)` - AND column IS NOT NULL
- `and_where_between(col, min, max)` - AND column BETWEEN min AND max

#### Real-World Examples

```rust
// E-commerce: Flash sale eligibility
let flash_sale_products = Product::query()
    .where_eq("active", true)
    .where_gt("stock", 100)
    .where_gte("rating", 4.3)
    .where_null("discount_percent")  // Not already discounted
    .get()
    .await?;

// Inventory: Reorder alerts
let reorder_needed = Product::query()
    .where_eq("active", true)
    .begin_or()
        .or_where_lt("stock", 50).and_where_gt("rating", 4.5)  // Popular items low
        .or_where_lt("stock", 30)  // Any item critically low
    .end_or()
    .order_by("stock", Order::Asc)
    .get()
    .await?;

// Marketing: Cross-sell recommendations
let recommendations = Product::query()
    .where_eq("active", true)
    .begin_or()
        .or_where_eq("brand", "Apple")
        .or_where_eq("brand", "Samsung").and_where_gt("price", 500.0)
        .or_where_eq("featured", true).and_where_not("category", "Electronics")
    .end_or()
    .order_by("rating", Order::Desc)
    .limit(10)
    .get()
    .await?;

// Search: Multi-pattern name matching
let search_results = Product::query()
    .begin_or()
        .or_where_like("name", "iPhone%")
        .or_where_like("name", "Galaxy%")
        .or_where_like("name", "%Pro%")
    .end_or()
    .where_eq("active", true)
    .get()
    .await?;

// Analytics: Price segmentation
let segmented = Product::query()
    .begin_or()
        .or_where_eq("category", "Electronics")
        .or_where_eq("category", "Books")
    .end_or()
    .begin_or()
        .or_where_gt("price", 1000.0)   // Premium
        .or_where_lt("price", 50.0)     // Budget
    .end_or()
    .order_by("price", Order::Desc)
    .get()
    .await?;
```

### Ordering

```rust
// Basic ordering - works with both strings and typed columns
User::query()
    .order_by("created_at", Order::Desc)           // String-based
    .order_by(User::columns.name, Order::Asc)      // Typed column
    .get()
    .await?;

// Convenience methods - also work with typed columns
User::query().order_asc(User::columns.name)        // ORDER BY name ASC
User::query().order_desc(User::columns.created_at) // ORDER BY created_at DESC
User::query().latest()                              // ORDER BY created_at DESC
User::query().oldest()                              // ORDER BY created_at ASC
Post::query().latest_by(Post::columns.published_at) // ORDER BY published_at DESC
Post::query().oldest_by("published_at")             // ORDER BY published_at ASC
```

`Order::reverse()` gives the other direction, for a sort a request flips.

Take a request's sort string with `sort()`: terms separated by commas, each a column with an optional direction — `name`, `name desc`, or `-created_at` for descending. Every column is checked as `order_by` checks one, so the string cannot smuggle an expression in:

```rust
// ?sort=-created_at,name
let posts = Post::query().sort(&params.sort).get().await?;
```

`order_by` takes one direction: `order_by("name desc", Order::Asc)` names two and fails the query. `reorder(column, direction)` replaces every order the query has so far, such as one a scope added.

A model named `Order` shadows the prelude's `Order` enum; write `SortOrder::Desc` instead, the same enum under a second name.

Where NULLs sort is the backend's: PostgreSQL puts them last in ascending order and first in descending order, MySQL and SQLite the other way round. Sort on `IS NULL` first to get one order everywhere:

```rust
User::query()
    .order_by_raw("last_login_at IS NULL", Order::Asc) // NULLs last on every backend
    .order_asc(User::columns.last_login_at)
    .get()
    .await?;
```

### Pagination

```rust
// Limit and offset
User::query()
    .limit(10)
    .offset(20)
    .get()
    .await?;

// Page-based pagination
User::query()
    .order_asc(User::columns.id)
    .page(3, 25)  // Page 3, 25 per page
    .get()
    .await?;
```

Give a paged query a unique order, such as one ending in the primary key. Without one, PostgreSQL returns rows in storage order, which an `UPDATE` changes, so consecutive pages can repeat one row and skip another. `Model::paginate(page, per_page)` orders by the primary key for you.

### Chunked Processing

```rust
async fn process(_user: User) -> tideorm::Result<()> {
    Ok(())
}

User::query()
    .chunk(500, |batch| async move {
        for user in batch {
            process(user).await?;
        }
        Ok(())
    })
    .await?;

// Or pull the batches yourself, holding any state across `.await`s
let mut emails = Vec::new();
let mut batches = User::query().chunks(500)?;
while let Some(batch) = batches.next().await? {
    for user in batch {
        emails.push(user.email.clone());
        process(user).await?;
    }
}
```

`chunk()` and `chunks()` walk the current query by a single-column primary-key cursor instead of loading the full result set into memory at once. That means callbacks may safely update or delete already-processed rows without later batches skipping records. Existing filters, `limit()`, and cache settings remain in effect. If you want descending traversal, order explicitly by the primary key before calling `chunk()`. `chunk()` rejects `offset()` and other custom ordering because those conflict with stable cursor traversal. On a join that repeats a key, the rows that share it are handed over in one batch, so only a key repeated more often than the chunk size fails.

There is no row-by-row stream: `chunks()` is how to read a result set larger than memory. Each batch is one query, so only one batch is held at a time, and the next is read when you ask for it.

### Execution Methods

```rust
// Get all matching records
let users = User::query()
    .where_eq("active", true)
    .get()
    .await?;  // Vec<User>

// Get first record
let user = User::query()
    .where_eq("email", "admin@example.com")
    .first()
    .await?;  // Option<User>

// Get first or fail
let user = User::query()
    .where_eq("id", 1)
    .first_or_fail()
    .await?;  // Result<User>

// Count (efficient SQL COUNT)
let count = User::query()
    .where_eq("active", true)
    .count()
    .await?;  // u64

// Check existence
let exists = User::query()
    .where_eq("email", "admin@example.com")
    .exists()
    .await?;  // bool

// Bulk delete (efficient single DELETE statement)
let deleted = User::query()
    .where_eq("status", "inactive")
    .delete()
    .await?;  // u64 (rows affected)

// Rows as JSON, for projections that are not a whole model
let rows = User::query()
    .select(["id", "email"])
    .get_json()
    .await?;  // Vec<serde_json::Value>

// Lock the rows read until the transaction ends (SELECT ... FOR UPDATE)
let user = User::query()
    .where_eq("id", 1)
    .lock_for_update()
    .first()
    .await?;
```

Use `lock_for_update()` inside a transaction before changing what you read; see [Concurrent Updates](models.md#concurrent-updates).

A query reads the model's columns by name rather than `SELECT *`, so a column added to the table while the application runs does not disturb it.

`select()` takes column names, field names or typed columns, as an array, a `Vec`, or a tuple of typed columns of different types (`(User::columns.id, User::columns.email)`); `add_select(column)` adds one column to the projection so far.

`get()` builds complete models, so it refuses a `select()` that leaves model columns out: the missing ones would read as `None` or a default, and saving such a model would write those back over the stored values. Read a partial row with `get_json()`.

`get_json()` uses the model's column types to decode its columns on every backend — SQLite stores booleans as integers and JSON and dates as text, and the model's types are what put them back. Other columns (aliases, aggregates, window values) are decoded by the type the database declares: text comes back verbatim, binary as an array of bytes, `NUMERIC`/`DECIMAL` as a decimal string. SQLite declares no type for an expression, so it is read by what SQLite stores: a float sum or a running total is a number, and text is a string however it looks, unless a document-returning SQLite JSON function or `->` computed an object or array. A single-path `json_extract` returns a string member as plain SQL text, so its text cannot tell `"[2]"` from `[2]` and stays a string; read a document with `->` instead (`doc -> '$.tags'`), which always returns JSON, in any form: chained, after another call, or with a parameter as its path. With multiple paths, `json_extract` returns a JSON array and is decoded as one. Any other expression over a JSON value, such as `COALESCE`, `CASE`, `MAX` or a scalar subquery reading a JSON column, reads as text. `Database::raw_json` has no model, so it reports what the database declares and stores: on SQLite a boolean expression is `0`/`1` and a `JSON` column's object or array is a document, a MariaDB `JSON` or `UUID` column reads as one, as MySQL's do, and a MySQL `DATETIME` has no offset. A MySQL `TIME` is a signed duration, such as `-01:30:00` or `100:00:00`. PostgreSQL types with no Rust counterpart come back as the server prints them: an `INTERVAL` as its ISO 8601 duration, `TIMETZ`, `INET`, `CIDR` and the built-in ranges as their text, a `MONEY` as a decimal string; an array keeps its `NULL`s and dimensions. A value TideORM cannot read fails the call rather than reading as `null`.

Floating-point NaN and infinities are returned as the strings `"NaN"`, `"inf"`, and
`"-inf"`, including inside PostgreSQL arrays. They remain distinct from SQL NULL,
which is returned as JSON `null`.

SQLite follows selected JSON expressions through subqueries, CTEs and the views a statement
reads, including renamed columns. An inner alias does not change an unrelated outer text expression's type. If a
`UNION` combines a JSON expression with ordinary text in the same output column, that
column keeps SQLite's stored text rather than guessing which rows are JSON documents. SQLite
describes a compound's columns by its first arm, though, so a `JSON` table column there makes
the column a document for every row. A table column is a document only when it is declared
`JSON` or `JSONB`: an untyped column, or one of a type SQLite's driver does not name, reads its
text as a string however it looks.

### Joins

Name both sides of a join as `table.column` (or `alias.column`); a bare column name invalidates the query, and the error surfaces when it runs. Everywhere else a bare name of one of the model's columns means the model's table — filters, `select()`, ordering and aggregates are written `table.column` once the query joins — so reach a joined table's column as `table.column`. Two selected columns may not share an output name, since a row can hold only one of them: alias one (`tags.id AS tag_id`).

```rust
let posts = Post::query()
    .inner_join("users", "posts.user_id", "users.id")
    .where_eq("users.active", true)
    .get()
    .await?;
```

`join::<R>(JoinType::Inner, left, right)` joins model `R`'s table, its columns typed, and `join_as::<R>(join_type, alias, left, right)` joins it under an alias, writing `R`'s typed columns with the alias:

```rust
let posts = Post::query()
    .join::<User>(JoinType::Inner, Post::columns.user_id, User::columns.id)
    .where_eq(User::columns.active, true)
    .get()
    .await?;
```

`left_join` and `right_join` keep the unmatched rows of their outer side, and `inner_join_as`, `left_join_as` and `right_join_as` give the joined table an alias, which is how a table joins itself or the same table twice. A right join's unmatched rows have `NULL` in every model column, which `get()` cannot turn into models, so read one with `get_json()`:

```rust
// Every user, with or without posts
let rows = Post::query()
    .right_join("users", "posts.user_id", "users.id")
    .select_raw("users.name AS author")
    .select_raw("posts.title AS title")
    .get_json()
    .await?;

// Each employee with their manager, if any
let rows = Employee::query()
    .left_join_as("employees", "manager", "employees.manager_id", "manager.id")
    .select_raw("employees.name AS name")
    .select_raw("manager.name AS manager")
    .get_json()
    .await?;
```

### Aggregates

`sum`, `avg`, `min`, `max` and `count_distinct` each run one statement, and each reads its result as the type you ask for. `sum` returns that type; `avg`, `min` and `max` return an `Option`, which is `None` over no rows. A sum is never read through a float, so an integer or `Decimal` total stays exact, and `min`/`max` work on any column — a number, a text, a date:

```rust
let revenue: Decimal = Order::query().where_eq("paid", true).sum("total").await?;
let average: Option<f64> = Product::query().avg(Product::columns.price).await?;
let first_signup: Option<DateTime<Utc>> = User::query().min("created_at").await?;
let oldest = User::query().max::<i32>("age").await?;
```

`aggregates()` computes several over the same rows in one statement and returns them as the tuple you ask for, typed the same way:

```rust
let (orders, revenue, largest): (u64, Decimal, Option<Decimal>) = Sale::query()
    .where_eq("region", "EU")
    .aggregates(&[Aggregate::count(), Aggregate::sum("amount"), Aggregate::max("amount")])
    .await?;
```

`count()` ignores `limit()` and `offset()`, so a paged query counts its total across every page; the aggregates, `Aggregate::count()` included, work on the rows the limit and offset leave.

### Grouping

`group_by` groups the rows, and `having` keeps the groups whose aggregate passes a comparison, with the value bound; several `having` calls must all hold. `having_raw` takes a raw condition, as trusted SQL:

```rust
let busy_regions: Vec<RegionTotal> = Sale::query()
    .select_raw("region, SUM(revenue) AS revenue")
    .group_by("region")
    .having(Aggregate::sum("revenue").gt(5_000))
    .having(Aggregate::count().gte(3))
    .get_as()
    .await?;

Sale::query().group_by("rep").having_raw("COUNT(DISTINCT region) > 1");

// Values a raw condition compares with are bound, so they may come from a request
Sale::query().group_by("rep").having_raw_with("SUM(revenue) > ?", [params.min_revenue.into()]);
```

`Aggregate` offers `gt`, `gte`, `lt`, `lte`, `eq` and `ne`.

### Filtering by Related Rows

`where_has::<R>` keeps the rows with a related `R` row that passes a closure over `R`'s own query, so `R`'s soft-delete scope applies and any filter works; `where_doesnt_have::<R>` keeps the rows with none. The first key is `R`'s column that holds the second, this model's:

```rust
// Users with a published post
User::query().where_has::<Post>(Post::columns.user_id, User::columns.id, |posts| {
    posts.where_eq(Post::columns.published, true)
});

// Posts whose author is active: the key sits on this side
Post::query().where_has::<User>(User::columns.id, Post::columns.user_id, |users| {
    users.where_eq(User::columns.active, true)
});
```

A model related to itself works the same way: `Category::query().where_has::<Category>("parent_id", "id", |children| children.where_eq("active", true))` keeps the categories with an active child. The related rows are read under an alias of their own, so a nested `where_has`, a typed column or a qualified `"categories.active"` inside the closure names the child, not the outer row.

`find(id)` on a query looks a key up among the rows the query matches: `Post::query().where_eq("author_id", me).find(post_id)`. A union is refused, since the key would filter its first query only.

### Columns, Values, Your Own Types and Pages

`pluck` reads one column of every row, `value` the column of the first row, and `get_as` each row as a type of your own whose fields are the columns or their aliases — the shape of a join or a grouped `select_raw()` that no model has. On a `union()` the column is read off the union's rows, and an encrypted column comes back decrypted, as `get()` returns it:

```rust
let emails: Vec<String> = User::query().where_eq("active", true).pluck("email").await?;
let newest: Option<String> = Post::query().latest().value("title").await?;

#[derive(Deserialize)]
struct AuthorPosts { author: String, posts: i64 }

let rows: Vec<AuthorPosts> = Post::query()
    .inner_join("users", "posts.user_id", "users.id")
    .select_raw("users.name AS author, COUNT(*) AS posts")
    .group_by("users.name")
    .get_as()
    .await?;
```

`paginate(page, per_page)` returns a `Paginated<M>`: the page's models in `items`, and in `total` how many rows match across every page, with `last_page()` and `has_next_page()` worked out from them. It serializes as `{"items": .., "total": .., "page": .., "per_page": .., "last_page": ..}`, ready to return from an API:

```rust
let page = Post::query().where_eq("published", true).order_desc("id").paginate(2, 20).await?;
println!("page {} of {} ({} posts)", page.page, page.last_page(), page.total);
```

`cursor_paginate(per_page, cursor)` pages by the primary key instead, the way `chunks()` does: pass `None` for the first page and each page's `next_cursor` for the one after it. It returns a `CursorPage<M>`, serialized as `{"items": .., "next_cursor": ..}`, whose `next_cursor` is `None` once a page comes back short. It counts nothing and skips no rows by OFFSET, so a deep page costs what the first does, and rows written between requests are neither repeated nor skipped. The query follows `chunk()`'s rules (ordered only by the primary key, no `offset()` or GROUP BY), and refuses `limit()`. A cursor is opaque but not secret: it names the key to continue from, the query's filters still apply, and one that does not decode fails with `Error::Validation` on the field `cursor`:

```rust
let page = Post::query()
    .where_eq("published", true)
    .order_desc("id")
    .cursor_paginate(20, params.cursor.as_deref())
    .await?;
```

### UNION Queries

Combine results from multiple queries:

```rust
// UNION - combines results and removes duplicates
let users = User::query()
    .where_eq("active", true)
    .union(User::query().where_eq("role", "admin"))
    .get()
    .await?;

// UNION ALL - includes all results (faster, keeps duplicates)
let orders = Order::query()
    .where_eq("status", "pending")
    .union_all(Order::query().where_eq("status", "processing"))
    .union_all(Order::query().where_eq("status", "shipped"))
    .order_by("created_at", Order::Desc)
    .get()
    .await?;

// Raw UNION for complex queries
let results = User::query()
    .union_raw("SELECT * FROM archived_users WHERE year = 2023")
    .get()
    .await?;

// The same with its values bound; write `?` on every backend
let results = User::query()
    .union_raw_with("SELECT * FROM archived_users WHERE year = ?", [year.into()])
    .get()
    .await?;
```

A union's `order_by` sorts the combined rows, so a column is written by its bare output name there even when a query of the union joins another table.

### Window Functions

Perform calculations across sets of rows. Every shorthand takes the output column's alias, the function's own arguments, the `PARTITION BY` columns (one column, a tuple or array of them, or `()` for none), and the window's `ORDER BY` column and direction:

```rust
use tideorm::prelude::*;

// ROW_NUMBER - assign sequential numbers
let products = Product::query()
    .row_number("row_num", "category", "price", Order::Desc)
    .get_json()
    .await?;
// SQL: ROW_NUMBER() OVER (PARTITION BY "category" ORDER BY "price" DESC) AS "row_num"

// RANK - rank with gaps for ties
let employees = Employee::query()
    .rank("salary_rank", Employee::columns.department_id, Employee::columns.salary, Order::Desc)
    .get_json()
    .await?;

// DENSE_RANK - rank without gaps, over the whole result
let students = Student::query()
    .dense_rank("score_rank", (), "score", Order::Desc)
    .get_json()
    .await?;

// Running totals with SUM window, per region
let sales = Sale::query()
    .running_sum("running_total", "amount", "region", "date", Order::Asc)
    .get_json()
    .await?;

// LAG - access previous row value (NULL for each user's first order)
let orders = Order::query()
    .lag("prev_total", "total", 1, "user_id", "created_at", Order::Asc)
    .get_json()
    .await?;

// LEAD - access next row value
let appointments = Appointment::query()
    .lead("next_date", "date", 1, "patient_id", "date", Order::Asc)
    .get_json()
    .await?;

// NTILE - distribute into buckets, per category and brand
let products = Product::query()
    .ntile("price_quartile", 4, ("category", "brand"), "price", Order::Asc)
    .get_json()
    .await?;

// Custom window function with full control
let results = Order::query()
    .window(
        WindowFunction::new(WindowFunctionType::sum("amount"), "total_sales")
            .partition_by("region")
            .order_by("month", Order::Asc)
            .frame(FrameType::Rows, FrameBound::UnboundedPreceding, FrameBound::CurrentRow)
    )
    .get_json()
    .await?;

// LAG with a value where there is no earlier row. The default is trusted SQL,
// written in as given, hence the `_raw` in the name.
let orders = Order::query()
    .window(
        WindowFunction::new(WindowFunctionType::lag_with_raw_default("total", 1, "0"), "prev_total")
            .partition_by("user_id")
            .order_by("created_at", Order::Asc)
    )
    .get_json()
    .await?;
```

`WindowFunctionType::sum`, `avg`, `min`, `max`, `count`, `first_value`, `last_value`, `nth_value`, `lag` and `lead` take a column name or a typed column (`Sale::columns.amount`), which is written with its table.

### Common Table Expressions (CTEs)

Define temporary named result sets:

```rust
use tideorm::prelude::*;

// Simple CTE
let orders = Order::query()
    .with_cte(CTE::new(
        "high_value_orders",
        "SELECT * FROM orders WHERE total > 1000",
    ))
    .where_raw("id IN (SELECT id FROM high_value_orders)")
    .get()
    .await?;

// CTE from another query builder
let active_users = User::query()
    .where_eq("active", true)
    .select(["id", "name", "email"]);

let posts = Post::query()
    .with_query("active_users", active_users)
    .inner_join("active_users", "posts.user_id", "active_users.id")
    .get()
    .await?;

// CTE with column aliases
let stats = Sale::query()
    .with_cte(CTE::with_columns(
        "daily_stats",
        ["sale_date", "total_sales", "order_count"],
        "SELECT DATE(created_at), SUM(amount), COUNT(*) FROM sales GROUP BY DATE(created_at)",
    ))
    .where_raw("date IN (SELECT sale_date FROM daily_stats WHERE total_sales > 10000)")
    .get()
    .await?;

// Recursive CTE for hierarchical data
let employees = Employee::query()
    .with_recursive_cte(
        "org_tree",
        ["id", "name", "manager_id", "level"],
        // Base case: top-level managers
        "SELECT id, name, manager_id, 0 FROM employees WHERE manager_id IS NULL",
        // Recursive: employees under managers
        "SELECT e.id, e.name, e.manager_id, t.level + 1 
         FROM employees e 
         INNER JOIN org_tree t ON e.manager_id = t.id"
    )
    .where_raw("id IN (SELECT id FROM org_tree)")
    .get()
    .await?;
```

---


---

## Full-Text Search

TideORM provides full-text search capabilities across PostgreSQL (tsvector/tsquery), MySQL (FULLTEXT), and SQLite (FTS5).

Enable the feature explicitly when you need the full-text search API:

```toml
tideorm = { version = "0.17.6", features = ["postgres", "fulltext"] }
```

### Search Basics

```rust
use tideorm::prelude::*;

// Simple full-text search
let results = Article::search(&["title", "content"], "rust programming")
    .get()
    .await?;

// Search with ranking (ordered by relevance)
let ranked = Article::search(&["title", "content"], "rust async")
    .with_ranking()
    .limit(10)
    .get_ranked()
    .await?;

for result in ranked {
    println!("{}: {} (rank: {:.2})", 
        result.record.id, 
        result.record.title, 
        result.rank
    );
}

// Count matching results
let count = Article::search(&["title", "content"], "rust")
    .count()
    .await?;

// Get first matching result
let first = Article::search(&["title"], "rust")
    .first()
    .await?;
```

`search` takes its columns as names (`["title", "content"]`) or typed columns (`(Article::TITLE, Article::CONTENT)`), as `select` does.

### Search Modes

```rust
use tideorm::fulltext::{SearchMode, FullTextConfig};

// Natural language search (default)
Article::search(&["content"], "learn rust programming").get().await?;

// Boolean search with operators
Article::search(&["content"], "+rust +async -javascript")
    .mode(SearchMode::Boolean)
    .get()
    .await?;

// Phrase search (exact phrase matching)
Article::search(&["content"], "async await")
    .mode(SearchMode::Phrase)
    .get()
    .await?;

// Prefix search (for autocomplete)
Article::search(&["title"], "prog")
    .mode(SearchMode::Prefix)
    .get()
    .await?;
```

Every mode builds the backend's syntax from the words of the search text, so an operator a user types is never parsed as syntax. In `Boolean` mode each term is required and one written `-term` or `-"a phrase"` is left out; MySQL also reads its `~ < > *` operators, and a MySQL term without `+` is optional. SQLite's `NOT` needs something to subtract from, so there a search of exclusions only matches nothing. `Phrase` matches the words in order, `Prefix` words beginning with each search word, and `Proximity(n)` the words within `n` words of each other in either order (on PostgreSQL at most 64 apart). A search with no word left to search for matches nothing on every backend.

On PostgreSQL the text search configuration (`language`, `english` by default) is written into the statement as a constant, the way `FullTextIndex` writes it into the index, so a search can use that index; it must therefore be a configuration name. On SQLite a search reads the columns it names from the table's FTS5 index, which `FullTextIndex` fills with the rows already in the table when it is created, and `get_ranked()` reports the negated `bm25()` score, so a higher rank is a better match on every backend.

A search on a soft-delete model leaves trashed rows out, as a query does; `with_trashed()` reads them too and `only_trashed()` reads nothing else:

```rust
let binned = Article::search(&["title"], "rust").only_trashed().get().await?;
```

### Search Configuration

```rust
use tideorm::fulltext::{FullTextConfig, SearchMode, SearchWeights};

let config = FullTextConfig::new()
    .language("english")        // Text analysis language
    .mode(SearchMode::Boolean)  // Search mode
    // Custom weights for ranking (title > summary > content)
    .weights(SearchWeights::new(1.0, 0.5, 0.3, 0.1));

let results = Article::search(&["title", "summary", "content"], "rust programming")
    .config(config)
    .get()
    .await?;
```

`stop_words`, `min_word_length` and `max_word_length` leave terms out of the search text before it is sent, on every backend; the index is untouched. A search whose every term is left out matches nothing, and a `SearchMode::Phrase` search is sent as written.

```rust
let config = FullTextConfig::new()
    .stop_words(["the", "a"])
    .min_word_length(3);
// Searches for "rust" and "ownership" only.
let results = Article::search(&["content"], "the rust of ownership")
    .config(config)
    .get()
    .await?;
```

### Text Highlighting

`highlight()` marks the whole-word matches in the searched columns of each `get_ranked()` result. The record's text is HTML-escaped around the tags, so a stored `<script>` reaches the page as text; call `escape_html(false)` for tags that are not HTML:

```rust
use tideorm::fulltext::HighlightConfig;

let ranked = Article::search((Article::TITLE, Article::CONTENT), "rust async")
    .highlight(HighlightConfig::default()) // `<mark>` tags, 10 words around the first match
    .get_ranked()
    .await?;

// `<b>` tags, 5 words around the match, at most 200 characters
let config = HighlightConfig::default().tags("<b>", "</b>").fragment_words(5).max_length(200);

for field in &ranked[0].highlights {
    println!("{}: {}", field.field, field.highlighted);
}
```

`HighlightConfig::highlight` marks any text the same way, escaping it as a search does:

```rust
use tideorm::fulltext::HighlightConfig;

let marked = HighlightConfig::default().whole_text().highlight(&comment.body, "fox");
// "a &lt;script&gt; about the <mark>fox</mark>" for "a <script> about the fox"
```

`highlight_text` and `generate_snippet` mark text without escaping it, so keep them for text that is not HTML:

```rust
use tideorm::fulltext::{highlight_text, generate_snippet};

let text = "The quick brown fox jumps over the lazy dog.";

// Highlight search terms
let highlighted = highlight_text(text, "fox lazy", "<mark>", "</mark>");
// Result: "The quick brown <mark>fox</mark> jumps over the <mark>lazy</mark> dog."

// Generate snippet with context
let long_text = "Lorem ipsum... The fox jumped... More text here...";
let snippet = generate_snippet(long_text, "fox", 5, "<b>", "</b>");
// Result: "...dolor sit amet. The <b>fox</b> jumped over the..."
```

### Creating Full-Text Indexes

```rust
use tideorm::fulltext::{FullTextIndex, PgFullTextIndexType};
use tideorm::config::DatabaseType;

// Create index definition
let index = FullTextIndex::new(
    "idx_articles_search",
    "articles",
    ["title", "content"],
)
.language("english")
.pg_index_type(PgFullTextIndexType::GIN);

// Generate the statements for your database, then run each in order
let statements = index.to_sql(DatabaseType::Postgres);
// PostgreSQL: CREATE INDEX "idx_articles_search" ON "articles"
//             USING GIN ((to_tsvector('english', ...)))

let statements = index.to_sql(DatabaseType::MySQL);
// MySQL: CREATE FULLTEXT INDEX `idx_articles_search` ON `articles`(`title`, `content`)

let statements = index.to_sql(DatabaseType::SQLite);
// SQLite: the FTS5 virtual table, then the triggers that keep it in sync

for statement in &statements {
    Database::execute(statement).await?;
}
```

A SQLite table has one full-text index, the `<table>_fts` table its searches read, whatever the index is named. Applying the same index again changes nothing; one over a column that index does not hold fails with `no such column` instead of keeping the first index's columns, so drop `<table>_fts` and its triggers to index other columns. A table named with its attached database (`tenant.posts`) gets its index in that database.

The SQLite index follows the table's rowid, which a `VACUUM` keeps only for a table with an `INTEGER PRIMARY KEY`. After vacuuming a UUID- or text-keyed table, run `index.sqlite_rebuild_sql()` before anything else touches it, or searches return other rows.

MySQL's boolean-mode operators (`+ - < > ( ) ~ * " @`) are sanitized before a query reaches `MATCH ... AGAINST`: each term keeps one leading operator and a trailing `*`, a quoted phrase stays a phrase, and a query left with no searchable term matches nothing instead of failing with a syntax error. In natural-language mode the operators are dropped.

### PostgreSQL-Specific Features

```rust
use tideorm::fulltext::pg_headline_sql;

// ts_headline for server-side highlighting, with its text bound as $1 to $3
let (headline, params) = pg_headline_sql(
    "content",           // column
    "search query",      // search terms
    "english",           // language
    "<b>", "</b>"        // highlight tags
);
// headline: ts_headline($1::regconfig, "content", plainto_tsquery($1::regconfig, $2), $3)
let rows = Database::raw_json_with_params(
    &format!("SELECT id, {headline} AS headline FROM articles"),
    params,
)
.await?;
```

---

## Multi-Database Support

TideORM automatically detects your database type and generates appropriate SQL syntax. The same code works seamlessly across PostgreSQL, MySQL, and SQLite.

### Connecting to Different Databases

```rust
// PostgreSQL
TideConfig::init()
    .database("postgres://user:pass@localhost/mydb")
    .connect()
    .await?;

// MySQL / MariaDB
TideConfig::init()
    .database("mysql://user:pass@localhost/mydb")
    .connect()
    .await?;

// SQLite
TideConfig::init()
    .database("sqlite://./data.db?mode=rwc")
    .connect()
    .await?;
```

### Explicit Database Type

```rust
TideConfig::init()
    .database_type(DatabaseType::MySQL)
    .database("mysql://localhost/mydb")
    .connect()
    .await?;
```

### Database Feature Detection

JSON, upsert, window functions, and CTEs work on every backend. The two
capabilities that differ are worth checking:

```rust
let db_type = require_db()?.backend();

if db_type.supports_arrays() {
    // Native array columns (PostgreSQL only); elsewhere arrays are JSON
}

if db_type.supports_returning() {
    // `BatchUpdateBuilder::execute_returning()` works (PostgreSQL and SQLite)
}
```

### Database-Specific JSON Operations

TideORM translates JSON filters to each backend's syntax:

```rust
// This query works on all databases with JSON support
Product::query()
    .where_json_contains("metadata", serde_json::json!({"featured": true}))
    .get()
    .await?;
```

**Generated SQL by database:**

| Operation | PostgreSQL | MySQL | SQLite |
|-----------|------------|-------|--------|
| JSON Contains | `(col)::jsonb @> '{"key":1}'` | `JSON_CONTAINS(col, '{"key":1}')` | `json_each(col)` + subquery |
| Key Exists | `jsonb_typeof(col) = 'object' AND (col)::jsonb ? 'key'` | `JSON_CONTAINS_PATH(col, 'one', '$."key"')` | `json_type(col, '$."key"') IS NOT NULL` |
| Path Exists | `(col)::jsonb @? 'strict $.path'` | `JSON_CONTAINS_PATH(col, 'one', '$.path')` | `json_type(col, '$.path') IS NOT NULL` |

`where_json_key_not_exists` and `where_json_path_not_exists` negate the two tests. On every backend a member that holds JSON `null` exists, and a `NULL` column matches neither a test nor its negation. A path is `$` followed by `.key`, `."quoted key"` or `[index]` steps, or those steps alone, read from the root (`a.b[0]` is `$.a.b[0]`). Every backend reads it strictly: a key exists only in an object, never as a string element of an array, a step never looks inside an array it does not index, and an index never reads a value that is not an array as an array of one (MySQL's indexed steps check their parent is an array). A path with other steps fails the query, except on PostgreSQL, which reads it as the JSONPath written.

PostgreSQL's JSON operators exist only for `jsonb`, so the column is cast: they work on a `json` column (what `t.json(..)` creates) too, and on a `jsonb` column the cast is dropped at planning, so its GIN index still applies.

SQLite has no containment operators, so TideORM rebuilds PostgreSQL's `@>` and `<@` from `json_each`: an object matches key by key, an array element by element, and a bare scalar matches a top-level array that holds it. MySQL's `JSON_CONTAINS` lets a value match an array holding it at any depth, so on its own `{"tags": "a"}` would match `{"tags": ["a", "b"]}`; TideORM adds a type check for each path of the document you pass, so MySQL and MariaDB follow PostgreSQL there too. Below an array the paths are not known in advance, so MySQL's reading stays there: `[{"k": [1, 2]}]` contains `[{"k": 1}]` on MySQL and not on PostgreSQL.

### Database-Specific Array Operations

Array operations are fully supported on PostgreSQL. On MySQL/SQLite, arrays are stored as JSON:

```rust
// PostgreSQL native arrays
Product::query()
    .where_array_contains("tags", vec!["sale", "featured"])
    .get()
    .await?;
```

**Generated SQL:**

| Operation | PostgreSQL | MySQL/SQLite |
|-----------|------------|--------------|
| Contains | `col @> ARRAY['a','b']` | `JSON_CONTAINS(col, '["a","b"]')` |
| Contained By | `col <@ ARRAY['a','b']` | `JSON_CONTAINS('["a","b"]', col)` |
| Overlaps | `col && ARRAY['a','b']` | `JSON_OVERLAPS(col, '["a","b"]')` (MySQL 8+) |

### Database-Specific Optimizations

applies optimizations based on your database:

| Feature | PostgreSQL | MySQL | SQLite |
|---------|------------|-------|--------|
| Parameter Style | `$1, $2, ...` | `?, ?, ...` | `?, ?, ...` |
| Identifier Quoting | `"column"` | `` `column` `` | `"column"` |
| Float Casting | `FLOAT8` | `DOUBLE` | `REAL` |

### Feature Compatibility Matrix

| Feature | PostgreSQL | MySQL | SQLite |
|---------|:----------:|:-----:|:------:|
| JSON/JSONB | ✅ | ✅ | ✅ (JSON1) |
| Native JSON Operators | ✅ | ✅ | ❌ |
| Native Arrays | ✅ | ❌ | ❌ |
| RETURNING Clause | ✅ | ❌ | ✅ (3.35+) |
| Upsert | ✅ | ✅ | ✅ |
| Window Functions | ✅ | ✅ (8.0+) | ✅ (3.25+) |
| CTEs | ✅ | ✅ (8.0+) | ✅ (3.8+) |
| Full-Text Search | ✅ | ✅ | ✅ (FTS5) |
| Schemas | ✅ | ✅ | ❌ |
| `DISTINCT ON` | ✅ | ❌ | ❌ |
| Row Locks (`lock_for_update`) | ✅ | ✅ | ❌ (locks the whole database) |

---

## Raw SQL Queries

For complex queries that can't be expressed with the query builder:

```rust
// Execute raw SQL and return model instances
let users: Vec<User> = Database::raw::<User>(
    "SELECT * FROM users WHERE age > 18"
).await?;

// With parameters (use $1, $2 for PostgreSQL, ? for MySQL/SQLite)
let users: Vec<User> = Database::raw_with_params::<User>(
    "SELECT * FROM users WHERE age > $1 AND status = $2",
    vec![18.into(), "active".into()]
).await?;

// Execute raw SQL statement (INSERT, UPDATE, DELETE)
let affected = Database::execute(
    "UPDATE users SET active = false WHERE last_login < NOW() - INTERVAL '1 year'"
).await?;

// Execute with parameters
let affected = Database::execute_with_params(
    "DELETE FROM users WHERE status = $1",
    vec!["banned".into()]
).await?;

// Rows of any shape, read as a type of your own
#[derive(FromQueryResult)]
struct Totals { country: String, users: i64 }

let totals: Vec<Totals> = Database::raw_as(
    "SELECT country, COUNT(*) AS users FROM users GROUP BY country"
).await?;
```

`FromQueryResult` reads each field from the column of its name, as that field's type, the way a model reads its columns, so alias a computed column to its field (`AS users`); an `Option` field takes SQL `NULL`. `raw_as_with_params` binds values the same way, and a `Database` handle offers `query_raw_as` and `query_raw_as_with_params` to run on that database rather than the global one.

Name the columns in raw SQL rather than `SELECT *` when the table can change under a running application. Statements are prepared and cached per connection (see `statement_cache_capacity` under [Driver Options](getting-started.md#driver-options)), and after a column is added PostgreSQL rejects a cached `SELECT *` (`cached plan must not change result type`) while SQLite can report the old column list, until the connection is replaced. The query builder is not affected: it always names the model's columns.

Every raw statement invalidates cached reads of its database once it has run, reads included, and so does one that fails: its text cannot show which tables it writes, as a `SELECT` can write through a function, a view or a trigger, and a statement before a failing one may already have committed. Independent databases keep their cached results. Separate pools using the same SQLite file or the same PostgreSQL endpoint and database share invalidation; MySQL databases on one endpoint share it because triggers and foreign keys can cross schemas. SQLite handles that use `ATTACH`, and connections whose database cannot be identified, retain conservative global invalidation. Entries inserted manually with `QueryCache::set` or `set_tagged` have no database identity and are invalidated by any database write.

The cache lives in one process. To keep several processes' caches in step, give the cache a hook: it is called with a `CacheInvalidation` (`Database(name)`, `Table(name)` or `All`, serializable with serde) after each invalidation. A write's comes once as it runs and again when its transaction commits. Publish each event on a channel of your own, such as Redis or NATS, and hand what the other processes publish to `apply_invalidation`, which drops the same entries without calling the hook again. A database's name is built from the connection's host, port and database, so connect every process the same way. The hook runs on the writing task, so hand the event off rather than wait on the network:

```rust
use tideorm::prelude::*;

let (events, mut outgoing) = tokio::sync::mpsc::unbounded_channel::<CacheInvalidation>();
QueryCache::global().set_invalidation_hook(move |event| {
    let _ = events.send(event.clone());
});
tokio::spawn(async move {
    while let Some(event) = outgoing.recv().await {
        publish(serde_json::to_string(&event)?).await; // your broker
    }
    Ok::<_, serde_json::Error>(())
});

// In the task subscribed to the other processes:
let event: CacheInvalidation = serde_json::from_str(&message)?;
QueryCache::global().apply_invalidation(&event);
```

---

## Query Logging

Enable SQL query logging for development/debugging:

```bash
# Set environment variable
TIDE_LOG_QUERIES=true cargo run
```

When enabled, every statement is printed to stderr: query-builder and migration statements before they run, the rest — `find`, `save`, `update`, `delete`, upserts and raw SQL — once they complete. The variable is read once, when the first statement runs. Bound values are never logged: a statement appears with its placeholders (`$1`, `?`). `QueryLogger` records them too, except the migrator's, with the query builder's tagged by table. `TIDE_LOG_LEVEL` (`error`, `warn`, `info`, `debug`, `trace`) and `TIDE_SLOW_QUERY_MS` configure the structured `QueryLogger` the way `QueryLogger::global()` does in code; settings made in code take precedence.

`QueryLogger::global()` returns a builder; nothing changes until `enable()`, `disable()` or `apply()` ends it. `set_sink` sends what the logger prints somewhere other than stderr, such as `tracing`, and `QueryLogger::reset()` puts the logger back as the environment configures it, for tests:

```rust
use std::time::Duration;
use tideorm::logging::{LogLevel, QueryLogger};

QueryLogger::global()
    .level(LogLevel::Debug)
    .slow_query_threshold(Duration::from_millis(50))
    .history_limit(200)
    .enable();

QueryLogger::set_sink(|entry, text| {
    if !entry.success {
        tracing::warn!("{text}");
    }
});

for entry in QueryLogger::slow_queries() {
    println!("{entry:?}");
}
```

A sink runs on the task that ran the statement, so one that writes to a file or the network should hand the text off and return. A bounded channel keeps that from holding up queries, dropping lines instead of waiting when the writer falls behind:

```rust
let (lines, mut pending) = tokio::sync::mpsc::channel::<String>(1024);
QueryLogger::set_sink(move |_entry, text| {
    let _ = lines.try_send(text.to_owned());
});
tokio::spawn(async move {
    while let Some(text) = pending.recv().await {
        write_log_line(&text).await; // your writer
    }
});
```

To look at a query before it runs, print `debug()`: the statement with its values written in, the parameterized statement, and the clauses it was built from. A query that a terminal would refuse — an unsafe `order_by` taken from a request, a bad raw fragment, a zero page — reports the reason on an `Invalid:` line, and `validate()` returns the same error without running anything, so a handler can answer 400 first:

```rust
let query = Post::query().where_eq("published", true).order_by(params.sort.as_str(), Order::Asc);
println!("{}", query.debug());
query.validate()?;
```

---

## Error Handling

TideORM provides rich error types with optional context:

```rust
// Get context from errors
if let Err(e) = User::find_or_fail(999).await {
    if let Some(ctx) = e.context() {
        println!("Error in table: {:?}", ctx.table);
        println!("Column: {:?}", ctx.column);
        println!("Query: {:?}", ctx.query);
    }
}

// Create errors with context
use tideorm::error::{Error, ErrorContext};

let ctx = ErrorContext::new()
    .table("users")
    .column("email")
    .query("SELECT * FROM users WHERE email = $1");

return Err(Error::not_found("User not found").with_context(ctx));
```

### Classifying Database Failures

`failure_kind()` says what the database reported, independent of backend and message wording, and `is_retryable()` whether running the operation again can succeed:

```rust
use tideorm::error::DbFailureKind;

match User::create(user).await {
    Ok(user) => { /* created */ }
    Err(e) if e.failure_kind() == DbFailureKind::UniqueViolation => { /* email taken */ }
    Err(e) if e.is_retryable() => { /* deadlock, lock timeout, dropped connection: retry */ }
    Err(e) => return Err(e),
}
```

`is_unique_violation()`, `is_foreign_key_violation()` and `is_not_null_violation()` ask the same about one kind. `validation_errors()` returns every failure behind an `Error::Validation`, field by field (see [Handling Validation Errors](models.md#handling-validation-errors)).

Constraint violations (`UniqueViolation`, `ForeignKeyViolation`, `NotNullViolation`, `CheckViolation`) are never retryable, and neither is `InvalidValue`: a value too long for its column, out of its range, or not valid for its type. SQLite checks none of that, so a string longer than its `VARCHAR(255)` is stored there and refused by PostgreSQL and MySQL; `#[validate(max_length = 255)]` keeps the backends in step. `Deadlock`, `SerializationFailure`, `LockNotAvailable` (including MySQL's lock wait timeout and a busy SQLite database), `StatementTimeout`, `ConnectionTimeout` and `ConnectionClosed` are. A connection the server closes under a statement — a restart, a failover, MySQL's `KILL` — is `ConnectionClosed`; the pool replaces it, so the next call gets a fresh one.

Opening a connection is classified too: `ConnectionRefused` (nothing listens there, or the server is not up yet) is retryable; `AuthenticationFailed`, `UnknownDatabase`, `TlsFailed` and `InvalidConfiguration` (a bad URL or option) are not, nor is `NotConnected`, the error of a model used before any connection was set up. A statement refused on a transaction that a failure already aborted reports that failure's kind, so the refusals after a deadlock are retryable too. The message is never read: an error with no classified failure, such as one built with `Error::query(..)`, is not retryable whatever it says.

### Answering a Request

`http_status()` gives a generic status for each variant (`NotFound` 404,
`Validation` 422, `Connection` 503, `Transaction` 409, `Query` 400, and so on),
but a duplicate key and a deadlock are both `Error::Query`, so an endpoint looks
at `failure_kind()` first:

```rust
use tideorm::error::{DbFailureKind, Error};
use tideorm::prelude::{JsonValue, json};

/// The status and body an endpoint answers a TideORM error with. The body
/// carries the stable `code()`, never the error's text.
fn error_response(e: &Error) -> (u16, JsonValue) {
    if let Some(errors) = e.validation_errors() {
        // 422 with every failure, field by field: {"email": ["..."]}
        return (422, json!({ "code": e.code(), "errors": errors.all() }));
    }
    let status = match e.failure_kind() {
        // http_status() answers 400 for every Error::Query, a duplicate included
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

- **What the client sees.** An error's `Display` text can hold the server's
  message, table, column and constraint names and SQL; `log_format()` adds the
  query and its context, whose conditions show `?` for the values you filtered
  by (use `debug()` on the query to see them). Send `code()` (`TIDE_NOT_FOUND`, `TIDE_VALIDATION`, ..),
  which does not change between releases, and keep the rest in your logs.
  Validation messages are written for users, so they can go out as they are.
- **The database is unavailable.** Every failure to open a connection is an
  `Error::Connection`, which `http_status()` answers with 503. Only
  `ConnectionRefused` and the timeouts are retryable: an `AuthenticationFailed`
  or `UnknownDatabase` is a deployment fault that a retry will not fix, and
  belongs in an alert as well as a 503.

#### Retrying a Transaction

Retry the whole transaction, never one statement inside it: after a deadlock the
server has already rolled the whole transaction back.

```rust
use std::time::Duration;

let mut attempt = 0u64;
let order = loop {
    match db.transaction(async |_tx| place_order(&cart).await).await {
        Err(e) if e.is_retryable() && attempt < 3 => {
            attempt += 1;
            tokio::time::sleep(Duration::from_millis(50 * attempt)).await;
        }
        outcome => break outcome?,
    }
};
```

> **Warning:** retryable means a new attempt can succeed, not that the failed
> one wrote nothing. A `ConnectionClosed` that cuts off the `COMMIT` leaves it
> unknown whether the transaction committed, and the retry then writes it a
> second time. The same holds for a single `create()`
> retried outside a transaction. Make a retried write idempotent: insert with a
> key the request carries (a client-chosen UUID, an idempotency key under a
> unique index) or use an upsert, so a second run finds the first one's row
> instead of adding another.

---

