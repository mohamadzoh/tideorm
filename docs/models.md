# Models

## Model Definition

### Default Behavior (Recommended)

Define most models with `#[tideorm::model(table = "...")]`:

```rust
#[tideorm::model(table = "products")]
pub struct Product {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub price: f64,
}
```

The `#[tideorm::model]` macro automatically implements:
- `Debug` - for printing/logging
- `Clone` - for cloning instances  
- `Default` - for creating default instances
- `Serialize` - for JSON serialization
- `Deserialize` - for JSON deserialization

You can derive any of these yourself, and TideORM then skips its own, as long as your derive is a separate `#[derive(...)]` attribute **below** `#[tideorm::model(...)]` (or below `#[derive(Model)]`). A derive above it, or in the same list as `Model` (`#[derive(Model, Debug)]`), is never shown to TideORM, and the two implementations fail with "conflicting implementations" (E0119): move it below, or opt out of TideORM's with the `skip_*` attributes.

```rust
#[tideorm::model(table = "users")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub display_name: String, // serialized as "displayName"
}
```

Your derive then decides the JSON: `to_json()` and hidden attributes follow its renames, but relation fields serialize as `null` until loaded, and a body that leaves out a relation or a managed timestamp only deserializes if the field is `#[serde(default)]`.

### Reserved Attribute Names

`params` is reserved.

When TideORM builds `to_hash_map()` output and the serialized `params` value is
an object or array, it is omitted from the resulting map. Avoid using `params`
for structured model attributes if you need that data to appear in
`to_hash_map()` output.

### Custom Implementations (When Needed)

If you need full control over generated derives, use `skip_derives` and provide your own:

```rust
#[tideorm::model(table = "products", skip_derives)]
#[index("category")]
#[index("active")]
#[index(name = "idx_price_category", columns = "price,category")]
#[unique_index("sku")]
pub struct Product {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    
    pub name: String,
    pub sku: String,
    pub category: String,
    pub price: i64,
    
    #[tideorm(nullable)]
    pub description: Option<String>,
    
    pub active: bool,
}

// Provide your own implementations
impl Debug for Product { /* custom impl */ }
impl Clone for Product { /* custom impl */ }
```

### Model Attributes

#### Struct-Level Attributes

Use these inline in `#[tideorm::model(...)]`, or in a `#[tideorm(...)]` attribute on a `#[derive(Model)]` struct — not both on one struct.

