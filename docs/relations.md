# Relations

Relations connect a model to the rows of other models. You declare each one as a struct field, then load it on demand from one model or eagerly for a whole query, which avoids one query per row (the N+1 problem).

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub email: String,

    #[tideorm(has_one = "Profile", foreign_key = "user_id")]
    pub profile: HasOne<Profile>,

    #[tideorm(has_many = "Post", foreign_key = "user_id")]
    pub posts: HasMany<Post>,
}

#[tideorm::model(table = "profiles")]
pub struct Profile {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub bio: String,

    #[tideorm(belongs_to = "User", foreign_key = "user_id")]
    pub user: BelongsTo<User>,
}

#[tideorm::model(table = "posts")]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub content: String,

    #[tideorm(belongs_to = "User", foreign_key = "user_id")]
    pub author: BelongsTo<User>,

    #[tideorm(has_many = "Comment", foreign_key = "post_id")]
    pub comments: HasMany<Comment>,
}

async fn show(id: i64) -> tideorm::Result<()> {
    let user = User::find_or_fail(id).await?;
    let posts: Vec<Post> = user.posts.load().await?;
    println!("{} wrote {} posts", user.name, posts.len());
    Ok(())
}
```

Relation fields are not columns. The derive wires each wrapper from the model's own key fields when a row loads, when a model deserializes, and when you build one with `Default`.

## Relation Types

The field's wrapper type decides the kind of relation:

| Wrapper | Attribute | Relation | Required keys |
|---|---|---|---|
| `HasOne<T>` | `has_one` | One-to-one (a user has one profile) | `foreign_key` |
| `HasMany<T>` | `has_many` | One-to-many (a user has many posts) | `foreign_key` |
| `BelongsTo<T>` | `belongs_to` | The inverse side (a post belongs to a user) | `foreign_key` |
| `HasManyThrough<T, P>` | `has_many_through` | Many-to-many through the pivot model `P` | `foreign_key`, `related_key` |
| `MorphOne<T>` | none | Polymorphic one-to-one | `morph_name` |
| `MorphMany<T>` | none | Polymorphic one-to-many | `morph_name` |
| `MorphTo<T>` | none | The inverse of a polymorphic relation | `morph_name` |
| `SelfRef<T>` | none | The parent row in the same table | none |
| `SelfRefMany<T>` | none | The child rows in the same table | none |

The `has_one`/`has_many`/`belongs_to`/`has_many_through` attribute may be left out; when present it must name the same kind as the wrapper. A mismatch or a missing required key is a compile-time error, and the message suggests the conventional key (`foreign_key = "user_id"`). Keys are never assumed, since a model with two relations to one model would silently join both on the same key.

## Relation Attributes

| Attribute | Meaning | Default |
|---|---|---|
| `foreign_key` | The column holding the key: on the related table for `HasOne`/`HasMany`/`HasManyThrough`'s pivot, on this model for `BelongsTo` and `SelfRef` | Required, except `SelfRef`/`SelfRefMany`: `parent_id` |
| `local_key` | The column on this model the foreign key matches | The primary key, whatever its column is called |
| `owner_key` | The column on the owner a `BelongsTo` foreign key matches | `id` |
| `related_key` | The pivot column holding the related model's key (`HasManyThrough`) | Required |
| `pivot` | The pivot table (`HasManyThrough`) | The pivot model's table |
| `morph_name` | The prefix of the `<name>_type` and `<name>_id` columns (polymorphic relations) | Required |

The derive emits a constant per relation, named after the field in upper case (`User::POSTS == "posts"`). Use it wherever a relation name is a string, such as `with(User::POSTS)`.

## Loading a Relation

Each wrapper queries the related rows when you ask:

```rust
let user = User::find_or_fail(1).await?;

let profile: Option<Profile> = user.profile.load().await?;
let posts: Vec<Post> = user.posts.load().await?;

let post = Post::find_or_fail(1).await?;
let author: Option<User> = post.author.load().await?;

