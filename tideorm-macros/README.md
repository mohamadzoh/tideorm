# TideORM Macros

Procedural macros for [TideORM](https://github.com/mohamadzoh/tideorm).

## Overview

This crate provides derive macros to simplify defining ORM models in TideORM. Instead of manually implementing traits, you can use the `#[derive(Model)]` macro to automatically generate the necessary implementations.

## Installation

This crate is typically used as a dependency of the main `tideorm` crate. If you need to use it directly:

```toml
[dependencies]
tideorm-macros = "0.4.3"
```

## Usage

### Basic Model

```rust
use tideorm_macros::Model;

#[derive(Model)]
#[tide(table = "users")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub email: String,
}
```

### Field Attributes

| Attribute | Description |
|-----------|-------------|
| `#[tide(primary_key)]` | Mark field as primary key |
| `#[tide(auto_increment)]` | Enable auto-increment for primary keys |
| `#[tide(column = "name")]` | Override the database column name |
| `#[tide(nullable)]` | Mark field as nullable |
| `#[tide(default = "expr")]` | Set a default value expression |
| `#[tide(skip)]` | Skip this field in queries |
| `#[tide(timestamp)]` | Mark as timestamp field (created_at, updated_at) |

### Relation Attributes

| Attribute | Description |
|-----------|-------------|
| `#[tide(has_one = "Model")]` | Define a has-one relationship |
| `#[tide(has_many = "Model")]` | Define a has-many relationship |
| `#[tide(belongs_to = "Model")]` | Define a belongs-to relationship |
| `#[tide(has_many_through = "Model")]` | Define a has-many-through relationship |
| `#[tide(foreign_key = "col")]` | Specify the foreign key column |
| `#[tide(owner_key = "col")]` | Specify the owner/local key |
| `#[tide(pivot = "table")]` | Specify pivot table for many-to-many |

## License

MIT
