# JSON and Arrays

TideORM filters JSON documents and arrays, and updates a part of one in place, with the same methods on every backend. PostgreSQL's `jsonb` operators define what each filter means; MySQL, MariaDB and SQLite are given SQL that answers the same way.

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "posts")]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub metadata: serde_json::Value,
}

async fn featured_posts() -> tideorm::Result<Vec<Post>> {
    Post::query()
        .where_json_contains("metadata", json!({"featured": true}))
        .where_json_path_exists("metadata", "$.cover.url")
        .get()
        .await
}

async fn feature(id: i64) -> tideorm::Result<u64> {
    Post::update_all()
        .where_eq("id", id)
        .json_set("metadata", "$.featured", true)
        .execute()
        .await
}
```

## Columns

A `serde_json::Value` field is a JSON column (`JsonValue` in the prelude is the same type). In a migration, `t.json(..)` creates `JSON` and `t.jsonb(..)` creates `JSONB` on PostgreSQL and `JSON` elsewhere; SQLite stores either as text. See [Migrations](migrations.md).

Only PostgreSQL has array columns: a `Vec<T>` field, or `t.integer_array(..)`/`t.text_array(..)` in a migration. MySQL, MariaDB and SQLite have none, so keep an array there in a JSON column (a `serde_json::Value` field holding an array); the array filters and setters below work on it as a JSON array. `backend().supports_arrays()` tells which kind you have:

```rust,ignore
if require_db()?.backend().supports_arrays() {
    // PostgreSQL: native array columns
}
```

## Filtering JSON

| Method | Keeps the rows whose JSON column |
|---|---|
| `where_json_contains(col, value)` | contains `value`, as PostgreSQL's `@>` reads it |
| `where_json_contained_by(col, value)` | is contained by `value` (`<@`) |
| `where_json_key_exists(col, key)` | has the top-level `key` |
| `where_json_key_not_exists(col, key)` | lacks the top-level `key` |
| `where_json_path_exists(col, path)` | has a member at `path` |
| `where_json_path_not_exists(col, path)` | has no member at `path` |
| `where_eq(col, value)`, `where_not`, `where_in`, `where_not_in` | equals the document `value` (or one of them) |

Every one has `or_where_*` and `and_where_*` forms, as other filters do (see [Where Each Filter Family Works](queries.md#where-each-filter-family-works)). The containment value is anything `serde::Serialize`, usually `json!(..)`.

```rust,ignore
Post::query().where_json_contains("metadata", json!({"tags": ["rust"]}));
Post::query().where_json_contained_by("metadata", json!({"featured": true, "draft": false}));
Post::query().where_json_key_exists("metadata", "cover");
Post::query().where_json_path_not_exists("metadata", "authors[1].name");
Post::query().where_eq("metadata", json!({"featured": true, "draft": false}));
```

### Containment

Containment follows PostgreSQL's `@>` on every backend: an object contains another when it has each of its keys holding a contained value, an array contains another when each element of the other is contained by some element of it (order and repeats do not count), and a scalar contains only an equal scalar. At the top level only, a scalar is contained by an array that holds it.

- **PostgreSQL** casts the column to `jsonb`, since the operators exist only there. They work on a `json` column (what `t.json(..)` creates) too, and on a `jsonb` column the cast is dropped at planning, so its GIN index still applies.
- **SQLite** has no containment operators, so TideORM builds the test from `json_each` rows: an object matches key by key, an array element by element.
- **MySQL and MariaDB** use `JSON_CONTAINS`, which on its own lets a value match an array holding it at any depth: `{"tags": "a"}` would match `{"tags": ["a", "b"]}`. TideORM adds a type check for each path of the document you pass, so they follow PostgreSQL there too. Below an array the paths are not known in advance, so MySQL's reading stays there: `[{"k": [1, 2]}]` contains `[{"k": 1}]` on MySQL and MariaDB and not on PostgreSQL.

### Keys and Paths

A key or path exists when there is a member there, one holding JSON `null` included. A `NULL` column matches neither a test nor its negation.

A path is `$` followed by `.key`, `."quoted key"`, `['quoted key']` or `[index]` steps, or those steps alone, read from the root: `a.b[0]` is `$.a.b[0]`. A bare `.key` must be a plain identifier (an ASCII letter or `_`, then letters, digits or `_`); quote any other key. Every backend reads a path strictly:

- a key exists only in an object, never as a string element of an array;
- a step never looks inside an array it does not index;
- an index never reads a value that is not an array as an array of one (MySQL's and MariaDB's indexed steps check their parent is an array).

A path with other steps, such as a wildcard or a filter, fails the query, except on PostgreSQL (not CockroachDB), which reads it as the JSONPath written.

### Equality

On a JSON column, `where_eq`, `where_not`, `where_in` and `where_not_in` compare documents, not their text: an object's key order does not count, numbers compare by value (`1.0` is `1`), and an array's order does. `1`, `"1"` and `true` all differ. Text you pass is the JSON string, not a document parsed from it. `serde_json::Value::Null` matches the JSON `null` document and `None` the SQL `NULL` rows; see [Values](queries.md#values).

### What Each Backend Runs

Every value and path is bound as a parameter (`?` below).

| Test | PostgreSQL | MySQL, MariaDB | SQLite |
|---|---|---|---|
| contains | `(col)::jsonb @> ?` | `JSON_CONTAINS(col, ?)` plus a `JSON_TYPE` check per path | a structural test over `json_each(col)` rows |
| contained by | `(col)::jsonb <@ ?` | `JSON_CONTAINS(?, col)` plus a `JSON_TYPE` check per path | a structural test over `json_each(col)` rows |
| key exists | `jsonb_typeof((col)::jsonb) = 'object' AND (col)::jsonb ? ?` | `JSON_CONTAINS_PATH(col, 'one', ?)` | `json_type(col, ?) IS NOT NULL` |
| path exists | `jsonb_path_exists((col)::jsonb, 'strict <path>')`; any other JSONPath with `@?` | `JSON_CONTAINS_PATH(col, 'one', ?)` plus an array check per index step | `json_type(col, ?) IS NOT NULL` |
| equality | `(col)::jsonb = ?` | MySQL `col = CAST(? AS JSON)`, MariaDB `JSON_EQUALS(col, ?)` | a structural test over `json_each(col)` rows |

The negations wrap the same test so that a `NULL` column stays unmatched.

## Filtering Arrays

| Method | Keeps the rows whose array column |
|---|---|
| `where_array_contains(col, values)` | holds every one of `values` (`@>`) |
| `where_array_contained_by(col, values)` | holds nothing but `values` (`<@`) |
| `where_array_overlaps(col, values)` | holds at least one of `values` (`&&`) |

`values` is any list (a `Vec`, an array, a slice); each element is bound as the array's element type on PostgreSQL and as a JSON value elsewhere. They have `or_where_*` and `and_where_*` forms too.

```rust,ignore
// Posts tagged both "rust" and "orm"
Post::query().where_array_contains("tags", ["rust", "orm"]);
// Posts with no tag outside the list
Post::query().where_array_contained_by("tags", ["rust", "orm", "sql"]);
// Posts with either tag
Post::query().where_array_overlaps("tags", ["rust", "go"]);
```

- Order and repeats do not count.
- An empty list: `where_array_contains` keeps every non-`NULL` array, `where_array_contained_by` only empty arrays, and `where_array_overlaps` nothing.
- A `NULL` in a `where_array_contained_by` list also allows `NULL` elements.
- A `NULL` column matches none of them.
- On a JSON array (MySQL, MariaDB, SQLite), a column holding an object or a scalar matches none of them, and elements compare as JSON values: `1` is not `true` or `"1"`, and the string `"{\"a\":1}"` is not that object.

| Test | PostgreSQL (native array) | MySQL, MariaDB (JSON array) | SQLite (JSON array) |
|---|---|---|---|
| contains | `(? = ANY(col) AND ? = ANY(col) ..)` | `JSON_CONTAINS(col, ?)` of the whole list, with type checks | an `EXISTS` over `json_each(col)` per value, ANDed |
| contained by | `col IS NOT NULL AND NOT EXISTS (..)` over `unnest(col)` for an element outside the list | `JSON_CONTAINS(?, col)`, with type checks | `col IS NOT NULL AND NOT EXISTS (..)` over `json_each(col)` |
| overlaps | `(? = ANY(col) OR ? = ANY(col) ..)` | `JSON_CONTAINS(col, ?)` per value, ORed | an `EXISTS` over `json_each(col)` per value, ORed |

On MySQL, MariaDB and SQLite every array test also checks that the column holds an array (`JSON_TYPE(col) = 'ARRAY'`, `json_type(col) = 'array'`). To test an array inside a JSON document on PostgreSQL, use `where_json_contains` with the array: the array filters there need a native array column.

## Updating JSON and Arrays

A bulk update (`Model::update_all()` or `query().update_all()`) changes part of a document or an array in the database, without reading it into Rust and writing it back, which would undo another writer's change to the rest of the document:

```rust,ignore
Post::update_all()
    .where_eq("id", id)
    .json_set("metadata", "$.cover.url", "https://cdn.example.com/1.png")
    .array_append("tags", "rust")
    .execute()
    .await?;

