//! Model trait and utilities.

#[allow(missing_docs)]
mod api;
mod batch;
mod builders;
mod crud;
mod meta;
mod nested;
mod serialization;

pub use api::Model;
pub use batch::{BatchUpdateBuilder, UpdateValue};
pub use builders::{CreateBuilder, OnConflictBuilder, UpdateBuilder};
pub use meta::{IndexDefinition, ModelMeta};
pub use nested::{NestedSave, NestedSaveBuilder};

#[cfg(test)]
#[path = "../testing/model_tests.rs"]
mod tests;
