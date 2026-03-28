//! Model APIs and shared model-side helpers.
//!
//! This module is the entry point for CRUD helpers, batch updates, nested save
//! behavior, and model metadata.
//!
//! If model behavior looks wrong, start in the submodule that matches the
//! failing operation:
//! - `crud` for normal create, update, delete, and lookup paths
//! - `batch` for bulk update behavior
//! - `nested` for relation-aware saves
//! - `serialization` when attributes are not loading or persisting as expected

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
pub use nested::{NestedSave, NestedSaveBuilder, SavedRelation};

#[cfg(test)]
#[path = "../testing/model_tests.rs"]
mod tests;