let has_profile: bool = user.profile.exists().await?;
let has_posts: bool = user.posts.exists().await?;
let post_count: u64 = user.posts.count().await?;
```

`load()` returns rows in no particular order. To order, filter or page them, use `load_with`, whose closure gets the relation's query already filtered and adds to it:

```rust
let recent_posts = user.posts.load_with(|query| {
    query
        .where_eq("published", true)
        .where_gt("views", 100)
        .order_desc("created_at")
        .limit(10)
}).await?;

let profile = user.profile.load_with(|query| query.where_not_null("avatar")).await?;
```

| Wrapper | `load` | `load_with` | `exists` | `count` |
|---|---|---|---|---|
| `HasOne`, `BelongsTo`, `MorphOne`, `SelfRef` | `Option<T>` | `Option<T>` | yes | no |
| `HasMany`, `HasManyThrough`, `MorphMany`, `SelfRefMany` | `Vec<T>` | `Vec<T>` | yes | `u64` |

A few rules hold for every wrapper:

- An owner whose key is NULL (an unset nullable `local_key`) has no related rows: `load()` returns none, `count()` zero, and `attach`, `detach` and `sync` refuse it, rather than matching every row whose foreign key is NULL.
- A model may hold several relations to one model, such as an `author` and an `editor` that are both `BelongsTo<User>`. Each loads by its own keys, eagerly too.
- A relation matches one key column. For a composite-key model, set `local_key` where needed, and write your own query when the relation must match several columns.
- A model read through `User::query_with(&db)` or `User::find_with(id, &db)` keeps that database, and its relations query there.

### `load()` and `get_cached()`

A wrapper also holds a cache: the rows an eager load, an entity manager load or deserialization put there.

- `load()` queries whenever a connection is reachable, even with rows cached, so a deserialized payload cannot pass itself off as database state. It serves the cache only when there is nothing to query through. It does not store what it reads.
- `get_cached()` never queries. It returns `Option<&T>` for a single-row wrapper and `Option<&[T]>` for a list.
- `is_loaded()` tells a relation that was loaded and has no rows from one that was never loaded.
- `as_mut()` edits the cached rows in place. `load()` does not see such edits; save the rows themselves to persist them.
- `clear()` drops the cached rows and marks the relation loaded and empty.

Under an entity manager the manager's cache wins instead; see [Entity Manager](entity-manager.md).

## Eager Loading

When you know which relations you need, call `with()` (or `with_many()`) on a query. TideORM loads each requested relation for all rows with set-based queries, not one per row:

```rust
let users = User::query()
    .where_eq("active", true)
    .with(User::PROFILE)
    .with(User::POSTS)
    .get()
    .await?;

for user in &users {
    let profile = user.profile.get_cached();
    let posts = user.posts.get_cached().unwrap_or_default();
    println!("{} has {} posts", user.name, posts.len());
}
```

Name a nested relation with a dot: `with("posts.comments")` loads every user's posts and every post's comments. Calls accumulate, and shared prefixes load once. A relation name is checked when the query runs; a typo is an "Unknown relation" error.

`with()` keeps the query's filters, ordering, pagination, cache settings and database handle. Filter the query before `with()`, or reach any query method after it through `query(|q| ..)`. That closure shapes the root query only; constrain a relation with `load_with` instead:

```rust
let users = User::query()
    .with(User::POSTS)
    .query(|q| q.where_gt("age", 18).where_null("banned_at").order_desc("id"))
    .get()
    .await?;
```

The eager builder runs with `get()`, `first()`, `first_or_fail()`, `find(id)`, `find_or_fail(id)`, `paginate(page, per_page)` and `count()`. Each returns the models themselves, each requested relation's rows in its wrapper. `User::query_with(&db).with(..)` reads the relations from `db` as well.

Eager loading works for `HasOne`, `HasMany`, `BelongsTo`, `HasManyThrough`, `MorphOne` and `MorphMany`. `MorphTo`, `SelfRef` and `SelfRefMany` have no eager path, and `with()` on one is an error that points you at the lazy `load()`.

Keys match by value, as the database matches them: an `i64` key finds the rows of an `i32` foreign key or of a text `*_id` column holding it. Where the column's collation matches keys that differ in case or accents (SQLite `NOCASE`, MySQL's default collations), each row goes to the parent whose key it holds byte for byte.

### Hidden Attributes in Eager-Loaded Payloads

`to_json()` writes a cached relation inline, and filters each payload by the **target model's** `hidden` list, at any depth. A `User` that declares `hidden = "password_hash"` keeps it out of `post.to_json(None)`, though the call was made on `Post`:

```rust
#[tideorm::model(table = "users", hidden = "password_hash")]
pub struct User { /* ... */ }

