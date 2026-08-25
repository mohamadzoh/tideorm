//! Model relations.
//!
//! Relations are declared as *struct fields* — this is the defining shape of the
//! library. A `#[tideorm(..)]` attribute on the field tells the derive which
//! columns link the two models, and the derive's `with_relations()` rebuilds the
//! wrapper from the model's own column values every time a model is loaded:
//!
//! ```ignore
//! #[tideorm::model(table = "users")]
//! pub struct User {
//!     #[tideorm(primary_key, auto_increment)]
//!     pub id: i64,
//!     #[tideorm(has_one = "Profile", foreign_key = "user_id")]
//!     pub profile: HasOne<Profile>,
//!     #[tideorm(has_many = "Post", foreign_key = "user_id")]
//!     pub posts: HasMany<Post>,
//! }
//!
//! let posts = user.posts.load().await?;
//! ```
//!
//! ## Choosing a wrapper
//!
//! | Wrapper | Shape |
//! |---|---|
//! | [`HasOne`] / [`HasMany`] | The foreign key lives on the *related* table. |
//! | [`BelongsTo`] | The foreign key lives on *this* table. |
//! | [`HasManyThrough`](crate::relations::HasManyThrough) | Reached through a pivot table. |
//! | [`MorphOne`](crate::relations::MorphOne) / [`MorphMany`](crate::relations::MorphMany) / [`MorphTo`](crate::relations::MorphTo) | Linked by a `(type, id)` column pair. |
//! | [`SelfRef`](crate::relations::SelfRef) / [`SelfRefMany`](crate::relations::SelfRefMany) | Parent and children within one table. |
//!
//! ## Loading
//!
//! Every wrapper has `load()`, and most also have `load_with(|q| ..)` to add
//! ordering, paging or extra filters to the relation's own query. That is
//! per-model — lazy loading in a loop is N+1. Eager loading
//! (`User::query().with("posts.comments")`) resolves each level for all parents
//! at once instead; see [`EagerQueryBuilder`](crate::relations::EagerQueryBuilder).
//!
//! ## Cross-cutting behaviour worth knowing
//!
//! - **Serde destroys the wrappers' runtime state.** Only the cached payload
//!   survives a round trip; the keys and connection that make a relation
//!   loadable do not. A model rebuilt from JSON has dead relations until
//!   [`refresh_runtime_relations_from`](crate::internal::InternalModel::refresh_runtime_relations_from)
//!   re-derives them from the fresh column values, which is what TideORM's own
//!   JSON paths do. Any new such path must call it too.
//! - **`load()` is not uniform.** [`HasOne`], [`HasMany`], [`BelongsTo`] and
//!   [`HasManyThrough`](crate::relations::HasManyThrough) prefer the database whenever a connection is reachable,
//!   so a deserialized payload cannot pass itself off as database state. The
//!   morph and self-referencing wrappers are cache-first: a cached value is
//!   returned as-is. Each type's own documentation states which it is.
//! - **Eager loading respects soft deletes**, applying the *related* model's
//!   scope, and batches nested levels rather than recursing per parent.
//! - **[`MorphTo`](crate::relations::MorphTo), [`SelfRef`](crate::relations::SelfRef) and [`SelfRefMany`](crate::relations::SelfRefMany) have no eager path.**
//!   Requesting one with `.with(..)` is an error naming the limitation, not a
//!   silent empty result.

mod direct;
mod eager;
mod helpers;
mod many_to_many;
mod metadata;
mod polymorphic;
mod self_referencing;

#[cfg(test)]
pub(crate) use crate::internal::Value;
#[cfg(test)]
pub(crate) use helpers::build_self_ref_tree_sql;

#[cfg(feature = "entity-manager")]
pub use crate::entity_manager::TrackedHasMany as HasMany;
#[cfg(not(feature = "entity-manager"))]
pub use direct::HasMany;
#[cfg(feature = "entity-manager")]
pub(crate) use direct::HasMany as DirectHasMany;
pub use direct::{BelongsTo, HasOne};
// `EagerLoadModel` is exported from this module and from nowhere else on
// purpose: it is `#[doc(hidden)]` machinery whose only method is `__eager_load`,
// and macro-generated code names it through the fully qualified
// `::tideorm::relations::EagerLoadModel` path. It is deliberately absent from
// the crate root and the prelude — see the matching note in `lib.rs` — so do not
// "restore" it there for symmetry with the other relation exports.
pub use eager::{
    EagerLoadExt, EagerLoadModel, EagerQueryBuilder, RelationConstraints, RelationExt,
    RelationPath, RelationTree, WithRelations,
};
pub use many_to_many::{HasManyThrough, WithPivot};
pub use metadata::{RelationInfo, RelationType};
pub use polymorphic::{MorphMany, MorphOne, MorphResult, MorphResult3, MorphResult4, MorphTo};
pub use self_referencing::{SelfRef, SelfRefMany};

#[cfg(test)]
#[path = "../../tests/unit/relations_tests.rs"]
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
