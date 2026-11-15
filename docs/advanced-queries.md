# Joins, Aggregates and Advanced SQL

This page covers the parts of the [query builder](queries.md) that go past filtering a single table: choosing columns, joins, aggregates and grouping, filtering by related rows, subqueries, unions, window functions, CTEs, row locks, and updating or deleting many rows at once.

```rust
use tideorm::prelude::*;

#[derive(serde::Deserialize)]
struct AuthorPosts {
    author: String,
    posts: i64,
}

async fn prolific_authors() -> tideorm::Result<Vec<AuthorPosts>> {
    Post::query()
        .inner_join("users", "posts.user_id", "users.id")
        .where_eq("users.active", true)
        .select_raw("users.name AS author, COUNT(*) AS posts")
        .group_by("users.name")
        .having(Aggregate::count().gte(10))
        .get_as()
        .await
}
```

## Selecting Columns

A query reads the model's own columns unless you choose others:

| Method | Effect |
|---|---|
| `select(columns)` | the columns to read: names, field names or typed columns, as an array, a `Vec`, a slice, or a tuple of typed columns of different types (`(User::ID, User::EMAIL)`); a second call replaces the first |
| `add_select(column)` | adds one column to the columns selected so far; on a query that selected none, the projection becomes that one column |
| `select_raw(expression)` | adds a trusted SQL expression, such as `COUNT(*) AS total` |
| `select_subquery(query, alias)` | adds a scalar subquery, its values still bound |
| `distinct()` | `SELECT DISTINCT` |

The three kinds accumulate: typed columns come first, then raw expressions, then subqueries, and only a query that selected nothing reads the model's columns. Another model's typed column is written with its table, for a joined query.