let post = Post::query().with(Post::AUTHOR).first().await?.unwrap();
// {"id": 1, "title": "…", "author": {"id": 7, "email": "…"}}: no password_hash
let body = post.to_json(None);
```

The owning model's `hidden` list is not applied to a relation payload, so a column one model hides and another does not keeps its value in the payload that does not hide it. Two payloads fall back to the owning model's list, because their type is not known statically: attachment blobs and `MorphTo` relations.

### Relations and Serde

TideORM's generated serde writes a relation's cached rows when they are loaded, and leaves the wrappers out of the stored columns. Deserializing a model rebuilds its wrappers from its own key fields and keeps the cached payload, so a JSON round trip keeps loaded relations. A wrapper deserialized on its own, outside a model, carries only its cache, not the keys it would query by. `MorphTo` keeps no cache: it serializes nothing and ignores a payload.

## Many-to-Many

`HasManyThrough<T, P>` reaches `T` through rows of the pivot model `P`:

```rust
#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,

    #[tideorm(has_many_through = "Role", pivot = "user_roles", foreign_key = "user_id", related_key = "role_id")]
    pub roles: HasManyThrough<Role, UserRole>,
}

#[tideorm::model(table = "roles")]
pub struct Role {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
}

#[tideorm::model(table = "user_roles")]
pub struct UserRole {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub role_id: i64,
}

async fn manage_roles(user: &User, role_id: i64) -> tideorm::Result<()> {
    let roles: Vec<Role> = user.roles.load().await?;

    user.roles.attach(role_id).await?;
    user.roles.attach_many([4_i64, 5]).await?;   // all or none, in one transaction
    let removed: u64 = user.roles.detach(role_id).await?;
    let removed: u64 = user.roles.detach_many([4_i64, 5]).await?;
    let removed: u64 = user.roles.detach_all().await?;

    // Replace the whole set
    user.roles.sync([1_i64, 2, 3]).await?;
    Ok(())
}
```

`foreign_key` names the pivot column holding this model's key and `related_key` the one holding the related key. Both may name the pivot model's fields or their renamed columns, and a pivot key need not share the owner's integer type. `pivot` names the pivot model's own table: the relation reads, deletes and loads pivot rows through that model, so a relation naming another table is refused. Leave it out and it is the pivot model's table.

Reading:

- `load()`, `count()` and eager loading return each related row once, however many pivot rows link it.
- `load_with()` joins the pivot table, so its closure can order by or read a pivot column, and it returns a row per pivot row that links it.

Writing:

- `attach()` writes only the two key columns. A pivot table with other `NOT NULL` columns and no defaults needs a direct `UserRole::create` instead.
- `attach()` of a pair already linked succeeds without a second row, even when two calls race. When another unique key of the pivot keeps the pair out, such as one on `role_id` alone, `attach()` and `sync()` return an error rather than report a link they did not make.
- `sync()` deletes every pivot row of the owner and inserts the wanted ids afresh, in one transaction (a savepoint inside yours), so extra columns on a pivot row are lost. Duplicate ids are collapsed and every id is checked before anything runs. An empty list detaches everything.
- When the pivot model has soft deletes, a trashed pivot row no longer links its pair: reads leave it out, `attach()` restores it, and `sync()` deletes it with the live rows.

| Backend | How `attach()` inserts a missing pair |
|---|---|
| PostgreSQL, SQLite | One `INSERT .. SELECT .. WHERE NOT EXISTS .. ON CONFLICT DO NOTHING` |
| MySQL, MariaDB | A read, then a plain `INSERT` (InnoDB's locks for an `INSERT .. SELECT` let two concurrent attaches deadlock); a duplicate counts as not written |

On every backend, a pair that was not written is read back, which catches a conflict on another unique key of the pivot.

## Polymorphic Relations

A polymorphic relation links rows through a `(type, id)` column pair instead of a foreign key to one table. Here an image belongs to a post or a video:

```rust
#[tideorm::model(table = "images")]
pub struct Image {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub path: String,
    pub imageable_type: String, // "posts" or "videos"
    pub imageable_id: i64,