| Attribute | Description |
|-----------|-------------|
| `#[tideorm(table = "name")]` | Custom table name |
| `#[tideorm(schema = "name")]` | The schema the table lives in — a database, on MySQL. Every statement names the table as `schema.table`, and joins accept `"schema.table"` too |
| `#[tideorm(soft_delete)]` | Soft deletes through a `deleted_at` column (see [Soft Deletes](#soft-deletes)) |
| `#[tideorm(deleted_at_column = "name")]` | The soft-delete column, when it is not `deleted_at` |
| `#[tideorm(timestamps)]` | Mark the model as timestamped; `created_at`/`updated_at` fields are managed either way |
| `#[tideorm(hidden = "field_a,field_b")]` | Leave these fields out of `to_json()`. Plain `serde` serialization and `query().get_json()` still include them |
| `#[tideorm(tokenize)]` | Enable [record tokenization](#record-tokenization) |
| `#[tideorm(translatable = "..", languages = "..", fallback_language = "..")]` | Translated fields; requires the `translations` feature and a manual `HasTranslations` impl (see [Relations](relations.md)) |
| `#[tideorm(has_one_files = "..", has_many_files = "..")]` | File attachment slots; requires the `attachments` feature and a manual `HasAttachments` impl |
| `#[tideorm(searchable = "..")]` | Columns for full-text search |
| `#[tideorm(skip_derives)]` | Skip auto-generated Debug, Clone, Default, Serialize, Deserialize |
| `#[tideorm(skip_debug)]` | Skip auto-generated Debug impl only |
| `#[tideorm(skip_clone)]` | Skip auto-generated Clone impl only |
| `#[tideorm(skip_default)]` | Skip auto-generated Default impl only |
| `#[tideorm(skip_serialize)]` | Skip auto-generated Serialize impl only |
| `#[tideorm(skip_deserialize)]` | Skip auto-generated Deserialize impl only |
| `#[tideorm(encrypted = "field_a,field_b")]` | Encrypt selected persisted string columns on write and decrypt them on load. Requires the `encrypted-fields` feature. |
| `#[index("col")]` | Create an index |
| `#[unique_index("col")]` | Create a unique index |
| `#[index(name = "idx", columns = "a,b")]` | Named composite index |

#### Field-Level Attributes

| Attribute | Description |
|-----------|-------------|
| `#[tideorm(primary_key)]` | Mark as primary key |
| `#[tideorm(auto_increment)]` | Auto-increment field for a single-column primary key |
| `#[tideorm(nullable)]` | Optional/nullable field |
| `#[tideorm(column = "name")]` | Custom column name |
| `#[tideorm(default = "value")]` | The column's `DEFAULT` in the tables schema sync creates. Inserts send every field's value, so a model built with `..Default::default()` stores the field's Rust default, not this one |
| `#[tideorm(skip)]` | Not a column: never read or written, so it holds its `Default` on every model a query or write returns, and deserializing may leave it out |

A `Uuid` primary key still at the nil UUID when the row is inserted gets a random (v4) key, whether through `save()`, `create()`, `insert_all()` or an upsert. Set the key yourself, in `before_create` for instance, to use another UUID version.

---

### Encrypted Fields

Enable the `encrypted-fields` Cargo feature before using `encrypted = "..."`.

```toml
[dependencies]
tideorm = { version = "0.12.0", features = ["postgres", "encrypted-fields"] }
```

Use `encrypted = "..."` on the model when specific persisted string columns should be stored encrypted in the database but remain plain strings in your Rust model.

```rust
#[tideorm::model(
    table = "customers",
    encrypted = "customer_phone_number, backup_phone"
)]
pub struct Customer {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    #[tideorm(column = "customer_phone_number")]
    pub phone_number: String,
    pub backup_phone: Option<String>,
}
```

Encrypted field behavior:

- TideORM encrypts these fields before `create()`, `save()`, `update()`, `insert_all()`, nested saves, and batch `update_all().set(...)` writes.
- TideORM decrypts them automatically on `find()`, query-builder reads, eager loads, and raw model hydration.
- This feature is separate from `#[tideorm(tokenize)]`. It uses the same configured encryption key, but it protects regular columns rather than primary-key tokens.
- Each encrypted field uses a scoped key derived from the configured encryption secret plus the model table and column name, so ciphertext from one attribute does not decrypt under another attribute's context.
- Encrypted columns must contain TideORM encrypted payloads or `NULL`. Plaintext legacy rows and older global-scope payloads are rejected on load; migrate that data explicitly before enabling the feature.
- Query predicates are not rewritten yet. Filters such as `where_eq("customer_phone_number", "...")` still compare against the stored database value, so plaintext lookups on encrypted columns are not currently transparent.
- Configure the encryption key once during startup with `TideConfig::init().encryption_key("...")` or `TokenConfig::set_encryption_key("...")`.
- Supported encrypted field types are `String`, `Text`, `Option<String>`, and `Option<Text>`.

---

### Composite Primary Keys

TideORM supports composite primary keys by declaring `#[tideorm(primary_key)]` on multiple fields:

```rust
#[tideorm::model(table = "user_roles")]
pub struct UserRole {
    #[tideorm(primary_key)]
    pub user_id: i64,
    #[tideorm(primary_key)]
    pub role_id: i64,
    pub granted_by: String,
}

let role = UserRole::find((1_i64, 2_i64)).await?;
```

Composite primary key notes:

- CRUD methods use tuples in the same order as the key fields are declared.
- `#[tideorm(auto_increment)]` only works with a single primary key field.
- `#[tideorm(tokenize)]` requires exactly one primary key field.
- When defining relations on a composite-key model, set `local_key = "..."` explicitly if the relation would otherwise rely on the implicit `id` key.


---

## CRUD Operations

### Create

```rust
let user = User {
    email: "john@example.com".to_string(),
    name: "John Doe".to_string(),
    active: true,
    ..Default::default()
};
let user = user.save().await?;
println!("Created user with id: {}", user.id);
```

For auto-increment primary keys, TideORM treats `0` as an unsaved record marker internally. You usually do not need to assign it yourself when constructing a new model. Natural keys, composite keys, and non-auto-increment primary keys are considered persisted unless the primary key value is empty.

### Read

```rust
// Get all
let users = User::all().await?;

// Find by primary key
let user = User::find(1).await?;  // Option<User>

// Composite primary key example
let membership = UserRole::find((1_i64, 2_i64)).await?;

// Query builder (see above)
let users = User::query().where_eq("active", true).get().await?;
```

### Update

```rust
let mut user = User::find(1).await?.unwrap();
user.name = "Jane Doe".to_string();
let user = user.update().await?;
```

### Dirty Tracking

Requires the `dirty-tracking` feature.

Persisted models loaded or saved through TideORM keep a baseline of their last known persisted column values. Use `changed_fields()` to inspect which persisted fields differ from that baseline, and `original_value()` to inspect the previous value before saving.

Both return an outer `Option` that distinguishes **"no baseline is known"** (`None`) from **"a baseline exists and nothing changed"** (`Some(vec![])`). Treat `None` as unknown rather than unchanged — otherwise a model with no snapshot silently skips its write.

```rust
let mut user = User::find(1).await?.unwrap();
user.name = "Jane Doe".to_string();

assert_eq!(user.changed_fields()?, Some(vec!["name"]));

// The inner `Option` is the column's own value, which may itself be NULL.
assert_eq!(
    user.original_value("name")?,
    Some(Some(serde_json::json!("John Doe")))
);
```

The usual gate reads:

```rust
match user.changed_fields()? {
    // Nothing to compare against: save rather than guess.
    None => user.save().await?,
    Some(changed) if !changed.is_empty() => user.save().await?,
    Some(_) => user,
};
```

`changed_fields()` only reports persisted model fields, not runtime relation wrappers. The tracked baseline is refreshed by TideORM loads such as `find()`, query results, `reload()`, `save()`, and `update()`. Bulk mutation helpers such as `update_all()` and query-builder deletes invalidate the baseline for that model type.

Because TideORM models are plain Rust structs without hidden instance-local tracking state, dirty tracking follows the latest persisted snapshot TideORM knows for a primary key. If you keep multiple in-memory copies of the same row and one of them saves first, reload the stale copies before relying on their original values.

### Delete

```rust
// Delete instance
let user = User::find(1).await?.unwrap();
user.delete().await?;

// Delete by ID
User::destroy(1).await?;

// Bulk delete
User::query()
    .where_eq("active", false)
    .delete()
    .await?;
```

On a soft-delete model `delete()`, `destroy()` and `query().delete()` mark the row deleted instead of removing it; `force_delete()` removes it for good.

---

## Schema Synchronization (Development Only)

TideORM can automatically sync your database schema with your models during development:

```rust
TideConfig::init()
    .database("postgres://localhost/mydb")
    .models_matching("src/models/*.model.rs")
    .sync(true)  // Enable auto-sync (development only!)
    .connect()
    .await?;
```

`models_matching(...)` filters compiled `#[tideorm::model]` types by their source file path, so patterns like `src/models/*`, `src/models/*.model.rs`, and `src/models/**/*.rs` work as long as those modules are still included through normal Rust `mod` declarations.

Sync also creates the indexes a model declares with `#[index]` and `#[unique_index]`. On an existing table it adds the ones that are missing, and a failure there is logged as a warning rather than stopping startup.

Or export schema to a file:

```rust
TideConfig::init()
    .database("postgres://localhost/mydb")
    .schema_file("schema.sql")  // Generate SQL file
    .connect()
    .await?;
```

> ⚠️ **Warning**: Do NOT use `sync(true)` in production! Use proper migrations instead.

`force_sync(true)` goes further: on every connect it drops and recreates the table of each registered model, deleting all of its rows. It is for throwaway development and test databases only.

---

## Soft Deletes

TideORM supports soft deletes for models that have a `deleted_at` column:

```rust
#[tideorm::model(table = "posts", soft_delete)]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub deleted_at: Option<DateTime<Utc>>,
}
```

The `SoftDelete` impl is generated automatically. If your field or column uses a
different name, declare it on the model:

```rust
#[tideorm::model(table = "posts", soft_delete, deleted_at_column = "archived_on")]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub archived_on: Option<DateTime<Utc>>,
}
```

### Querying Soft-Deleted Records

```rust
// By default, soft-deleted records are excluded
let active_posts = Post::query().get().await?;

// Include soft-deleted records
let all_posts = Post::query()
    .with_trashed()
    .get()
    .await?;

// Only get soft-deleted records (trash bin)
let trashed_posts = Post::query()
    .only_trashed()
    .get()
    .await?;
```

The scope applies everywhere: `find(id)`, `exists(id)`, every query terminal, `update_all()`, relation loads and full-text search leave trashed rows out. To reach them:

- `Post::query().with_trashed().find(id)` reads a trashed row by key; `reload()` refreshes the record in hand even when it is trashed.
- `Post::update_all().with_trashed()` updates the trash too; `Post::query().only_trashed().restore()` restores it.

Deleting marks: `delete()`, `destroy(id)`, `query().delete()` and `delete_all()` set `deleted_at` instead of removing the row. `force_delete()` — on a model, or on a query such as `only_trashed().force_delete()`, which empties the trash — removes rows for good. Deleting `only_trashed()` rows with `delete()` is an error, since they are deleted already.

### Soft Delete Operations

```rust
use tideorm::SoftDelete;

// Soft delete (sets deleted_at to now)
let post = post.soft_delete().await?;

// Restore a soft-deleted record
let post = post.restore().await?;

// Permanently delete
post.force_delete().await?;
```

---

## Scopes (Reusable Query Fragments)

For model-local, chainable scopes, declare a dedicated scope impl block with `#[tideorm::scopes]`:

```rust
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

let users = User::query()
    .active()
    .verified()
    .role("admin")
    .get()
    .await?;
```

If you call a model's named scopes from a different module than the `#[tideorm::scopes]` block, bring the generated extension trait into scope first:

```rust
use crate::models::UserQueryScopes as _;
```

The lower-level `.scope(...)` helper still works when you want ad hoc reusable fragments that are not tied to one model type:

```rust
// Define scope functions
fn active<M: Model>(q: QueryBuilder<M>) -> QueryBuilder<M> {
    q.where_eq("active", true)
}

fn recent<M: Model>(q: QueryBuilder<M>) -> QueryBuilder<M> {
    q.order_desc("created_at").limit(10)
}

// Apply scopes
let users = User::query()
    .scope(active)
    .scope(recent)
    .get()
    .await?;
```

### Conditional Scopes

```rust
// Apply scope conditionally
let include_inactive = false;
let users = User::query()
    .when(include_inactive, |q| q.with_trashed())
    .get()
    .await?;

// Apply scope based on Option value
let status_filter: Option<&str> = Some("active");
let users = User::query()
    .when_some(status_filter, |q, status| q.where_eq("status", status))
    .get()
    .await?;
```

---

## Transactions

TideORM provides clean transaction support:

```rust
// Model-centric transactions
User::transaction(|tx| Box::pin(async move {
    // All operations in here are in a single transaction
    let user = User::create(User { ... }).await?;
    let profile = Profile::create(Profile { user_id: user.id, ... }).await?;
    
    // Return Ok to commit, Err to rollback
    Ok((user, profile))
})).await?;

// Database-level transactions
db.transaction(|tx| Box::pin(async move {
    // ... operations ...
    Ok(result)
})).await?;
```

If the closure returns `Ok`, the transaction is committed.
If it returns `Err` or panics, the transaction is rolled back.

### Concurrent Updates

A transaction does not stop two requests from reading the same row at the same time. `update()` writes every column of the model it was given, so in a read-check-write the last writer wins and silently undoes the other: two orders of 3 against a stock of 4 both pass the check, and both ship.

Lock the rows you are about to change with `lock_for_update()` (`SELECT ... FOR UPDATE`). A second transaction that locks the same row waits until the first commits, then reads what it wrote:

```rust
let shipped = Item::transaction(|_tx| Box::pin(async move {
    let mut item = Item::query()
        .where_eq("id", id)
        .lock_for_update()
        .first_or_fail()
        .await?;
    if item.stock < quantity {
        return Ok(false);
    }
    item.stock -= quantity;
    item.update().await?;
    Ok(true)
})).await?;
```

For a plain counter, a single conditional statement needs no lock: `Item::update_all().decrement("stock", quantity).where_eq("id", id).where_gte("stock", quantity).execute()` returns `0` when the stock ran out.

SQLite has no row locks, so `lock_for_update()` adds nothing there: the first write of a transaction locks the whole database, and the second of two competing transactions fails with a retryable `LockNotAvailable` error instead.

---

## Auto-Timestamps

TideORM automatically manages `created_at` and `updated_at` fields:

```rust
#[tideorm::model(table = "posts")]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub content: String,
    pub created_at: DateTime<Utc>,  // Auto-set on save()
    pub updated_at: DateTime<Utc>,  // Auto-set on save() and update()
}

// No need to set timestamps manually
let post = Post {
    title: "Hello".into(),
    content: "World".into(),
    ..Default::default()
};
let mut post = post.save().await?;
// created_at and updated_at are now set to the current time

post.title = "Updated Title".into();
let post = post.update().await?;
// updated_at is refreshed, created_at remains unchanged
```

A field named — or mapped with `column = ".."` to — `created_at` or `updated_at` is managed when it holds a `DateTime<Utc>` or a `NaiveDateTime` (set to the current UTC time, matching `timestamps_naive()` columns), optional or not. The value you set on such a field is replaced; a field of any other type keeps it.

Every write keeps the creation time: `update()` never writes `created_at`, and an upsert that finds the row already there refreshes `updated_at` but leaves `created_at` alone unless you name it in `update_columns`. A request body can leave both fields out, so a model deserialized from `{"email": ".."}` saves with fresh timestamps, and a body that sends `created_at` cannot rewrite it. `update_all()` writes only what it is told to, so set `updated_at` there yourself if you need it.

---

## Callbacks / Hooks

Implement lifecycle callbacks for your models:

```rust
use tideorm::callbacks::Callbacks;

#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub password_hash: String,
}

impl Callbacks for User {
    fn before_save(&mut self) -> tideorm::Result<()> {
        // Normalize email before saving
        self.email = self.email.to_lowercase().trim().to_string();
        Ok(())
    }
    
    fn after_create(&self) -> tideorm::Result<()> {
        println!("User {} created with id {}", self.email, self.id);
        // Could send welcome email, create audit log, etc.
        Ok(())
    }
    
    fn before_delete(&self) -> tideorm::Result<()> {
        // Prevent deletion of important accounts
        if self.email == "admin@example.com" {
            return Err(tideorm::Error::validation("email", "Cannot delete admin account"));
        }
        Ok(())
    }
}
```

### Available Callbacks

| Callback | When Called |
|----------|-------------|
| `before_validation` | Before validation runs |
| `after_validation` | After validation passes |
| `before_save` | Before create or update |
| `after_save` | After create or update |
| `before_create` | Before inserting new record |
| `after_create` | After inserting new record |
| `before_update` | Before updating existing record |
| `after_update` | After updating existing record |
| `before_delete` | Before deleting record |
| `after_delete` | After deleting record |

An `Err` from a `before_*` hook stops the operation before anything is written. An `after_*` hook runs once the statement has succeeded: its `Err` is returned from `save()`, `update()` or `delete()`, but the row is already written. Inside `Database::transaction`, propagating that error with `?` rolls the write back with the rest of the transaction; outside one, it stays. Callbacks run for single-model writes only — `insert_all()`, `update_all()`, upserts and `query().delete()` skip them.

---

## Batch Operations

For efficient bulk operations:

```rust
// Insert multiple records at once
let users = vec![
    User { name: "John".into(), email: "john@example.com".into(), ..Default::default() },
    User { name: "Jane".into(), email: "jane@example.com".into(), ..Default::default() },
    User { name: "Bob".into(), email: "bob@example.com".into(), ..Default::default() },
];
let inserted = User::insert_all(users).await?;
// Every model is validated before any is written; callbacks do not run.

// Bulk update with conditions
let affected = User::update_all()
    .set("active", false)
    .set("updated_at", Utc::now())
    .where_eq("last_login_before", "2024-01-01")
    .execute()
    .await?;

// Or update the rows a query already selects, scopes included
let affected = User::query()
    .inactive()
    .update_all()
    .set("status", "dormant")
    .execute()
    .await?;
```

Like a query, `update_all()` leaves soft-deleted rows out; `.with_trashed()` reaches them. `query().update_all()` keeps the query's own scope, filters and the database `query_with` names. A query that joins, groups, pages or unions cannot become an `UPDATE`, and running one fails.

---

## Model Validation

Declare rules on fields with `#[validate(..)]`. `create()`, `update()`, `save()`, `insert_all()` and upserts check every rule before anything reaches the database and fail with a validation error — for `insert_all()`, naming the failing model's position in the batch. Call `validate()` yourself to collect all failures at once:

```rust
use tideorm::validation::Validate;

#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    #[validate(required, email)]
    pub email: String,
    #[validate(min_length = 3, max_length = 20, alphanumeric)]
    pub username: String,
    #[validate(range(18, 120))]
    pub age: i32,
}

let user = User {
    email: "not-an-email".into(),
    username: "x".into(),
    age: 12,
    ..Default::default()
};

if let Err(errors) = user.validate() {
    for (field, message) in errors.errors() {
        println!("{}: {}", field, message);
    }
}
```

Supported rules: `required`, `email`, `url`, `alpha`, `alphanumeric`, `numeric`, `uuid`, `min_length = n`, `max_length = n`, `length = n`, `min = n`, `max = n`, `range(min, max)` (or `range = "min..max"`), and `regex = "pattern"`. Lengths count characters, not bytes. `min`, `max` and `range` reject `NaN`. A `regex` pattern that does not compile fails validation for every value, with an error naming the pattern.

Rules on a `#[tideorm(skip)]` field run too, for a value that is checked but never stored, such as a password confirmation. A skip field holds its `Default` on every model a query returns, so make it an `Option`: `None` passes every rule except `required`. `#[validate]` on a relation field is a compile error; the rules belong on the related model's fields.

### Applying Rules by Hand

`Validator::validate_rule` checks one value against one `ValidationRule` and returns the failure message, and `ValidationBuilder::new(field)` collects the rules for one field. `ValidationRule` also has `In` and `NotIn` for allow/deny lists:

```rust
use tideorm::validation::{ValidationBuilder, ValidationRule, Validator};

let email = "user@example.com".to_string();
assert!(Validator::validate_rule(&email, &ValidationRule::Email, "email").is_none());

let (field, rules) = ValidationBuilder::new("username")
    .required()
    .min_length(3)
    .max_length(20)
    .alphanumeric()
    .build();

let username = "ab".to_string();
for rule in &rules {
    if let Some(message) = Validator::validate_rule(&username, rule, &field) {
        println!("{}", message);
    }
}
```

### Handling Validation Errors

```rust
use tideorm::validation::ValidationErrors;

let mut errors = ValidationErrors::new();
errors.add("email", "Email is required");
errors.add("email", "Email format is invalid");
errors.add("password", "Password must be at least 8 characters");

// Check if there are errors
if errors.has_errors() {
    // Get all errors for a specific field
    let email_errors = errors.field_errors("email");
    for msg in email_errors {
        println!("Email error: {}", msg);
    }
    
    // Display all errors
    println!("{}", errors);
}

// Convert to TideORM Error
let tide_error: tideorm::error::Error = errors.into();
```

---

## Record Tokenization

TideORM provides secure tokenization for record IDs, converting them to encrypted, URL-safe tokens. This prevents exposing internal database IDs in URLs and APIs.

### Tokenization Quick Start

Enable tokenization with the `#[tideorm(tokenize)]` attribute:

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "users", tokenize)]  // Enable tokenization
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
}

