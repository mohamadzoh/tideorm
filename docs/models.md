# Defining Models

A model is a plain Rust struct that maps to one table. Each instance is one row, and the struct carries the methods that read and write it.

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

That struct is ready to use: `User::find(1)`, `User::query()`, `user.save()` and the rest are described in [Reading and Writing](crud.md).

## `#[tideorm::model]` and `#[derive(Model)]`

`#[tideorm::model(..)]` is shorthand. It rewrites the struct to `#[derive(tideorm::Model)]` and moves its arguments into a `#[tideorm(..)]` attribute, so these two definitions are the same model:

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "posts", soft_delete)]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub deleted_at: Option<DateTime<Utc>>,
}
```

```rust
use tideorm::prelude::*;

#[derive(Model)]
#[tideorm(table = "posts", soft_delete)]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub deleted_at: Option<DateTime<Utc>>,
}
```

Put the struct options in one place: inline in `#[tideorm::model(..)]`, or in a `#[tideorm(..)]` attribute on a `#[derive(Model)]` struct. Giving them in both, or adding `#[derive(Model)]` to a struct that already has `#[tideorm::model]`, is a compile error. Both forms apply only to structs with named fields, and a model cannot be generic: it maps one struct to one table.

## Generated Implementations

Besides the TideORM traits, a model gets `Debug`, `Clone`, `Default`, `Serialize` and `Deserialize`.

You can derive any of these yourself, and TideORM then skips its own, as long as your derive is a separate `#[derive(..)]` attribute **below** `#[tideorm::model(..)]` (or below `#[derive(Model)]`). A derive above it, or in the same list as `Model` (`#[derive(Model, Debug)]`), is never shown to TideORM, and the two implementations fail with "conflicting implementations" (E0119). Move it below, or opt out of TideORM's implementation with a `skip_*` option.

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "users")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub display_name: String, // serialized as "displayName"
}
```

Your derive then decides the JSON. `to_json()` and the hidden attributes follow its renames. Relation fields serialize as `null` until they are loaded, and a request body that leaves out a relation or a managed timestamp deserializes only if that field is `#[serde(default)]`.

Do not put `#[serde(flatten)]` on a hidden field or on a typed relation. Flattening removes the boundary TideORM needs to filter that field's contents: `try_to_json()` returns a configuration error, `to_json()` logs it and returns `{}`, and an affected eager-loaded model renders as `{}`. Remove `flatten`, or mark the field `skip_serializing`. Public fields can still be flattened.

To write all of these impls yourself, use `skip_derives`:

```rust
use std::fmt;
use tideorm::prelude::*;

#[tideorm::model(table = "products", skip_derives)]
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Product {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub sku: String,
    pub price: i64,
}

impl fmt::Debug for Product {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Product").field("sku", &self.sku).finish()
    }
}
```

Whatever you skip, implement yourself where your code needs it; every model must be `Clone`, `Serialize` and `Deserialize`.

## Table and Column Names

Without `table = ".."`, the table is the struct name in snake case, pluralized: `User` is `users`, `BlogPost` is `blog_posts`, `Category` is `categories`. A column is the field name in snake case (any `r#` prefix removed, so `r#type` is `type`) unless `#[tideorm(column = "..")]` names it.

The snake-case rules split words at `_`, at a lowercase letter followed by an uppercase one, between letters and digits, and before the last capital of an acronym:

| Name | Snake case |
|---|---|
| `UserProfile` | `user_profile` |
| `HTTPRequest` | `http_request` |
| `XMLParser` | `xml_parser` |
| `sha256` | `sha_256` |
| `S3Object` | `s_3_object` (table `s_3_objects`) |
| `_private` | `_private` |

Digits split off as their own word, so a field `sha256` maps to the column `sha_256`. Use `#[tideorm(column = "sha256")]` when the column is named otherwise.

Pluralization adds `es` after `s`, `x`, `z`, `ch` and `sh`, turns a final `y` after a consonant into `ies`, and adds `s` otherwise. A short list of irregular words is recognised when the whole snake-case name is that word: `person`, `man`, `woman`, `child`, `tooth`, `foot`, `mouse`, `goose`, `leaf`, `knife`, `life`, `wife`, `wolf`, `calf`, `half`, `loaf`, `self`, `shelf`, `thief`, `quiz` and `fez`. So `Person` is `people`, but `SalesPerson` is `sales_persons`. Name the table explicitly when the generated one is not what you want.

## Struct Options

