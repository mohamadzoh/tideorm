use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::marker::PhantomData;
#[cfg(feature = "entity-manager")]
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::model::Model;
use crate::query::QueryBuilder;

use super::helpers::{cached_ref, ensure_relation_configured, preserve_cached_value};
use super::require_scalar_relation_key;

fn has_active_database() -> bool {
    crate::database::__current_db().is_ok()
}

mod belongs_to;
mod has_many;
mod has_one;

pub use belongs_to::BelongsTo;
pub use has_many::HasMany;
pub use has_one::HasOne;
