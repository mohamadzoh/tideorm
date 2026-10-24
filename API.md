# TideORM API cheat sheet

Use `use tideorm::prelude::*;`. The generated model implements `Model` and exposes
`User::query()`. [Full API documentation](https://docs.rs/tideorm) and
[standalone examples](examples/README.md) cover the complete surface.

## Models and startup

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "users")]
struct User {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    active: bool,
}
```

The macro generates `User::ID: Column<i64>`, `User::NAME: Column<String>`,
`User::ACTIVE: Column<bool>`, and the existing `User::columns.id` etc.
A relation field `posts` generates `User::POSTS: &'static str` for `.with(User::POSTS)`.
Renamed database columns keep the Rust field's constant name; the constant carries
the database name. Relation constants check spelling, but are still strings:
using another model's relation constant is checked at runtime.

```rust,ignore
TideConfig::init()
    .database("sqlite::memory:") // enable sqlite and runtime-tokio
    .max_connections(1)
    .auto_models()               // all models linked through normal Rust modules
    .sync(true)                  // development schema creation
    .connect().await?;
```

`auto_models()` discovers inventory metadata; no file glob is needed. It includes
models from linked dependencies. Use `.models::<User>()` or `.models::<(User, Post)>()`
to sync a subset.
Registration is process-wide and idempotent per schema/table. Registration alone
does not run SQL. For deployed schemas, use migrations instead of sync.

## Queries

All filters and modifiers consume and return the builder unless indicated.
`C: ColumnValue<V>` means a typed `Column<T>` takes a `T` or a `&T`, so
`where_in(User::ID, &ids)` works; a string column accepts any serializable value.
`String` and `Option<String>` columns accept `&str`. Nullable columns accept
`Option<T>` (including `None::<T>`) or `T`. Integer, float and decimal columns
accept narrower numbers that convert losslessly (an `i32` for an `i64` column).

```text
where_eq<C: ColumnValue<V>, V>(column: C, value: V) -> Self
where_not / where_gt / where_gte / where_lt / where_lte: same arguments
where_in<C: ColumnValue<V>, V>(column: C, values: impl IntoIterator<Item=V>) -> Self
where_not_in: same arguments
where_between<C: ColumnValue<L> + ColumnValue<H>, L, H>(column: C, min: L, max: H) -> Self
where_not_between: same arguments
where_null / where_not_null(column: impl IntoColumnName) -> Self
where_like / where_not_like(column: impl IntoColumnName, pattern: &str) -> Self
where_contains / where_starts_with / where_ends_with(column: impl IntoColumnName, value: &str) -> Self
where_col(condition: ColumnCondition) -> Self
where_raw(sql: &str) -> Self
where_raw_with(sql: &str, params: Vec<DbValue>) -> Self
when(condition: bool, f: impl FnOnce(Self) -> Self) -> Self
when_some<T>(option: Option<T>, f: impl FnOnce(Self, T) -> Self) -> Self
select(columns: Vec<&str>) -> Self
select_raw(sql: &str) -> Self
order_by(column: impl IntoColumnName, order: Order) -> Self
order_asc / order_desc(column: impl IntoColumnName) -> Self
sort(spec: &str) -> Self                        // "-created_at,name"
limit(n: u64) / offset(n: u64) -> Self
page(page: u64, per_page: u64) -> Self          // one-based
with(relation: &str) -> EagerQueryBuilder<M>
with_many(relations: &[&str]) -> EagerQueryBuilder<M>
with_trashed() / only_trashed() -> Self
consolidate(&self) -> QueryFragment<M>
apply(fragment: &QueryFragment<M>) -> Self
validate(&self) -> Result<()>
build_sql_preview(&self) -> String             // diagnostics, not execution
```

Scalar comparisons, ranges and IN lists enforce the typed column's value type,
including their OR/AND variants. SQL pattern, JSON, array and column-comparison
operators have their own argument types; they do not enforce every SQL operator's
applicability to a column type. String names remain useful for dynamic queries
and are validated against model metadata before execution.

