A derive macro to implement `tideorm::orm::ActiveEnum` trait for enums.

# Limitations

This derive macros can only be used on enums.

# Macro Attributes

All macro attributes listed below have to be annotated in the form of `#[orm(attr = value)]`.

- For enum
    - `rs_type`: Define `ActiveEnum::Value` for non-native enums (`db_type != "Enum"`)
        - Possible values: `String`, `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`
        - Note that value has to be passed as string, i.e. `rs_type = "i8"`
    - `db_type`: Define `ColumnType` returned by `ActiveEnum::db_type()`
        - Possible values: all available enum variants of `ColumnType`, e.g. `Enum`, `String(StringLen::None)`, `String(StringLen::N(1))`, `Integer`
        - Note that value has to be passed as string, i.e. `db_type = "Integer"`
    - `enum_name`: Define `String` returned by `ActiveEnum::name()`
        - This attribute is optional with default value being the name of enum in PascalCase, split into words as TideORM splits table and column names (`S3Kind` is `S3Kind`, digits are words of their own)
        - Note that value has to be passed as string, i.e. `enum_name = "MyEnum"`
    - Constraints for native enums (`db_type = "Enum"`):
        - `rs_type` is optional; it defaults to `Enum`. If specified it must be `String` or `Enum`.
        - `num_value` and numeric discriminants are not allowed.

- For enum variant
    - `string_value` or `num_value`:
        - For `string_value`, value should be passed as string, i.e. `string_value = "A"`
        - For `num_value`, value should be passed as integer, i.e. `num_value = 1` or `num_value = 1i32`
        - Note that only one of it can be specified, and all variants of an enum have to annotate with the same `*_value` macro attribute

# Usage

```ignore
use tideorm::orm::{DeriveActiveEnum, entity::prelude::*};

#[derive(EnumIter, DeriveActiveEnum)]
#[orm(rs_type = "i32", db_type = "Integer")]
pub enum Color {
    Black = 0,
    White = 1,
}
```