// Configure encryption key once at startup
TokenConfig::set_encryption_key("your-32-byte-secret-key-here-xx");

// Tokenize a record
let user = User::find(1).await?.unwrap();
let token = user.tokenize()?;  // "iIBmdKYhJh4_vSKFlBTP..."

// Decode token to the model's primary key type (doesn't hit database)
let id = User::detokenize(&token)?;  // 1

// Fetch record directly from token
let same_user = User::from_token(&token).await?;
assert_eq!(user.id, same_user.id);
```

If no encryption key is configured, tokenization now returns a configuration error instead of panicking at runtime. In most applications, the simplest setup is to provide the key through `TideConfig` during startup:

```rust
let encryption_key = std::env::var("ENCRYPTION_KEY")?;

TideConfig::init()
    .database("postgres://localhost/mydb")
    .encryption_key(&encryption_key)
    .connect()
    .await?;
```

### Tokenization Methods

When a model has `#[tideorm(tokenize)]`, these methods are available:

| Method | Description |
|--------|-------------|
| `user.tokenize()` | Convert record to token (instance method) |
| `user.to_token()` | Alias for `tokenize()` |
| `User::tokenize_id(42)` | Tokenize an ID without having the record |
| `User::detokenize(&token)` | Decode token to the model's primary key type |
| `User::decode_token(&token)` | Alias for `detokenize()` |
| `User::from_token(&token).await` | Decode token and fetch record from DB |
| `user.regenerate_token()` | Generate a fresh token; the default encoder uses a new random nonce each time |