| Option | Meaning |
|---|---|
| `table = "name"` | The table name. |
| `schema = "name"` | The schema the table lives in (a database, on MySQL and MariaDB). Every statement names the table as `schema.table`, and joins accept `"schema.table"` too. |
| `soft_delete` | Deleting marks rows instead of removing them. See [Soft Deletes and Scopes](soft-deletes.md). |
| `deleted_at_column = "name"` | The soft-delete field or column, when it is not `deleted_at`. Requires `soft_delete`. |
| `timestamps` | Mark the model as timestamped. `created_at` and `updated_at` fields are managed either way; see [Callbacks and Timestamps](callbacks.md). |
| `hidden = "a,b"` | Leave these out of `to_json()`. See [Hidden Attributes](#hidden-attributes). |
| `tokenize` | Generate `to_token()` and `from_token()`. See [Tokenization](tokenization.md). |
| `encrypted = "a,b"` | Store these fields encrypted. Requires the `encrypted-fields` feature; see [Encrypted Fields](encrypted-fields.md). |
| `searchable = "a,b"` | The columns `searchable_fields()` lists, to pass to a full-text search. See [Full-Text Search](full-text-search.md). |
| `translatable = "a,b"`, `languages = "en,fr"`, `fallback_language = "en"` | Translated fields. Requires the `translations` feature and a `HasTranslations` impl; see [Translations](translations.md). |
| `has_one_files = "a"`, `has_many_files = "b"` | File attachment slots. Requires the `attachments` feature and a `HasAttachments` impl; see [File Attachments](attachments.md). |
| `skip_derives` | Skip the generated `Debug`, `Clone`, `Default`, `Serialize` and `Deserialize`. |
| `skip_debug`, `skip_clone`, `skip_default`, `skip_serialize`, `skip_deserialize` | Skip one of them. |

List options are one comma-separated string. An empty name in the list (`"a,,b"`) is a compile error.

### Hidden Attributes

`hidden` names fields, columns (which hide their field) or attachment relations. A name that is one field's name and another field's column hides both, and a name that matches nothing is a compile error. Without `hidden`, the soft-delete field (`deleted_at` by default) is hidden.

Hidden attributes are left out of `to_json()`, `try_to_json()` and `to_hash_map()`. Plain serde (`serde_json::to_value(&user)`, a framework's `Json(user)`) and `query().get_json()` still include them. `TideConfig::hidden_attributes([..])` hides names from `to_json()` for every model; see [Configuration](configuration.md).

### Indexes

Indexes are declared on the struct or on a field. [Schema sync](schema.md) creates them, and `ModelMeta::indexes()` and `unique_indexes()` list them.

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "products")]
#[index("category")]
#[index(name = "idx_price_category", columns = "price,category")]
#[unique_index("sku")]
pub struct Product {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub sku: String,
    pub category: String,
    pub price: i64,
    #[index]
    pub active: bool,
}
```

| Form | Index |
|---|---|
| `#[index("col")]`, `#[index("a,b")]`, `#[index("a", "b")]`, `#[index(a, b)]` | On those columns, named `idx_<table>_<columns>` |
| `#[index(name = "idx", columns = "a,b")]` | On those columns, under that name |
| `#[unique_index(..)]` | The same forms, unique; a generated name ends in `_unique` |
| `#[index]`, `#[unique_index]` on a field | On that field's column |

A generated name longer than 63 bytes is shortened with a hash. Columns may be named by field or by column; a name the model does not declare is a compile error, and so is an index over an encrypted field.

## Field Options

| Option | Meaning |
|---|---|
| `primary_key` | Part of the primary key. Mark each field of a composite key. |
| `auto_increment` | The database numbers this key. Only on a single-column integer primary key. |
| `column = "name"` | The column name, when it is not the field name in snake case. |
| `nullable` | The column accepts NULL. An `Option` field is nullable without it. |
| `default = "value"` | The column's `DEFAULT` in tables that schema sync creates. Inserts send every field's value, so a model built with `..Default::default()` stores the field's Rust default, not this one. |
| `skip` | Not a column. It is never read or written, holds its `Default` on every model a query or write returns, and may be left out when deserializing. |

Every other field is a column, and its Rust type decides the column type (see [Databases](databases.md) for the mappings). A field type TideORM cannot store fails to compile at that field. Relation fields (`HasOne<T>`, `HasMany<T>`, `BelongsTo<T>` and the rest) are not columns; their options are on the [Relations](relations.md) page.

A `skip` field is useful for values a request carries but the table does not, such as a password confirmation checked by [validation](validation.md).

## Primary Keys

Every model has a primary key; a struct without a `#[tideorm(primary_key)]` field is a compile error. A primary key field cannot be an `Option`.

`auto_increment` needs `primary_key` and an integer type. On such a key, `0` means "not saved yet": `save()` inserts a model whose key is `0`, and the database assigns the key. See [Reading and Writing](crud.md#how-save-decides) for how `save()` treats other keys.

A `Uuid` primary key still at the nil UUID when the row is inserted gets a random (v4) key, whether through `save()`, `create()`, `insert_all()` or an upsert. Set the key yourself, in `before_create` for instance, to use another UUID version.

### Composite Keys

Mark each field of the key. Methods that take a key take a tuple, in the order the fields are declared:

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "user_roles")]
pub struct UserRole {
    #[tideorm(primary_key)]
    pub user_id: i64,
    #[tideorm(primary_key)]
    pub role_id: i64,
    pub granted_by: String,
}

