# TideORM Documentation

Complete documentation for TideORM - a developer-friendly ORM for Rust.

## Table of Contents

- [Configuration](#configuration)
  - [Basic Connection](#basic-connection)
  - [Pool Configuration](#pool-configuration)
  - [Database Types](#database-types)
- [Model Definition](#model-definition)
  - [Model Attributes](#model-attributes)
- [Query Builder](#query-builder)
  - [WHERE Conditions](#where-conditions)
  - [Ordering](#ordering)
  - [Pagination](#pagination)
  - [Execution Methods](#execution-methods)
  - [UNION Queries](#union-queries)
  - [Window Functions](#window-functions)
  - [Common Table Expressions (CTEs)](#common-table-expressions-ctes)
- [CRUD Operations](#crud-operations)
- [Schema Synchronization](#schema-synchronization-development-only)
- [Soft Deletes](#soft-deletes)
- [Scopes](#scopes-reusable-query-fragments)
- [Transactions](#transactions)
- [Auto-Timestamps](#auto-timestamps)
- [Callbacks / Hooks](#callbacks--hooks)
- [Batch Operations](#batch-operations)
- [File Attachments](#file-attachments)
- [Translations (i18n)](#translations-i18n)
- [Model Validation](#model-validation)
- [Full-Text Search](#full-text-search)
- [Multi-Database Support](#multi-database-support)
- [Raw SQL Queries](#raw-sql-queries)
- [Query Logging](#query-logging)
- [Error Handling](#error-handling)

---

## Configuration

### Basic Connection

```rust
// Simple connection
Database::init("postgres://localhost/mydb").await?;

// With TideConfig (recommended)
TideConfig::init()
    .database("postgres://localhost/mydb")
    .connect()
    .await?;
```

### Pool Configuration

```rust
TideConfig::init()
    .database("postgres://localhost/mydb")
    .max_connections(20)        // Maximum pool size
    .min_connections(5)         // Minimum idle connections
    .connect_timeout(Duration::from_secs(10))
    .idle_timeout(Duration::from_secs(300))
    .max_lifetime(Duration::from_secs(3600))
    .connect()
    .await?;
```

### Database Types

```rust
TideConfig::init()
    .database_type(DatabaseType::Postgres)  // or MySQL, SQLite
    .database("postgres://localhost/mydb")
    .connect()
    .await?;
```

---

## Model Definition

```rust
#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "products")]
#[index("category")]
#[index("active")]
#[index(name = "idx_price_category", columns = "price,category")]
#[unique_index("sku")]
pub struct Product {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    
    pub name: String,
    pub sku: String,
    pub category: String,
    pub price: i64,
    
    #[tide(nullable)]
    pub description: Option<String>,
    
    pub active: bool,
}
```

### Model Attributes

| Attribute | Description |
|-----------|-------------|
| `#[tide(table = "name")]` | Custom table name |
| `#[tide(primary_key)]` | Mark as primary key |
| `#[tide(auto_increment)]` | Auto-increment field |
| `#[tide(nullable)]` | Optional/nullable field |
| `#[index("col")]` | Create an index |
| `#[unique_index("col")]` | Create a unique index |
| `#[index(name = "idx", columns = "a,b")]` | Named composite index |

---

## Query Builder

TideORM provides a fluent query builder with all common operations.

### WHERE Conditions

```rust
// Equality
User::query().where_eq("status", "active")
User::query().where_not("role", "admin")

// Comparison
User::query().where_gt("age", 18)     // >
User::query().where_gte("age", 18)    // >=
User::query().where_lt("age", 65)     // <
User::query().where_lte("age", 65)    // <=

// Pattern matching
User::query().where_like("name", "%John%")
User::query().where_not_like("email", "%spam%")

// IN / NOT IN
User::query().where_in("role", vec!["admin", "moderator"])
User::query().where_not_in("status", vec!["banned", "suspended"])

// NULL checks
User::query().where_null("deleted_at")
User::query().where_not_null("email_verified_at")

// Range
User::query().where_between("age", 18, 65)

// Combine conditions (AND)
User::query()
    .where_eq("active", true)
    .where_gt("age", 18)
    .where_not_null("email")
    .get()
    .await?;
```

### Ordering

```rust
// Basic ordering
User::query()
    .order_by("created_at", Order::Desc)
    .order_by("name", Order::Asc)
    .get()
    .await?;

// Convenience methods
User::query().order_asc("name")        // ORDER BY name ASC
User::query().order_desc("created_at") // ORDER BY created_at DESC
User::query().latest()                 // ORDER BY created_at DESC
User::query().oldest()                 // ORDER BY created_at ASC
```

### Pagination

```rust
// Limit and offset
User::query()
    .limit(10)
    .offset(20)
    .get()
    .await?;

// Page-based pagination
User::query()
    .page(3, 25)  // Page 3, 25 per page
    .get()
    .await?;

// Aliases
User::query().take(10).skip(20)  // Same as limit(10).offset(20)
```

### Execution Methods

```rust
// Get all matching records
let users = User::query()
    .where_eq("active", true)
    .get()
    .await?;  // Vec<User>

// Get first record
let user = User::query()
    .where_eq("email", "admin@example.com")
    .first()
    .await?;  // Option<User>

// Get first or fail
let user = User::query()
    .where_eq("id", 1)
    .first_or_fail()
    .await?;  // Result<User>

// Count (efficient SQL COUNT)
let count = User::query()
    .where_eq("active", true)
    .count()
    .await?;  // u64

// Check existence
let exists = User::query()
    .where_eq("email", "admin@example.com")
    .exists()
    .await?;  // bool

// Bulk delete (efficient single DELETE statement)
let deleted = User::query()
    .where_eq("status", "inactive")
    .delete()
    .await?;  // u64 (rows affected)
```

### UNION Queries

Combine results from multiple queries:

```rust
// UNION - combines results and removes duplicates
let users = User::query()
    .where_eq("active", true)
    .union(User::query().where_eq("role", "admin"))
    .get()
    .await?;

// UNION ALL - includes all results (faster, keeps duplicates)
let orders = Order::query()
    .where_eq("status", "pending")
    .union_all(Order::query().where_eq("status", "processing"))
    .union_all(Order::query().where_eq("status", "shipped"))
    .order_by("created_at", Order::Desc)
    .get()
    .await?;

// Raw UNION for complex queries
let results = User::query()
    .union_raw("SELECT * FROM archived_users WHERE year = 2023")
    .get()
    .await?;
```

### Window Functions

Perform calculations across sets of rows:

```rust
use tideorm::prelude::*;

// ROW_NUMBER - assign sequential numbers
let products = Product::query()
    .row_number("row_num", Some("category"), "price", Order::Desc)
    .get_raw()
    .await?;
// SQL: ROW_NUMBER() OVER (PARTITION BY "category" ORDER BY "price" DESC) AS "row_num"

// RANK - rank with gaps for ties
let employees = Employee::query()
    .rank("salary_rank", Some("department_id"), "salary", Order::Desc)
    .get_raw()
    .await?;

// DENSE_RANK - rank without gaps
let students = Student::query()
    .dense_rank("score_rank", None, "score", Order::Desc)
    .get_raw()
    .await?;

// Running totals with SUM window
let sales = Sale::query()
    .running_sum("running_total", "amount", "date", Order::Asc)
    .get_raw()
    .await?;

// LAG - access previous row value
let orders = Order::query()
    .lag("prev_total", "total", 1, Some("0"), "user_id", "created_at", Order::Asc)
    .get_raw()
    .await?;

// LEAD - access next row value
let appointments = Appointment::query()
    .lead("next_date", "date", 1, None, "patient_id", "date", Order::Asc)
    .get_raw()
    .await?;

// NTILE - distribute into buckets
let products = Product::query()
    .ntile("price_quartile", 4, "price", Order::Asc)
    .get_raw()
    .await?;

// Custom window function with full control
let results = Order::query()
    .window(
        WindowFunction::new(WindowFunctionType::Sum("amount".to_string()), "total_sales")
            .partition_by("region")
            .order_by("month", Order::Asc)
            .frame(FrameType::Rows, FrameBound::UnboundedPreceding, FrameBound::CurrentRow)
    )
    .get_raw()
    .await?;
```

### Common Table Expressions (CTEs)

Define temporary named result sets:

```rust
use tideorm::prelude::*;

// Simple CTE
let orders = Order::query()
    .with_cte(CTE::new(
        "high_value_orders",
        "SELECT * FROM orders WHERE total > 1000".to_string()
    ))
    .where_raw("id IN (SELECT id FROM high_value_orders)")
    .get()
    .await?;

// CTE from another query builder
let active_users = User::query()
    .where_eq("active", true)
    .select(vec!["id", "name", "email"]);

let posts = Post::query()
    .with_query("active_users", active_users)
    .inner_join("active_users", "posts.user_id", "active_users.id")
    .get()
    .await?;

// CTE with column aliases
let stats = Sale::query()
    .with_cte_columns(
        "daily_stats",
        vec!["sale_date", "total_sales", "order_count"],
        "SELECT DATE(created_at), SUM(amount), COUNT(*) FROM sales GROUP BY DATE(created_at)"
    )
    .where_raw("date IN (SELECT sale_date FROM daily_stats WHERE total_sales > 10000)")
    .get()
    .await?;

// Recursive CTE for hierarchical data
let employees = Employee::query()
    .with_recursive_cte(
        "org_tree",
        vec!["id", "name", "manager_id", "level"],
        // Base case: top-level managers
        "SELECT id, name, manager_id, 0 FROM employees WHERE manager_id IS NULL",
        // Recursive: employees under managers
        "SELECT e.id, e.name, e.manager_id, t.level + 1 
         FROM employees e 
         INNER JOIN org_tree t ON e.manager_id = t.id"
    )
    .where_raw("id IN (SELECT id FROM org_tree)")
    .get()
    .await?;
```

---

## CRUD Operations

### Create

```rust
let user = User {
    id: 0,  // Auto-generated
    email: "john@example.com".to_string(),
    name: "John Doe".to_string(),
    active: true,
};
let user = user.save().await?;
println!("Created user with id: {}", user.id);
```

### Read

```rust
// Get all
let users = User::all().await?;

// Find by ID
let user = User::find(1).await?;  // Option<User>

// Query builder (see above)
let users = User::query().where_eq("active", true).get().await?;
```

### Update

```rust
let mut user = User::find(1).await?.unwrap();
user.name = "Jane Doe".to_string();
let user = user.update().await?;
```

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
#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "posts", soft_delete)]
pub struct Post {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub deleted_at: Option<DateTime<Utc>>,
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

Define reusable query patterns that can be applied to any query:

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
#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "posts")]
pub struct Post {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub content: String,
    pub created_at: DateTime<Utc>,  // Auto-set on save()
    pub updated_at: DateTime<Utc>,  // Auto-set on save() and update()
}

// No need to set timestamps manually
let post = Post {
    id: 0,
    title: "Hello".into(),
    content: "World".into(),
    created_at: Utc::now(),  // Will be overwritten
    updated_at: Utc::now(),  // Will be overwritten
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

#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "users")]
pub struct User {
    #[tide(primary_key, auto_increment)]
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
    User { id: 0, name: "John".into(), email: "john@example.com".into() },
    User { id: 0, name: "Jane".into(), email: "jane@example.com".into() },
    User { id: 0, name: "Bob".into(), email: "bob@example.com".into() },
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

## File Attachments

TideORM provides a file attachment system for managing file relationships. Attachments are stored in a JSONB column with metadata.

### Model Setup

```rust
#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "products")]
#[tide(has_one_file = "thumbnail")]
#[tide(has_many_files = "images,documents")]
pub struct Product {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub files: Option<Json>,  // JSONB column storing attachments
}
```

### Relation Types

| Type | Description | Use Case |
|------|-------------|----------|
| `has_one_file` | Single file attachment | Avatar, thumbnail, profile picture |
| `has_many_files` | Multiple file attachments | Gallery images, documents, media |

### Attaching Files

```rust
use tideorm::prelude::*;

// Attach a single file (hasOne) - replaces any existing
product.attach("thumbnail", "uploads/thumb.jpg")?;

// Attach multiple files (hasMany) - accumulates
product.attach("images", "uploads/img1.jpg")?;
product.attach("images", "uploads/img2.jpg")?;

// Attach multiple at once
product.attach_many("images", vec![
    "uploads/img3.jpg",
    "uploads/img4.jpg",
])?;

// Attach with metadata
let attachment = FileAttachment::with_metadata(
    "uploads/document.pdf",
    Some("My Document.pdf"),  // Original filename
    Some(1024 * 1024),        // File size (1MB)
    Some("application/pdf"),  // MIME type
);
product.attach_with_metadata("documents", attachment)?;

// Add custom metadata
let attachment = FileAttachment::new("uploads/photo.jpg")
    .add_metadata("width", 1920)
    .add_metadata("height", 1080)
    .add_metadata("photographer", "John Doe");
product.attach_with_metadata("images", attachment)?;

// Save to persist changes
product.update().await?;
```

### Detaching Files

```rust
// Remove thumbnail (hasOne)
product.detach("thumbnail", None)?;

// Remove specific file (hasMany)
product.detach("images", Some("uploads/img1.jpg"))?;

// Remove all files from relation (hasMany)
product.detach("images", None)?;

// Remove multiple specific files
product.detach_many("images", vec!["img2.jpg", "img3.jpg"])?;

product.update().await?;
```

### Syncing Files (Replace All)

```rust
// Replace all images with new ones
product.sync("images", vec![
    "uploads/new1.jpg",
    "uploads/new2.jpg",
])?;

// Clear all images
product.sync("images", vec![])?;

// Sync with metadata
let attachments = vec![
    FileAttachment::with_metadata("img1.jpg", Some("Photo 1"), Some(1024), Some("image/jpeg")),
    FileAttachment::with_metadata("img2.jpg", Some("Photo 2"), Some(2048), Some("image/jpeg")),
];
product.sync_with_metadata("images", attachments)?;

product.update().await?;
```

### Getting Files

```rust
// Get single file (hasOne)
if let Some(thumb) = product.get_file("thumbnail")? {
    println!("Thumbnail: {}", thumb.key);
    println!("Filename: {}", thumb.filename);
    println!("Created: {}", thumb.created_at);
    if let Some(size) = thumb.size {
        println!("Size: {} bytes", size);
    }
}

// Get multiple files (hasMany)
let images = product.get_files("images")?;
for img in images {
    println!("Image: {} ({})", img.filename, img.key);
}

// Check if has files
if product.has_files("images")? {
    let count = product.count_files("images")?;
    println!("Product has {} images", count);
}
```

### FileAttachment Structure

Each attachment stores:

| Field | Type | Description |
|-------|------|-------------|
| `key` | `String` | File path/key (e.g., "uploads/2024/01/image.jpg") |
| `filename` | `String` | Extracted filename |
| `created_at` | `String` | ISO 8601 timestamp when attached |
| `original_filename` | `Option<String>` | Original filename if different |
| `size` | `Option<u64>` | File size in bytes |
| `mime_type` | `Option<String>` | MIME type |
| `metadata` | `HashMap` | Custom metadata fields |

### JSON Storage Format

Attachments are stored in JSONB with this structure:

```json
{
  "thumbnail": {
    "key": "uploads/thumb.jpg",
    "filename": "thumb.jpg",
    "created_at": "2024-01-15T10:30:00Z"
  },
  "images": [
    {
      "key": "uploads/img1.jpg",
      "filename": "img1.jpg",
      "created_at": "2024-01-15T10:30:00Z",
      "size": 1048576,
      "mime_type": "image/jpeg"
    },
    {
      "key": "uploads/img2.jpg",
      "filename": "img2.jpg",
      "created_at": "2024-01-15T10:31:00Z"
    }
  ]
}
```

---

## Translations (i18n)

TideORM provides a translation system for multilingual content. Translations are stored in a JSONB column.

### Model Setup

```rust
#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "products")]
#[tide(translatable = "name,description")]
pub struct Product {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    
    // Default/fallback values
    pub name: String,
    pub description: String,
    
    pub price: f64,
    
    // JSONB column for translations
    pub translations: Option<Json>,
}
```

### Setting Translations

```rust
use tideorm::prelude::*;

// Set individual translation
product.set_translation("name", "ar", "اسم المنتج")?;
product.set_translation("name", "fr", "Nom du produit")?;
product.set_translation("description", "ar", "وصف المنتج")?;

// Set multiple translations at once
let mut names = HashMap::new();
names.insert("en", "Product Name");
names.insert("ar", "اسم المنتج");
names.insert("fr", "Nom du produit");
product.set_translations("name", names)?;

// Sync translations (replace all for a field)
let mut new_names = HashMap::new();
new_names.insert("en", "New Product Name");
new_names.insert("de", "Neuer Produktname");
product.sync_translations("name", new_names)?;

// Save to persist
product.update().await?;
```

### Getting Translations

```rust
// Get specific translation
if let Some(name) = product.get_translation("name", "ar")? {
    println!("Arabic name: {}", name);
}

// Get with fallback chain: requested -> fallback language -> default field value
let name = product.get_translated("name", "ar")?;

// Get all translations for a field
let all_names = product.get_all_translations("name")?;
for (lang, value) in all_names {
    println!("{}: {}", lang, value);
}

// Get all translations for a language
let arabic = product.get_translations_for_language("ar")?;
// Returns: {"name": "اسم المنتج", "description": "وصف المنتج"}
```

### Checking Translations

```rust
// Check if specific translation exists
if product.has_translation("name", "ar")? {
    println!("Arabic name available");
}

// Check if field has any translations
if product.has_any_translation("name")? {
    println!("Name has translations");
}

// Get available languages for a field
let languages = product.available_languages("name")?;
println!("Name available in: {:?}", languages);
```

### Removing Translations

```rust
// Remove specific translation
product.remove_translation("name", "fr")?;

// Remove all translations for a field
product.remove_field_translations("name")?;

// Clear all translations
product.clear_translations()?;

product.update().await?;
```

### JSON Output with Translations

```rust
// Get JSON with translated fields (removes raw translations column)
let mut opts = HashMap::new();
opts.insert("language".to_string(), "ar".to_string());
let json = product.to_translated_json(Some(opts));
// Result: {"id": 1, "name": "اسم المنتج", "description": "وصف المنتج", "price": 99.99}

// Get JSON with fallback (if Arabic not available, uses fallback language)
let json = product.to_translated_json(Some(opts));

// Get JSON including all translations (for admin interfaces)
let json = product.to_json_with_all_translations();
// Result includes raw translations field
```

### Translation Configuration

When implementing `HasTranslations` manually:

```rust
impl HasTranslations for Product {
    fn translatable_fields() -> Vec<&'static str> {
        vec!["name", "description"]
    }
    
    fn allowed_languages() -> Vec<String> {
        vec!["en".to_string(), "ar".to_string(), "fr".to_string(), "de".to_string()]
    }
    
    fn fallback_language() -> String {
        "en".to_string()
    }
    
    fn get_translations_data(&self) -> Result<TranslationsData, TranslationError> {
        match &self.translations {
            Some(json) => Ok(TranslationsData::from_json(json)),
            None => Ok(TranslationsData::new()),
        }
    }
    
    fn set_translations_data(&mut self, data: TranslationsData) -> Result<(), TranslationError> {
        self.translations = Some(data.to_json());
        Ok(())
    }
    
    fn get_default_value(&self, field: &str) -> Result<serde_json::Value, TranslationError> {
        match field {
            "name" => Ok(serde_json::json!(self.name)),
            "description" => Ok(serde_json::json!(self.description)),
            _ => Err(TranslationError::InvalidField(format!("Unknown field: {}", field))),
        }
    }
}
```

### JSON Storage Format

Translations are stored in JSONB with this structure:

```json
{
  "name": {
    "en": "Wireless Headphones",
    "ar": "سماعات لاسلكية",
    "fr": "Écouteurs sans fil"
  },
  "description": {
    "en": "High-quality wireless headphones",
    "ar": "سماعات لاسلكية عالية الجودة",
    "fr": "Écouteurs sans fil de haute qualité"
  }
}
```

### Combining Attachments and Translations

Models can use both features together:

```rust
#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "products")]
#[tide(translatable = "name,description")]
#[tide(has_one_file = "thumbnail")]
#[tide(has_many_files = "images")]
pub struct Product {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub description: String,
    pub price: f64,
    pub translations: Option<Json>,
    pub files: Option<Json>,
}

// Use both features
product.set_translation("name", "ar", "اسم المنتج")?;
product.attach("thumbnail", "uploads/thumb.jpg")?;
product.attach_many("images", vec!["img1.jpg", "img2.jpg"])?;
product.update().await?;
```

---

## Model Validation

TideORM provides a comprehensive validation system for model data.

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

## Full-Text Search

TideORM provides full-text search capabilities across PostgreSQL (tsvector/tsquery), MySQL (FULLTEXT), and SQLite (FTS5).

### Search Basics

```rust
use tideorm::prelude::*;

// Simple full-text search
let results = Article::search(&["title", "content"], "rust programming")
    .await?;

// Search with ranking (ordered by relevance)
let ranked = Article::search_ranked(&["title", "content"], "rust async")
    .limit(10)
    .get_ranked()
    .await?;

for result in ranked {
    println!("{}: {} (rank: {:.2})", 
        result.record.id, 
        result.record.title, 
        result.rank
    );
}

// Count matching results
let count = Article::search(&["title", "content"], "rust")
    .count()
    .await?;

// Get first matching result
let first = Article::search(&["title"], "rust")
    .first()
    .await?;
```

### Search Modes

```rust
use tideorm::fulltext::{SearchMode, FullTextConfig};

// Natural language search (default)
Article::search(&["content"], "learn rust programming").await?;

// Boolean search with operators
Article::search(&["content"], "+rust +async -javascript")
    .mode(SearchMode::Boolean)
    .get()
    .await?;

// Phrase search (exact phrase matching)
Article::search(&["content"], "async await")
    .mode(SearchMode::Phrase)
    .get()
    .await?;

// Prefix search (for autocomplete)
Article::search(&["title"], "prog")
    .mode(SearchMode::Prefix)
    .get()
    .await?;
```

### Search Configuration

```rust
use tideorm::fulltext::{FullTextConfig, SearchMode, SearchWeights};

let config = FullTextConfig::new()
    .language("english")        // Text analysis language
    .mode(SearchMode::Boolean)  // Search mode
    .min_word_length(3)         // Minimum word length to index
    .max_word_length(50)        // Maximum word length
    // Custom weights for ranking (title > summary > content)
    .weights(SearchWeights::new(1.0, 0.5, 0.3, 0.1));

let results = Article::search_with_config(
    &["title", "summary", "content"],
    "rust programming",
    config
).get().await?;
```

### Text Highlighting

```rust
use tideorm::fulltext::{highlight_text, generate_snippet};

let text = "The quick brown fox jumps over the lazy dog.";

// Highlight search terms
let highlighted = highlight_text(text, "fox lazy", "<mark>", "</mark>");
// Result: "The quick brown <mark>fox</mark> jumps over the <mark>lazy</mark> dog."

// Generate snippet with context
let long_text = "Lorem ipsum... The fox jumped... More text here...";
let snippet = generate_snippet(long_text, "fox", 5, "<b>", "</b>");
// Result: "...dolor sit amet. The <b>fox</b> jumped over the..."
```

### Creating Full-Text Indexes

```rust
use tideorm::fulltext::{FullTextIndex, PgFullTextIndexType};
use tideorm::config::DatabaseType;

// Create index definition
let index = FullTextIndex::new(
    "idx_articles_search",
    "articles",
    vec!["title".to_string(), "content".to_string()]
)
.language("english")
.pg_index_type(PgFullTextIndexType::GIN);

// Generate SQL for your database
let sql = index.to_sql(DatabaseType::Postgres);
// PostgreSQL: CREATE INDEX "idx_articles_search" ON "articles" 
//             USING GIN ((to_tsvector('english', ...)))

let sql = index.to_sql(DatabaseType::MySQL);
// MySQL: CREATE FULLTEXT INDEX `idx_articles_search` ON `articles`(`title`, `content`)

let sql = index.to_sql(DatabaseType::SQLite);
// SQLite: Creates FTS5 virtual table + sync triggers
```

### PostgreSQL-Specific Features

```rust
use tideorm::fulltext::pg_headline_sql;

// Generate ts_headline SQL for server-side highlighting
let headline_sql = pg_headline_sql(
    "content",           // column
    "search query",      // search terms
    "english",           // language
    "<b>", "</b>"        // highlight tags
);
// Result: ts_headline('english', "content", plainto_tsquery(...), ...)
```

---

## Multi-Database Support

TideORM automatically detects your database type and generates appropriate SQL syntax. The same code works seamlessly across PostgreSQL, MySQL, and SQLite.

### Connecting to Different Databases

```rust
// PostgreSQL
TideConfig::init()
    .database("postgres://user:pass@localhost/mydb")
    .connect()
    .await?;

// MySQL / MariaDB
TideConfig::init()
    .database("mysql://user:pass@localhost/mydb")
    .connect()
    .await?;

// SQLite
TideConfig::init()
    .database("sqlite://./data.db?mode=rwc")
    .connect()
    .await?;
```

### Explicit Database Type

```rust
TideConfig::init()
    .database_type(DatabaseType::MySQL)
    .database("mysql://localhost/mydb")
    .connect()
    .await?;
```

### Database Feature Detection

Check which features are supported by the current database:

```rust
let db_type = Database::global().backend();

// Feature checks
if db_type.supports_json() {
    // JSON/JSONB operations available
}

if db_type.supports_arrays() {
    // Native array operations (PostgreSQL only)
}

if db_type.supports_returning() {
    // RETURNING clause for INSERT/UPDATE
}

if db_type.supports_upsert() {
    // ON CONFLICT / ON DUPLICATE KEY support
}

if db_type.supports_window_functions() {
    // OVER(), ROW_NUMBER(), etc.
}

if db_type.supports_cte() {
    // WITH ... AS (Common Table Expressions)
}

if db_type.supports_fulltext_search() {
    // Full-text search capabilities
}
```

### Database-Specific JSON Operations

TideORM automatically translates JSON queries to the appropriate syntax:

```rust
// This query works on all databases with JSON support
Product::query()
    .where_json_contains("metadata", serde_json::json!({"featured": true}))
    .get()
    .await?;
```

**Generated SQL by database:**

| Operation | PostgreSQL | MySQL | SQLite |
|-----------|------------|-------|--------|
| JSON Contains | `col @> '{"key":1}'` | `JSON_CONTAINS(col, '{"key":1}')` | `json_each(col)` + subquery |
| Key Exists | `col ? 'key'` | `JSON_CONTAINS_PATH(col, 'one', '$.key')` | `json_extract(col, '$.key') IS NOT NULL` |
| Path Exists | `col @? '$.path'` | `JSON_CONTAINS_PATH(col, 'one', '$.path')` | `json_extract(col, '$.path') IS NOT NULL` |

### Database-Specific Array Operations

Array operations are fully supported on PostgreSQL. On MySQL/SQLite, arrays are stored as JSON:

```rust
// PostgreSQL native arrays
Product::query()
    .where_array_contains("tags", vec!["sale", "featured"])
    .get()
    .await?;
```

**Generated SQL:**

| Operation | PostgreSQL | MySQL/SQLite |
|-----------|------------|--------------|
| Contains | `col @> ARRAY['a','b']` | `JSON_CONTAINS(col, '["a","b"]')` |
| Contained By | `col <@ ARRAY['a','b']` | `JSON_CONTAINS('["a","b"]', col)` |
| Overlaps | `col && ARRAY['a','b']` | `JSON_OVERLAPS(col, '["a","b"]')` (MySQL 8+) |

### Database-Specific Optimizations

TideORM applies optimizations based on your database:

| Feature | PostgreSQL | MySQL | SQLite |
|---------|------------|-------|--------|
| Optimal Batch Size | 1000 | 1000 | 500 |
| Parameter Style | `$1, $2, ...` | `?, ?, ...` | `?, ?, ...` |
| Identifier Quoting | `"column"` | `` `column` `` | `"column"` |
| Float Casting | `FLOAT8` | `DOUBLE` | `REAL` |

### Feature Compatibility Matrix

| Feature | PostgreSQL | MySQL | SQLite |
|---------|:----------:|:-----:|:------:|
| JSON/JSONB | ✅ | ✅ | ✅ (JSON1) |
| Native JSON Operators | ✅ | ✅ | ❌ |
| Native Arrays | ✅ | ❌ | ❌ |
| RETURNING Clause | ✅ | ❌ | ✅ (3.35+) |
| Upsert | ✅ | ✅ | ✅ |
| Window Functions | ✅ | ✅ (8.0+) | ✅ (3.25+) |
| CTEs | ✅ | ✅ (8.0+) | ✅ (3.8+) |
| Full-Text Search | ✅ | ✅ | ✅ (FTS5) |
| Schemas | ✅ | ✅ | ❌ |

---

## Raw SQL Queries

For complex queries that can't be expressed with the query builder:

```rust
// Execute raw SQL and return model instances
let users: Vec<User> = Database::raw::<User>(
    "SELECT * FROM users WHERE age > 18"
).await?;

// With parameters (use $1, $2 for PostgreSQL, ? for MySQL/SQLite)
let users: Vec<User> = Database::raw_with_params::<User>(
    "SELECT * FROM users WHERE age > $1 AND status = $2",
    vec![18.into(), "active".into()]
).await?;

// Execute raw SQL statement (INSERT, UPDATE, DELETE)
let affected = Database::execute(
    "UPDATE users SET active = false WHERE last_login < NOW() - INTERVAL '1 year'"
).await?;

// Execute with parameters
let affected = Database::execute_with_params(
    "DELETE FROM users WHERE status = $1",
    vec!["banned".into()]
).await?;
```

---

## Query Logging

Enable SQL query logging for development/debugging:

```bash
# Set environment variable
TIDE_LOG_QUERIES=true cargo run
```

When enabled, all SQL queries will be logged to stderr.

---

## Error Handling

TideORM provides rich error types with optional context:

```rust
// Get context from errors
if let Err(e) = User::find_or_fail(999).await {
    if let Some(ctx) = e.context() {
        println!("Error in table: {:?}", ctx.table);
        println!("Column: {:?}", ctx.column);
        println!("Query: {:?}", ctx.query);
    }
}

// Create errors with context
use tideorm::error::{Error, ErrorContext};

let ctx = ErrorContext::new()
    .table("users")
    .column("email")
    .query("SELECT * FROM users WHERE email = $1");

return Err(Error::not_found("User not found").with_context(ctx));
```
