# Relations

This chapter covers field-declared relations, attachment and translation helpers, and the runtime relation wrappers provided by TideORM.

## Model Relations

TideORM supports SeaORM-style relations defined as struct fields. Relations are lazy-loaded on demand.

### Defining Relations

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub email: String,
    
    // One-to-one: User has one Profile
    #[tideorm(has_one = "Profile", foreign_key = "user_id")]
    pub profile: HasOne<Profile>,
    
    // One-to-many: User has many Posts
    #[tideorm(has_many = "Post", foreign_key = "user_id")]
    pub posts: HasMany<Post>,
}

#[tideorm::model(table = "profiles")]
pub struct Profile {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub bio: String,
    
    // Inverse: Profile belongs to User
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
    
    // Inverse: Post belongs to User
    #[tideorm(belongs_to = "User", foreign_key = "user_id")]
    pub author: BelongsTo<User>,
    
    // One-to-many: Post has many Comments
    #[tideorm(has_many = "Comment", foreign_key = "post_id")]
    pub comments: HasMany<Comment>,
}
```

### Relation Types

| Type | Attribute | Description |
|------|-----------|-------------|
| `HasOne<T>` | `has_one` | One-to-one relationship (e.g., User has one Profile) |
| `HasMany<T>` | `has_many` | One-to-many relationship (e.g., User has many Posts) |
| `BelongsTo<T>` | `belongs_to` | Inverse relationship (e.g., Post belongs to User) |
| `HasManyThrough<T, P>` | `has_many_through` | Many-to-many via pivot table |
| `MorphOne<T>` | - | Polymorphic one-to-one |
| `MorphMany<T>` | - | Polymorphic one-to-many |

### Relation Attributes

| Attribute | Description | Required |
|-----------|-------------|----------|
| `foreign_key` | Foreign key column on related table | Yes |
| `local_key` | Local key (defaults to primary key) | No |
| `owner_key` | Owner key for BelongsTo | No |
| `pivot` | Pivot table name for HasManyThrough | For through relations |
| `related_key` | Related key on pivot table | For through relations |

### Loading Relations

Relation helper fields such as `HasOne<T>`, `HasMany<T>`, and `BelongsTo<T>` are runtime helpers, not persisted columns. TideORM's generated serde implementation skips them during serialization and restores them with defaults during deserialization, so they do not leak into JSON payloads.

```rust
// Load a HasOne relation
let user = User::find(1).await?.unwrap();
let profile: Option<Profile> = user.profile.load().await?;

// Load a HasMany relation
let posts: Vec<Post> = user.posts.load().await?;

// Load a BelongsTo relation
let post = Post::find(1).await?.unwrap();
let author: Option<User> = post.author.load().await?;

// Check if relation exists
let has_profile = user.profile.exists().await?;  // bool
let has_posts = user.posts.exists().await?;      // bool

// Count related records
let post_count = user.posts.count().await?;      // u64
```

### Loading with Constraints

```rust
// Load posts with custom conditions
let recent_posts = user.posts.load_with(|query| {
    query
        .where_eq("published", true)
        .where_gt("views", 100)
        .order_desc("created_at")
        .limit(10)
}).await?;

// Load profile with constraints
let profile = user.profile.load_with(|query| {
    query.where_not_null("avatar")
}).await?;
```

### Many-to-Many Relations

```rust
#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    
    // Many-to-many: User has many Roles through user_roles pivot table
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

// Usage
let user = User::find(1).await?.unwrap();

// Load all roles
let roles = user.roles.load().await?;

// Attach a role
user.roles.attach(role_id).await?;

// Detach a role
user.roles.detach(role_id).await?;

// Sync roles (replace all with new set)
user.roles.sync(vec![
    serde_json::json!(1),
    serde_json::json!(2),
    serde_json::json!(3),
]).await?;
```

### Polymorphic Relations

```rust
use tideorm::prelude::*;

// Images can belong to Posts or Videos (polymorphic)
#[tideorm::model(table = "images")]
pub struct Image {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub path: String,
    pub imageable_type: String,  // "posts" or "videos"
    pub imageable_id: i64,
}

#[tideorm::model(table = "posts")]
pub struct Post {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    
    // Polymorphic: Post has many Images
    pub images: MorphMany<Image>,
}

