# TideORM Macros

Procedural macros for [TideORM](https://github.com/mohamadzoh/tideorm).

## Overview

This crate provides derive macros to simplify defining ORM models in TideORM. Instead of manually implementing traits, you can use the `#[derive(Model)]` macro directly, or the higher-level `#[tideorm::model(...)]` attribute from the main crate.

## Installation

This crate is typically used as a dependency of the main `tideorm` crate. If you need to use it directly:

```toml
[dependencies]
tideorm-macros = "0.8.6"
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

### Field Attributes

| Attribute | Description |
|-----------|-------------|
| `#[tideorm(primary_key)]` | Mark field as primary key |
| `#[tideorm(auto_increment)]` | Enable auto-increment for primary keys |
| `#[tideorm(column = "name")]` | Override the database column name |
| `#[tideorm(nullable)]` | Mark field as nullable |
| `#[tideorm(default = "expr")]` | Set a default value expression |
| `#[tideorm(skip)]` | Skip this field in queries |
| `#[tideorm(timestamp)]` | Mark as timestamp field (created_at, updated_at) |

### Relation Attributes

| Attribute | Description |
|-----------|-------------|
| `#[tideorm(has_one = "Model")]` | Define a has-one relationship |
| `#[tideorm(has_many = "Model")]` | Define a has-many relationship |
| `#[tideorm(belongs_to = "Model")]` | Define a belongs-to relationship |
| `#[tideorm(has_many_through = "Model")]` | Define a has-many-through relationship |
| `#[tideorm(foreign_key = "col")]` | Specify the foreign key column |
| `#[tideorm(owner_key = "col")]` | Specify the owner/local key |
| `#[tideorm(pivot = "table")]` | Specify pivot table for many-to-many |

## License

MIT
