# TideORM Macros

Procedural macros for [TideORM](https://github.com/mohamadzoh/tideorm).

## Overview

This crate provides derive macros to simplify defining ORM models in TideORM. Instead of manually implementing traits, you can use the `#[derive(Model)]` macro directly, or the higher-level `#[tideorm::model(...)]` attribute from the main crate.

## Installation

This crate is typically used as a dependency of the main `tideorm` crate. If you need to use it directly:

```toml
[dependencies]
tideorm-macros = "0.12"
```

## Usage

### Preferred TideORM Attribute Macro

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub email: String,
}
```

This expands to `#[derive(Model)]` plus the equivalent `#[tideorm(...)]` model options.

Any existing `#[derive(...)]` attributes on the struct are preserved. The macro adds TideORM's generated derives only when they are still missing.

### Basic Model

```rust
use tideorm_macros::Model;

#[derive(Model)]
#[tideorm(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub email: String,
}
```

### Composite Primary Keys

You can mark more than one field with `#[tideorm(primary_key)]` to generate a composite primary key:

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

let key = (1_i64, 2_i64);
let record = UserRole::find(key).await?;
```

Composite primary key rules:

- Use `#[tideorm(primary_key)]` on each key field in declaration order.
- `#[tideorm(auto_increment)]` is only supported for single-column primary keys.
- `#[tideorm(tokenize)]` requires exactly one primary key field.
- Relation definitions that would otherwise rely on the implicit `id` local key must set `local_key = "..."` explicitly for composite-key models.

For `has_many_through`, declare `pivot`, `foreign_key`, and `related_key` explicitly. The macro now rejects missing required relation metadata at compile time.

### Field Attributes

| Attribute | Description |
|-----------|-------------|
| `#[tideorm(primary_key)]` | Mark field as part of the primary key |
| `#[tideorm(auto_increment)]` | Enable auto-increment for a single-column primary key |
| `#[tideorm(column = "name")]` | Override the database column name |
| `#[tideorm(nullable)]` | Mark field as nullable |
| `#[tideorm(default = "expr")]` | Set a default value expression |
| `#[tideorm(skip)]` | Skip this field in queries |

### Relation Attributes

| Attribute | Description |
|-----------|-------------|
| `#[tideorm(has_one = "Model")]` | Name a `HasOne<T>` relation |
| `#[tideorm(has_many = "Model")]` | Name a `HasMany<T>` relation |
| `#[tideorm(belongs_to = "Model")]` | Name a `BelongsTo<T>` relation |
| `#[tideorm(has_many_through = "Model")]` | Name a `HasManyThrough<T, P>` relation |
| `#[tideorm(foreign_key = "col")]` | Specify the foreign key column |
| `#[tideorm(local_key = "col")]` | Specify the local key (defaults to `id`) |
| `#[tideorm(owner_key = "col")]` | Specify the owner key of a `BelongsTo`, or the related key of a `HasManyThrough` |
| `#[tideorm(related_key = "col")]` | Specify the pivot column pointing at the related model |
| `#[tideorm(morph_name = "name")]` | Configure the base name for polymorphic relation columns |
| `#[tideorm(pivot = "table")]` | Specify pivot table for many-to-many |

The wrapper type decides the relation kind: a `has_one`-style attribute is optional, but it must name the same kind as the field's wrapper. `HasOne`, `HasMany` and `BelongsTo` require `foreign_key`; polymorphic wrappers require `morph_name`; self-referencing wrappers default `foreign_key` to `parent_id` and `local_key` to `id`. A missing requirement or a mismatched attribute is a compile-time error.

## License

MIT
