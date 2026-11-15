# Typed Columns

Every model gets a typed constant for each of its columns. Use them instead of string names, and the compiler catches a misspelt column or a value of the wrong type.

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub age: Option<i32>,
    pub active: bool,
}

async fn adults() -> tideorm::Result<Vec<User>> {
    User::query()
        .where_eq(User::ACTIVE, true)
        .where_gte(User::AGE, 18)
        .order_asc(User::NAME)
        .get()
        .await
}
```

`User::query().where_eq(User::ACTIVE, "yes")` does not compile, and neither does `User::ACTVIE`.

## Two Spellings

The derive generates each column twice:

- an associated constant named after the field in upper case: `User::ID`, `User::NAME`, `User::AGE`, `User::ACTIVE`;
- a field of the `User::columns` struct (type `UserColumns`) named after the field: `User::columns.id`, `User::columns.name`.

Both are the same `Column<T>`, where `T` is the field's Rust type: `User::AGE` is a `Column<Option<i32>>`. The `columns` struct keeps the names apart from the model's own associated items and lists every column in your editor's completion. A field mapped with `#[tideorm(column = "..")]` keeps its field name here; the constant carries the column name.

Each relation field gets a constant holding its name too, for eager loading: `User::POSTS` is `"posts"`.

## Where They Work

Every method that takes a column name takes a typed column too, and string names keep working. Column lookups accept both the database column name and the Rust field name.

| Use | Methods |
|---|---|
| filters | `where_eq`, `where_not`, `where_ne`, `where_gt`, `where_gte`, `where_lt`, `where_lte`, `where_like`, `where_not_like`, `where_in`, `where_not_in`, `where_null`, `where_not_null`, `where_between`, `where_not_between`, and their `or_where_*` and `and_where_*` forms |
| ordering and grouping | `order_by`, `order_asc`, `order_desc`, `group_by` |
| aggregates | `sum`, `avg`, `min`, `max`, `count_distinct` |
| HAVING | `having(Aggregate::sum(..).gt(..))`, `Aggregate::count().gte(..)` and the rest |
| window functions | `partition_by`, `order_by` on the window builder |
| reads | `pluck`, `value` |
| writes | `set`, `coalesce` and the other setters of a [batch update](crud.md#batch-updates), upsert conflict columns (`on_conflict`, `insert_or_update`), `is_field_dirty` |

```rust,ignore
User::query().where_in(User::columns.role, ["admin", "moderator"]);    // any list
User::query().where_not_in(User::columns.status, vec!["banned"]);
User::query().where_null(User::columns.deleted_at);
User::query().where_between(User::columns.age, 18, 65);

User::query()
    .order_by(User::columns.created_at, Order::Desc)
    .group_by(User::columns.role);

// All or_where_* calls form one OR group, ANDed with the rest of the query.
User::query()
    .or_where_eq(User::columns.role, "admin")
    .or_where_eq(User::columns.role, "moderator");
```

Aggregates read the result as the type you ask for:

```rust,ignore
let total: Decimal = Order::query().sum(Order::columns.amount).await?;
let average: Option<f64> = Product::query().avg(Product::columns.price).await?;
let oldest: Option<i32> = User::query().max(User::columns.age).await?;
```

`pluck` and `value` read a typed column as its field's type, with no annotation: `User::query().pluck(User::ID)` is a `Vec<i64>`, and `value(User::AGE)` an `Option<Option<i32>>`. A column named by a string reads as whatever type you ask for.

## Which Values a Column Takes

A typed column takes the values a field of its type would hold:

| Column | Takes |
|---|---|
| `Column<T>` | a `T` or a `&T`, so a borrowed list works in `where_in` |
| `Column<Option<T>>` | a `T`, a `&T` or an `Option<T>` |
| `Column<String>`, `Column<Option<String>>` | also a `&str` |
| `Column<i64>` | also `i8`, `i16`, `i32`, `u8`, `u16`, `u32` |
| `Column<i32>` | also `i8`, `i16`, `u8`, `u16` |
| `Column<f64>` | also `f32`, `i8`, `i16`, `i32`, `u8`, `u16`, `u32` |
| `Column<Decimal>` | also every integer type up to `i64` and `u64` |

The widenings are the conversions that lose nothing. A column named by a string takes any value and binds it as the column's type, reading a `serde_json::Value` or text as that type (a UUID or timestamp written as text, for instance).

Ordering comparisons (`where_gt` and the rest, `where_between`) take numbers, `Uuid`, the chrono date and time types, `Decimal`, and `&str` on text columns, which compare in the column's collation.

## Conditions From a Column

A typed column also builds a condition of its own, applied with `where_col`, `or_where_col` or `and_where_col`. The condition traits are in the prelude:

| Trait | Methods | Columns |
|---|---|---|
| `ColumnEq` | `eq`, `ne` | any |
| `ColumnOrd` | `gt`, `gte`, `lt`, `lte`, `between` | numbers, `Uuid`, chrono types, `Decimal`, text |
| `ColumnIn` | `is_in`, `not_in` | any |
| `ColumnLike` | `like`, `not_like`, `contains`, `starts_with`, `ends_with` | `String`, `Option<String>` |
| `ColumnNullable` | `is_null`, `is_not_null` | any `Option` column |

```rust,ignore
User::query().where_col(User::columns.age.gte(18));
User::query().where_col(User::columns.email.ends_with("@example.com"));
User::query().where_col(User::columns.age.is_null());
```

See [Query Builder](queries.md) for every filter.

## Columns of Other Models

A typed column knows its model's table. In a query of another model it is written qualified, so `User::columns.id` in a `Post` query means `users.id`, not `posts.id`; this is what makes joins and subqueries with typed columns unambiguous. `Column::name()` returns the bare name and `Column::table()` the table.

A batch update refuses a typed column of another model.

## Defining Columns by Hand

For a column no model field declares, such as a view's computed column, define a constant yourself:

```rust,ignore
use tideorm::prelude::*;

pub const FULL_NAME: Column<String> = Column::new("full_name");

let users = User::query().where_eq(FULL_NAME, "John Doe").get().await?;
```

`Column::new(name)` has no table and is never qualified. `Column::of(table, name)` and `Column::of_schema(schema, table, name)` name the table (and schema), as the generated constants do.

Methods that take several columns (`partition_by`, conflict columns) accept a list of names or typed columns, a `Vec`, or a tuple of up to 12 typed columns of different types, such as `(User::TENANT_ID, User::EMAIL)`.
