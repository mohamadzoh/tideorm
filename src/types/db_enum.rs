use serde::{Deserialize, Serialize};
use std::fmt;

/// Enum wrapper for database enums
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbEnum<E>(pub E);

impl<E: Serialize> Serialize for DbEnum<E> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de, E: Deserialize<'de>> Deserialize<'de> for DbEnum<E> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        E::deserialize(deserializer).map(DbEnum)
    }
}

impl<E: fmt::Display> fmt::Display for DbEnum<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<E> From<E> for DbEnum<E> {
    fn from(e: E) -> Self {
        DbEnum(e)
    }
}

impl<E> DbEnum<E> {
    /// Get the inner value
    pub fn into_inner(self) -> E {
        self.0
    }

    /// Get a reference to the inner value
    pub fn inner(&self) -> &E {
        &self.0
    }
}