```rust,ignore
let users = User::query()
    .where_eq(User::ACTIVE, true)               // "yes" is a compile error
    .where_contains(User::NAME, "Ada")         // literal text, escapes % and _
    .order_asc(User::ID)
    .get().await?;
```

`or_where_*` calls share one OR group ANDed with the other filters. For an OR of
AND branches, use `.begin_or().or_where_eq(...).and_where_eq(...).or_where_eq(...).end_or()`.
The same value checks apply in OR branches and batch-update filters.

## Async terminals and writes

```text
get(self) -> Result<Vec<M>>
first(self) -> Result<Option<M>>
first_or_fail(self) -> Result<M>
find(self, id: M::PrimaryKey) -> Result<Option<M>>
find_or_fail(self, id: M::PrimaryKey) -> Result<M>
count(self) -> Result<u64>
exists(self) -> Result<bool>
get_json(self) -> Result<Vec<serde_json::Value>>
get_as<T: DeserializeOwned>(self) -> Result<Vec<T>>
pluck<T: DeserializeOwned>(self, column: impl IntoColumnName) -> Result<Vec<T>>
value<T: DeserializeOwned>(self, column: impl IntoColumnName) -> Result<Option<T>>
paginate(self, page: u64, per_page: u64) -> Result<Paginated<M>>
sum<T: DeserializeOwned>(self, column: impl IntoColumnName) -> Result<T>
avg<T: DeserializeOwned>(self, column: impl IntoColumnName) -> Result<Option<T>>
min / max<T: DeserializeOwned>(self, column: impl IntoColumnName) -> Result<Option<T>>
delete(self) / delete_all(self) / soft_delete(self) / restore(self) / force_delete(self) -> Result<u64>
```

Check the individual terminal's documentation for projection, soft-delete and
unfiltered mutation rules. Eager terminals return `WithRelations<M>` wrappers:
`get() -> Result<Vec<WithRelations<M>>>`, `first()` and `find(id)` return
`Result<Option<WithRelations<M>>>`. Use `.query(|q| q.where_eq(...))` to modify
the root query after `.with(...)`.

```rust,ignore
let user = User { id: 0, name: "Ada".into(), active: true }.save().await?;
let mut user = User::find(user.id).await?.unwrap();
user.name = "Ada Lovelace".into();
let user = user.update().await?;
user.delete().await?;

User::query().where_eq(User::ACTIVE, false)
    .update_all().set("active", true).execute().await?;

User::transaction(|_tx| Box::pin(async {
    // Model operations use this transaction's scoped connection.
    User { id: 0, name: "Grace".into(), active: true }.save().await?;
    Ok(()) // Err rolls back
})).await?;
```

## Cargo features and toolchain

Default features: `postgres`, `runtime-tokio`. To use only SQLite:

```toml
[dependencies]
tideorm = { version = "0.16.2", default-features = false, features = ["sqlite", "runtime-tokio"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

| API | Required feature |
| --- | --- |
| Models, typed columns, relations, scopes, migrations, transactions, inventory discovery | No module flag; database operations need a backend and runtime |
| PostgreSQL / MySQL and MariaDB / SQLite | `postgres` / `mysql` / `sqlite` |
| Tokio / async-std driver runtime | `runtime-tokio` / `runtime-async-std` |
| `changed_fields()`, `original_value()` | `dirty-tracking` |
| `EntityManager`, managed instances and `flush()` | `entity-manager` |
| `HasAttachments`, attachment helpers | `attachments` |
| `HasTranslations`, translation helpers | `translations` |
| `FullTextSearch`, highlighting helpers | `fulltext` |
| `#[tideorm(encrypted = "...")]` automatic field encryption | `encrypted-fields` |

A method missing at compile time may need its feature and the corresponding
trait from the prelude. docs.rs includes all module flags, so its API is broader
than the default build.

MSRV: **Rust 1.94**, required by SeaORM 2.0.2 and SQLx 0.9. The project's MSRV CI
checks that version. Select it explicitly with `cargo +1.94.0 check`; declaring
1.91 would not make the current dependencies build there.
