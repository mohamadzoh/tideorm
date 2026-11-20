# File Attachments

Requires the `attachments` feature.

File attachments keep references to files, such as a product's thumbnail or its gallery, in a JSON column of the model. TideORM stores each file's key, name, timestamp and metadata; the file bytes live wherever you store them (a bucket, a CDN, a disk).

```toml
[dependencies]
tideorm = { version = "0.18.2", default-features = false, features = ["postgres", "runtime-tokio", "attachments"] }
```

## Model Setup

Name the attachment slots on the model, and implement `HasAttachments` to say which column holds them:

```rust
use tideorm::prelude::*;

#[tideorm::model(
    table = "products",
    has_one_files = "thumbnail",
    has_many_files = "images,documents"
)]
pub struct Product {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub files: Option<JsonValue>, // JSON or JSONB column holding the attachments
}

impl HasAttachments for Product {
    fn has_one_files() -> Vec<&'static str> {
        vec!["thumbnail"]
    }

    fn has_many_files() -> Vec<&'static str> {
        vec!["images", "documents"]
    }

    fn get_files_data(&self) -> Result<FilesData, AttachmentError> {
        Ok(self.files.as_ref().map(FilesData::from_json).unwrap_or_default())
    }

    fn set_files_data(&mut self, data: FilesData) -> Result<(), AttachmentError> {
        self.files = Some(data.to_json());
        Ok(())
    }
}
```

Every option goes inside the one `#[tideorm::model(..)]` attribute; combining it with a separate `#[tideorm(..)]` on the struct is a compile error. The attribute records the slot names on the model, which `to_json()` uses. The derive does not generate `HasAttachments`, because only you know which column holds the payload. Once your impl exists, the methods below are available on `Product` with `tideorm::prelude::*` in scope.

Name the column field `files`: `to_json()` renders attachments from the field named `files` (by its serialized name, if your serde derive renames it).

| Slot kind | Holds | Typical use |
|---|---|---|
| `has_one_files` | One file; attaching replaces it | Avatar, thumbnail, profile picture |
| `has_many_files` | A list of files; attaching appends | Gallery images, documents, media |

Every method changes the model in memory only. Save the model afterwards (`product.update().await?`) or nothing is stored.

## Attaching Files

```rust
// hasOne: replaces any existing file
product.attach("thumbnail", "uploads/thumb.jpg")?;

// hasMany: appends
product.attach("images", "uploads/img1.jpg")?;
product.attach("images", "uploads/img2.jpg")?;

// Several at once (hasMany only)
product.attach_many("images", ["uploads/img3.jpg", "uploads/img4.jpg"])?;

// With metadata
let attachment = FileAttachment::with_metadata(
    "uploads/document.pdf",
    Some("My Document.pdf"),  // original filename
    Some(1024 * 1024),        // size in bytes
    Some("application/pdf"),  // MIME type
);
product.attach_with_metadata("documents", attachment)?;

// With custom metadata
let attachment = FileAttachment::new("uploads/photo.jpg")
    .add_metadata("width", 1920)
    .add_metadata("height", 1080)
    .add_metadata("photographer", "John Doe");
product.attach_with_metadata("images", attachment)?;

product.update().await?;
```

Custom metadata sits beside the attachment's own fields in its JSON. An entry named like a field that is set (`key`, `size`, ..) is left out, since the field owns that name. One named like an unset optional field is kept, and reads back as metadata when it is not that field's type, so `add_metadata("size", "12KB")` round-trips.

A slot name that the model does not declare is an `AttachmentError::InvalidRelation` ("Unknown file relation"), and `attach_many`/`detach_many` on a hasOne slot is refused the same way.

## Detaching Files

```rust
// hasOne: remove the thumbnail
product.detach("thumbnail", None)?;

// hasOne: remove the thumbnail only while it is this file
product.detach("thumbnail", Some("uploads/old-thumb.jpg"))?;

// hasMany: remove one file
product.detach("images", Some("uploads/img1.jpg"))?;

// hasMany: remove every file
product.detach("images", None)?;

// hasMany: remove several files
product.detach_many("images", ["uploads/img2.jpg", "uploads/img3.jpg"])?;

product.update().await?;
```

## Syncing Files

`sync` replaces a slot's contents:

