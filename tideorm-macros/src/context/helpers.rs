use super::*;

use std::collections::HashSet;

use crate::parse::{RelationKind, is_utc_datetime_type, option_inner_type};

pub(super) fn split_csv(value: Option<&String>) -> Option<Vec<String>> {
    value.map(|value| {
        value
            .split(',')
            .map(|part| part.trim().to_string())
            .collect()
    })
}

fn combine_error(errors: &mut Option<syn::Error>, error: syn::Error) {
    match errors {
        Some(existing) => existing.combine(error),
        None => *errors = Some(error),
    }
}

fn into_result(errors: Option<syn::Error>) -> syn::Result<()> {
    match errors {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

pub(super) fn validate_primary_key_fields(
    struct_ident: &Ident,
    fields: &[ModelField],
    tokenize_enabled: bool,
) -> syn::Result<()> {
    let primary_key_fields: Vec<&ModelField> =
        fields.iter().filter(|field| field.primary_key).collect();

    if primary_key_fields.is_empty() {
        // Nothing more specific to point at: the offending thing is the *absence*
        // of an attribute, so the struct name is the tightest honest span.
        return Err(syn::Error::new_spanned(
            struct_ident,
            "TideORM models require exactly one #[tideorm(primary_key)] field",
        ));
    }

    if primary_key_fields.len() > 1 {
        if tokenize_enabled {
            // Point at the second primary key — the one that makes the set ambiguous.
            return Err(syn::Error::new_spanned(
                primary_key_fields[1].ident(),
                "#[tideorm(tokenize)] requires exactly one #[tideorm(primary_key)] field",
            ));
        }

        let mut errors = None;
        for field in primary_key_fields
            .iter()
            .filter(|field| field.auto_increment)
        {
            combine_error(
                &mut errors,
                syn::Error::new_spanned(
                    field.ident(),
                    "composite primary keys do not support #[tideorm(auto_increment)]",
                ),
            );
        }
        into_result(errors)?;
    }

    for field in fields {
        if field.auto_increment && !field.primary_key {
            return Err(syn::Error::new_spanned(
                field.ident(),
                "#[tideorm(auto_increment)] requires #[tideorm(primary_key)] on the same field",
            ));
        }
    }

    Ok(())
}

/// Checks every field's relation declaration against its type.
///
/// The wrapper type decides the relation kind, so a `has_one = ".."` style
/// attribute has to sit on the wrapper it names, and each kind has to spell out
/// the keys it has no sensible default for rather than silently joining on `id`.
pub(super) fn validate_relation_fields(fields: &[ModelField]) -> syn::Result<()> {
    let mut errors = None;

    for field in fields {
        let kind = field.relation_kind();

        for declared in field.declared_relation_kinds() {
            if kind != Some(declared) {
                let attribute = declared.attribute().unwrap_or_default();
                let message = format!(
                    "#[tideorm({attribute} = \"...\")] requires a `{}<..>` field",
                    declared.wrapper()
                );
                combine_error(&mut errors, syn::Error::new_spanned(&field.ty, message));
            }
        }

        let Some(kind) = kind else {
            continue;
        };

        let required: &[(&str, &Option<String>)] = match kind {
            RelationKind::HasOne | RelationKind::HasMany | RelationKind::BelongsTo => {
                &[("foreign_key", &field.foreign_key)]
            }
            RelationKind::HasManyThrough => &[
                ("pivot", &field.pivot),
                ("foreign_key", &field.foreign_key),
                ("related_key", &field.related_key),
            ],
            RelationKind::MorphOne | RelationKind::MorphMany | RelationKind::MorphTo => {
                &[("morph_name", &field.morph_name)]
            }
            RelationKind::SelfRef | RelationKind::SelfRefMany => &[],
        };
        let missing: Vec<String> = required
            .iter()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| format!("#[tideorm({name} = \"...\")]"))
            .collect();
        if missing.is_empty() {
            continue;
        }

        let subject = match kind.attribute() {
            Some(attribute) => format!("{attribute} relations"),
            None => "polymorphic relation fields".to_string(),
        };
        let message = format!("{subject} require {}", missing.join(", "));
        combine_error(&mut errors, syn::Error::new_spanned(field.ident(), message));
    }

    into_result(errors)
}

pub(super) fn resolve_encrypted_fields<'a>(
    struct_ident: &Ident,
    fields: &'a [ModelField],
    requested: &[String],
) -> syn::Result<Vec<&'a ModelField>> {
    let mut resolved = Vec::new();
    let mut seen = HashSet::new();

    for requested_name in requested {
        let field = find_db_field(fields, requested_name).ok_or_else(|| {
            // The name comes from an attribute string, so there is no token of its
            // own to span; the struct is the closest real location.
            syn::Error::new_spanned(
                struct_ident,
                format!(
                    "#[tideorm(encrypted = ...)] references unknown field or column '{}'",
                    requested_name
                ),
            )
        })?;

        if !field.supports_encryption() {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "#[tideorm(encrypted = ...)] only supports String/Text fields and Option<String>/Option<Text> fields",
            ));
        }

        if seen.insert(field.name()) {
            resolved.push(field);
        }
    }

    Ok(resolved)
}

/// The soft-delete timestamp field `key` names, which has to be an
/// `Option<chrono::DateTime<chrono::Utc>>` in some spelling.
pub(super) fn resolve_soft_delete_field(
    struct_ident: &Ident,
    fields: &[ModelField],
    key: &str,
) -> syn::Result<(Ident, String)> {
    let field = find_db_field(fields, key).ok_or_else(|| {
        syn::Error::new_spanned(
            struct_ident,
            format!(
                "soft_delete requires a field or column named '{}'; set #[tideorm(deleted_at_column = \"...\")] to override",
                key
            ),
        )
    })?;

    if !option_inner_type(&field.ty).is_some_and(is_utc_datetime_type) {
        return Err(syn::Error::new_spanned(
            &field.ty,
            "soft_delete field must have type Option<chrono::DateTime<chrono::Utc>>",
        ));
    }

    Ok((field.ident().clone(), field.column_name()))
}

/// Rejects malformed `#[index(..)]` / `#[unique_index(..)]` attributes and index
/// definitions that reference columns the model does not declare.
pub(super) fn validate_index_definitions(
    indexes: &[IndexDef],
    unique_indexes: &[IndexDef],
    db_fields: &[ModelField],
) -> syn::Result<()> {
    let mut errors = None;

    for index in indexes.iter().chain(unique_indexes) {
        if let Some(error) = &index.error {
            combine_error(&mut errors, error.clone());
            continue;
        }

        for column in &index.columns {
            if find_db_field(db_fields, column).is_none() {
                let attribute = index.attribute_name();
                let message =
                    format!("#[{attribute}(..)] references unknown field or column '{column}'");
                combine_error(&mut errors, syn::Error::new(index.span, message));
            }
        }
    }

    into_result(errors)
}

pub(super) fn build_index_impls(
    table_name: &str,
    indexes: &[IndexDef],
    unique: bool,
) -> Vec<TokenStream2> {
    indexes
        .iter()
        .map(|index| {
            let name = index.get_name(table_name);
            let columns = &index.columns;
            quote!(::tideorm::model::IndexDefinition::new(#name, vec![#(#columns.to_string()),*], #unique))
        })
        .collect()
}
