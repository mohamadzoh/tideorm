Implements traits for types that wrap a database value type.

This procedure macro implements `From<T> for Value`, `tideorm::orm::TryGetTable`, and
`sql_builder::ValueType` for the wrapper type `T`.

It also implements `sql_builder::postgres_array::NotU8`.
It implements `TryFromU64` if the wrapped types are recognized primitives, i.e.
`"i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64"`.

The wrapped type must be `tideorm::orm::Value` compatible.

## Usage

```rust,ignore
use tideorm::orm::DeriveValueType;

#[derive(DeriveValueType)]
struct MyString(String);

#[derive(DeriveValueType)]
struct MyNumber(i32);
```

It's also possible to derive value type for enum-strings.
Basically the underlying type is String, and the custom must implement methods `to_str` and `from_str`.

## Example

```rust,ignore
use tideorm::orm::{DeriveValueType, sql_builder::ValueTypeErr};

#[derive(DeriveValueType)]
#[orm(value_type = "String")]
pub enum Tag {
    Hard,
    Soft,
}

impl std::fmt::Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Hard => "hard",
                Self::Soft => "soft",
            }
        )
    }
}

impl std::str::FromStr for Tag {
    type Err = ValueTypeErr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "hard" => Self::Hard,
            "soft" => Self::Soft,
            _ => return Err(ValueTypeErr),
        })
    }
}
```

`from_str` defaults to `std::str::FromStr::from_str`. `to_str` defaults to `std::string::ToString::to_string`.
They can be overridden with custom functions.

```rust,ignore
use tideorm::orm::{DeriveValueType, sql_builder::ValueTypeErr};

#[derive(DeriveValueType)]
#[orm(
    value_type = "String",
    from_str = "Tag::from_str",
    to_str = "Tag::to_str"
)]
pub enum Tag {
    Color,
    Grey,
}

impl Tag {
    fn to_str(&self) -> &'static str {
        match self {
            Self::Color => "color",
            Self::Grey => "grey",
        }
    }

    fn from_str(s: &str) -> Result<Self, ValueTypeErr> {
        Ok(match s {
            "color" => Self::Color,
            "grey" => Self::Grey,
            _ => return Err(ValueTypeErr),
        })
    }
}
```
