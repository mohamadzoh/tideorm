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

use serde::{Deserialize, Deserializer, Serialize, Serializer};
#[cfg(feature = "entity-manager")]
use std::sync::Arc;

#[cfg(feature = "entity-manager")]
use crate::entity_manager::{EntityManager, TideEntityManagerMeta};
use crate::error::Result;
use crate::model::Model;
use crate::query::QueryBuilder;

use super::helpers::{
    QuerySource, ensure_relation_configured, preserve_cached_value, required_key,
};
#[cfg(feature = "entity-manager")]
use super::helpers::{SnapshotOwner, register_loaded};

mod belongs_to;
mod has_many;
mod has_one;

pub use belongs_to::BelongsTo;
pub use has_many::HasMany;
pub use has_one::HasOne;

#[cfg(all(test, feature = "entity-manager"))]
#[path = "../../tests/unit/direct_entity_manager_relation_tests.rs"]
mod entity_manager_tests;
