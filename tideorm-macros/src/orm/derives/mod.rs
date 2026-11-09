mod active_enum;
mod active_model;
mod attributes;
pub(crate) mod case_style;
mod column;
mod entity;
mod model;
mod value_type;
mod value_type_match;

use proc_macro2::Ident;
use syn::Type;

pub use active_enum::*;
pub(crate) use active_model::expand_active_model;
pub(crate) use column::{expand_column, expand_primary_key};
pub(crate) use entity::expand_entity;
pub(crate) use model::expand_model;
pub use value_type::*;

/// Whether `name` needs no escaping, so `Iden::quoted` can borrow it: the rule of
/// `sql_builder::is_static_iden`, which the builder's `Iden` derive also follows.
pub(crate) fn is_static_iden(name: &str) -> bool {
    name.chars()
        .take(1)
        .all(|c| c == '_' || c.is_ascii_alphabetic())
        && name.chars().all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// A model field as the engine's expanders take it.
pub(crate) struct EngineField<'a> {
    pub(crate) ident: &'a Ident,
    pub(crate) ty: &'a Type,
    /// The field's `Column` variant, or `None` for a field a row does not
    /// hold, which a decoded row gives its `Default`.
    pub(crate) column: Option<&'a Ident>,
    /// Whether the column is a TideORM encrypted field's: it holds the
    /// payload as text, which a row decrypts into the field's own type.
    pub(crate) encrypted: bool,
}

#[cfg(test)]
#[path = "../../../tests/unit/orm/derives_tests.rs"]
mod tests;