### Model-Specific Tokens

Tokens are bound to their model type. A User token cannot decode a Product:

```rust
#[tideorm::model(table = "users", tokenize)]
pub struct User { /* ... */ }

#[tideorm::model(table = "products", tokenize)]
pub struct Product { /* ... */ }

// Same ID, different tokens
let user_token = User::tokenize_id(1)?;
let product_token = Product::tokenize_id(1)?;
assert_ne!(user_token, product_token);  // Different!

// Cross-model decoding fails
assert!(User::detokenize(&product_token).is_err());  // Error!
```

### Using Tokens in APIs

Tokens are URL-safe and perfect for REST APIs. The model itself still carries its primary key, so returning it as it is (`Json(user)`, or `to_json()` without hiding the key) puts the raw id back into the response. Hide the key and send the token in its place:

```rust
#[tideorm::model(table = "users", tokenize, hidden = "id")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
}

// In your API handler
async fn get_user(token: String) -> tideorm::Result<Json<serde_json::Value>> {
    let user = User::from_token(&token).await?;
    let mut body = user.to_json(None);          // `hidden` drops the raw id
    body["token"] = serde_json::json!(user.tokenize()?);
    Ok(Json(body))
}

// Example URLs:
// GET /api/users/iIBmdKYhJh4_vSKFlBTPgWRlbW8tZW5isZqLo_EU4YI
// GET /api/products/1NhY5XxAm_D53flvEc-5JmRlbW8tZW5iShKwXZjCb9s
```

