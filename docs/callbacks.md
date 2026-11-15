# Callbacks and Timestamps

Callbacks run your code at fixed points of a model's writes: to normalize input, to refuse a write, or to queue follow-up work. Timestamps are the one piece of that TideORM does for you: `created_at` and `updated_at` fields are stamped on every write.

## Callbacks

Implement the `Callbacks` trait on the model and override the hooks you need; the others do nothing.

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub password_hash: String,
}

impl Callbacks for User {
    fn before_save(&mut self) -> tideorm::Result<()> {
        self.email = self.email.trim().to_lowercase();
        Ok(())
    }

    fn after_create(&self) -> tideorm::Result<()> {
        // Hooks cannot query: queue the welcome email here, and create
        // related records once `create()` has returned.
        println!("user {} created with id {}", self.email, self.id);
        Ok(())
    }

    fn before_delete(&self) -> tideorm::Result<()> {
        if self.email == "admin@example.com" {
            return Err(tideorm::Error::validation("email", "Cannot delete admin account"));
        }
        Ok(())
    }
}
```

Hooks must be in the `impl Callbacks for User` block. A method named `before_save` in an inherent `impl User` block is never called, and nothing warns about it.

### Available Hooks

| Hook | Receives | Runs |
|---|---|---|
| `before_validation` | `&mut self` | before the validation rules |
| `after_validation` | `&self` | after the rules pass |
| `before_save` | `&mut self` | before an insert or update |
| `before_create` | `&mut self` | before an insert |
| `after_create` | `&self` | after an insert |
| `before_update` | `&mut self` | before an update |
| `after_update` | `&self` | after an update |
| `after_save` | `&self` | after an insert or update |
| `before_delete` | `&self` | before a delete |
| `after_delete` | `&self` | after a delete; not when no row matched |

Every hook returns `tideorm::Result<()>`.

### Order

| Write | Hooks, in order |
|---|---|
| `create()`, or `save()` of a new model | `before_validation`, validation, `after_validation`, `before_save`, `before_create`, INSERT, `after_create`, `after_save` |
| `update()`, or `save()` of a stored model | `before_validation`, validation, `after_validation`, `before_save`, `before_update`, UPDATE, `after_update`, `after_save` |
| `delete()`, `destroy(id)`, `force_delete()` | `before_delete`, DELETE, `after_delete` |

`before_save` runs after validation, so what it changes is not validated: normalize in `before_validation` when the result must pass the rules. A managed `created_at` or `updated_at` is stamped after `before_save`, so a value the hook writes there is replaced. `before_create` is the place to set a key of your own, such as a UUID of a version other than v4.

On a [soft-delete model](soft-deletes.md), `delete()` marks the row and still runs the delete hooks.

### Errors

An `Err` from a `before_*` hook stops the write before anything reaches the database, and is returned to the caller.

An `after_*` hook runs once the statement has succeeded. Its `Err` is returned from `save()`, `update()` or `delete()`, but the row is already written. Inside a [transaction](transactions.md), propagating that error with `?` rolls the write back with the rest of the transaction; outside one, the write stays.

### Which Writes Run Hooks

Hooks run for single-model writes only: `create()`, `save()`, `update()`, `delete()`, `destroy(id)` and the `SoftDelete` methods. Nested saves run each model's own `save()`, hooks included.

These skip them: `insert_all()`, `update_all()`, upserts (`on_conflict`, `insert_or_update`) and the query writes (`delete()`, `delete_all()`, `soft_delete()`, `force_delete()`, `restore()` on a query).

Generic code still runs a model's hooks as long as it calls the model's own methods (`M::create(m)`, `m.save()`, `m.update()`, `m.delete()`): the hook dispatch is generated into each model's methods.

### Limits

Hooks are synchronous and cannot query the database. Work that needs the database, such as creating related records, belongs after the call returns:

```rust,ignore
let user = User::create(user).await?;
Profile::create(Profile { user_id: user.id, ..Default::default() }).await?;
```

Run both in [`db.transaction(..)` or `db.atomically(..)`](transactions.md) to make them succeed or fail together.

## Timestamps

A model with `created_at` and `updated_at` fields gets them set on every write:

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "posts")]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub content: String,
    pub created_at: DateTime<Utc>, // set on insert
    pub updated_at: DateTime<Utc>, // set on insert and update
}

async fn publish() -> tideorm::Result<Post> {
    let post = Post {
        title: "Hello".into(),
        content: "World".into(),
        ..Default::default()
    };
    let mut post = post.save().await?; // created_at and updated_at are now set

    post.title = "Updated Title".into();
    post.update().await // updated_at is refreshed, created_at stays
}
```

A field is managed when it is named `created_at` or `updated_at`, or mapped to such a column with `#[tideorm(column = "..")]`, and holds a `DateTime<Utc>` or a `NaiveDateTime`, optional or not. A `NaiveDateTime` gets the current UTC time without its zone, which is what `timestamps_naive()` migration columns hold. The value you set on a managed field is replaced; a field of any other type, such as a `String` named `created_at`, keeps the value you set. Both fields of one write get the same instant, and a model with only `created_at` still has it stamped.

What each write does:

| Write | `created_at` | `updated_at` |
|---|---|---|
| `create()`, `save()` of a new model, `insert_all()` | set | set |
| `update()`, `save()` of a stored model | never written | set |
| upsert that inserts | set | set |
| upsert that finds the row | kept, unless named in `update_columns` | set when it is in the update set: by default it is, `update_columns([..])` must name it, and `do_nothing()` writes nothing |
| `update_all()`, query writes | not touched | not touched: set it yourself |

A request body can leave both fields out, so a model deserialized from `{"title": ".."}` saves with fresh timestamps, and a body that sends `created_at` cannot rewrite it.

The `timestamps` model option marks a model as timestamped (`ModelMeta::has_timestamps()`); a model with both fields counts as timestamped without it. The fields are managed either way.

To set `updated_at` in a batch update:

```rust,ignore
Post::update_all()
    .set(Post::TITLE, title)
    .set(Post::UPDATED_AT, Utc::now())
    .where_eq(Post::ID, id)
    .execute()
    .await?;
```
