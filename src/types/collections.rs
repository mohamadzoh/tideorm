use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Comma-separated string wrapper
///
/// Stores arrays as comma-separated strings in the database.
pub struct CommaSeparated<T> {
    values: Vec<T>,
}

impl<T> CommaSeparated<T> {
    /// Create a new comma-separated list
    pub fn new(values: Vec<T>) -> Self {
        Self { values }
    }

    /// Get the values
    pub fn values(&self) -> &[T] {
        &self.values
    }

    /// Get mutable values
    pub fn values_mut(&mut self) -> &mut Vec<T> {
        &mut self.values
    }

    /// Into inner values
    pub fn into_inner(self) -> Vec<T> {
        self.values
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Get length
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Add a value
    pub fn push(&mut self, value: T) {
        self.values.push(value);
    }

    /// Check if contains a value
    pub fn contains(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        self.values.contains(value)
    }
}

impl<T: FromStr> CommaSeparated<T>
where
    T::Err: fmt::Debug,
{
    /// Parse from comma-separated string
    pub fn from_string(s: &str) -> Self {
        let values = s
            .split(',')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        Self { values }
    }
}

impl<T> From<Vec<T>> for CommaSeparated<T> {
    fn from(values: Vec<T>) -> Self {
        Self::new(values)
    }
}

impl<T> FromIterator<T> for CommaSeparated<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            values: iter.into_iter().collect(),
        }
    }
}

impl<T> IntoIterator for CommaSeparated<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a CommaSeparated<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

impl<T: Serialize> Serialize for CommaSeparated<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.values.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for CommaSeparated<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Vec::<T>::deserialize(deserializer).map(|v| Self { values: v })
    }
}

impl<T: Default> Default for CommaSeparated<T> {
    fn default() -> Self {
        Self { values: Vec::new() }
    }
}

impl<T: fmt::Display> fmt::Display for CommaSeparated<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self
            .values
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",");
        write!(f, "{}", s)
    }
}

impl<T: Clone> Clone for CommaSeparated<T> {
    fn clone(&self) -> Self {
        Self {
            values: self.values.clone(),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for CommaSeparated<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommaSeparated")
            .field("values", &self.values)
            .finish()
    }
}

impl<T: PartialEq> PartialEq for CommaSeparated<T> {
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}

impl<T: Eq> Eq for CommaSeparated<T> {}

/// Collection wrapper for JSON array columns
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collection<T> {
    items: Vec<T>,
}

impl<T> Collection<T> {
    /// Create a new collection
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Create from a vector
    pub fn from_vec(items: Vec<T>) -> Self {
        Self { items }
    }

    /// Get all items
    pub fn all(&self) -> &[T] {
        &self.items
    }

    /// Get first item
    pub fn first(&self) -> Option<&T> {
        self.items.first()
    }

    /// Get last item
    pub fn last(&self) -> Option<&T> {
        self.items.last()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get count
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Add an item
    pub fn add(&mut self, item: T) {
        self.items.push(item);
    }

    /// Remove item at index
    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index < self.items.len() {
            Some(self.items.remove(index))
        } else {
            None
        }
    }

    /// Filter items
    pub fn filter<F: Fn(&T) -> bool>(&self, predicate: F) -> Self
    where
        T: Clone,
    {
        Self {
            items: self
                .items
                .iter()
                .filter(|i| predicate(i))
                .cloned()
                .collect(),
        }
    }

    /// Map items
    pub fn map<U, F: Fn(&T) -> U>(&self, mapper: F) -> Collection<U> {
        Collection {
            items: self.items.iter().map(mapper).collect(),
        }
    }

    /// Find item
    pub fn find<F: Fn(&T) -> bool>(&self, predicate: F) -> Option<&T> {
        self.items.iter().find(|i| predicate(i))
    }

    /// Check if any matches
    pub fn any<F: Fn(&T) -> bool>(&self, predicate: F) -> bool {
        self.items.iter().any(predicate)
    }

    /// Check if all match
    pub fn every<F: Fn(&T) -> bool>(&self, predicate: F) -> bool {
        self.items.iter().all(predicate)
    }

    /// Pluck values (for collections of objects)
    pub fn pluck<U, F: Fn(&T) -> U>(&self, extractor: F) -> Vec<U> {
        self.items.iter().map(extractor).collect()
    }

    /// Sort items (returns new collection)
    pub fn sorted<F: FnMut(&T, &T) -> std::cmp::Ordering>(&self, compare: F) -> Self
    where
        T: Clone,
    {
        let mut items = self.items.clone();
        items.sort_by(compare);
        Self { items }
    }

    /// Take first n items
    pub fn take(&self, n: usize) -> Self
    where
        T: Clone,
    {
        Self {
            items: self.items.iter().take(n).cloned().collect(),
        }
    }

    /// Skip first n items
    pub fn skip(&self, n: usize) -> Self
    where
        T: Clone,
    {
        Self {
            items: self.items.iter().skip(n).cloned().collect(),
        }
    }

    /// Convert to vector
    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.items.clone()
    }

    /// Into inner vector
    pub fn into_inner(self) -> Vec<T> {
        self.items
    }
}

impl<T> From<Vec<T>> for Collection<T> {
    fn from(items: Vec<T>) -> Self {
        Self::from_vec(items)
    }
}

impl<T> Default for Collection<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> FromIterator<T> for Collection<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            items: iter.into_iter().collect(),
        }
    }
}

impl<T> IntoIterator for Collection<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a Collection<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<T: Serialize> Serialize for Collection<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.items.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Collection<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Vec::<T>::deserialize(deserializer).map(|v| Self { items: v })
    }
}