```rust
product.sync("images", ["uploads/new1.jpg", "uploads/new2.jpg"])?;

// An empty list clears the slot
product.sync("images", Vec::<&str>::new())?;

// With metadata
let attachments = vec![
    FileAttachment::with_metadata("img1.jpg", Some("Photo 1"), Some(1024), Some("image/jpeg")),
    FileAttachment::with_metadata("img2.jpg", Some("Photo 2"), Some(2048), Some("image/jpeg")),
];
product.sync_with_metadata("images", attachments)?;

product.update().await?;
```

On a hasOne slot, `sync` keeps the first file, or clears the slot when the list is empty.

## Reading Files

```rust
// hasOne
if let Some(thumb) = product.get_file("thumbnail")? {
    println!("Thumbnail: {} ({}), attached {}", thumb.key, thumb.filename, thumb.created_at);
    if let Some(size) = thumb.size {
        println!("Size: {size} bytes");
    }
}

// hasMany
for image in product.get_files("images")? {
    println!("Image: {} ({})", image.filename, image.key);
}

if product.has_files("images")? {
    let count = product.count_files("images")?;
    println!("Product has {count} images");
}
```

A stored entry that does not read as an attachment makes `get_file`/`get_files` return `AttachmentError::InvalidData`. `all_file_relations()`, `is_has_one_relation(name)`, `is_has_many_relation(name)` and `validate_relation(name)` answer questions about the slots themselves.

`AttachmentError` converts into `tideorm::Error` with `?`: an unknown slot becomes a validation error, malformed data a conversion error.

## FileAttachment

Each attachment holds:

