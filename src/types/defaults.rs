use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

/// Trait for models with computed/accessor attributes
pub trait Accessor {
    /// Get a computed attribute value
    fn get_accessor(&self, key: &str) -> Option<serde_json::Value>;

    /// List all accessor keys
    fn accessor_keys() -> Vec<&'static str> {
        vec![]
    }
}

/// Trait for models with mutator attributes
pub trait Mutator {
    /// Transform and set a value
    fn set_mutator(&mut self, key: &str, value: serde_json::Value) -> bool;

    /// List all mutator keys
    fn mutator_keys() -> Vec<&'static str> {
        vec![]
    }
}

/// Wrapper for fields with default values
#[derive(Clone, Debug)]
pub struct WithDefault<T> {
    value: Option<T>,
    _marker: PhantomData<T>,
}

impl<T: Clone> WithDefault<T> {
    /// Create a new WithDefault (no value set)
    pub fn none() -> Self {
        Self {
            value: None,
            _marker: PhantomData,
        }
    }

    /// Create with a value
    pub fn some(value: T) -> Self {
        Self {
            value: Some(value),
            _marker: PhantomData,
        }
    }

    /// Get the value or the provided default
    pub fn unwrap_or(&self, default: T) -> T {
        self.value.clone().unwrap_or(default)
    }

    /// Get the value or call a function for the default
    pub fn unwrap_or_else<F: FnOnce() -> T>(&self, f: F) -> T {
        self.value.clone().unwrap_or_else(f)
    }

    /// Check if value is set
    pub fn is_some(&self) -> bool {
        self.value.is_some()
    }

    /// Check if value is not set
    pub fn is_none(&self) -> bool {
        self.value.is_none()
    }

    /// Get the inner Option
    pub fn into_option(self) -> Option<T> {
        self.value
    }
}

impl<T: Default + Clone> Default for WithDefault<T> {
    fn default() -> Self {
        Self::some(T::default())
    }
}

impl<T: Serialize + Clone> Serialize for WithDefault<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.value.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for WithDefault<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Option::<T>::deserialize(deserializer).map(|v| Self {
            value: v,
            _marker: PhantomData,
        })
    }
}
