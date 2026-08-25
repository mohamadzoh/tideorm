use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{Attribute, GenericArgument, Meta, Path, PathArguments, Token, Type};

use crate::context::BuildContext;
use crate::parse::ModelField;

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
        if let Meta::List(list) = &attr.meta {
            if let Ok(paths) =
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

/// Return the payload of an `Option<..>` type, or `None` when `ty` is not an `Option`.
///
/// This inspects the parsed type instead of testing the stringified type for the
/// substring `"Option"`, so `OptionalMode`, `MyOptions` and `Vec<Option<String>>`
/// are all correctly reported as non-optional.
pub(crate) fn option_inner_type(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Group(group) => option_inner_type(&group.elem),
        Type::Paren(paren) => option_inner_type(&paren.elem),
        Type::Path(type_path) => {
            let segment = type_path.path.segments.last()?;
            if segment.ident != "Option" {
                return None;
            }

            match &segment.arguments {
                PathArguments::AngleBracketed(arguments) => {
                    arguments.args.iter().find_map(|argument| match argument {
                        GenericArgument::Type(inner) => Some(inner),
                        _ => None,
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Whether `ty` is an `Option<..>` and therefore maps to a nullable column.
pub(crate) fn is_optional_type(ty: &Type) -> bool {
    option_inner_type(ty).is_some()
}

/// Whether `field` carries `expected` as either its Rust field name or its column name.
fn matches_timestamp_name(field: &ModelField, expected: &str) -> bool {
    field.ident.as_ref().is_some_and(|ident| ident == expected)
        || BuildContext::column_name(field) == expected
}

/// Whether the model declares both `created_at` and `updated_at`.
///
/// This is only the shorthand that lets a model opt into `ModelMeta::has_timestamps`
/// without spelling `#[tideorm(timestamps)]`; it is *not* what decides which columns
/// get populated. That decision is per field, in [`auto_timestamp_value`].
pub(crate) fn has_timestamp_pair(fields: &[ModelField]) -> bool {
    let has_created_at = fields
        .iter()
        .any(|field| matches_timestamp_name(field, "created_at"));
    let has_updated_at = fields
        .iter()
        .any(|field| matches_timestamp_name(field, "updated_at"));
    has_created_at && has_updated_at
}

/// `Utc::now()` shaped to fit `ty`, or `None` when `ty` is not a chrono UTC timestamp.
///
/// An `Option<DateTime<Utc>>` column needs `Some(..)`, a bare `DateTime<Utc>` needs the
/// value itself. Both generated paths take the shape from here so an optional column
/// cannot end up with `Some(..)` on INSERT and a bare value on UPDATE — that mismatch
/// is an `E0308` inside the generated `ActiveModel`, invisible to token-level tests.
fn timestamp_now_value(ty: &Type) -> Option<TokenStream2> {
    match option_inner_type(ty) {
        Some(inner) if is_utc_datetime_type(inner) => {
            Some(quote!(Some(::tideorm::chrono::Utc::now())))
        }
        None if is_utc_datetime_type(ty) => Some(quote!(::tideorm::chrono::Utc::now())),
        _ => None,
    }
}

/// The value the generated insert path assigns to an auto-managed timestamp field,
/// or `None` when the caller's own value must be preserved.
///
/// A field qualifies on its own merits: it is named — or aliased with
/// `#[tideorm(column = ...)]` — `created_at` or `updated_at`, *and* it holds a chrono
/// UTC timestamp. Anything else keeps whatever the caller set, so backfills, seeding
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
    let is_timestamp_column = AUTO_TIMESTAMP_COLUMNS
        .iter()
        .any(|expected| matches_timestamp_name(field, expected));
    if !is_timestamp_column {
        return None;
    }

    timestamp_now_value(&field.ty)
}

/// The value the generated update path assigns to `updated_at`, or `None` for every
/// other field.
///
/// `created_at` is excluded on purpose: an UPDATE must not rewrite the creation time.
pub(crate) fn auto_updated_at_value(field: &ModelField) -> Option<TokenStream2> {
    if !matches_timestamp_name(field, "updated_at") {
        return None;
    }

    timestamp_now_value(&field.ty)
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

/// Whether `ty` spells `chrono::DateTime<chrono::Utc>` in any qualified form.
fn is_utc_datetime_type(ty: &Type) -> bool {
    match ty {
        Type::Group(group) => is_utc_datetime_type(&group.elem),
        Type::Paren(paren) => is_utc_datetime_type(&paren.elem),
        Type::Path(type_path) => {
            let Some(segment) = type_path.path.segments.last() else {
                return false;
            };
            if segment.ident != "DateTime" {
                return false;
            }
            let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                return false;
            };
            arguments.args.iter().any(|argument| match argument {
                GenericArgument::Type(inner) => terminal_ident_is(inner, "Utc"),
                _ => false,
            })
        }
        _ => false,
    }
}

/// Whether the final path segment of `ty` is named `expected`.
fn terminal_ident_is(ty: &Type, expected: &str) -> bool {
    match ty {
        Type::Group(group) => terminal_ident_is(&group.elem, expected),
        Type::Paren(paren) => terminal_ident_is(&paren.elem, expected),
        Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == expected),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{detect_existing_derives, is_optional_type, option_inner_type, pluralize};

    #[test]
    fn detect_existing_derives_finds_serialize_and_deserialize_together() {
        let item: syn::DeriveInput = syn::parse_quote! {
            #[derive(Serialize, Deserialize)]
            struct Example;
        };

        let existing = detect_existing_derives(&item.attrs);

        assert!(existing.has_serialize);
        assert!(existing.has_deserialize);
    }

    #[test]
    fn detect_existing_derives_finds_path_qualified_serialize() {
        let item: syn::DeriveInput = syn::parse_quote! {
            #[derive(serde::Serialize)]
            struct Example;
        };

        let existing = detect_existing_derives(&item.attrs);

        assert!(existing.has_serialize);
        assert!(!existing.has_deserialize);
    }

    #[test]
    fn detect_existing_derives_does_not_match_substrings() {
        let item: syn::DeriveInput = syn::parse_quote! {
            #[derive(MyDebugHelper, Cloneable, Defaultish, SerializeWith, DeserializeSeed)]
            struct Example;
        };

        let existing = detect_existing_derives(&item.attrs);

        assert!(!existing.has_debug);
        assert!(!existing.has_clone);
        assert!(!existing.has_default);
        assert!(!existing.has_serialize);
        assert!(!existing.has_deserialize);
    }

    #[test]
    fn detect_existing_derives_finds_path_qualified_standard_derives() {
        let item: syn::DeriveInput = syn::parse_quote! {
            #[derive(core::fmt::Debug, std::clone::Clone, ::serde::Deserialize, serde::Serialize)]
            struct Example;
        };

        let existing = detect_existing_derives(&item.attrs);

        assert!(existing.has_debug);
        assert!(existing.has_clone);
        assert!(existing.has_serialize);
        assert!(existing.has_deserialize);
    }

    #[test]
    fn pluralize_handles_common_irregular_nouns() {
        assert_eq!(pluralize("person"), "people");
        assert_eq!(pluralize("child"), "children");
        assert_eq!(pluralize("mouse"), "mice");
    }

    #[test]
    fn pluralize_handles_f_and_fe_endings() {
        assert_eq!(pluralize("leaf"), "leaves");
        assert_eq!(pluralize("knife"), "knives");
        assert_eq!(pluralize("profile"), "profiles");
        assert_eq!(pluralize("roof"), "roofs");
        assert_eq!(pluralize("belief"), "beliefs");
        assert_eq!(pluralize("chef"), "chefs");
        assert_eq!(pluralize("cliff"), "cliffs");
        assert_eq!(pluralize("proof"), "proofs");
        assert_eq!(pluralize("staff"), "staffs");
    }

    #[test]
    fn pluralize_handles_z_suffixes() {
        assert_eq!(pluralize("quiz"), "quizzes");
        assert_eq!(pluralize("fez"), "fezzes");
        assert_eq!(pluralize("bus"), "buses");
        assert_eq!(pluralize("status"), "statuses");
        assert_eq!(pluralize("topaz"), "topazes");
        assert_eq!(pluralize("index"), "indexes");
    }

    #[test]
    fn option_detection_is_structural_not_a_substring_test() {
        assert!(is_optional_type(&syn::parse_quote!(Option<String>)));
        assert!(is_optional_type(&syn::parse_quote!(
            std::option::Option<i64>
        )));
        assert!(option_inner_type(&syn::parse_quote!(Option<i64>)).is_some());

        assert!(!is_optional_type(&syn::parse_quote!(OptionalMode)));
        assert!(!is_optional_type(&syn::parse_quote!(MyOptions)));
        assert!(!is_optional_type(&syn::parse_quote!(Vec<Option<String>>)));
        assert!(option_inner_type(&syn::parse_quote!(String)).is_none());
    }

    /// Expand a model definition through the real derive and return its tokens with all
    /// whitespace removed, which is how the assertions below match generated fragments.
    fn expand_model_tokens(input: syn::DeriveInput) -> String {
        use darling::FromDeriveInput;

        let existing_derives = detect_existing_derives(&input.attrs);
        let (indexes, unique_indexes) = crate::parse::parse_index_attributes(&input.attrs);
        let model_input =
            crate::parse::ModelInput::from_derive_input(&input).expect("model input should parse");

        crate::generate_model_impl(&model_input, indexes, unique_indexes, &existing_derives)
            .to_string()
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect()
    }

    /// The body of one generated fn, sliced out of the normalized expansion.
    ///
    /// The insert and update paths emit setters that read identically, so a `contains` over
    /// the whole expansion cannot tell which path it matched — which is exactly how an
    /// `updated_at` that was right on INSERT and wrong on UPDATE slipped through.
    fn generated_fn_body<'a>(expanded: &'a str, start_marker: &str, end_marker: &str) -> &'a str {
        let start = expanded
            .find(start_marker)
            .unwrap_or_else(|| panic!("expansion should contain `{start_marker}`"));
        let rest = &expanded[start..];
        let end = rest
            .find(end_marker)
            .unwrap_or_else(|| panic!("`{end_marker}` should follow `{start_marker}`"));
        &rest[..end]
    }

    /// The INSERT path, `InternalModel::into_active_model`.
    fn insert_setters(expanded: &str) -> &str {
        generated_fn_body(
            expanded,
            "fninto_active_model(self)",
            "fntry_into_active_model",
        )
    }

    /// The UPDATE path, `__into_update_active_model`.
    fn update_setters(expanded: &str) -> &str {
        generated_fn_body(
            expanded,
            "fn__into_update_active_model",
            "fn__into_delete_active_model",
        )
    }

    /// Auto-population is decided per column, and `has_timestamps()` reports the same
    /// answer.
    ///
    /// Gating on the `created_at`/`updated_at` *pair* left a lone `created_at` written
    /// through verbatim, so a default-constructed model persisted `1970-01-01T00:00:00Z`
    /// while `has_timestamps()` still claimed the model was managed.
    #[test]
    fn timestamp_population_and_has_timestamps_agree() {
        let now = "ActiveValue::Set(::tideorm::chrono::Utc::now())";

        // A lone `created_at` is still populated, and reported.
        let lone = expand_model_tokens(syn::parse_quote! {
            struct Signup {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                created_at: chrono::DateTime<chrono::Utc>,
            }
        });
        let insert = insert_setters(&lone);
        assert!(insert.contains(&format!("created_at:{}", now)));
        assert!(!insert.contains("created_at:ActiveValue::Set(self.created_at)"));
        assert!(lone.contains("fnhas_timestamps()->bool{true}"));

        // An aliased column counts the same way: the column name is what matters.
        let aliased = expand_model_tokens(syn::parse_quote! {
            struct Registration {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                #[tideorm(column = "created_at")]
                signed_up: chrono::DateTime<chrono::Utc>,
            }
        });
        assert!(insert_setters(&aliased).contains(&format!("signed_up:{}", now)));
        assert!(aliased.contains("fnhas_timestamps()->bool{true}"));

        // The declared attribute agrees with the columns it manages.
        let declared = expand_model_tokens(syn::parse_quote! {
            #[tideorm(timestamps)]
            struct Session {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                created_at: chrono::DateTime<chrono::Utc>,
                updated_at: chrono::DateTime<chrono::Utc>,
            }
        });
        let insert = insert_setters(&declared);
        assert!(insert.contains(&format!("created_at:{}", now)));
        assert!(insert.contains(&format!("updated_at:{}", now)));
        assert!(declared.contains("fnhas_timestamps()->bool{true}"));

        // A `created_at` of an unmanageable type keeps the caller's value, and is not
        // reported as managed either.
        let untyped = expand_model_tokens(syn::parse_quote! {
            struct Event {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                created_at: i64,
            }
        });
        let insert = insert_setters(&untyped);
        assert!(insert.contains("created_at:ActiveValue::Set(self.created_at)"));
        assert!(!insert.contains(&format!("created_at:{}", now)));
        assert!(untyped.contains("fnhas_timestamps()->bool{false}"));

        // No timestamp columns at all: nothing to populate, nothing to report.
        let none = expand_model_tokens(syn::parse_quote! {
            struct Tag {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                name: String,
            }
        });
        assert!(none.contains("fnhas_timestamps()->bool{false}"));
    }

    /// An `Option<DateTime<Utc>>` timestamp has to get `Some(..)` on *both* paths.
    ///
    /// The update path used to emit a bare `Set(Utc::now())` for `updated_at`, which is an
    /// `E0308` inside the generated `ActiveModel` for an optional column — the shape the
    /// insert path advertises. `created_at` must survive an update untouched.
    #[test]
    fn optional_timestamp_columns_are_populated_with_some() {
        let expanded = expand_model_tokens(syn::parse_quote! {
            struct AuditEntry {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                created_at: Option<chrono::DateTime<chrono::Utc>>,
                updated_at: Option<chrono::DateTime<chrono::Utc>>,
            }
        });

        let now = "ActiveValue::Set(Some(::tideorm::chrono::Utc::now()))";
        let insert = insert_setters(&expanded);
        assert!(insert.contains(&format!("created_at:{}", now)));
        assert!(insert.contains(&format!("updated_at:{}", now)));

        let update = update_setters(&expanded);
        assert!(update.contains(&format!("updated_at:{}", now)));
        assert!(update.contains("created_at:ActiveValue::Set(self.created_at)"));
    }

    /// A non-optional pair keeps the bare `Utc::now()` shape on the update path, and an
    /// `updated_at` TideORM cannot manage is written through instead of type-erroring.
    #[test]
    fn the_update_path_matches_the_insert_path_on_updated_at() {
        let plain = expand_model_tokens(syn::parse_quote! {
            struct Article {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                created_at: chrono::DateTime<chrono::Utc>,
                updated_at: chrono::DateTime<chrono::Utc>,
            }
        });
        let update = update_setters(&plain);
        assert!(update.contains("updated_at:ActiveValue::Set(::tideorm::chrono::Utc::now())"));
        assert!(update.contains("created_at:ActiveValue::Set(self.created_at)"));

        let untyped = expand_model_tokens(syn::parse_quote! {
            struct Reading {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                updated_at: i64,
            }
        });
        let update = update_setters(&untyped);
        assert!(update.contains("updated_at:ActiveValue::Set(self.updated_at)"));
        assert!(!update.contains("updated_at:ActiveValue::Set(::tideorm::chrono::Utc::now())"));
    }

    /// A composite key is unsaved when *any* component is still at its default.
    ///
    /// Requiring every component to be default was tried and reverted: it reads better for
    /// a persisted `(42, "")`, but it makes the failure silent, routing a genuinely new row
    /// to `update()` where it quietly affects zero rows. ORing routes it to `create()`,
    /// where a real collision surfaces loudly as a duplicate-key error. The runtime
    /// counterpart is `test_is_new_treats_defaulted_composite_primary_key_component_as_unsaved`.
    #[test]
    fn composite_primary_keys_are_new_when_any_component_is_default() {
        let expanded = expand_model_tokens(syn::parse_quote! {
            struct Membership {
                #[tideorm(primary_key)]
                user_id: i64,
                #[tideorm(primary_key)]
                role_id: i64,
            }
        });

        let expected = "false||__tideorm_is_default(&pk_0)||__tideorm_is_default(&pk_1)";
        assert!(expanded.contains(expected));
        assert!(!expanded.contains("true&&__tideorm_is_default"));
    }

    #[test]
    fn skipped_fields_are_defaulted_in_generated_constructors() {
        let expanded = expand_model_tokens(syn::parse_quote! {
            struct User {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                email: String,
                #[tideorm(skip)]
                cached_label: Option<String>,
            }
        });

        let expected = "id:model.id,email:model.email,cached_label:Default::default(),";
        assert!(expanded.contains(expected));
        assert!(!expanded.contains("cached_label:model.cached_label"));
    }

    /// Two relations to the same model are rejected rather than deduplicated.
    ///
    /// Rust permits one `Related<X>` impl per entity pair, and sea-orm's eager loaders
    /// resolve through that impl alone. Keeping only the first one compiled, but made
    /// `.with("editor")` join on `author_id` and turned a mixed `HasMany`/`HasOne` pair
    /// into a runtime cardinality error. A compile error beats silently wrong rows.
    #[test]
    fn two_relations_to_the_same_model_are_rejected() {
        let expanded = expand_model_tokens(syn::parse_quote! {
            struct Article {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                author_id: i64,
                editor_id: i64,
                #[tideorm(belongs_to = "User", foreign_key = "author_id")]
                author: BelongsTo<User>,
                #[tideorm(belongs_to = "User", foreign_key = "editor_id")]
                editor: BelongsTo<User>,
            }
        });

        assert!(expanded.contains("compile_error!"));
        // Both conflicting fields and the shared target are named.
        assert!(expanded.contains("relations`author`and`editor`bothtarget`User`"));
        // ...along with the reason and the way out.
        assert!(expanded.contains("sea-ormpermitsone`Related<User>`implperentitypair"));
        assert!(expanded.contains("load`editor`explicitlywithitsownqueryinsteadofeagerly"));
        assert!(!expanded.contains("orm::Related<<Useras"));

        // Distinct targets are untouched: one `Related` impl each, one arm per field.
        let ok = expand_model_tokens(syn::parse_quote! {
            struct Post {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                user_id: i64,
                #[tideorm(belongs_to = "User", foreign_key = "user_id")]
                author: BelongsTo<User>,
                #[tideorm(has_many = "Comment", foreign_key = "post_id")]
                comments: HasMany<Comment>,
            }
        });

        assert!(!ok.contains("compile_error!"));
        assert_eq!(ok.matches("orm::Related<<Useras").count(), 1);
        assert_eq!(ok.matches("orm::Related<<Commentas").count(), 1);
        assert!(ok.contains("Self::Author=>"));
        assert!(ok.contains("Self::Comments=>"));
    }

    /// `deleted_at_column` without `soft_delete` used to compile to hard-delete
    /// semantics with the override silently dropped.
    #[test]
    fn deleted_at_column_without_soft_delete_is_rejected() {
        let expanded = expand_model_tokens(syn::parse_quote! {
            #[tideorm(deleted_at_column = "archived_at")]
            struct Post {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                archived_at: Option<chrono::DateTime<chrono::Utc>>,
            }
        });

        assert!(expanded.contains("compile_error!"));
        assert!(expanded.contains("hasnoeffectwithout"));

        // The pair together stays valid.
        let ok = expand_model_tokens(syn::parse_quote! {
            #[tideorm(soft_delete, deleted_at_column = "archived_at")]
            struct Article {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                archived_at: Option<chrono::DateTime<chrono::Utc>>,
            }
        });
        assert!(!ok.contains("compile_error!"));
    }

    #[test]
    fn searchable_must_name_a_real_field_or_column() {
        let expanded = expand_model_tokens(syn::parse_quote! {
            #[tideorm(searchable = "emial")]
            struct User {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                email: String,
            }
        });

        assert!(expanded.contains("compile_error!"));
        assert!(expanded.contains("referencesunknownfieldorcolumn'emial'"));
    }

    /// `tokenization_enabled` is emitted on `Tokenizable` only, never on `ModelMeta`.
    ///
    /// 0.10 removed `tokenization_enabled`/`token_encoder`/`token_decoder` from `ModelMeta`:
    /// nothing in the runtime read them, the macro never emitted two of the three, and
    /// declaring them on both traits is what made the accessor an E0034 ambiguity. Emitting
    /// the method into the `ModelMeta` impl now would not compile downstream, since the
    /// trait no longer declares it.
    #[test]
    fn tokenization_state_is_reported_on_tokenizable_only() {
        let plain = expand_model_tokens(syn::parse_quote! {
            struct Plain {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
            }
        });
        assert!(!plain.contains("fntokenization_enabled()"));

        let tokenized = expand_model_tokens(syn::parse_quote! {
            #[tideorm(tokenize)]
            struct Tokenized {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
            }
        });
        assert_eq!(
            tokenized
                .matches("fntokenization_enabled()->bool{true}")
                .count(),
            1,
            "exactly one impl (Tokenizable) should carry the accessor"
        );
    }

    #[test]
    fn infallible_conversions_never_handle_encrypted_columns_in_plaintext() {
        let expanded = expand_model_tokens(syn::parse_quote! {
            #[tideorm(encrypted = "phone")]
            struct Customer {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                phone: String,
            }
        });

        assert!(expanded.contains("phone:ActiveValue::NotSet"));
        assert!(!expanded.contains("phone:ActiveValue::Set(self.phone)"));
        assert!(expanded.contains("__encrypt_model_field"));
        assert!(expanded.contains("from_entity_modelcannotdecryptencryptedfields"));
    }
}