    #[tideorm(morph_name = "imageable")]
    pub imageable: MorphTo<Post>,
}

#[tideorm::model(table = "posts")]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,

    #[tideorm(morph_name = "imageable")]
    pub images: MorphMany<Image>,
}
```

`morph_name = "imageable"` names the `imageable_type` and `imageable_id` columns, which the child model must declare; the derive fails if either is missing. The type column holds the owner's **table name** (`"posts"`), not the struct name. It is the name without its schema, so two owners whose tables share a name in different schemas write the same type, and each would read the other's children: give owner tables distinct names. An owner that keys its children by another column names it with `local_key`, as `#[tideorm(morph_name = "imageable", local_key = "uuid")]`.

`MorphOne` and `MorphMany` load like `HasOne` and `HasMany`, eagerly too (`with("images")`). `MorphTo<T>` loads the owner:

```rust
let image = Image::find_or_fail(1).await?;

// The default target, T: an error when the row names another table
let post: Option<Post> = image.imageable.load().await?;

// Owners of several types
if image.imageable.is_type::<Video>() {
    let video: Option<Video> = image.imageable.load_as::<Video>().await?;
}
let table: Option<&str> = image.imageable.type_value();
```

`load_as::<U>()` returns `Ok(None)` when the type column names another table, so you can try each type it may hold. It finds the owner by the `local_key` its `MorphOne`/`MorphMany` declares, else by its primary key. `MorphTo` always queries and has no eager path.

## Self-Referencing Relations

`SelfRef` and `SelfRefMany` model a hierarchy inside one table, such as an org chart, categories or a comment thread:

```rust
#[tideorm::model(table = "employees")]
pub struct Employee {
    #[tideorm(primary_key)]
    pub id: i64,
    pub name: String,
    pub manager_id: Option<i64>,

    #[tideorm(foreign_key = "manager_id")]
    pub manager: SelfRef<Employee>,

    #[tideorm(foreign_key = "manager_id")]
    pub reports: SelfRefMany<Employee>,
}

async fn org_chart(id: i64) -> tideorm::Result<()> {
    let employee = Employee::find_or_fail(id).await?;

    let manager: Option<Employee> = employee.manager.load().await?;
    let has_manager = employee.manager.exists().await?;

    let reports: Vec<Employee> = employee.reports.load().await?;
    let report_count = employee.reports.count().await?;

    // The whole subtree, three levels deep, in one recursive query
    let tree: Vec<Employee> = employee.reports.load_tree(3).await?;
    Ok(())
}
```

`foreign_key` defaults to `parent_id`. `local_key` defaults to the primary key, whatever its column is called, and can be set explicitly.

`load_tree(max_depth)` fetches the subtree below the row with one recursive CTE query, which avoids one `SELECT` per node on a large hierarchy. It respects the configured `local_key`. Rows come back flat, ordered by depth (direct children first); rebuild the structure from their foreign keys. A node appears once even when the keys form a cycle, at the shallowest depth it was reached. Soft deletes apply at every level, so a trashed node also prunes its descendants. `load_tree(0)` returns nothing without querying, and `load_tree(1)` equals `load()`. It always needs a live connection.

## Nested Save

The `NestedSave` methods save a parent and its children together, setting each child's foreign key to the parent's key:

