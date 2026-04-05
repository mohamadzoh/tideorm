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

User-defined `#[derive(...)]` attributes are preserved. TideORM only adds the generated derives that are still missing unless you opt out with the `skip_*` attributes.

### Reserved Attribute Names

`params` is reserved for presenter payloads.

When TideORM builds `to_hash_map()` output and the serialized `params` value is
an object or array, it is omitted from the resulting map. Avoid using `params`
for presenter-facing structured model attributes if you need that data to appear
in `to_hash_map()` output.

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

Use these either inline in `#[tideorm::model(...)]` or in a separate `#[tideorm(...)]` attribute.

| Attribute | Description |
|-----------|-------------|
| `#[tideorm(table = "name")]` | Custom table name |
| `#[tideorm(skip_derives)]` | Skip auto-generated Debug, Clone, Default, Serialize, Deserialize |
| `#[tideorm(skip_debug)]` | Skip auto-generated Debug impl only |
| `#[tideorm(skip_clone)]` | Skip auto-generated Clone impl only |
| `#[tideorm(skip_default)]` | Skip auto-generated Default impl only |
| `#[tideorm(skip_serialize)]` | Skip auto-generated Serialize impl only |
| `#[tideorm(skip_deserialize)]` | Skip auto-generated Deserialize impl only |
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
| `#[tideorm(default = "value")]` | Default value |
| `#[tideorm(skip)]` | Skip field in queries |

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

```rust
let mut user = User::find(1).await?.unwrap();
user.name = "Jane Doe".to_string();

assert_eq!(user.changed_fields()?, vec!["name"]);
assert_eq!(
    user.original_value("name")?,
    Some(serde_json::json!("John Doe"))
);
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

---

## Schema Synchronization (Development Only)

TideORM can automatically sync your database schema with your models during development:

```rust
TideConfig::init()
    .database("postgres://localhost/mydb")
    .sync(true)  // Enable auto-sync (development only!)
    .connect()
    .await?;
```

Or export schema to a file:

```rust
TideConfig::init()
    .database("postgres://localhost/mydb")
    .schema_file("schema.sql")  // Generate SQL file
    .connect()
    .await?;
```

> ⚠️ **Warning**: Do NOT use `sync(true)` in production! Use proper migrations instead.

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
User::transaction(|tx| async move {
    // All operations in here are in a single transaction
    let user = User::create(User { ... }).await?;
    let profile = Profile::create(Profile { user_id: user.id, ... }).await?;
    
    // Return Ok to commit, Err to rollback
    Ok((user, profile))
}).await?;

// Database-level transactions
db.transaction(|tx| async move {
    // ... operations ...
    Ok(result)
}).await?;
```

If the closure returns `Ok`, the transaction is committed.
If it returns `Err` or panics, the transaction is rolled back.

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
let post = post.save().await?;
// created_at and updated_at are now set to the current time

post.title = "Updated Title".into();
let post = post.update().await?;
// updated_at is refreshed, created_at remains unchanged
```

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
            return Err(tideorm::Error::validation("Cannot delete admin account"));
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

// Bulk update with conditions
let affected = User::update_all()
    .set("active", false)
    .set("updated_at", Utc::now())
    .where_eq("last_login_before", "2024-01-01")
    .execute()
    .await?;
```

---

## Model Validation

TideORM includes built-in validation rules and validation helpers for model data.

### Built-in Validation Rules

```rust
use tideorm::validation::{ValidationRule, Validator, ValidationBuilder};

// Available validation rules
ValidationRule::Required           // Field must not be empty
ValidationRule::Email              // Valid email format
ValidationRule::Url                // Valid URL format
ValidationRule::MinLength(n)       // Minimum string length
ValidationRule::MaxLength(n)       // Maximum string length
ValidationRule::Min(n)             // Minimum numeric value
ValidationRule::Max(n)             // Maximum numeric value
ValidationRule::Range(min, max)    // Numeric range
ValidationRule::Regex(pattern)     // Custom regex pattern
ValidationRule::Alpha              // Only alphabetic characters
ValidationRule::Alphanumeric       // Only alphanumeric characters
ValidationRule::Numeric            // Only numeric characters
ValidationRule::Uuid               // Valid UUID format
ValidationRule::In(values)         // Value must be in list
ValidationRule::NotIn(values)      // Value must not be in list
```

