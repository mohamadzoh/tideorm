# Entity Manager

The optional `entity-manager` feature adds an explicit persistence context for aggregate workflows.

An `EntityManager` owns a database handle, caches loaded models by primary key, tracks loaded aggregate-side relations for aggregate saves, and also supports managed lifecycles through `persist`, `merge`, `remove`, `detach`, and `flush`.

## Enabling the Feature

```toml
[dependencies]
tideorm = { version = "0.9.9", features = ["postgres", "entity-manager"] }
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

`merge(...)` attaches a detached instance into the current context. `detach(...)` keeps the in-memory value but removes it from future flushes. `clear()` detaches the whole context.

## Compatibility Helpers

The `EntityManager` facade is the recommended API, but the generated compatibility entry points remain available:

- `Model::find_in_entity_manager(pk, &entity_manager)`
- `relation.load_in_entity_manager(&entity_manager)`
- `save_with_entity_manager(&model, &entity_manager)`

The explicit tracked-collection helper also remains available when needed:

```rust
tideorm::entity_manager::TrackedHasManyEntityManagerExt::load(&mut user.posts, &entity_manager)
    .await?;
```

When a model itself was loaded through `EntityManager::find(...)` or `find_in_entity_manager(...)`, plain relation read helpers such as `load()`, `load_with(...)`, `count()`, and `exists()` continue to query through that same entity-manager database handle even if no global database is configured.

Use `entity_manager.load(&mut relation)` or `relation.load_in_entity_manager(&entity_manager)` when the relation should become tracked for aggregate synchronization on `save()` or `flush()`.

## What Aggregate Saves Synchronize

- Root saves use the entity manager's database handle.
- Loaded `HasOne<T>`, `HasMany<T>`, and `HasManyThrough<T, P>` relations are synchronized with the saved aggregate.
- New related models are inserted, changed related models are updated, removed `HasOne<T>` or `HasMany<T>` children are deleted by the child model's own primary key, and `HasManyThrough<T, P>` pivot rows are attached or detached as needed.
- Both `foreign_key` and `local_key` are honored, so non-`id` relation keys work.
- Nested loaded child graphs continue through the same context.
- Unloaded relations remain untouched.

## Primary Key Support

Entity-manager identity tracking works with the same primary-key shapes as the generated model APIs:

- Auto-increment numeric keys, such as `User::find_in_entity_manager(1, &entity_manager)`.
- Natural keys, such as `ApiKey::find_in_entity_manager("api-key-1".to_string(), &entity_manager)`.
- Composite keys, such as `Membership::find_in_entity_manager((team_id, member_id), &entity_manager)`.

Tracked `HasOne<T>` and `HasMany<T>` synchronization uses the related model's actual primary key for updates and deletes, so natural-key and composite-key children work the same way as numeric-key children.

## Notes

- `EntityManager` is explicit. It does not replace TideORM's global database APIs.
- `EntityManager::save()` and `save_with_entity_manager()` are designed for aggregate workflows around loaded models plus loaded `HasOne<T>`, `HasMany<T>`, and `HasManyThrough<T, P>` relations.
- `BelongsTo<T>` participates in entity-manager-aware loads and identity reuse, but aggregate saves do not cascade `BelongsTo<T>` updates.
- Reuse the same `EntityManager` for aggregate loads, managed edits, and flushes when you want one consistent persistence context.