### Custom Encoders

For custom tokenization logic, implement the `Tokenizable` trait manually:

```rust
use tideorm::tokenization::{Tokenizable, TokenEncoder, TokenDecoder};

#[tideorm::model(table = "documents")]
pub struct Document {
    #[tideorm(primary_key)]
    pub id: i64,
    pub title: String,
}

#[async_trait::async_trait]
impl Tokenizable for Document {
    type TokenPrimaryKey = i64;

    fn token_model_name() -> &'static str { "Document" }
    fn token_primary_key(&self) -> Self::TokenPrimaryKey { self.id }
    
    // Custom encoder - prefix with "DOC-"
    fn token_encoder() -> Option<TokenEncoder> {
        Some(|id, _model| Ok(format!("DOC-{}", id)))
    }
    
    // Custom decoder
    fn token_decoder() -> Option<TokenDecoder> {
        Some(|token, _model| {
            Ok(token.strip_prefix("DOC-").map(ToOwned::to_owned))
        })
    }
    
    async fn from_token(token: &str) -> tideorm::Result<Self> {
        let id = Self::decode_token(token)?;
        Self::find(id).await?.ok_or_else(|| 
            tideorm::Error::not_found("Document not found")
        )
    }
}
```

### Global Custom Encoder

Set a custom encoder for all models:

