//! Filter values JSON cannot hold.
//!
//! `serde_json` writes a NaN or an infinite float as `null`, and a filter reads
//! `null` as SQL NULL, so `where_eq("price", f64::NAN)` would become
//! `price IS NULL`. Such a value is refused instead. Only a value whose JSON
//! holds a `null` is serialized a second time, through [`NullProbe`], to tell a
//! NaN from a `None`.

use serde::Serialize;
use serde::ser;

/// `value` as the JSON a filter carries, or why it cannot be one.
pub(crate) fn checked_filter_value(value: impl Serialize) -> Result<serde_json::Value, String> {
    let json = super::filter_value(&value);
    if holds_null(&json) && value.serialize(NullProbe::<false>).unwrap_or(false) {
        return Err(
            "a filter value holds a NaN or infinite float, which SQL cannot compare".to_string(),
        );
    }
    Ok(json)
}

/// Preserve a top-level Option::None before JSON erases its distinction from
/// a JSON null document. Some(null), references and transparent newtypes work
/// through Serde's own representation, without Rust type-name heuristics.
pub(crate) fn checked_assignment_value(
    value: impl Serialize,
) -> Result<(serde_json::Value, bool), String> {
    let json = checked_filter_value(&value)?;
    let sql_null = json.is_null()
        && value
            .serialize(NullProbe::<true>)
            .map_err(|e| e.to_string())?;
    Ok((json, sql_null))
}

fn holds_null(json: &serde_json::Value) -> bool {
    match json {
        serde_json::Value::Null => true,
        serde_json::Value::Array(items) => items.iter().any(holds_null),
        serde_json::Value::Object(members) => members.values().any(holds_null),
        _ => false,
    }
}

/// Probe either non-finite floats (NONE=false) or a top-level optional null
/// (NONE=true); collections always recurse for non-finite validation.
struct NullProbe<const NONE: bool>;

/// The same question over the members of a sequence, a map or a struct.
#[derive(Default)]
struct Members(bool);

impl Members {
    fn visit<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), serde_json::Error> {
        self.0 |= value.serialize(NullProbe::<false>)?;
        Ok(())
    }
}

macro_rules! finite_scalars {
    ($($method:ident: $ty:ty),*) => {$(
        fn $method(self, _: $ty) -> Result<bool, Self::Error> {
            Ok(false)
        }
    )*};
}

impl<const NONE: bool> ser::Serializer for NullProbe<NONE> {
    type Ok = bool;
    type Error = serde_json::Error;
    type SerializeSeq = Members;
    type SerializeTuple = Members;
    type SerializeTupleStruct = Members;
    type SerializeTupleVariant = Members;
    type SerializeMap = Members;
    type SerializeStruct = Members;
    type SerializeStructVariant = Members;

    finite_scalars!(
        serialize_bool: bool, serialize_i8: i8, serialize_i16: i16, serialize_i32: i32,
        serialize_i64: i64, serialize_i128: i128, serialize_u8: u8, serialize_u16: u16,
        serialize_u32: u32, serialize_u64: u64, serialize_u128: u128, serialize_char: char,
        serialize_str: &str, serialize_bytes: &[u8], serialize_unit_struct: &'static str
    );

    fn serialize_f32(self, value: f32) -> Result<bool, Self::Error> {
        Ok(!NONE && !value.is_finite())
    }

    fn serialize_f64(self, value: f64) -> Result<bool, Self::Error> {
        Ok(!NONE && !value.is_finite())
    }

    fn serialize_none(self) -> Result<bool, Self::Error> {
        Ok(NONE)
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<bool, Self::Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<bool, Self::Error> {
        Ok(false)
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<bool, Self::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        value: &T,
    ) -> Result<bool, Self::Error> {
        value.serialize(self)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Members, Self::Error> {
        Ok(Members::default())
    }

    fn serialize_tuple(self, _: usize) -> Result<Members, Self::Error> {
        Ok(Members::default())
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Members, Self::Error> {
        Ok(Members::default())
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Members, Self::Error> {
        Ok(Members::default())
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Members, Self::Error> {
        Ok(Members::default())
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Members, Self::Error> {
        Ok(Members::default())
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Members, Self::Error> {
        Ok(Members::default())
    }
}

macro_rules! positional_members {
    ($($trait:ident::$method:ident),*) => {$(
        impl ser::$trait for Members {
            type Ok = bool;
            type Error = serde_json::Error;

            fn $method<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
                self.visit(value)
            }

            fn end(self) -> Result<bool, Self::Error> {
                Ok(self.0)
            }
        }
    )*};
}

positional_members!(
    SerializeSeq::serialize_element,
    SerializeTuple::serialize_element,
    SerializeTupleStruct::serialize_field,
    SerializeTupleVariant::serialize_field
);

macro_rules! named_members {
    ($($trait:ident),*) => {$(
        impl ser::$trait for Members {
            type Ok = bool;
            type Error = serde_json::Error;

            fn serialize_field<T: ?Sized + Serialize>(
                &mut self,
                _: &'static str,
                value: &T,
            ) -> Result<(), Self::Error> {
                self.visit(value)
            }

            fn end(self) -> Result<bool, Self::Error> {
                Ok(self.0)
            }
        }
    )*};
}

named_members!(SerializeStruct, SerializeStructVariant);

impl ser::SerializeMap for Members {
    type Ok = bool;
    type Error = serde_json::Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.visit(key)
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.visit(value)
    }

    fn end(self) -> Result<bool, Self::Error> {
        Ok(self.0)
    }
}
