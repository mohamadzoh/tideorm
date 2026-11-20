# Entity Manager

Requires the `entity-manager` feature.

An `EntityManager` is an explicit persistence context: one database handle, an identity map that hands out one instance per loaded row, and a set of managed entities whose changes it writes back together. Use it when you load an aggregate (a user and its posts), change the graph, and want to write it back as a unit, or when you want to stage inserts, updates and deletes and write them all in one transaction.

```toml
[dependencies]
tideorm = { version = "0.18.2", default-features = false, features = ["postgres", "runtime-tokio", "entity-manager"] }
```

Use the backend feature you need (`postgres`, `mysql` or `sqlite`) alongside `entity-manager`.

Create one manager per request or unit of work. Its identity map keeps every model it has loaded until the manager is dropped or `clear()`ed, and `find` answers from that map without asking the database again, so a manager shared for the life of a process grows without bound and keeps serving rows other connections have since changed. Managers are independent of each other: each has its own identity map.

The manager is explicit: it does not replace TideORM's global database APIs. Everything it does goes through its own database handle, so it works without any global connection.

## Aggregate Workflow

Load a root with `find`, load its relations with `load`, change the graph, and write it back with `save`:

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,

    #[tideorm(has_many = "Post", foreign_key = "user_id")]
    pub posts: HasMany<Post>,
}

#[tideorm::model(table = "posts")]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
}

async fn add_draft(db: Database) -> tideorm::Result<()> {
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

`EntityManager::new` takes a `Database` or an `Arc<Database>` and returns an `Arc<EntityManager>`. `database()` returns the handle it reads and writes through.

`find::<T>(pk)` serves the row from the identity map when the manager already holds it, and otherwise reads it through the manager's database (`Model::find_with`). `load(&mut relation)` loads a `HasOne`, `HasMany`, `BelongsTo` or `HasManyThrough` relation inside the context: its rows come from the identity map when it holds them, so two relations reaching one row share its instance, and the manager records what the relation held, which a later `save()` compares against.

`save(&entity)` writes the root and its loaded relations in one transaction and returns the saved aggregate.

### What a Save Writes

- The root is written through the manager's database handle.
- Loaded `HasOne<T>`, `HasMany<T>` and `HasManyThrough<T, P>` relations are synchronized with the saved aggregate: new related models are inserted, changed ones updated, `HasOne`/`HasMany` children removed from the relation are deleted by the child model's own primary key, and `HasManyThrough` pivot rows are attached or detached as needed.
- Both `foreign_key` and `local_key` are honoured, so relations keyed by a column other than `id` work.
- Nested loaded child graphs continue through the same context.
- Relations that were not loaded are left untouched.
- `BelongsTo<T>` takes part in manager-aware loads and identity reuse, but a save does not cascade updates through it.

## Managed Lifecycle Workflow

`find_managed`, `persist` and `merge` hand out `Managed<T>` handles. `flush()` writes every pending insert, update and delete in one transaction:

```rust
async fn managed_example(db: Database) -> tideorm::Result<()> {
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

| Method | What it does |
|---|---|
| `find_managed::<T>(pk)` | Loads the row as a managed, clean entity; one the context already manages comes back as the same handle |
| `persist(entity)` | Schedules an insert. An entity with a client-assigned key that the context already manages is replaced in place, and its handle returned |
| `merge(entity)` | Attaches a detached instance: a managed one takes on its values, otherwise a new entry is created, clean against what the identity map holds. An entity with no primary key yet is persisted instead |
| `remove(&managed)` | Schedules a delete; an entity never inserted is simply dropped |
| `detach(&managed)` | Stops managing it: later flushes ignore it, while the handle keeps its value |
| `clear()` | Detaches every managed entity and empties the identity map |
| `flush()` | Writes inserts, then updates, then deletes, in one transaction |

Persisting or merging an entity the context was about to remove takes the removal back and writes it; merging one a `persist()` has not flushed keeps it an insert.

A `Managed<T>` handle exposes:

- `get()`: a copy of the entity as it is now.
- `read(|entity| ..)`: runs a closure on the entity without copying it.
- `edit(|entity| ..)`: changes the entity in place; the next flush writes what changed.
- `replace(entity)`: puts a new value in place of the tracked one.
- `state()`: `EntityState::New`, `Managed`, `Removed` or `Detached`.

Clones of a handle share one entity. The entity is locked while `read` or `edit` runs, so calling `edit` from inside `read`, or reading from inside `edit`, through this or another handle, waits forever.

`register(entity)` files a copy of a model you loaded elsewhere in the identity map, without the relations it loaded, and returns the instance for its row: the model itself, unless the context already tracks that row, in which case the tracked instance wins. A model with no primary key yet is handed straight back.

### Flush Order

A flush orders writes by table so a parent row exists before the child that references it: referenced rows insert first and delete last. Within one table, rows that reference each other through a declared self-reference are ordered the same way.

### Saves and Managed Entities

When the context saves a row it also holds as a managed entity, through `save()` or a relation sync, the managed entity moves onto what was stored: the fields it has not changed take the stored values, the ones it changed keep its edits, so a later flush does not write the values it loaded back over the newer save. A relation copy of the row, loaded before, is not moved; reload it.

A managed entity keeps the primary key it was loaded or saved with: a flush refuses one whose key was changed, rather than write it over the row holding the new key. Detach it and persist a new entity instead.

### Concurrency and Failure

A manager runs one `flush()` or `save()` at a time: a second one waits for the first, so an entity waiting to be inserted is inserted once. A flush or save called inside another, such as from a callback, joins it instead. Another manager runs its own.

An edit made to a managed entity's columns while a flush writes it stays over the row as stored, and the entity stays dirty for the next flush. An edit to its relations cannot be merged with the rows the flush wrote for them: the flush fails and rolls back instead, which keeps the edit, and the next flush writes it.

A flush that fails, is cancelled part way, or runs inside a transaction that later rolls back leaves the context as it was before the flush, since none of what it wrote was committed. The one outcome no client can know is a flush cancelled while its `COMMIT` is on the wire: the database may have committed it. The context is restored then too, so reload what the flush wrote before flushing it again.

## Relations of a Model the Manager Loaded

When a model was loaded through `find` or `find_managed`, every relation it holds, the polymorphic and self-referencing ones included, queries through the manager's database: `load()`, `load_with(..)`, `count()` and `exists()` work even with no global database configured.

Use `entity_manager.load(&mut relation)` when the relation should also be tracked, so that `save()` synchronizes it. See [Relations](relations.md) for the relation types themselves.

## Primary Keys

Identity tracking works with the same primary-key shapes as the generated model APIs:

- Auto-increment numeric keys: `entity_manager.find::<User>(1)`.
- Natural keys: `entity_manager.find::<ApiKey>("api-key-1".to_string())`.
- Composite keys: `entity_manager.find::<Membership>((team_id, member_id))`.

`HasOne` and `HasMany` synchronization updates and deletes children by the related model's own primary key, so natural-key and composite-key children work as numeric-key children do.

## Generic Code

Generic code names one bound for the models a manager handles, `ManagedModel`, which every model derives with this feature on:

```rust
use std::sync::Arc;

fn track<M: ManagedModel>(entity_manager: &Arc<EntityManager>, model: M) -> Managed<M> {
    entity_manager.persist(model)
}
```

Reuse one manager for aggregate loads, managed edits and flushes when you want one consistent persistence context.