```rust
// Set global custom encoder
TokenConfig::set_encoder(|id, model| {
    Ok(format!("{}-{}", model.to_lowercase(), id))
});

TokenConfig::set_decoder(|token, model| {
    let prefix = format!("{}-", model.to_lowercase());
    Ok(token.strip_prefix(&prefix).map(ToOwned::to_owned))
});
```

Calling `TokenConfig::set_encryption_key`, `TokenConfig::set_encoder`, or `TokenConfig::set_decoder` again replaces the previous global override. Use `TokenConfig::reset()` to clear all tokenization overrides and return to the default encoder/decoder configuration.

### Tokenization Security

**Features:**
- **Authenticated encryption**: Default tokens use XChaCha20-Poly1305
- **Model binding**: Model name is authenticated as associated data, preventing cross-model reuse
- **Tamper detection**: Modified tokens fail authentication and are rejected
- **Randomized output**: The default encoder uses a fresh nonce, so the same record can produce different valid tokens
- **URL-safe**: Base64-URL encoding (A-Za-z0-9-_), no escaping needed

**Best Practices:**
- Use a high-entropy secret from the environment; 32+ characters is a good baseline
- Store keys in environment variables, never in code
- Changing the key invalidates all existing tokens
- If you override the encoder/decoder, you are responsible for preserving equivalent security guarantees
- Consider token rotation for high-security applications

```rust
// Configure from environment variable
TokenConfig::set_encryption_key(
    &std::env::var("ENCRYPTION_KEY").expect("ENCRYPTION_KEY must be set")
);
```

---


---

## Advanced ORM Features

TideORM includes a broad set of advanced model and query helpers through its own API surface:

### Strongly-Typed Columns

