//! What a value's JSON cannot say about it.
//!
//! JSON has a single `null`. `serde_json` writes a NaN or an infinite float
//! as `null`, a `Some` of anything it writes as `null` (a JSON `null` in an
//! `Option<serde_json::Value>`, `Some(None)`) reads back as `None`, and a
//! `None` and a JSON `null` document are written alike. One walk over the
//! value finds each, without serializing anything.

use serde::Serialize;
use serde::ser;

/// What `serde_json` writes as a `null` of a value that is not one.
#[derive(Clone, Copy, Default)]
pub(crate) struct JsonLoss {
    /// A NaN or infinite float.
    pub(crate) non_finite: bool,
    /// The value is an `Option::None`, through any `Some`s and newtypes, so
    /// its `null` is SQL NULL rather than a JSON `null` document.
    pub(crate) none: bool,
    /// A `null` inside a `Some`, which reads back as `None`.
    pub(crate) null_in_some: bool,
}

impl JsonLoss {
    fn merge(self, other: Self) -> Self {
        Self {
            non_finite: self.non_finite || other.non_finite,
            none: self.none || other.none,
            null_in_some: self.null_in_some || other.null_in_some,
        }
    }
}

/// What `value`'s JSON loses, or the error its `Serialize` reports.
pub(crate) fn json_loss<T: Serialize + ?Sized>(value: &T) -> Result<JsonLoss, serde_json::Error> {
    value.serialize(Probe {
        top: true,
        in_some: false,
    })
}

/// A serializer that writes nothing and notes what [`JsonLoss`] holds.
#[derive(Clone, Copy)]
struct Probe {
    /// At the value itself, through `Some`s and newtypes.
    top: bool,
    /// Inside a `Some`, where a `null` would read back as `None`.
    in_some: bool,
}

impl Probe {
    /// A member of a sequence, a map or a struct, which `serde_json` writes
    /// inside brackets, where a `null` reads back as a `null`.
    const MEMBER: Self = Self {
        top: false,
        in_some: false,
    };

    fn null(self) -> JsonLoss {
        JsonLoss {
            null_in_some: self.in_some,
            ..JsonLoss::default()
        }
    }

    fn float(value: f64) -> JsonLoss {
        JsonLoss {
            non_finite: !value.is_finite(),
            ..JsonLoss::default()
        }
    }
}

/// The loss of a compound value: its members'.
#[derive(Default)]
struct Members(JsonLoss);

impl Members {
    fn visit<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), serde_json::Error> {
        self.0 = self.0.merge(value.serialize(Probe::MEMBER)?);
        Ok(())
    }
}

macro_rules! written {
    ($($method:ident($type:ty)),* $(,)?) => {
        $(fn $method(self, _: $type) -> Result<JsonLoss, serde_json::Error> {
            Ok(JsonLoss::default())
        })*
    };
}

impl ser::Serializer for Probe {
    type Ok = JsonLoss;
    type Error = serde_json::Error;
    type SerializeSeq = Members;
    type SerializeTuple = Members;
    type SerializeTupleStruct = Members;
    type SerializeTupleVariant = Members;
    type SerializeMap = Members;
    type SerializeStruct = Members;
    type SerializeStructVariant = Members;

    written!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_i128(i128),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_u128(u128),
        serialize_char(char),
        serialize_str(&str),
        serialize_bytes(&[u8]),
    );

    fn serialize_f32(self, value: f32) -> Result<JsonLoss, serde_json::Error> {
        Ok(Self::float(value.into()))
    }

    fn serialize_f64(self, value: f64) -> Result<JsonLoss, serde_json::Error> {
        Ok(Self::float(value))
    }

    fn serialize_none(self) -> Result<JsonLoss, serde_json::Error> {
        Ok(JsonLoss {
            none: self.top,
            ..self.null()
        })
    }

    fn serialize_some<T: Serialize + ?Sized>(
        self,
        value: &T,
    ) -> Result<JsonLoss, serde_json::Error> {
        value.serialize(Probe {
            in_some: true,
            ..self
        })
    }

    fn serialize_unit(self) -> Result<JsonLoss, serde_json::Error> {
        Ok(self.null())
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<JsonLoss, serde_json::Error> {
        Ok(self.null())
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<JsonLoss, serde_json::Error> {
        Ok(JsonLoss::default())
    }

    // `serde_json` writes a newtype struct as its content.
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<JsonLoss, serde_json::Error> {
        value.serialize(self)
    }

    // ..and a newtype variant inside an object.
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        value: &T,
    ) -> Result<JsonLoss, serde_json::Error> {
        value.serialize(Probe {
            in_some: false,
            ..self
        })
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Members, serde_json::Error> {
        Ok(Members::default())
    }

    fn serialize_tuple(self, _: usize) -> Result<Members, serde_json::Error> {
        Ok(Members::default())
    }

    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Members, serde_json::Error> {
        Ok(Members::default())
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Members, serde_json::Error> {
        Ok(Members::default())
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Members, serde_json::Error> {
        Ok(Members::default())
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Members, serde_json::Error> {
        Ok(Members::default())
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Members, serde_json::Error> {
        Ok(Members::default())
    }
}

macro_rules! positional_members {
    ($($trait:ident::$method:ident),* $(,)?) => {$(
        impl ser::$trait for Members {
            type Ok = JsonLoss;
            type Error = serde_json::Error;

            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), serde_json::Error> {
                self.visit(value)
            }

            fn end(self) -> Result<JsonLoss, serde_json::Error> {
                Ok(self.0)
            }
        }
    )*};
}

positional_members!(
    SerializeSeq::serialize_element,
    SerializeTuple::serialize_element,
    SerializeTupleStruct::serialize_field,
    SerializeTupleVariant::serialize_field,
);

macro_rules! named_members {
    ($($trait:ident),* $(,)?) => {$(
        impl ser::$trait for Members {
            type Ok = JsonLoss;
            type Error = serde_json::Error;

            fn serialize_field<T: Serialize + ?Sized>(
                &mut self,
                _: &'static str,
                value: &T,
            ) -> Result<(), serde_json::Error> {
                self.visit(value)
            }

            fn end(self) -> Result<JsonLoss, serde_json::Error> {
                Ok(self.0)
            }
        }
    )*};
}

named_members!(SerializeStruct, SerializeStructVariant);

impl ser::SerializeMap for Members {
    type Ok = JsonLoss;
    type Error = serde_json::Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), serde_json::Error> {
        self.visit(key)
    }

    fn serialize_value<T: Serialize + ?Sized>(
        &mut self,
        value: &T,
    ) -> Result<(), serde_json::Error> {
        self.visit(value)
    }

    fn end(self) -> Result<JsonLoss, serde_json::Error> {
        Ok(self.0)
    }
}