async fn load() -> tideorm::Result<Option<UserRole>> {
    UserRole::find((1_i64, 2_i64)).await
}
```

- `auto_increment` is refused on a composite key.
- `tokenize` needs exactly one key field.
- A relation on a composite-key model needs an explicit `local_key = ".."`.
- `BatchUpdateBuilder::limit` is refused for a composite key on PostgreSQL and SQLite.

## Generated Items

The derive also emits:

- A typed column constant per column (`User::EMAIL`, a `Column<String>`) and the `User::columns` struct with one field per column (`User::columns.email`). See [Typed Columns](typed-columns.md).
- A constant per relation field holding its name (`User::POSTS` is `"posts"`), for eager loading.
- `ModelMeta`, whose associated functions describe the model: `table_name()`, `schema_name()`, `primary_key_names()`, `primary_key_name()`, `primary_key_auto_increment()`, `column_names()`, `field_names()`, `hidden_attributes()`, `searchable_fields()`, `translatable_fields()`, `encrypted_fields()`, `soft_delete_enabled()`, `deleted_at_column()`, `has_timestamps()`, `indexes()` and `unique_indexes()`, among others.
- `Validate` from the `#[validate(..)]` rules (see [Validation](validation.md)), and `Tokenizable` when `tokenize` is set.
- A registration that lets `TideConfig::auto_models()` and `models_matching(..)` find the model (see [Schema Sync and Export](schema.md)).

## Reserved Names

`params` is reserved for presenter payloads: when a model's serialized `params` value is an object or an array, `to_hash_map()` leaves it out. Use another name for structured data you need in `to_hash_map()` output.

## Compile-Time Checks

Mistakes in a model's declaration are compile errors at the model, field or attribute:

- a generic struct, or a struct with no primary key;
- an `Option` primary key, `auto_increment` on a non-integer, non-key or composite key, or `tokenize` without exactly one key;
- `deleted_at_column` without `soft_delete`, or a soft-delete field whose type is not `Option<DateTime<Utc>>`;
- a `hidden`, index or `searchable` name the model does not declare, or one that is ambiguous;
- an encrypted primary key, or an index or `searchable` entry over an encrypted field;
- a relation option on a field whose type is not the matching wrapper, a wrapper inside `Option`, `Box`, `Rc` or `Arc`, or a relation missing its `foreign_key`;
- a `#[validate(..)]` rule on a relation field, or an unknown rule;
- a field type TideORM cannot store.

Rules from your project's `tideorm.toml` add checks of their own; see [Project Rules](project-rules.md).

## Choosing the Database a Call Uses

Model methods take no connection argument. They run on the database the surrounding code chooses:

| You want | Use |
|---|---|
| The application's database | nothing: `User::find(id)`, `User::query()`, `user.save()` |
| One read from another database, such as a replica | `User::query_with(&replica)`, `User::find_with(id, &replica)`; eager loads of that query read there too |
| Every call in a piece of code on another database, such as a tenant's | `tenant_db.scope(async { .. })` |
| Several writes that succeed or fail together | `db.transaction(..)` or `db.atomically(..)`, see [Transactions](transactions.md) |

A call that names no database uses the innermost scope around it (a transaction is one), else the global database. A call that names one (`query_with`, `find_with`) joins the innermost enclosing transaction opened on that database's pool, and otherwise runs on that database's own connection, even inside another database's transaction: a replica read inside a primary's transaction goes to the replica, outside the transaction. Model writes (`create`, `save`, `update`, `delete`) name no database, so run them inside a `scope` or a transaction of the database they belong to.

That lets one service function serve every database, with the caller choosing where it runs:

```rust
use tideorm::prelude::*;

async fn rename(id: i64, name: String) -> tideorm::Result<User> {
    let mut user = User::find_or_fail(id).await?;
    user.name = name;
    user.update().await
}

async fn examples(tenant_db: &Database, primary: &Database, replica: &Database) -> tideorm::Result<()> {
    let user = tenant_db.scope(rename(7, "Ada".into())).await?;    // the tenant's database
    let user = primary.atomically(rename(7, "Ada".into())).await?; // in a transaction
    let total = Order::query_with(replica).count().await?;          // a replica read
    Ok(())
}
```

`scope` runs every call inside it on that database without opening a transaction:

```rust
use tideorm::prelude::*;

async fn sign_up(tenant_db: &Database, user: User) -> tideorm::Result<User> {
    tenant_db
        .scope(async {
            let user = User::create(user).await?;
            Profile::create(Profile { user_id: user.id, ..Default::default() }).await?;
            Ok(user)
        })
        .await
}
```

`Database::current()` returns the database a call naming none would use here. [Databases](databases.md) covers connecting to several databases.