Compile-time type safety for column operations. The compiler catches type mismatches before runtime.

**Auto-Generated Columns**

When you define a model with `#[tideorm::model]`, typed columns are automatically generated as an attribute on the model:

```rust
#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub age: Option<i32>,
    pub active: bool,
}

// A `UserColumns` struct is automatically generated with typed column accessors.
// Access columns via `User::columns`:
// User::columns.id, User::columns.name, User::columns.age, User::columns.active
```

**Unified Type-Safe Queries**

All query methods accept BOTH string column names AND typed columns. Use `User::columns.field_name` for compile-time safety:

```rust
// SAME method works with both strings AND typed columns:
User::query().where_eq("name", "Alice")                    // String-based (runtime checked)
User::query().where_eq(User::columns.name, "Alice")        // Typed column (compile-time checked)

// Type-safe query - compiler catches typos!
let users = User::query()
    .where_eq(User::columns.name, "Alice")     // ✓ Type-safe
    .where_gt(User::columns.age, 18)           // ✓ Type-safe
    .where_eq(User::columns.active, true)      // ✓ Type-safe
    .get()
    .await?;

// All query methods support typed columns:
User::query().where_eq(User::columns.name, "Alice")              // =
User::query().where_not(User::columns.role, "admin")             // <>
User::query().where_gt(User::columns.age, 18)                    // >
User::query().where_gte(User::columns.age, 18)                   // >=
User::query().where_lt(User::columns.age, 65)                    // <
User::query().where_lte(User::columns.age, 65)                   // <=
User::query().where_like(User::columns.email, "%@test.com")      // LIKE
User::query().where_not_like(User::columns.email, "%spam%")      // NOT LIKE
User::query().where_in(User::columns.role, ["admin", "mod"])     // IN, from any list
User::query().where_not_in(User::columns.status, vec!["banned"]) // NOT IN
User::query().where_null(User::columns.deleted_at)               // IS NULL
User::query().where_not_null(User::columns.email)                // IS NOT NULL
User::query().where_between(User::columns.age, 18, 65)           // BETWEEN

// Ordering and grouping also support typed columns:
User::query()
    .order_by(User::columns.created_at, Order::Desc)
    .order_asc(User::columns.name)
    .group_by(User::columns.role)
    .get()
    .await?;

// Aggregations with typed columns, read as the type you ask for:
let total: Decimal = Order::query().sum(Order::columns.amount).await?;
let average: Option<f64> = Product::query().avg(Product::columns.price).await?;
let max_age: Option<i32> = User::query().max(User::columns.age).await?;

// OR conditions with typed columns. All or_where_* calls are combined into one
// OR group, ANDed with the rest of the query:
User::query()
    .or_where_eq(User::columns.role, "admin")
    .or_where_eq(User::columns.role, "moderator")
    .get()
    .await?;
```

**Why Use Typed Columns?**

- **Compile-time safety**: Wrong column names won't compile
- **IDE autocomplete**: `User::columns.` shows all available columns with their types
- **Refactoring-friendly**: Rename a field and the compiler tells you everywhere to update
- **No conflicts**: Columns are accessed via `.columns`, won't override other struct attributes
- **Backward compatible**: String column names still work for quick prototyping

**Manual Column Definitions (Advanced)**

If you need custom behavior or computed columns, you can define columns manually:

```rust
use tideorm::columns::Column;

// Custom columns that map to different DB column names
pub const FULL_NAME: Column<String> = Column::new("full_name");
pub const COMPUTED_FIELD: Column<i32> = Column::new("computed_field");

// Use in queries
User::query().where_eq(FULL_NAME, "John Doe").get().await?;
```

**Typed Column Support Summary:**

All these methods accept both `"column_name"` (string) and `Model::columns.field` (typed):

| Category | Methods |
|----------|---------|
| **WHERE** | `where_eq`, `where_not`, `where_gt`, `where_gte`, `where_lt`, `where_lte`, `where_like`, `where_not_like`, `where_in`, `where_not_in`, `where_null`, `where_not_null`, `where_between` |
| **OR WHERE** | `or_where_eq`, `or_where_not`, `or_where_gt`, `or_where_gte`, `or_where_lt`, `or_where_lte`, `or_where_like`, `or_where_not_like`, `or_where_in`, `or_where_not_in`, `or_where_null`, `or_where_not_null`, `or_where_between` |
| **ORDER BY** | `order_by`, `order_asc`, `order_desc` |
| **GROUP BY** | `group_by` |
| **Aggregations** | `sum`, `avg`, `min`, `max`, `count_distinct` |
| **HAVING** | `having_sum_gt`, `having_avg_gt` |
| **Window** | `partition_by`, `order_by` (in WindowFunctionBuilder) |

### Self-Referencing Relations

Support for hierarchical data like org charts, categories, or comment threads:

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

// Usage:
let emp = Employee::find(5).await?.unwrap();

let manager_rel = emp.manager.clone();
let reports_rel = emp.reports.clone();

// Load parent (manager)
let manager = manager_rel.load().await?;
let has_manager = manager_rel.exists().await?;