### Using the Validator

```rust
use tideorm::validation::{Validator, ValidationRule};
use std::collections::HashMap;

// Create a validator with rules
let validator = Validator::new()
    .field("email", vec![ValidationRule::Required, ValidationRule::Email])
    .field("username", vec![
        ValidationRule::Required,
        ValidationRule::MinLength(3),
        ValidationRule::MaxLength(20),
        ValidationRule::Alphanumeric,
    ])
    .field("age", vec![ValidationRule::Range(18.0, 120.0)]);

// Validate data
let mut data = HashMap::new();
data.insert("email".to_string(), "user@example.com".to_string());
data.insert("username".to_string(), "johndoe123".to_string());
data.insert("age".to_string(), "25".to_string());

match validator.validate_map(&data) {
    Ok(_) => println!("Validation passed!"),
    Err(errors) => {
        for (field, message) in errors.errors() {
            println!("{}: {}", field, message);
        }
    }
}
```

### ValidationBuilder with Custom Rules

```rust
use tideorm::validation::ValidationBuilder;

let validator = ValidationBuilder::new()
    .add("email", ValidationRule::Required)
    .add("email", ValidationRule::Email)
    .add("username", ValidationRule::Required)
    .add("username", ValidationRule::MinLength(3))
    // Add custom validation logic
    .custom("username", |value| {
        let reserved = ["admin", "root", "system"];
        if reserved.contains(&value.to_lowercase().as_str()) {
            Err(format!("Username '{}' is reserved", value))
        } else {
            Ok(())
        }
    })
    .build();
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

Tokens are URL-safe and perfect for REST APIs:

```rust
// In your API handler
async fn get_user(token: String) -> Result<Json<User>> {
    let user = User::from_token(&token).await?;
    Ok(Json(user))
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
User::query().where_in(User::columns.role, vec!["admin", "mod"]) // IN
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

// Aggregations with typed columns:
let total = Order::query().sum(Order::columns.amount).await?;
let average = Product::query().avg(Product::columns.price).await?;
let max_age = User::query().max(User::columns.age).await?;

// OR conditions with typed columns:
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
| **OR WHERE** | `or_where_eq`, `or_where_not`, `or_where_gt`, `or_where_gte`, `or_where_lt`, `or_where_lte`, `or_where_like`, `or_where_in`, `or_where_not_in`, `or_where_null`, `or_where_not_null`, `or_where_between` |
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

`save_with_many` batches related inserts through TideORM's bulk insert path, and `delete_with_many` removes related rows with a single `WHERE IN` delete. `update_with_many` batches existing related rows through an upsert-style write and then reloads them once. If any related model still looks new, `update_with_many` falls back to per-row updates so create-vs-update semantics stay unchanged.

`NestedSaveBuilder` is `Send`, so you can hold it across await points or move it into task executors such as `tokio::spawn` before calling `.save()`.

### Join Result Consolidation

Transform flat JOIN results into nested structures:

```rust
use tideorm::prelude::JoinResultConsolidator;

// Flat JOIN results: Vec<(Order, LineItem)>
let flat = Order::query()
    .find_also_related::<LineItem>()
    .get()
    .await?;
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

Select specific columns from related tables with automatic JOINs:

```rust
// Select specific columns from both tables
let results = User::query()
    .select_with_linked::<Profile>(
        &["id", "name"],           // Local columns
        &["bio", "avatar_url"],    // Linked columns
        "user_id"                  // Foreign key for join
    )
    .get::<(i64, String, String, Option<String>)>()
    .await?;

// All local columns + specific linked columns
let results = User::query()
    .select_also_linked::<Profile>(
        &["bio"],                  // Just the linked columns
        "user_id"
    )
    .get::<(User, String)>()
    .await?;
```

### Additional Advanced Features

```rust
// has_related() - EXISTS subqueries
let cakes = Cake::query()
    .has_related("fruits", "cake_id", "id", "name", "Mango")
    .get().await?;

// eq_any() / ne_all() - PostgreSQL array optimizations
let users = User::query()
    .eq_any("id", vec![1, 2, 3, 4, 5])    // "id" = ANY(ARRAY[...])
    .ne_all("role", vec!["banned"])        // "role" <> ALL(ARRAY[...])
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

