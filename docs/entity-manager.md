# Entity Manager

The optional `entity-manager` feature adds an explicit persistence context for aggregate workflows.

An `EntityManager` owns a database handle, caches loaded models by primary key, tracks loaded aggregate-side relations for aggregate saves, and also supports managed lifecycles through `persist`, `merge`, `remove`, `detach`, and `flush`.

Create one per request or unit of work. Its identity map keeps every model it has loaded until the manager is dropped or `clear()`ed, and `find` answers from that map without asking the database again, so a manager shared for the life of a process grows without bound and keeps serving rows other connections have since changed.

## Enabling the Feature

```toml
[dependencies]
tideorm = { version = "0.13.1", features = ["postgres", "entity-manager"] }
```

Use the backend feature you need (`postgres`, `mysql`, or `sqlite`) alongside `entity-manager`.

## Aggregate Workflow

```rust
use std::sync::Arc;

use tideorm::prelude::*;

#[tideorm::model(table = "users")]
struct User {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,

    #[tideorm(has_many = "Post", foreign_key = "user_id")]
    posts: HasMany<Post>,
}

#[tideorm::model(table = "posts")]
struct Post {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    title: String,
}

async fn aggregate_example(db: Arc<Database>) -> tideorm::Result<()> {
    let entity_manager = EntityManager::new(db);

    let mut user = entity_manager
        .find::<User>(1)
        .await?
        .expect("user should exist");

    entity_manager.load(&mut user.posts).await?;

    user.posts
        .as_mut()
        .expect("posts should be loaded")
        .push(Post {
            id: 0,
            user_id: 0,
            title: "Draft".to_string(),
        });

    let user = entity_manager.save(&user).await?;

    assert!(user.posts.get_cached().is_some());
    Ok(())
}
```

Use this facade when you want to load a root aggregate, explicitly load its relations, mutate the graph, then persist the loaded graph back through the same context.

## Managed Lifecycle Workflow

```rust
async fn managed_example(db: Arc<Database>) -> tideorm::Result<()> {
    let entity_manager = EntityManager::new(db);

    let user = entity_manager
        .find_managed::<User>(1)
        .await?
        .expect("user should exist");

    user.edit(|user| user.name = "Updated".to_string());

    entity_manager.flush().await?;

    let inserted = entity_manager.persist(User {
        id: 0,
        name: "New User".to_string(),
        posts: Default::default(),
    });
    entity_manager.flush().await?;
    assert!(inserted.get().id > 0);

    entity_manager.remove(&user);
    entity_manager.flush().await?;

    Ok(())
}
```

Managed entities expose:

- `managed.get()` to clone the current managed value.
- `managed.edit(...)` to mutate the managed value in place.
- `managed.replace(...)` to replace the current managed value wholesale.
- `managed.state()` to inspect whether the entity is `New`, `Managed`, `Removed`, or `Detached`.

`merge(...)` attaches a detached instance into the current context. `detach(...)` keeps the in-memory value but removes it from future flushes. `clear()` detaches the whole context. Persisting or merging an entity the context was about to remove takes the removal back and writes it; merging one a `persist()` has not flushed keeps it an insert.

When the context saves a row it also holds as a managed entity, through `entity_manager.save(..)` or a relation sync, the managed entity moves onto what was stored: the fields it has not changed take the stored values, the ones it changed keep its edits, so a later flush does not write the values it loaded back over the newer save. A relation copy of the row, loaded before, is not moved; reload it.

A managed entity keeps the primary key it was loaded or saved with: a flush refuses one whose key was changed, rather than write it over the row holding the new key. Detach it and persist a new entity instead.

A manager runs one `flush()` or `save()` at a time: a second one waits for the first, so an entity waiting to be inserted is inserted once. A flush or save called inside another, such as from a callback, joins it instead. An edit made to a managed entity while a flush writes it stays over the row as stored, and the entity stays dirty for the next flush.

A flush that fails, is cancelled part way, or runs inside a transaction that later rolls back leaves the context as it was before the flush, since none of what it wrote was committed. The one outcome no client can know is a flush cancelled while its `COMMIT` is on the wire: the database may have committed it. The context is restored then too, so reload what the flush wrote before flushing it again.

## Relations of a Model the Manager Loaded

When a model was loaded through `EntityManager::find(...)` or `find_managed(...)`, every relation it holds, the polymorphic and self-referencing ones included, queries through the entity manager's database handle: `load()`, `load_with(...)`, `count()` and `exists()` work even if no global database is configured.

Use `entity_manager.load(&mut relation)` when the relation should become tracked for aggregate synchronization on `save()` or `flush()`.

## What Aggregate Saves Synchronize

- Root saves use the entity manager's database handle.
- Loaded `HasOne<T>`, `HasMany<T>`, and `HasManyThrough<T, P>` relations are synchronized with the saved aggregate.
- New related models are inserted, changed related models are updated, removed `HasOne<T>` or `HasMany<T>` children are deleted by the child model's own primary key, and `HasManyThrough<T, P>` pivot rows are attached or detached as needed.
- Both `foreign_key` and `local_key` are honored, so non-`id` relation keys work.
- Nested loaded child graphs continue through the same context.
- Unloaded relations remain untouched.

## Primary Key Support

Entity-manager identity tracking works with the same primary-key shapes as the generated model APIs:

- Auto-increment numeric keys, such as `entity_manager.find::<User>(1)`.
- Natural keys, such as `entity_manager.find::<ApiKey>("api-key-1".to_string())`.
- Composite keys, such as `entity_manager.find::<Membership>((team_id, member_id))`.

Tracked `HasOne<T>` and `HasMany<T>` synchronization uses the related model's actual primary key for updates and deletes, so natural-key and composite-key children work the same way as numeric-key children.

## Notes

- `EntityManager` is explicit. It does not replace TideORM's global database APIs.
- `EntityManager::save()` is designed for aggregate workflows around loaded models plus loaded `HasOne<T>`, `HasMany<T>`, and `HasManyThrough<T, P>` relations.
- `BelongsTo<T>` participates in entity-manager-aware loads and identity reuse, but aggregate saves do not cascade `BelongsTo<T>` updates.
- Reuse the same `EntityManager` for aggregate loads, managed edits, and flushes when you want one consistent persistence context.