// Load children (direct reports)
let reports = reports_rel.load().await?;
let count = reports_rel.count().await?;

// Load entire subtree recursively in one recursive CTE query
let tree = reports_rel.load_tree(3).await?;  // 3 levels deep
```

`SelfRef` and `SelfRefMany` fields are wired automatically when you provide the self-referencing `foreign_key`. `local_key` defaults to `id` and can be overridden explicitly when needed.

`SelfRefMany::load_tree()` respects the configured `local_key` and fetches the
tree in one query, which avoids one SELECT per node on large hierarchies.

### Nested Save (Cascade Operations)

Save parent and related models together with automatic foreign key handling:

```rust
// Save parent with single related model
let (user, profile) = user.save_with_one(profile, "user_id").await?;
// profile.user_id is automatically set to user.id

// Save parent with multiple related models
let posts = vec![post1, post2, post3];
let (user, posts) = user.save_with_many(posts, "user_id").await?;
// All posts have user_id set to user.id

// Cascade updates
let (user, profile) = user.update_with_one(profile).await?;
let (user, posts) = user.update_with_many(posts).await?;

// Cascade delete (children first for referential integrity)
let deleted_count = user.delete_with_many(posts).await?;

// Builder API for complex nested saves
let (user, related_json) = NestedSaveBuilder::new(user)
    .with_one(profile, "user_id")
    .with_many(posts, "user_id")
    .with_many(comments, "author_id")
    .save()
    .await?;
```

Each nested operation runs in one transaction — a savepoint when you are already inside one — so a child that fails also rolls back the parent's write. Related rows are written one at a time through each model's own `create`, `update`, or `delete`, so their callbacks and validation run as usual. The foreign key may be given as the Rust field name or the database column name.

`NestedSaveBuilder` is `Send`, so you can hold it across await points or move it into task executors such as `tokio::spawn` before calling `.save()`.

### Join Result Consolidation

`JoinResultConsolidator` turns flat pairs — such as rows of a join read into `(Order, LineItem)` tuples — into nested structures:

```rust
use tideorm::prelude::JoinResultConsolidator;

// Flat pairs: Vec<(Order, LineItem)>
let flat: Vec<(Order, LineItem)> = orders_with_items;
// [(order1, item1), (order1, item2), (order2, item3)]

// Consolidate into nested: Vec<(Order, Vec<LineItem>)>
let nested = JoinResultConsolidator::consolidate_two(flat, |o| o.id);
// [(order1, [item1, item2]), (order2, [item3])]

// For LEFT JOINs with Option<B>
let nested = JoinResultConsolidator::consolidate_two_optional(flat, |o| o.id);

// Three-level nesting
let flat3: Vec<(Order, LineItem, Product)> = /* ... */;
let nested3 = JoinResultConsolidator::consolidate_three(flat3, |o| o.id, |i| i.id);
// Vec<(Order, Vec<(LineItem, Vec<Product>)>)>
```

### Linked Partial Select

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

A linked column that shares a name with a selected one must be aliased: two outputs cannot share a name.

### Additional Advanced Features

```rust
// where_has() - the related model's own query: its scope, types and any filter
let authors = User::query()
    .where_has::<Post>(Post::columns.user_id, User::columns.id, |posts| {
        posts.where_eq(Post::columns.published, true)
    })
    .get().await?;

// where_doesnt_have() - no related row passes the closure
let idle = User::query()
    .where_doesnt_have::<Post>(Post::columns.user_id, User::columns.id, |posts| posts)
    .get().await?;

// has_related() - EXISTS subqueries over a table, soft-deleted rows included
let cakes = Cake::query()
    .has_related("fruits", "cake_id", "id", "name", "Mango")
    .get().await?;

// has_no_related() - no related row matches; a cake with no fruits at all passes too
let cakes = Cake::query()
    .has_no_related("fruits", "cake_id", "id", "name", "Mango")
    .get().await?;

// where_exists() with a model query applies that model's soft-delete scope
let cakes = Cake::query()
    .where_exists(Fruit::query().where_raw("fruits.cake_id = cakes.id"))
    .get().await?;

// eq_any() / ne_all() - array membership, rendered as IN / NOT IN on every backend
let users = User::query()
    .eq_any("id", vec![1, 2, 3, 4, 5])    // "id" IN (1, 2, 3, 4, 5)
    .ne_all("role", vec!["banned"])        // "role" NOT IN ('banned')
    .get().await?;

// Unix timestamps
use tideorm::types::{UnixTimestamp, UnixTimestampMillis};
let ts = UnixTimestamp::now();
let dt = ts.to_datetime();

// Batch insert
let users: Vec<User> = User::insert_all(vec![u1, u2]).await?;

// consolidate() - Reusable query fragments
let active_scope = User::query()
    .where_eq("status", "active")
    .consolidate();
let admins = User::query().apply(&active_scope).where_eq("role", "admin").get().await?;

// Multi-column unique constraints (migrations)
builder.unique(&["user_id", "role_id"]);
builder.unique_named("uq_email_tenant", &["email", "tenant_id"]);

// CHECK constraints (migrations)
builder.string("email").check("email LIKE '%@%'");
```

---