// Note: MorphMany/MorphOne require manual setup with .with_parent()
```

---


---

## File Attachments

TideORM provides a file attachment system for managing file relationships. Attachments are stored in a JSONB column with metadata.

Enable the feature first:

```toml
[dependencies]
tideorm = { version = "0.8.4", features = ["postgres", "attachments"] }
```

### Model Setup

```rust
#[tideorm::model(table = "products")]
#[tideorm(has_one_file = "thumbnail")]
#[tideorm(has_many_files = "images,documents")]
pub struct Product {
    #[tideorm(primary_key, auto_increment)]
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

### File URL Generation

TideORM can automatically generate full URLs for file attachments. This is useful when you store file keys/paths in the database but need to serve them from a CDN or storage service.

#### Global Base URL

Configure a base URL that will be prepended to all file keys:

```rust
TideConfig::init()
    .database("postgres://localhost/mydb")
    .file_base_url("https://cdn.example.com/uploads")
    .connect()
    .await?;
```

Now when you call `to_json()`, file attachments will include a `url` field:

```json
{
  "thumbnail": {
    "key": "products/123/thumb.jpg",
    "filename": "thumb.jpg",
    "url": "https://cdn.example.com/uploads/products/123/thumb.jpg"
  }
}
```

#### Custom URL Generator

For more complex URL generation (signed URLs, image transformations, etc.), use a custom generator that receives **both the field name and the full `FileAttachment`**:

```rust
use tideorm::attachments::FileAttachment;

// Define a custom URL generator function with field name and full metadata access
fn smart_url_generator(field_name: &str, file: &FileAttachment) -> String {
    // Route based on field name first
    match field_name {
        "thumbnail" => {
            let quality = if file.size.unwrap_or(0) > 500_000 { "60" } else { "auto" };
            return format!("https://thumbs.example.com/q_{}/{}", quality, file.key);
        }
        "avatar" => {
            return format!("https://avatars.example.com/w_200,h_200/{}", file.key);
        }
        _ => {}
    }
    
    // Fall back to mime_type routing
    match file.mime_type.as_deref() {
        Some(m) if m.starts_with("video/") => {
            format!("https://stream.example.com/{}", file.key)
        }
        Some(m) if m.starts_with("image/") => {
            let quality = if file.size.unwrap_or(0) > 1_000_000 { "80" } else { "auto" };
            format!("https://images.example.com/q_{}/{}", quality, file.key)
        }
        _ => format!("https://cdn.example.com/{}", file.key),
    }
}

// Use it globally
TideConfig::init()
    .database("postgres://localhost/mydb")
    .file_url_generator(smart_url_generator)
    .connect()
    .await?;
```

**Parameters available to URL generators:**
- `field_name` - The attachment field name (e.g., "thumbnail", "avatar", "documents")
- `file` - The full `FileAttachment` struct with:
  - `key` - Storage key/path
  - `filename` - Extracted filename
  - `created_at` - Creation timestamp
  - `original_filename` - Original upload name (if available)
  - `size` - File size in bytes (if available)
  - `mime_type` - MIME type (if available)
  - `metadata` - Custom HashMap for additional data

#### Model-Specific URL Generator

Override the URL generator for specific models:

```rust
#[tideorm::model(table = "products")]
#[tideorm(has_one_file = "thumbnail")]
pub struct Product {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub files: Option<Json>,
}

impl ModelMeta for Product {
    // ... other required methods ...
    
    fn file_url_generator() -> FileUrlGenerator {
        |field_name, file| {
            match field_name {
                "thumbnail" => format!("https://products-cdn.example.com/thumb/{}", file.key),
                "gallery" => format!("https://products-cdn.example.com/gallery/{}", file.key),
                _ => format!("https://products-cdn.example.com/assets/{}", file.key),
            }
        }
    }
}
```

#### Manual URL Generation

Generate URLs programmatically:

```rust
use tideorm::prelude::*;
use tideorm::attachments::FileAttachment;

// Create a FileAttachment for URL generation
let file = FileAttachment::new("uploads/image.jpg");
let url = Config::generate_file_url("thumbnail", &file);

// With metadata for smarter URL generation
let file = FileAttachment::with_metadata(
    "uploads/video.mp4",
    Some("My Video.mp4"),
    Some(50_000_000),
    Some("video/mp4"),
);
let url = Config::generate_file_url("video", &file);

// Using model-specific generator
let url = Product::generate_file_url("thumbnail", &file);

// Using FileAttachment method directly
let attachment = product.get_file("thumbnail")?;
if let Some(thumb) = attachment {
    let url = thumb.url("thumbnail");  // Uses global generator with field name
    
    // Or with custom generator
    let url = thumb.url_with_generator("thumbnail", |field_name, file| {
        format!("https://custom-cdn.com/{}/{}", field_name, file.key)
    });
}
```

#### URL Generator Priority

URL generators are resolved in this order:

1. **Model-specific generator** - If the model overrides `file_url_generator()`
2. **Global custom generator** - If set via `TideConfig::file_url_generator()`
3. **Global base URL** - If set via `TideConfig::file_base_url()`
4. **Key as-is** - If no configuration, returns the key unchanged

---

## Translations (i18n)

TideORM provides a translation system for multilingual content. Translations are stored in a JSONB column.

Enable the feature first:

```toml
[dependencies]
tideorm = { version = "0.8.4", features = ["postgres", "translations"] }
```

### Model Setup

```rust
#[tideorm::model(table = "products")]
#[tideorm(translatable = "name,description")]
pub struct Product {
    #[tideorm(primary_key, auto_increment)]
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
#[tideorm::model(table = "products")]
#[tideorm(translatable = "name,description")]
#[tideorm(has_one_file = "thumbnail")]
#[tideorm(has_many_files = "images")]
pub struct Product {
    #[tideorm(primary_key, auto_increment)]
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