Post::update_all()
    .where_array_contains("tags", ["draft"])
    .array_remove("tags", "draft")
    .execute()
    .await?;
```

| Setter | Writes | Column |
|---|---|---|
| `json_set(col, path, value)` | the document with the member or element at `path` set to `value` | a JSON column, on every backend |
| `array_append(col, value)` | the array with `value` added at the end | a native array on PostgreSQL; a JSON column on MySQL, MariaDB and SQLite |
| `array_remove(col, value)` | the array without any element equal to `value` | as `array_append` |

A setter on a column of another type fails the update before it runs.

- **`json_set`** takes `$` followed by at least one `.key`, `."any key"`, `['any key']` or `[index]` step; a wildcard is refused. An existing member or element is replaced and a missing member of an existing object is added. A path whose parent is missing, or is not the object a key step needs or the array an index step needs, leaves the document as it is, and an index past an array's end appends the value. The same document comes out on every backend.
- **`array_append`** starts a new array in a `NULL` column. On a JSON column it appends a JSON value, an object included, and a document that is not an array becomes the first element of a new one.
- **`array_remove`** drops every element equal to the value and keeps the others in their order, with their types. It compares as JSON equality does: an object's key order does not count, `1.0` is `1`, and `1`, `"1"` and `true` differ. A `NULL` column, or a JSON document that is not an array, stays as it is. On MySQL and MariaDB it reads the array with `JSON_TABLE`, which TiDB lacks (see [Compatible Servers](compatible-servers.md)).

Values are written as their JSON documents on a JSON column: `json!(..)` for an object, text as a JSON string, and a `None` as JSON `null`. A NaN or infinite float fails the update before any row changes. The other setters (`set`, `coalesce`, `increment` and the rest) are on [Reading and Writing](crud.md).

## Reading JSON

A JSON field reads back as the document it holds on every backend, including SQLite, where it is stored as text. For JSON computed in a query, such as a `->` expression read with `get_json()`, see [Rows as JSON](queries.md#rows-as-json).
