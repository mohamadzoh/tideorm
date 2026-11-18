# Reading and Writing

Every model has methods to insert, read, update and delete its rows, plus batch versions for many rows at once. This page uses the `User` model from [Defining Models](models.md).

```rust
use tideorm::prelude::*;

async fn example() -> tideorm::Result<()> {
    let user = User::create(User {
        email: "ada@example.com".into(),
        name: "Ada".into(),
        active: true,
        ..Default::default()
    })
    .await?;

    let mut user = User::find_or_fail(user.id).await?;
    user.name = "Ada Lovelace".into();
    let user = user.update().await?;

    user.delete().await?;
    Ok(())
}
```

Every method runs on the database the surrounding code chooses; see [Choosing the Database a Call Uses](models.md#choosing-the-database-a-call-uses).

## Create

`Model::create(model)` always inserts. `model.save()` inserts a new model and updates a saved one (see [How save() Decides](#how-save-decides)). Both return the row as stored, with its generated key and timestamps. An insert sends every field, so a column's database default never takes the place of a field's value.

```rust
use tideorm::prelude::*;

async fn register(email: String, name: String) -> tideorm::Result<User> {
    let user = User { email, name, active: true, ..Default::default() };
    let user = user.save().await?;
    println!("created user {}", user.id);
    Ok(user)
}
```

For an auto-increment key, `0` marks a model that is not saved yet, so you do not set the key when building a new model. `create()` and `insert_all()` let the database number a key still at `0` and write a key the model holds, as an upsert does; reset a loaded model's key to `0` before creating a copy of it.

After writing a key the model holds, TideORM moves PostgreSQL's sequence for the key past it, so the next key the database numbers does not collide with it. This needs `UPDATE` permission on the sequence; without it, the sequence is left alone. MySQL's `AUTO_INCREMENT` and SQLite's rowid move by themselves.

## Read

| Call | Returns |
|---|---|
| `User::find(id)` | `Option<User>` for that key |
| `User::find_or_fail(id)` | the `User`, or `Error::NotFound` ("users with id = 7 not found") |
| `User::find_with(id, &db)` | `find` on a specific database |
| `User::exists(id)` | `true` when the key exists |
| `User::all()` | every row. There is no limit, so use a query or `paginate` for tables that grow |
| `User::first()` | one row, or `None`. No order is applied, so "first" is whichever row the database returns first |
| `User::last()` | the row with the highest primary key |
| `User::count()` | the number of rows |
| `User::exists_any()` | `true` when the table has a row; cheaper than `count()` |
| `User::paginate(page, per_page)` | one page and the total count (`Paginated`), pages numbered from 1, ordered by primary key |
| `User::query()` | a [query builder](queries.md) for everything else |
| `User::query_with(&db)` | a query builder on a specific database |

```rust
use tideorm::prelude::*;

async fn reads() -> tideorm::Result<()> {
    let user = User::find(1).await?;                        // Option<User>
    let role = UserRole::find((1_i64, 2_i64)).await?;       // composite key
    let active = User::query().where_eq(User::ACTIVE, true).get().await?;
    let page = User::paginate(1, 20).await?;
    Ok(())
}
```

`paginate` returns a validation error when `page` or `per_page` is 0, or when `per_page` or the page's offset is past `i64::MAX`. For another order, use `User::query().order_asc(..).paginate(page, per_page)`.

On a [soft-delete model](soft-deletes.md) every read leaves trashed rows out, `find(id)` and `exists(id)` included.

`model.reload()` reads the row again and returns a fresh copy, leaving the one in hand unchanged. It reads a trashed row too, and returns `Error::NotFound` when the row is gone. `model.is_new()` says whether `save()` would insert.

## Update

`model.update()` writes the model's columns to the row with its primary key and returns the row as stored. It returns `Error::NotFound` when no row has that key.

```rust
use tideorm::prelude::*;

async fn rename(id: i64, name: String) -> tideorm::Result<User> {
    let mut user = User::find_or_fail(id).await?;
    user.name = name;
    user.update().await
}
```

`update()` writes every column of the model, not only the fields you changed, so two requests that each change one field of the same row undo each other's change. To write only some columns, name them in a [batch update](#batch-updates), which runs no hooks or validation and stamps no timestamp:

```rust,ignore
User::update_all()
    .set(User::NAME, name)
    .set(User::UPDATED_AT, Utc::now())
    .where_eq(User::ID, id)
    .execute()
    .await?;
```

To refuse a write based on a stale read (optimistic concurrency), keep a version column and make the write conditional on it. Zero rows written means someone else wrote first:

```rust,ignore
let written = Document::update_all()
    .set(Document::BODY, body)
    .increment(Document::VERSION, 1)
    .where_eq(Document::ID, id)
    .where_eq(Document::VERSION, version_read)
    .execute()
    .await?;
if written == 0 {
    // reload and show the newer version, or report a conflict
}
```

Or lock the row for the read-check-write, as [Transactions](transactions.md) shows.

## How save() Decides

`save()` chooses between an insert and an update by the primary key:

- A key still at its default (`0`, an empty string, a nil UUID, or any component of a composite key at its default) inserts.
- A single auto-increment key that is set updates. A key no row holds is `Error::NotFound`, not an insert: reset the key to `0` to insert a copy.
- Any other key that is set (natural, UUID, composite) updates the row with that key, a trashed one included, and inserts when there is none.

For that last case, a model with [callbacks](callbacks.md) first checks whether the row exists, so the right hooks run; a model without callbacks tries the update and inserts when it matched no row.

## Delete

```rust
use tideorm::prelude::*;

async fn deletes() -> tideorm::Result<()> {
    if let Some(user) = User::find(1).await? {
        user.delete().await?;                 // this row
    }
    User::destroy(2).await?;                  // by key
    User::query()
        .where_eq(User::ACTIVE, false)
        .delete()
        .await?;                              // the rows a query matches
    Ok(())
}
```

`delete()` and `destroy(id)` run the delete [callbacks](callbacks.md) and return the number of rows deleted: `0` when the row was already gone. `destroy(id)` loads the row and deletes it, so its callbacks run too.

`query().delete()` needs at least one filter; an unfiltered delete, or a filter that is always true, is refused. To delete every row on purpose, use `User::delete_all()` (or `User::query().delete_all()`, which refuses filters). Query deletes run no callbacks.

On a soft-delete model, `delete()`, `destroy()`, `query().delete()` and `delete_all()` mark rows deleted instead of removing them, and `force_delete()` removes them. See [Soft Deletes and Scopes](soft-deletes.md).

## Choosing a Write

| Call | Hooks | Validates | Timestamps | No row matches | Returns |
|---|---|---|---|---|---|
| `Model::create(m)` | validation, save, create | yes | sets `created_at` and `updated_at` | always inserts | the stored row |
| `m.save()` | those of the create or update it runs | yes | as create or update | see [above](#how-save-decides) | the stored row |
| `m.update()` | validation, save, update | yes | sets `updated_at` | `Error::NotFound` | the stored row |
| `m.delete()`, `Model::destroy(id)` | delete; `after_delete` only when a row went | no | none | `Ok(0)` | rows affected |
| `Model::insert_all(models)` | none | yes, every model before any is written | as create | always inserts | the stored rows, in input order |
| `Model::on_conflict(..)`, `insert_or_update(..)` | none | yes | an insert sets both; an update refreshes `updated_at` only when it is in the update set | inserts | the stored row |
| `update_all()`…`execute()` | none | no | none: set `updated_at` yourself | `Ok(0)` | rows affected |
| `query()`…`delete()`, `delete_all()` | none | no | none | `Ok(0)` | rows affected |

The stored row is read back from the database, so it carries generated keys, defaults and stamped timestamps. Nested saves (`save_with_many(..)`, see [Relations](relations.md)) run each model's own `save()`, hooks included, in one transaction.

## Dirty Tracking

Requires the `dirty-tracking` feature.

Models loaded or saved through TideORM keep a baseline of their last known stored column values. `changed_fields()` lists the fields that differ from it, and `original_value(field)` returns a field's value in it.

Both return an outer `Option` that separates "no baseline is known" (`None`) from "a baseline exists and nothing changed" (`Some(vec![])`). Treat `None` as unknown rather than unchanged; otherwise a model with no baseline silently skips its write.

```rust,ignore
let mut user = User::find(1).await?.unwrap();
user.name = "Jane Doe".to_string();

assert_eq!(user.changed_fields()?, Some(vec!["name"]));

// The inner `Option` is the column's own value, `None` for NULL.
assert_eq!(
    user.original_value("name")?,
    Some(Some(DbValue::from("John Doe")))
);
```

`is_dirty()` and `is_field_dirty(field)` answer the same question with a `bool`, `None` again meaning no baseline. The usual gate saves unless the model is known to be unchanged:

```rust,ignore
// Nothing to compare against (`None`): save rather than guess.
if user.is_dirty()? != Some(false) {
    user = user.save().await?;
}

if user.is_field_dirty(User::columns.email)? == Some(true) {
    send_confirmation(&user).await?;
}
```

`original_value` and `is_field_dirty` take a field name, a column name or a typed column, and return an error for a name the model does not have.

- Only stored fields are tracked, not relation wrappers.
- The baseline is refreshed by `find()`, query results, `reload()`, `save()` and `update()`. Batch writes such as `update_all()` and query deletes forget the baselines of that model type in the task that runs them.
- A model built by hand or deserialized from JSON has no baseline, and neither has one whose baseline was evicted from the bounded store.

Models are plain structs with no hidden tracking state, so baselines are kept per async task: each task (a request a server handles, a background job) compares a row with the copy it last read or wrote, and two tasks editing one row side by side never see each other's writes. A task that did not read the row itself, and code outside any task (the body of `#[tokio::main]` or `block_on`), compares with the latest baseline TideORM knows for the key. A baseline taken inside a transaction that then rolls back is withdrawn, and the one before it comes back, unless the row got a newer baseline from outside the transaction in the meantime. Copies of one row within one task share a baseline: if one of them saves first, reload the others before relying on their original values.

Each database keeps its own baselines, since two databases can hold different rows under one key (`query_with(db)`, `find_with(id, db)`). A model does not record which database it came from, so when TideORM has read different rows for a key from two databases, both methods return `None` for that key rather than compare with the wrong row, until the row is read again.

## Returning Models from an Endpoint

Render a model for an API response with `try_to_json()`. It drops hidden attributes (the model's `hidden` list and the global ones from `TideConfig::hidden_attributes`), turns attachment fields into URLs, resolves translatable fields to one language, and returns an error when a field cannot be rendered:

```rust
use tideorm::prelude::*;

async fn show(id: i64) -> tideorm::Result<serde_json::Value> {
    let user = User::find_or_fail(id).await?;
    user.try_to_json(None)
}

async fn index() -> tideorm::Result<serde_json::Value> {
    let users = User::query().limit(50).get().await?;
    User::try_collection_to_json(&users, None)
}
```

The other ways to get JSON skip some of that:

| Call | Hidden fields | Attachments and translations | On failure |
|---|---|---|---|
| `model.try_to_json(options)` | left out | URLs, one language | `Err` |
| `model.to_json(options)` | left out | URLs, one language | logs it and returns `{}` |
| `serde_json::to_value(&model)`, a framework's `Json(model)` | **included** | as stored | serde's error |
| `query().get_json()` | **included**: every selected column | as stored | `Err` |

Plain serde and `get_json()` are for internal use (caches, queues, admin exports). Never send them to a client unless the model has nothing to hide.

The `options` map understands one key, `"language"`, which picks the translation to emit (by default the model's fallback language). `to_json_in("fr")` and `try_to_json_in("fr")` are shorthand for it. `Model::collection_to_json(models, options)` and `Model::try_collection_to_json(models, options)` render a `Vec<Model>`, `&Vec<Model>` or `&[Model]` as an array.

`to_hash_map()` flattens `to_json(None)` into a `HashMap<String, String>` for templates and forms: numbers and booleans become their text, `null` becomes `"null"`, and nested objects and arrays become JSON text. A `params` field holding an object or array is left out.

## Inserting Many Rows

`Model::insert_all(models)` inserts a `Vec` of models and returns them as stored, in input order:

```rust
use tideorm::prelude::*;

async fn seed() -> tideorm::Result<Vec<User>> {
    User::insert_all(vec![
        User { name: "John".into(), email: "john@example.com".into(), ..Default::default() },
        User { name: "Jane".into(), email: "jane@example.com".into(), ..Default::default() },
    ])
    .await
}
```

Every model is validated before any is written; a failure names the model's position ("the batch's model at index 1"). No callbacks run, `before_validation` included, so each model is validated as it was built. Managed timestamps are stamped as `create()` stamps them. An empty `Vec` returns an empty `Vec` without touching the database.

How the rows are written depends on the backend:

| Backend | Statement |
|---|---|
| PostgreSQL | multi-row `INSERT .. RETURNING`, split into chunks inside a transaction when rows times columns pass 65,535 parameters |
| SQLite | multi-row `INSERT .. RETURNING`, in chunks of up to 32,766 parameters; the rows are put back in input order by key |
| MariaDB 10.5+ | multi-row `INSERT .. RETURNING`, in chunks of up to 1 MB of values |
| MySQL | models that set their own integer or `Uuid` key: multi-row `INSERT`s of up to 1 MB of values, each read back with one `SELECT`; other models one `INSERT` per row |

A batch that repeats a key, or an auto-increment batch that mixes set and unset keys, is inserted one row at a time.

## Batch Updates

`Model::update_all()` starts a filtered `UPDATE` of many rows:

```rust
use tideorm::prelude::*;

async fn deactivate_stale(cutoff: DateTime<Utc>) -> tideorm::Result<u64> {
    User::update_all()
        .set(User::ACTIVE, false)
        .set(User::UPDATED_AT, Utc::now())
        .where_lt(User::UPDATED_AT, cutoff)
        .execute()
        .await
}
```

`execute()` runs one `UPDATE` and returns the number of rows changed. It runs no callbacks or validation and stamps no timestamps. It returns `Ok(0)` without touching the database when nothing is set, and an error when the builder has no filter. The model's query cache is invalidated on success.

`query().update_all()` updates the rows a query selects, keeping its scopes, filters and the database `query_with` names:

```rust,ignore
let affected = User::query()
    .inactive()          // a scope, see Soft Deletes and Scopes
    .update_all()
    .set("status", "dormant")
    .execute()
    .await?;
```

A query that joins, groups, pages or unions cannot become an `UPDATE`, and running one fails.

| Method | Writes |
|---|---|
| `set(col, value)` | the value |
| `coalesce(col, value)` | the value only where the column is NULL, so a backfill is safe to re-run |
| `increment(col, n)`, `decrement(col, n)` | `col + n`, `col - n`, computed by the database. Nothing clamps the result |
| `multiply(col, x)`, `divide(col, x)` | `col * x`, `col / x`. On an integer column the result is exact and rounded half away from zero. Zero divisors, non-finite factors and factors SQLite cannot apply exactly to integers are refused |
| `array_append(col, value)` | the array with the value added: `array_append` on PostgreSQL, `JSON_ARRAY_APPEND` on MySQL and MariaDB, `json_insert` on SQLite |
| `array_remove(col, value)` | the array without any element equal to the value. A non-array document or NULL stays as it is |
| `json_set(col, path, value)` | the JSON document with one path set; `path` is `$` followed by `.key`, `."any key"`, `['any key']` or `[index]` steps, and wildcards are refused |
| `set_value(col, UpdateValue)` | an assignment chosen at run time, one `UpdateValue` variant per setter above |
| `set_trusted_raw(col, sql)` | a raw SQL expression, unescaped. Only for text you wrote yourself, never user input; the `forbid_raw_sql` [project rule](project-rules.md) covers it |

Values are bound as the column's type, as filter values are: text can name a UUID or a timestamp, a `None` writes NULL, and a JSON column takes the value as a document (`serde_json::json!(..)` for an object; text stays a JSON string). A typed column takes the values a filter on it takes, so `.set(User::ACTIVE, "yes")` does not compile; a column named by a string takes any value. Boolean, UUID and binary columns need a value of their own type. A NaN or infinite float, or a value the column cannot hold, fails the update before any row changes. Setting one column twice keeps the last value. A typed column must belong to the model being updated.

Other builder methods:

- The `where_*`, `or_where_*`, `when` and `when_some` filters of the [query builder](queries.md). All `or_where_*` calls form one OR group, ANDed with the rest.
- `with_trashed()` reaches soft-deleted rows, which are left out by default; `without_trashed()` goes back to the default.
- `limit(n)` caps the rows touched. MySQL and MariaDB put `LIMIT` on the `UPDATE`; PostgreSQL and SQLite update the keys of a bounded subquery, which needs a single-column primary key, so a composite-key model fails there.
- `execute_returning()` returns the updated rows as models, on PostgreSQL and SQLite. MySQL and MariaDB refuse it before anything runs. The update and the decoding of its rows are not one unit: if a row fails to decode, the rows are written anyway, so run it in a [transaction](transactions.md) when that matters.

## Upserts

`Model::on_conflict(columns)` inserts a model or, when it collides with the unique key those columns form, updates the stored row, and returns the row as stored:

```rust,ignore
let user = User::on_conflict(["email"])
    .update_all_except(["role"])
    .insert(user)
    .await?;

let tag = Tag::on_conflict([Tag::columns.slug]).do_nothing().insert(tag).await?;
```

`Model::insert_or_update(model, columns)` is `on_conflict(columns).insert(model)` with the default update set. Columns are names or typed columns: `["email"]`, `[User::columns.email]`, a `Vec`, or a tuple of typed columns of different types, `(User::TENANT_ID, User::EMAIL)`.

| Builder method | On conflict |
|---|---|
| none (the default) | every column except the conflict columns and a managed `created_at` is overwritten |
| `update_columns([..])` | only these columns are written. Name `updated_at` to refresh it, and `created_at` only to overwrite it. Wins over `update_all_except` |
| `update_all_except([..])` | every column except these and a managed `created_at` |
| `do_nothing()` | the stored row is kept and returned, so the call reads "insert unless it exists" |

The conflict columns must be covered by a unique constraint or unique index: the database, not TideORM, decides what counts as a conflict. `insert(model)` validates the model first and runs no callbacks. An encrypted column cannot be a conflict column.

| Backend | How it runs |
|---|---|
| PostgreSQL, SQLite | `INSERT .. ON CONFLICT (columns) DO UPDATE .. RETURNING`. A conflict on another unique key fails the statement, and whether NULLs conflict is the constraint's decision (`NULLS NOT DISTINCT`). `do_nothing()` sets the conflict columns to themselves so the kept row is returned |
| MySQL, MariaDB | `ON DUPLICATE KEY UPDATE` fires on any unique key, so TideORM instead locks the row the conflict columns name (`SELECT .. FOR UPDATE`) and updates or inserts it, in one transaction. Primary-key columns in `update_columns` are refused, and so is a conflict target covered only by a prefix index |

An upsert that writes an auto-increment key the model holds moves PostgreSQL's sequence past it, as `create()` does. A [soft-deleted](soft-deletes.md) row is reached by an upsert, which writes the model's `deleted_at` back.