| Field | Type | Description |
|---|---|---|
| `key` | `String` | The storage key or path, such as `uploads/2024/01/image.jpg` |
| `filename` | `String` | The last segment of the key (`/` and `\` both separate) |
| `created_at` | `String` | When it was attached, as an RFC 3339 timestamp |
| `original_filename` | `Option<String>` | The upload's own name, if different |
| `size` | `Option<u64>` | Size in bytes |
| `mime_type` | `Option<String>` | MIME type |
| `metadata` | `HashMap<String, serde_json::Value>` | Custom fields |

Build one with `FileAttachment::new(key)` or `FileAttachment::with_metadata(key, original_filename, size, mime_type)`, add fields with `add_metadata(name, value)`, and render it with `to_json()`.

`FilesData` is the whole column's content, which your `HasAttachments` impl converts with `FilesData::from_json` (anything other than a JSON object reads as empty) and `to_json()`. Its methods (`set_one`, `get_one`, `try_get_one`, `remove_one`, `add_many`, `get_many`, `try_get_many`, `remove_from_many`, `remove_many`, `clear_many`, `has_files`, `count_files`) are what the trait's methods are built on; the `try_` forms report malformed data instead of treating it as absent.

## Storage Format

The column holds one JSON object, a member per slot:

```json
{
  "thumbnail": {
    "key": "uploads/thumb.jpg",
    "filename": "thumb.jpg",
    "created_at": "2024-01-15T10:30:00+00:00"
  },
  "images": [
    {
      "key": "uploads/img1.jpg",
      "filename": "img1.jpg",
      "created_at": "2024-01-15T10:30:00+00:00",
      "size": 1048576,
      "mime_type": "image/jpeg"
    },
    {
      "key": "uploads/img2.jpg",
      "filename": "img2.jpg",
      "created_at": "2024-01-15T10:31:00+00:00"
    }
  ]
}
```

## Keys Are Untrusted Input

TideORM stores a key as opaque text and never opens, reads or writes the file it names. `attach("avatar", "../../etc/passwd")` is stored verbatim, and the default URL generator joins it onto the base URL verbatim. Validating keys is your job: screen a key that arrives from a client with `FileAttachment::is_safe_key` before attaching it, or generate keys on the server and never accept them from the client.

```rust
if !FileAttachment::is_safe_key(&upload_key) {
    return Err(tideorm::Error::validation("avatar", "invalid file key"));
}
user.attach("avatar", &upload_key)?;
```

`is_safe_key` rejects a key that is empty or whitespace, contains a NUL byte, is absolute (`/x` or `\x`), contains `//` (URLs with a scheme or protocol-relative ones), contains `:` (Windows drive prefixes, alternate data streams, URL schemes) or has a `..` segment. Both `/` and `\` count as separators. A key that passes is only shaped like a safe key: your storage backend still confines it to its prefix and authorizes the access. `attach()` does not call it for you, because existing applications store keys it rejects.

## File URLs

`to_json()` removes the `files` column from the output and writes each slot as a top-level member, each file with a `url` member added:

```json
{
  "id": 1,
  "name": "Headphones",
  "thumbnail": {
    "key": "products/123/thumb.jpg",
    "filename": "thumb.jpg",
    "created_at": "2024-01-15T10:30:00+00:00",
    "url": "https://cdn.example.com/uploads/products/123/thumb.jpg"
  }
}
```

A slot named in the model's or the global hidden attributes stays out of the output, and the model's hidden attribute names are removed inside each file too.

### Base URLs

Prefix every key with a base URL, or give one slot a base of its own:

```rust
TideConfig::init()
    .database("postgres://localhost/mydb")
    .file_base_url("https://cdn.example.com/uploads")
    .file_base_url_for("avatar", "https://avatars.example.com")
    .connect()
    .await?;
```

The default generator joins the base and the key with one `/`, tolerating a slash on either side, and returns the bare key when no base applies.

### Custom URL Generator

For signed URLs, image transformations or routing by type, install a generator. It receives the slot name and the whole `FileAttachment`, and may capture what it needs:

```rust
use tideorm::attachments::FileAttachment;

fn smart_url_generator(field_name: &str, file: &FileAttachment) -> String {
    match field_name {
        "thumbnail" => {
            let quality = if file.size.unwrap_or(0) > 500_000 { "60" } else { "auto" };
            return format!("https://thumbs.example.com/q_{}/{}", quality, file.key);
        }
        "avatar" => return format!("https://avatars.example.com/w_200,h_200/{}", file.key),
        _ => {}
    }

    match file.mime_type.as_deref() {
        Some(m) if m.starts_with("video/") => format!("https://stream.example.com/{}", file.key),
        Some(m) if m.starts_with("image/") => {
            let quality = if file.size.unwrap_or(0) > 1_000_000 { "80" } else { "auto" };
            format!("https://images.example.com/q_{}/{}", quality, file.key)
        }
        _ => format!("https://cdn.example.com/{}", file.key),
    }
}

TideConfig::init()
    .database("postgres://localhost/mydb")
    .file_url_generator(smart_url_generator)
    .connect()
    .await?;
```

The generator replaces the base-URL settings unless it reads them itself (`Config::get_file_base_url_for(field)`, or by calling `Config::default_file_url_generator`). Like the other settings it is installed by `connect()` or `apply()`, and one of them without a generator puts the default back.

There is no per-model generator: every model renders its attachments with the one generator. Route per model inside it, by slot name or by something you stored in `metadata` at attach time.

The order in which a URL is decided:

1. The custom generator, if `file_url_generator()` installed one.
2. The slot's base URL (`file_base_url_for`), else the global one (`file_base_url`).
3. The key unchanged.

### Building URLs Yourself

```rust
use tideorm::attachments::FileAttachment;

// Through the installed generator
let url = FileAttachment::new("uploads/image.jpg").url("thumbnail");

if let Some(thumb) = product.get_file("thumbnail")? {
    let url = thumb.url("thumbnail");

    // A one-off generator for this call only
    let url = thumb.url_with_generator("thumbnail", |field_name, file| {
        format!("https://custom-cdn.com/{}/{}", field_name, file.key)
    });
}
```

`Config::get_file_url_generator()` returns the installed generator (or the default) as a `FileUrlGenerator`, an `Arc<dyn Fn(&str, &FileAttachment) -> String + Send + Sync>`.

## With Translations

A model can use attachments and [translations](translations.md) together, each with its own impl:

```rust
#[tideorm::model(
    table = "products",
    translatable = "name,description",
    has_one_files = "thumbnail",
    has_many_files = "images"
)]
pub struct Product {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub description: String,
    pub price: f64,
    pub translations: Option<JsonValue>,
    pub files: Option<JsonValue>,
}

product.set_translation("name", "ar", "اسم المنتج")?;
product.attach("thumbnail", "uploads/thumb.jpg")?;
product.attach_many("images", ["img1.jpg", "img2.jpg"])?;
product.update().await?;
```
