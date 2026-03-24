//! Model Relations System
//!
//! This module provides model relations using SeaORM-style field declarations.
//! Relations are defined as struct fields with attributes, following SeaORM's pattern.
//!
//! ## Supported Relations
//!
//! - `HasOne<E>`: One-to-one relation (e.g., User has_one Profile)
//! - `HasMany<E>`: One-to-many relation (e.g., User has_many Posts)
//! - `BelongsTo<E>`: Inverse of HasOne/HasMany (e.g., Post belongs_to User)
//!
//! ## Defining Relations Inside Models
//!
//! Relations are declared as fields in your model struct using `#[tideorm(...)]` relation attributes:
//!
//! Use `HasOne`, `HasMany`, and `BelongsTo` fields plus the relation attributes
//! on your model structs to declare relationships.
//!
//! ## Many-to-Many Relations
//!
//! Use `HasManyThrough` to model many-to-many relationships via a join model.
//!
//! ## Relation Constraints
//!
//! You can add constraints to relation queries:
//!
//! Use `load_with` to constrain relation queries with filters, ordering, and limits.

#[allow(missing_docs)]
mod direct;
#[allow(missing_docs)]
mod eager;
mod helpers;
#[allow(missing_docs)]
mod many_to_many;
#[allow(missing_docs)]
mod metadata;
#[allow(missing_docs)]
mod polymorphic;
#[allow(missing_docs)]
mod self_referencing;

#[cfg(test)]
pub(crate) use crate::internal::Value;
#[cfg(test)]
pub(crate) use helpers::build_self_ref_tree_sql;

pub use direct::{BelongsTo, HasMany, HasOne};
pub use eager::{
    EagerLoadExt, EagerLoadModel, EagerQueryBuilder, RelationConstraints, RelationExt,
    RelationLoader, RelationPath, RelationTree, WithRelations,
};
pub use many_to_many::{HasManyThrough, WithPivot};
pub use metadata::{RelationInfo, RelationType};
pub use polymorphic::{MorphMany, MorphOne, MorphResult, MorphResult3, MorphResult4, MorphTo};
pub use self_referencing::{SelfRef, SelfRefMany};

#[cfg(test)]
#[path = "../testing/relations_tests.rs"]
mod tests;

pub(crate) fn require_scalar_relation_key<'a>(
    value: &'a serde_json::Value,
    context: &str,
) -> crate::error::Result<&'a serde_json::Value> {
    if value.is_array() || value.is_object() {
        return Err(crate::error::Error::invalid_query(format!(
            "{} only supports scalar relation keys; composite primary keys require an explicit single-column relation key or a custom query",
            context
        )));
    }

    Ok(value)
}