```rust
// The profile's user_id is set to user.id
let (user, profile) = user.save_with_one(profile, "user_id").await?;

// Each post's user_id is set; a new post is inserted, a stored one updated
let (user, posts) = user.save_with_many(posts, "user_id").await?;

// Update parent and children, all already stored; no key is set
let (user, profile) = user.update_with_one(profile).await?;
let (user, posts) = user.update_with_many(posts).await?;

// Delete the children first, then the parent; returns the total row count
let deleted = user.delete_with_many(posts).await?;
let deleted = user.delete_with_one(profile).await?;
```

For several kinds of children at once, use `NestedSaveBuilder`:

```rust
let (user, saved) = NestedSaveBuilder::new(user)
    .with_one(profile, "user_id")
    .with_many(posts, "user_id")
    .with_many(comments, "author_id")
    .save()
    .await?;

let profile: Option<&Profile> = saved[0].as_one();
let posts: Option<&[Post]> = saved[1].as_many();
```

`save()` returns one `SavedRelation` per `with_one`/`with_many`, in the order you added them, holding the children as stored. `as_one`/`as_many` borrow them (`None` for another shape or model type), `into_one`/`into_many` take them and fail instead, and `is_one`/`is_many` report the shape.

Each nested operation runs in one transaction (a savepoint inside yours), so a child that fails also rolls back the parent's write. Children are written one at a time through each model's own `create`, `update` or `delete`, so their callbacks and validation run as usual. The foreign key may be the Rust field name, the column name or a typed column (`Post::USER_ID`), and the children may come in any list, a `Vec` or an array. `NestedSaveBuilder` is `Send`, so you can hold it across await points or move it into a spawned task before calling `save()`.

## Join Result Consolidation

`JoinResultConsolidator` turns flat tuples, such as rows of a join read as `(Order, LineItem)`, into nested ones:

```rust
use tideorm::prelude::JoinResultConsolidator;

// [(order1, item1), (order1, item2), (order2, item3)]
let flat: Vec<(Order, LineItem)> = orders_with_items;

// [(order1, [item1, item2]), (order2, [item3])]
let nested: Vec<(Order, Vec<LineItem>)> = JoinResultConsolidator::consolidate_two(flat, |o| o.id);

// LEFT JOIN rows, Vec<(Order, Option<LineItem>)>: a parent with no child gets an empty list
let nested = JoinResultConsolidator::consolidate_two_optional(flat_optional, |o| o.id);

// Three levels: Vec<(Order, Vec<(LineItem, Vec<Product>)>)>
let nested3 = JoinResultConsolidator::consolidate_three(flat3, |o| o.id, |i| i.id);

// Three levels where only the innermost may be absent: Vec<(Order, LineItem, Option<Product>)>
let nested3 = JoinResultConsolidator::consolidate_three_optional(flat3_optional, |o| o.id, |i| i.id);
```

## Linked Partial Select

`select_with_linked` left-joins another table and selects columns from both; `select_also_linked` selects every model column plus the linked ones. Read the rows into a struct of your own with `get_as()`, whose fields are the column names:

```rust
#[derive(serde::Deserialize)]
struct UserBio {
    id: i64,
    name: String,
    bio: Option<String>,
}

// users.id = profiles.user_id
let rows: Vec<UserBio> = User::query()
    .select_with_linked(vec!["id", "name"], "profiles", "id", "user_id", vec!["bio"])
    .get_as()
    .await?;

// Every user column, plus the profile's bio
let rows: Vec<serde_json::Value> = User::query()
    .select_also_linked("profiles", "id", "user_id", vec!["bio"])
    .get_json()
    .await?;
```

The arguments are the model's columns, the linked table, the model's join column, the linked table's join column and the linked columns. A linked column that shares a name with a selected one must be aliased: two outputs cannot share a name.

## Relations Under an Entity Manager

With the `entity-manager` feature, `entity_manager.load(&mut relation)` loads a `HasOne`, `HasMany`, `BelongsTo` or `HasManyThrough` relation inside the manager's identity map, and `entity_manager.save(&model)` writes the loaded relations back. Every relation of a model the manager loaded, polymorphic and self-referencing ones included, queries through the manager's database, even with no global database. See [Entity Manager](entity-manager.md).
