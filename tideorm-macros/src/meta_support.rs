use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{Attribute, Meta, Path, Token, Type};

use crate::parse::{
    ModelField, find_db_field, is_naive_datetime_type, is_utc_datetime_type, option_inner_type,
};

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ExistingDerives {
    pub(crate) has_debug: bool,
    pub(crate) has_clone: bool,
    pub(crate) has_default: bool,
    pub(crate) has_serialize: bool,
    pub(crate) has_deserialize: bool,
}

pub(crate) fn detect_existing_derives(attrs: &[Attribute]) -> ExistingDerives {
    let mut existing = ExistingDerives::default();
    for attr in attrs {
        if !attr.path().is_ident("derive") {
            continue;
        }
        if let Meta::List(list) = &attr.meta
            && let Ok(paths) =
                Punctuated::<Path, Token![,]>::parse_terminated.parse2(list.tokens.clone())
        {
            for path in paths {
                existing.has_debug |= path_matches(&path, "Debug");
                existing.has_clone |= path_matches(&path, "Clone");
                existing.has_default |= path_matches(&path, "Default");
                existing.has_serialize |= path_matches(&path, "Serialize");
                existing.has_deserialize |= path_matches(&path, "Deserialize");
            }
        }
    }
    existing
}

fn path_matches(path: &Path, expected: &str) -> bool {
    path.segments
        .last()
        .is_some_and(|segment| segment.ident == expected)
}

pub(crate) fn pluralize(word: &str) -> String {
    match word {
        "person" => return "people".to_string(),
        "man" => return "men".to_string(),
        "woman" => return "women".to_string(),
        "child" => return "children".to_string(),
        "tooth" => return "teeth".to_string(),
        "foot" => return "feet".to_string(),
        "mouse" => return "mice".to_string(),
        "goose" => return "geese".to_string(),
        "leaf" => return "leaves".to_string(),
        "knife" => return "knives".to_string(),
        "life" => return "lives".to_string(),
        "wife" => return "wives".to_string(),
        "wolf" => return "wolves".to_string(),
        "calf" => return "calves".to_string(),
        "half" => return "halves".to_string(),
        "loaf" => return "loaves".to_string(),
        "self" => return "selves".to_string(),
        "shelf" => return "shelves".to_string(),
        "thief" => return "thieves".to_string(),
        "quiz" => return "quizzes".to_string(),
        "fez" => return "fezzes".to_string(),
        _ => {}
    }

    if word.ends_with('s')
        || word.ends_with('x')
        || word.ends_with('z')
        || word.ends_with("ch")
        || word.ends_with("sh")
    {
        format!("{}es", word)
    } else if word.ends_with('y')
        && !word.ends_with("ay")
        && !word.ends_with("ey")
        && !word.ends_with("oy")
        && !word.ends_with("uy")
    {
        format!("{}ies", &word[..word.len() - 1])
    } else {
        format!("{}s", word)
    }
}

/// Column names TideORM populates itself when a model opts into timestamps.
const AUTO_TIMESTAMP_COLUMNS: [&str; 2] = ["created_at", "updated_at"];

/// Whether the model declares both `created_at` and `updated_at`.
///
/// This is only the shorthand that lets a model opt into `ModelMeta::has_timestamps`
/// without spelling `#[tideorm(timestamps)]`; it is *not* what decides which columns
/// get populated. That decision is per field, in [`auto_timestamp_value`].
pub(crate) fn has_timestamp_pair(fields: &[ModelField]) -> bool {
    AUTO_TIMESTAMP_COLUMNS
        .iter()
        .all(|name| find_db_field(fields, name).is_some())
}

/// `Utc::now()` shaped to fit `ty`, or `None` when `ty` is not a chrono timestamp.
///
/// A `NaiveDateTime` gets the current UTC time without its zone, which is what
/// `timestamps_naive()` columns hold. An `Option<..>` column needs `Some(..)`, a
/// bare one the value itself. Both generated paths take the shape from here so an
/// optional column cannot end up with `Some(..)` on INSERT and a bare value on
/// UPDATE — that mismatch is an `E0308` inside the generated `ActiveModel`,
/// invisible to token-level tests.
fn timestamp_now_value(ty: &Type) -> Option<TokenStream2> {
    let inner = option_inner_type(ty);
    let base = inner.unwrap_or(ty);
    let now = if is_utc_datetime_type(base) {
        quote!(::tideorm::chrono::Utc::now())
    } else if is_naive_datetime_type(base) {
        quote!(::tideorm::chrono::Utc::now().naive_utc())
    } else {
        return None;
    };
    Some(if inner.is_some() {
        quote!(Some(#now))
    } else {
        now
    })
}

/// The value the generated insert path assigns to an auto-managed timestamp field,
/// or `None` when the caller's own value must be preserved.
///
/// A field qualifies on its own merits: it is named — or aliased with
/// `#[tideorm(column = ...)]` — `created_at` or `updated_at`, *and* it holds a chrono
/// `DateTime<Utc>` or `NaiveDateTime`. Anything else keeps whatever the caller set, so backfills, seeding
/// and migrations do not lose explicit timestamps, and a `created_at` column of some
/// other type does not produce a type error inside the generated entity.
///
/// There is deliberately no additional model-level gate. Requiring both halves of the
/// `created_at`/`updated_at` pair silently reverted a lone `created_at` to
/// `Default::default()` — `1970-01-01T00:00:00Z` — on every insert. Gating on
/// `#[tideorm(timestamps)]` instead would add nothing either: the attribute is opt-in
/// only, and it can never widen the set beyond the columns this predicate already
/// accepts, so honouring it is exactly what this per-field rule does.
pub(crate) fn auto_timestamp_value(field: &ModelField) -> Option<TokenStream2> {
    if !AUTO_TIMESTAMP_COLUMNS
        .iter()
        .any(|name| field.is_named(name))
    {
        return None;
    }

    timestamp_now_value(&field.ty)
}

/// The value the generated update path assigns to `updated_at`, or `None` for every
/// other field.
///
/// `created_at` is excluded on purpose: an UPDATE must not rewrite the creation time.
pub(crate) fn auto_updated_at_value(field: &ModelField) -> Option<TokenStream2> {
    if !field.is_named("updated_at") {
        return None;
    }

    timestamp_now_value(&field.ty)
}

/// Whether `field` is a managed `created_at`: stamped on insert and never written
/// again, by an update or by an upsert that finds the row already there.
pub(crate) fn is_managed_created_at(field: &ModelField) -> bool {
    auto_timestamp_value(field).is_some() && auto_updated_at_value(field).is_none()
}

/// Whether the model carries at least one timestamp column TideORM populates itself.
///
/// This is the metadata counterpart of [`auto_timestamp_value`]: it answers `true`
/// exactly when some column would be written by the generated insert path.
pub(crate) fn has_managed_timestamp_columns(fields: &[ModelField]) -> bool {
    fields
        .iter()
        .any(|field| auto_timestamp_value(field).is_some())
}