`get()` builds complete models, so it refuses a `select()` that leaves model columns out. Read a partial projection with `get_json()`, `get_as::<T>()`, `pluck()` or `value()` (see [Running a Query](queries.md#running-a-query)):

```rust,ignore
let rows = User::query()
    .select((User::columns.id, User::columns.email))
    .add_select(User::columns.name)
    .select_subquery(Post::query().select(["id"]).where_raw("posts.user_id = users.id").limit(1), "latest_post")
    .get_json()
    .await?;
```

With `distinct()`, `count()` counts the deduplicated rows, and `exists()` is unaffected. A projection holding an encrypted column is refused, since its ciphertext differs for equal values. PostgreSQL requires every `ORDER BY` expression of a `SELECT DISTINCT` to be in the select list; TideORM checks that before sending the query, except for an `order_by_raw()` expression, which stays your responsibility.

Reading columns of a related table next to the model's (`select_with_linked`, `select_also_linked`) is covered on [Relations](relations.md).

## Joins

Name both sides of a join as `table.column` (or `alias.column`); a bare column name invalidates the query, and the error surfaces when it runs. Everywhere else a bare name of one of the model's columns means the model's table: filters, `select()`, ordering and aggregates are written `table.column` once the query joins, so reach a joined table's column as `table.column`. The `ON` clause is one equality.

```rust,ignore
let posts = Post::query()
    .inner_join("users", "posts.user_id", "users.id")
    .where_eq("users.active", true)
    .get()
    .await?;
```

`join::<R>(JoinType::Inner, left, right)` joins model `R`'s table with its columns typed, and `join_as::<R>(join_type, alias, left, right)` joins it under an alias, writing `R`'s typed columns with the alias:

```rust,ignore
let posts = Post::query()
    .join::<User>(JoinType::Inner, Post::columns.user_id, User::columns.id)
    .where_eq(User::columns.active, true)
    .get()
    .await?;
```

| Method | Join |
|---|---|
| `inner_join(table, left, right)`, `inner_join_as(table, alias, left, right)` | `INNER JOIN` |
| `left_join`, `left_join_as` | `LEFT JOIN`: keeps this model's rows with no match |
| `right_join`, `right_join_as` | `RIGHT JOIN`: keeps the joined table's rows with no match (SQLite 3.39 or later) |
| `join::<R>(join_type, left, right)`, `join_as::<R>(join_type, alias, left, right)` | any `JoinType` (`Inner`, `Left`, `Right`) to a model's table |

An alias is how a table joins itself or joins the same table twice. A right join's unmatched rows have `NULL` in every model column, which `get()` cannot turn into models, so read such a query with `get_json()`. Two selected columns may not share an output name, since a row holds only one of them: alias one (`tags.id AS tag_id`).

```rust,ignore
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

Grouping joined rows into parents with their children (`JoinResultConsolidator`) is covered on [Relations](relations.md).

## Aggregates

`sum`, `avg`, `min`, `max` and `count_distinct` each run one statement and read the result as the type you ask for. `sum` returns that type, zero over no rows; `avg`, `min` and `max` return an `Option`, `None` over no rows; `count_distinct` returns a `u64`. A sum is never read through a float, so an integer total past 2^53 or a `Decimal` total stays exact, and `min`/`max` work on any column type: a number, text, a date. A model's enum column reads as its variants.

```rust,ignore
let revenue: Decimal = Order::query().where_eq("paid", true).sum("total").await?;
let average: Option<f64> = Product::query().avg(Product::columns.price).await?;
let first_signup: Option<DateTime<Utc>> = User::query().min("created_at").await?;
let oldest = User::query().max::<i32>("age").await?;
let countries = User::query().count_distinct(User::columns.country).await?;
```

`aggregates()` computes several over the same rows in one statement, read into a tuple with one type per aggregate (or a `Vec`), each typed as the single terminal would read it:

```rust,ignore
let (orders, revenue, largest): (u64, Decimal, Option<Decimal>) = Order::query()
    .where_eq("region", "EU")
    .aggregates(&[Aggregate::count(), Aggregate::sum("total"), Aggregate::max("total")])
    .await?;
```

`Aggregate` offers `count()`, `count_distinct(col)`, `sum(col)`, `avg(col)`, `min(col)` and `max(col)`.

The aggregates honour joins, CTEs, unions and `limit()`/`offset()`, and refuse `group_by()`, `having()` and window functions, since a grouped aggregate has no single answer. `count()` is different: it ignores `limit()` and `offset()`, so a paged query counts its total across every page, while `Aggregate::count()` counts only the rows the limit and offset leave. `count_distinct` refuses an encrypted column.

## Grouping and Having

`group_by(column)` groups the rows, and `having(condition)` keeps the groups whose aggregate passes a comparison, with the value bound. Build a condition from an `Aggregate` with `gt`, `gte`, `lt`, `lte`, `eq` or `ne`; several `having` calls must all hold. Read grouped rows with `get_as()` or `get_json()`:

```rust,ignore
#[derive(serde::Deserialize)]
struct RegionTotal { region: String, revenue: Decimal }

let busy_regions: Vec<RegionTotal> = Order::query()
    .select_raw("region, SUM(total) AS revenue")
    .group_by("region")
    .having(Aggregate::sum("total").gt(5_000))
    .having(Aggregate::count().gte(3))
    .get_as()
    .await?;
```

`having_raw(sql)` takes a raw condition as trusted SQL, checked for statement separators and comments, never escaped. `having_raw_with(sql, values)` binds a value for each `?`, on every backend, so the values may come from a request:

```rust,ignore
Order::query().group_by("rep").having_raw("COUNT(DISTINCT region) > 1");
Order::query().group_by("rep").having_raw_with("SUM(total) > ?", [params.min_revenue.into()]);
```

## Filtering by Related Rows

`where_has::<R>(foreign_key, local_key, |q| ..)` keeps the rows with a related `R` row that the closure's query keeps. The closure receives `R`'s own query, so `R`'s soft-delete scope applies and any filter works; pass `|q| q` to ask only that a related row exists. `where_doesnt_have::<R>` keeps the rows with none, a row with no related rows at all included. The first key is `R`'s column that holds the second, this model's:

```rust,ignore
// Users with a published post
User::query().where_has::<Post>(Post::columns.user_id, User::columns.id, |posts| {
    posts.where_eq(Post::columns.published, true)
});

// Posts whose author is active: here the key sits on this side
Post::query().where_has::<User>(User::columns.id, Post::columns.user_id, |users| {
    users.where_eq(User::columns.active, true)
});

// Users who never posted
User::query().where_doesnt_have::<Post>(Post::columns.user_id, User::columns.id, |posts| posts);
```

A related query that joins, pages or unions fails the query, since a correlated `EXISTS` cannot keep those. A model related to itself works the same way: `Category::query().where_has::<Category>("parent_id", "id", |children| children.where_eq("active", true))` keeps the categories with an active child. The related rows are read under an alias of their own, so a nested `where_has`, a typed column or a qualified `"categories.active"` inside the closure names the child, not the outer row.

The `has_*` helpers do the same over a table named as text rather than a model. Every identifier must be a plain table or column name, and since the table is not modeled, its soft-deleted rows count:

| Method | Keeps the rows |
|---|---|
| `has_related(table, foreign_key, local_key, column, value)` | with a related row whose `column` equals `value` |
| `has_no_related(table, foreign_key, local_key, column, value)` | with no such related row; a row with no related rows at all passes |
| `has_any_related(table, foreign_key, local_key)` | with any related row |
| `has_no_related_at_all(table, foreign_key, local_key)` | with no related row |

```rust,ignore
User::query().has_related("posts", "user_id", "id", "title", "Hello");
```

## Subqueries

Another model's query can be a subquery of this one. It keeps its values bound and is rendered with the outer query for the backend it runs on, and a model query applies that model's soft-delete scope:

| Method | Condition |
|---|---|
| `where_in_subquery(column, query)` | `column IN (subquery)` |
| `where_not_in_subquery(column, query)` | `column NOT IN (subquery)` |
| `where_exists(query)` | `EXISTS (subquery)` |
| `where_not_exists(query)` | `NOT EXISTS (subquery)` |
| `select_subquery(query, alias)` | a scalar subquery in the projection |

```rust,ignore
let authors = User::query()
    .where_in_subquery(User::columns.id, Post::query().select(["user_id"]).where_eq("published", true))
    .get()
    .await?;

let with_comments = Post::query()
    .where_exists(Comment::query().where_raw("comments.post_id = posts.id"))
    .get()
    .await?;
```

## Unions

```rust,ignore
// UNION: combines the rows and removes duplicates
let users = User::query()
    .where_eq("active", true)
    .union(User::query().where_eq("role", "admin"))
    .get()
    .await?;

// UNION ALL: keeps every row
let posts = Post::query()
    .where_eq("status", "draft")
    .union_all(Post::query().where_eq("status", "review"))
    .union_all(Post::query().where_eq("status", "scheduled"))
    .order_by("created_at", Order::Desc)
    .get()
    .await?;

// A raw operand, trusted SQL; `_with` binds a value for each `?` on every backend
let users = User::query()
    .union_raw_with("SELECT * FROM archived_users WHERE year = ?", [year.into()])
    .get()
    .await?;
```

`union_raw` and `union_all_raw` take a raw operand with no values, `union_raw_with` and `union_all_raw_with` one with bound values; prefer a query builder operand whenever one can say it. A union's `order_by` sorts the combined rows, so a column is written by its bare output name there even when a query of the union joins another table.

An operand bound to a database must use the same database and transaction as the outer query. `union()` refuses a projection holding an encrypted column, since ciphertext cannot deduplicate plaintext; with `union_all()`, every operand must have the same encrypted table and column at each output position, so keep explicit columns or the model's default projection there.

## Window Functions

A window function adds a computed column to each row. Window values are not model fields, so read the rows with `get_json()` or `get_as()`. Every shorthand takes the output column's alias, the function's own arguments, the `PARTITION BY` columns (one column, a tuple or array of them, or `()` for none), and the window's `ORDER BY` column and direction:

```rust,ignore
// ROW_NUMBER() OVER (PARTITION BY "category" ORDER BY "price" DESC) AS "row_num"
Product::query().row_number("row_num", "category", "price", Order::Desc)

// RANK with gaps for ties; DENSE_RANK without, over the whole result
Employee::query().rank("salary_rank", Employee::columns.department_id, Employee::columns.salary, Order::Desc)
Student::query().dense_rank("score_rank", (), "score", Order::Desc)

// A running total per region
Order::query().running_sum("running_total", "total", "region", "created_at", Order::Asc)

// The previous and next values in each partition (NULL where there is none)
Order::query().lag("prev_total", "total", 1, "user_id", "created_at", Order::Asc)
Appointment::query().lead("next_date", "date", 1, "patient_id", "date", Order::Asc)

// Four buckets per category and brand, and each category's cheapest price
Product::query().ntile("price_quartile", 4, ("category", "brand"), "price", Order::Asc)
Product::query().first_value("cheapest", "price", "category", "price", Order::Asc)
```

For anything else, build a `WindowFunction` and pass it to `window()`:

```rust,ignore
let rows = Order::query()
    .window(
        WindowFunction::new(WindowFunctionType::sum("total"), "total_to_date")
            .partition_by("region")
            .order_by("created_at", Order::Asc)
            .frame(FrameType::Rows, FrameBound::UnboundedPreceding, FrameBound::CurrentRow),
    )
    .get_json()
    .await?;

// LAG with a value where there is no earlier row. The default is trusted SQL,
// written in as given, hence the `_raw` in the name.
let rows = Order::query()
    .window(
        WindowFunction::new(WindowFunctionType::lag_with_raw_default("total", 1, "0"), "prev_total")
            .partition_by("user_id")
            .order_by("created_at", Order::Asc),
    )
    .get_json()
    .await?;
```

`WindowFunctionType` has `RowNumber`, `Rank`, `DenseRank`, `Ntile(n)` and `Count(None)` (`COUNT(*)`), and constructors `sum`, `avg`, `min`, `max`, `count`, `first_value`, `last_value`, `nth_value(col, n)`, `lag(col, offset)`, `lead(col, offset)`, `lag_with_raw_default` and `lead_with_raw_default`, which take a column name or a typed column (`Order::columns.total`), written with its table. `Custom(sql)` is a function expression written as given: trusted SQL, checked for statement separators, comments and NUL characters.

- `FrameType` is `Rows`, `Range` or `Groups`. `Groups` needs PostgreSQL 11 or SQLite 3.28 or later; MySQL and MariaDB do not have it, and a query framed by it fails before it runs there. `Range` with an offset bound needs exactly one ordering column of a type the offset can be added to.
- `FrameBound` is `UnboundedPreceding`, `Preceding(n)`, `CurrentRow`, `Following(n)` or `UnboundedFollowing`.
- `last_value` reads the frame's last row, and the default frame ends at the current row, so give it a frame that ends at `UnboundedFollowing`.
- MariaDB's `LAG`/`LEAD` take no default, so on MySQL and MariaDB TideORM writes the default with a `CASE` around the function.
- Window functions are added to the projection after the other columns; they never replace the model's columns.

## Common Table Expressions

A CTE names a result set the query can read. `with_query` builds the CTE from another query, its values bound; `CTE::new` and `CTE::with_columns` take trusted SQL, which is checked as a subquery but not sanitized:

```rust,ignore
// A CTE from another query, joined like a table
let active_users = User::query().where_eq("active", true).select(["id", "name", "email"]);
let posts = Post::query()
    .with_query("active_users", active_users)
    .inner_join("active_users", "posts.user_id", "active_users.id")
    .get()
    .await?;

// A CTE from trusted SQL
let orders = Order::query()
    .with_cte(CTE::new("big_orders", "SELECT id FROM orders WHERE total > 1000"))
    .where_raw("id IN (SELECT id FROM big_orders)")
    .get()
    .await?;

// Named columns
let orders = Order::query()
    .with_cte(CTE::with_columns(
        "daily",
        ["day", "revenue"],
        "SELECT DATE(created_at), SUM(total) FROM orders GROUP BY DATE(created_at)",
    ))
    .where_raw("DATE(created_at) IN (SELECT day FROM daily WHERE revenue > 10000)")
    .get()
    .await?;

// A recursive CTE: the base case, then the recursive step
let employees = Employee::query()
    .with_recursive_cte(
        "org_tree",
        ["id", "name", "manager_id", "level"],
        "SELECT id, name, manager_id, 0 FROM employees WHERE manager_id IS NULL",
        "SELECT e.id, e.name, e.manager_id, t.level + 1 FROM employees e INNER JOIN org_tree t ON e.manager_id = t.id",
    )
    .where_raw("id IN (SELECT id FROM org_tree)")
    .get()
    .await?;
```

`with_query_columns(name, columns, query)` names the columns of a query-built CTE. `CTE::new(..).recursive()` marks a hand-built CTE recursive; one recursive CTE makes the whole `WITH` a `WITH RECURSIVE`, as SQL requires.

## Row Locks

`lock_for_update()` locks the rows a query reads until the transaction ends (`SELECT .. FOR UPDATE`). Use it for a read-check-write inside a transaction: a second transaction that locks the same rows waits until the first commits, then reads what it wrote. See [Transactions](transactions.md).

```rust,ignore
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

Outside a transaction the lock ends with the statement. `count()`, `exists()` and the aggregates lock the rows they read too, and a locked query never reads from the query cache. PostgreSQL refuses the lock on a query with `DISTINCT`, `GROUP BY` or a union. SQLite has no row locks and writes nothing: the first write of a transaction locks the whole database, so the second of two competing read-check-write transactions fails with a retryable `LockNotAvailable` error instead of racing.

## Bulk Updates and Deletes

A query can update or delete every row it matches in one statement. These statements skip model callbacks, validation and automatic timestamps, and load no rows:

| Terminal | Does |
|---|---|
| `update_all()` | returns a `BatchUpdateBuilder` over the query's rows; `.set(..)` and the other setters, then `.execute()` |
| `delete()` | deletes the matching rows; on a soft-delete model it marks them deleted instead |
| `delete_all()` | deletes every row of the table, or marks every live row deleted on a soft-delete model; takes no filter |
| `soft_delete()` | marks the live matching rows deleted (soft-delete models only) |
| `restore()` | clears the mark on the matching trashed rows (soft-delete models only) |
| `force_delete()` | removes the matching rows for good, trashed or not |

```rust,ignore
let affected = User::query()
    .where_lt(User::columns.last_login_at, cutoff)
    .update_all()
    .set(User::columns.status, "dormant")
    .execute()
    .await?;

let removed = Post::query().where_eq(Post::columns.status, "spam").delete().await?;
let emptied = Post::query().only_trashed().force_delete().await?; // empties the trash
```

Each returns the number of rows affected. `query().update_all()` keeps the query's filters, its soft-delete scope (trashed rows stay out unless the query includes them) and the database named by `query_with`; the setters, `set`, `coalesce`, `increment`, `json_set`, `array_append` and the rest, are covered on [Reading and Writing](crud.md), and the JSON and array ones on [JSON and Arrays](json-and-arrays.md#updating-json-and-arrays). Trash handling is covered on [Soft Deletes and Scopes](soft-deletes.md): `soft_delete()` stamps `deleted_at` with the application's clock (`Utc::now()`), not the database's, leaves an already trashed row's stamp alone and refuses `only_trashed()`; `delete()` refuses `only_trashed()` too.

### Safety Guards

An unfiltered bulk write is refused rather than run:

- `delete()`, `update_all()`, `soft_delete()` and `force_delete()` need at least one explicit filter that can exclude a row. `restore()` and `force_delete()` also accept `only_trashed()` alone, so they can restore or empty the whole trash; an `update_all()` of the trash still needs a filter of its own. `delete_all()` is the explicit way to clear a table, and it refuses filters (use `delete()` for those).
- A filter that renders as matching every row is refused too, whatever it looks like: an empty `where_not_in`, a raw `1 = 1`, or an OR group with such a branch. The default live-rows scope of a soft-delete model never counts as a filter.
- An `UPDATE` cannot keep a join, a union, a CTE, `group_by()`, `having()`, a window function, `limit()` or `offset()`, so a query holding one fails instead of updating more rows than it shows. A `DELETE` and the trash writes also refuse a `select()`, `distinct()` and an ordering.
