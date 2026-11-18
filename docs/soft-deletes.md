# Soft Deletes and Scopes

A soft-delete model keeps deleted rows in the table and marks them with a timestamp, so they can be restored or purged later. Scopes are named, reusable query conditions, such as "active users", that you chain like builder methods.

## Soft Deletes

Add `soft_delete` to the model and a `deleted_at: Option<DateTime<Utc>>` field:

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "posts", soft_delete)]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub deleted_at: Option<DateTime<Utc>>,
}

async fn trash(id: i64) -> tideorm::Result<()> {
    if let Some(post) = Post::find(id).await? {
        post.delete().await?; // sets deleted_at; the row stays
    }
    assert!(Post::find(id).await?.is_none()); // reads leave it out
    Ok(())
}
```

When the field or column has another name, declare it with `deleted_at_column`:

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "posts", soft_delete, deleted_at_column = "archived_on")]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub archived_on: Option<DateTime<Utc>>,
}
```

The field must be an `Option<DateTime<Utc>>`; any other type is a compile error, and so is `deleted_at_column` without `soft_delete`. Unless the model sets `hidden`, the soft-delete field is left out of `to_json()`. `ModelMeta::soft_delete_enabled()` and `deleted_at_column()` report the setup.

### Reading Trashed Rows

```rust,ignore
// Live rows only (the default)
let posts = Post::query().get().await?;

// Live and trashed rows
let all_posts = Post::query().with_trashed().get().await?;

// Trashed rows only
let trashed = Post::query().only_trashed().get().await?;
```

The scope applies to every read and filtered write: `find(id)`, `exists(id)`, `all()`, `count()`, every query terminal, `update_all()`, relation loads and full-text search leave trashed rows out. To reach them on purpose:

- `Post::query().with_trashed().find(id)` reads a trashed row by key.
- `post.reload()` refreshes the record in hand even when it is trashed.
- `Post::update_all().with_trashed()` updates the trash too.

`only_trashed()` on a model without soft delete makes the query fail when it runs.

Writes of a model by its key are not scoped: `update()`, `save()` and upserts reach a trashed row and write the model's `deleted_at` back, so saving a stale copy of a trashed row restores it. That is how `restore()` works.

### Deleting, Restoring and Purging

On a soft-delete model, the ordinary deletes mark rows instead of removing them: `delete()`, `destroy(id)`, `query().delete()` and `delete_all()` set `deleted_at` to the current time. The stamp is `Utc::now()` from your application, bound as a value, not the database clock (MySQL and MariaDB read theirs in the session time zone). A row that is already trashed keeps its stamp.

The `SoftDelete` trait, in the prelude, adds methods to the model:

```rust
use tideorm::prelude::*;

async fn lifecycle(post: Post) -> tideorm::Result<()> {
    let post = post.soft_delete().await?; // mark it, return it as stored
    assert!(post.is_deleted());

    let post = post.restore().await?;     // clear deleted_at and update()
    post.force_delete().await?;           // remove the row for good
    Ok(())
}
```

| Method | Does |
|---|---|
| `soft_delete()` | `delete()`, then returns the row as stored. The delete callbacks run, so a `before_delete` that refuses stops it |
| `restore()` | clears `deleted_at` and calls `update()`, with its validation and callbacks |
| `force_delete()` | removes the row, running the delete callbacks; returns the rows deleted |
| `deleted_at()`, `set_deleted_at(..)`, `is_deleted()` | read or set the stamp in memory |

The same operations exist on a query, for many rows at once. They run no callbacks:

| Query method | Rows | Filter |
|---|---|---|
| `delete()`, `soft_delete()` | marks the live rows it matches | required. Refused after `only_trashed()`, since those rows are deleted already |
| `delete_all()` | marks every live row | refuses filters |
| `restore()` | restores the trashed rows it matches | required, unless the query is `only_trashed()`, which restores the whole trash |
| `force_delete()` | removes the rows it matches, trashed ones included | required, unless the query is `only_trashed()`, which empties the trash |

```rust,ignore
// Restore one post
Post::query().only_trashed().where_eq(Post::ID, id).restore().await?;

// Purge everything trashed more than 30 days ago
let cutoff = Utc::now() - tideorm::chrono::Duration::days(30);
Post::query().only_trashed().where_lt(Post::DELETED_AT, cutoff).force_delete().await?;
```

