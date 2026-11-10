//! The three key-to-key relation wrappers: [`HasOne`], [`HasMany`] and
//! [`BelongsTo`].
//!
//! These are what an ordinary model declares. Each is stored as a struct field,
//! rebuilt from the model's own columns by the derive-generated
//! `with_relations()`, and loaded on demand. They differ from the wrappers in
//! the sibling modules in one behavioural way worth knowing: whenever a database
//! connection is reachable, `load()` re-queries rather than serving whatever is
//! cached, so a deserialized JSON payload can never pass itself off as database
//! state.

#[cfg(feature = "entity-manager")]
use std::sync::Arc;

#[cfg(feature = "entity-manager")]
use crate::entity_manager::EntityManager;
use crate::error::Result;
use crate::internal::Value;
use crate::model::Model;
use crate::query::QueryBuilder;

use super::helpers::{ensure_relation_configured, required_key, where_key};
#[cfg(feature = "entity-manager")]
use super::helpers::{identity_keys, load_many_in_entity_manager, load_one_in_entity_manager};
use super::state::{RelationState, relation_serde};

/// What [`HasOne`] and [`HasMany`] share: they differ only in what they
/// cache.
macro_rules! key_pair_relation {
    ($relation:ident) => {
        impl<E: Model> $relation<E> {
            fn ensure_configured(&self) -> Result<()> {
                ensure_relation_configured(
                    stringify!($relation),
                    &[self.foreign_key, self.local_key],
                )
            }

            fn parent_key(&self, context: &str) -> Result<&Value> {
                self.ensure_configured()?;
                required_key(&self.state.key, "Parent primary key", context)
            }

            /// The query for the related rows.
            fn query(&self, context: &str) -> Result<QueryBuilder<E>> {
                let pk = self.parent_key(context)?;
                Ok(where_key(self.state.source.query(), self.foreign_key, pk))
            }

            /// Declare the relation's key pair.
            ///
            /// Both names must be non-empty; every method rejects a wrapper
            /// built by [`Default`] (which leaves them `""`) with
            #[doc = concat!("\"", stringify!($relation), " relation is not configured\".")]
            /// Pair with `with_parent_pk` to make it loadable — normally the
            /// derive does both for you.
            #[must_use]
            pub fn new(foreign_key: &'static str, local_key: &'static str) -> Self {
                Self {
                    foreign_key,
                    local_key,
                    ..Self::default()
                }
            }

            /// Record the relation's field name and its owner's table, which
            /// tell it apart from another relation when a rebuilt model takes
            /// over a previous copy's state.
            #[cfg(feature = "entity-manager")]
            #[doc(hidden)]
            pub fn with_metadata(
                mut self,
                relation_name: &'static str,
                owner_table: &'static str,
            ) -> Self {
                self.relation_name = relation_name;
                self.owner_table = owner_table;
                self
            }

            #[doc(hidden)]
            pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
                let same_columns = self.foreign_key == previous.foreign_key
                    && self.local_key == previous.local_key;
                #[cfg(feature = "entity-manager")]
                let same_columns = same_columns
                    && self.relation_name == previous.relation_name
                    && self.owner_table == previous.owner_table;

                self.state.preserve_from(
                    &previous.state,
                    same_columns,
                    previous.state.owner_is_new,
                );
            }

            /// Supply the owning model's [`local_key`](Self::local_key) value,
            /// which is what the related rows are looked up by, and whether the
            /// owner is new (its `is_new()`): the rows cached for a new owner
            /// carry over when saving it rebuilds the relation under its stored
            /// key.
            ///
            /// Must be a scalar: a composite key is rejected at load time
            /// because a single-column `WHERE` cannot express it. Without this
            /// the wrapper is inert — every load method errors with "Parent
            /// primary key not set for relation".
            #[doc(hidden)]
            pub fn with_parent_pk(mut self, pk: impl Into<Value>, owner_is_new: bool) -> Self {
                self.state.set_owner_key(pk.into(), owner_is_new);
                self
            }
        }

        impl<E: Model> Default for $relation<E> {
            fn default() -> Self {
                Self {
                    foreign_key: "",
                    local_key: "",
                    #[cfg(feature = "entity-manager")]
                    relation_name: "",
                    #[cfg(feature = "entity-manager")]
                    owner_table: "",
                    state: RelationState::default(),
                }
            }
        }

        relation_serde!($relation<E>);
    };
}

mod belongs_to;
mod has_many;
mod has_one;

pub use belongs_to::BelongsTo;
pub use has_many::HasMany;
pub use has_one::HasOne;

#[cfg(all(test, feature = "entity-manager"))]
#[path = "../../tests/unit/direct_entity_manager_relation_tests.rs"]
mod entity_manager_tests;