A filter that is always true (an empty `NOT IN`, a raw `1 = 1`) counts as no filter. On a query, `soft_delete()` and `restore()` are errors for a model without soft delete; `force_delete()` works on any model.

## Scopes

Declare a model's scopes in one `#[tideorm::scopes]` impl block. Each scope takes the query first, and may take more arguments:

```rust
use tideorm::prelude::*;

#[tideorm::scopes]
impl User {
    pub fn active(query: QueryBuilder<Self>) -> QueryBuilder<Self> {
        query.where_eq(User::columns.active, true)
    }

    pub fn verified(query: QueryBuilder<Self>) -> QueryBuilder<Self> {
        query.where_not_null(User::columns.verified_at)
    }

    pub fn role(query: QueryBuilder<Self>, role: &str) -> QueryBuilder<Self> {
        query.where_eq(User::columns.role, role)
    }
}

async fn admins() -> tideorm::Result<Vec<User>> {
    User::query().active().verified().role("admin").get().await
}
```

The block generates a `UserQueryScopes` trait implemented for `QueryBuilder<User>`, which is what makes `.active()` callable on a query; the functions also stay callable as `User::active(query)`. A model takes one `#[tideorm::scopes]` block, so put every scope in it. To call the scopes from another module, import the trait:

```rust,ignore
use crate::models::UserQueryScopes as _;
```

Each scope must be a plain synchronous function with no `self` and no generic parameters, take `QueryBuilder<Self>` first and return it. Later parameters must be simple names; they can be `mut` and their types can name the model as `Self`. A scope cannot take the name of a `QueryBuilder` method (`latest`, `limit`, `distinct`, `where_eq`, `with_trashed`, ..): the builder's own method would answer the call, so such a name is a compile error. Doc comments and `#[cfg]` attributes carry over to the generated trait.

A scope adds to the query it is given, so scopes chain in any order and keep the filters, ordering, limit and values set before them. The `or_where_*` shorthand is the exception to watch: every `or_where_*` call on a query joins its one OR group, so two scopes that each use it OR their conditions together. A scope that offers alternatives should keep them in a group of its own, with `where_any` or `begin_or()` (see [OR Conditions](queries.md#or-conditions)).

### Scopes as Plain Functions

Ordinary functions over a `QueryBuilder` compose too, and can be generic over the model:

```rust
use tideorm::prelude::*;

fn active<M: Model>(q: QueryBuilder<M>) -> QueryBuilder<M> {
    q.where_eq("active", true)
}

fn recent<M: Model>(q: QueryBuilder<M>) -> QueryBuilder<M> {
    q.order_desc("created_at").limit(10)
}

async fn recent_active_users() -> tideorm::Result<Vec<User>> {
    recent(active(User::query())).get().await
}
```

### Conditional Scopes

`when` applies a closure when a condition holds, and `when_some` when an `Option` holds a value:

```rust
use tideorm::prelude::*;

async fn search(include_trashed: bool, status: Option<&str>) -> tideorm::Result<Vec<Post>> {
    Post::query()
        .when(include_trashed, |q| q.with_trashed())
        .when_some(status, |q, status| q.where_eq("status", status))
        .get()
        .await
}
```

Batch updates have `when` and `when_some` too.

### Saved Query Fragments

`consolidate()` captures a query's clauses as a `QueryFragment`, and `apply(&fragment)` adds them to another query. Use it to build a condition set once and reuse it:

```rust
use tideorm::prelude::*;

async fn active_admins() -> tideorm::Result<Vec<User>> {
    let active = User::query().where_eq("status", "active").consolidate();
    assert_eq!(active.condition_count(), 1);

    User::query().apply(&active).where_eq("role", "admin").get().await
}
```

A fragment holds the builder's clauses, not rendered SQL, so it keeps its bound values and works on any backend. `apply` replays it the way the matching builder calls would:

- WHERE conditions, OR groups, ORDER BY terms, selected expressions, GROUP BY columns, HAVING clauses, joins, unions, window functions and CTEs are appended. The fragment's `or_where_*` conditions join the query's own OR group.
- `limit`, `offset`, `select` and the cache options are last-wins: a value the fragment sets replaces the query's, and one it left unset keeps the query's.
- `distinct()` set in the fragment turns it on; otherwise the query's choice stays.
- `with_trashed()` or `only_trashed()` in the fragment replaces the query's soft-delete scope; a fragment with neither leaves it alone.
- If both carry an error from building, the query's own earlier error is the one reported.

Applying a fragment to an empty query reproduces the query it came from.
