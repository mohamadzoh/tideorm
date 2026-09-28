use super::*;

use crate::parse::{RelationKind, is_utc_datetime_type, option_inner_type};

/// The names a comma-separated attribute lists, trimmed; an empty one, such as
/// a trailing comma leaves, is an error.
pub(super) fn split_csv(
    struct_ident: &Ident,
    attribute: &str,
    value: Option<&String>,
) -> syn::Result<Option<Vec<String>>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let names: Vec<String> = value
        .split(',')
        .map(|part| part.trim().to_string())
        .collect();
    if names.iter().any(String::is_empty) {
        // The names come from an attribute string, so there is no token of their
        // own to span; the struct is the closest real location.
        return Err(syn::Error::new_spanned(
            struct_ident,
            format!("#[tideorm({attribute} = \"{value}\")] lists an empty name"),
        ));
    }
    Ok(Some(names))
}

/// The fields a field-list attribute names, by field or column name, each once.
pub(super) fn resolve_field_list<'a>(
    struct_ident: &Ident,
    attribute: &str,
    fields: &'a [ModelField],
    names: &[String],
) -> syn::Result<Vec<&'a ModelField>> {
    let mut resolved: Vec<&ModelField> = Vec::new();
    for name in names {
        let field = find_db_field(fields, name).ok_or_else(|| {
            syn::Error::new_spanned(
                struct_ident,
                format!(
                    "#[tideorm({attribute} = ...)] references unknown field or column '{name}'"
                ),
            )
        })?;
        if !resolved.iter().any(|seen| seen.name() == field.name()) {
            resolved.push(field);
        }
    }
    Ok(resolved)
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
        // A key the database does not number is left out of the INSERT, which
        // then fails; a `Uuid` key without `auto_increment` gets a random one.
        if field.auto_increment && !field.is_integer() {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "#[tideorm(auto_increment)] requires an integer primary key; a Uuid key \
                 gets a random value on create without it",
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
        if field.wraps_relation() {
            let message = format!(
                "a relation field is the `{wrapper}<..>` itself, not an Option, Box, Rc or Arc \
                 of one: an unloaded `{wrapper}` is already empty",
                wrapper = kind.wrapper()
            );
            combine_error(&mut errors, syn::Error::new_spanned(&field.ty, message));
            continue;
        }

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
    let resolved = resolve_field_list(struct_ident, "encrypted", fields, requested)?;
    for field in &resolved {
        if !field.supports_encryption() {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "#[tideorm(encrypted = ...)] only supports String/Text fields and Option<String>/Option<Text> fields",
            ));
        }

        // Every write stores a fresh ciphertext, so an encrypted key would
        // never match the plaintext `find`, `update` and `delete` look it up by.
        if field.primary_key {
            return Err(syn::Error::new_spanned(
                field.ident(),
                "#[tideorm(encrypted = ...)] cannot name a primary key field: its stored \
                 ciphertext changes on every write, so no lookup by the key could match it",
            ));
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
    db_fields: &[ModelField],
) -> syn::Result<()> {
    let mut errors = None;

    for index in indexes {
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

/// The model's unique indexes, or its other ones: named as declared, or by the
/// runtime as a migration names them, over the columns the table knows.
pub(super) fn build_index_impls(
    table_name: &str,
    indexes: &[IndexDef],
    unique: bool,
    db_fields: &[ModelField],
) -> Vec<TokenStream2> {
    indexes
        .iter()
        .filter(|index| index.unique == unique)
        .map(|index| {
            // An index may name a field; the table knows its column.
            let columns = index.columns.iter().map(|column| {
                find_db_field(db_fields, column).map_or_else(|| column.clone(), ModelField::column_name)
            });
            let columns = quote!(vec![#(#columns.to_string()),*]);
            match &index.name {
                Some(name) => quote!(::tideorm::model::IndexDefinition::new(#name, #columns, #unique)),
                None => quote!(::tideorm::model::IndexDefinition::__generated(#table_name, #columns, #unique)),
            }
        })
        .collect()
}

/// The keys `to_json` leaves out: the fields `hidden` names by field or
/// column name, as field names, and the attachment relations it names; else
/// the soft-delete field, or `deleted_at` on a model without one.
pub(super) fn resolve_hidden_fields(
    struct_ident: &Ident,
    fields: &[ModelField],
    attachments: &[&String],
    requested: Option<Vec<String>>,
    soft_delete_field: Option<&Ident>,
) -> syn::Result<Vec<String>> {
    let Some(requested) = requested else {
        return Ok(vec![soft_delete_field.map_or_else(
            || "deleted_at".to_string(),
            crate::parse::unraw_ident,
        )]);
    };

    requested
        .iter()
        .map(|name| {
            fields
                .iter()
                .find(|field| field.is_named(name))
                .map(ModelField::name)
                .or_else(|| attachments.contains(&name).then(|| name.clone()))
                .ok_or_else(|| {
                    syn::Error::new_spanned(
                        struct_ident,
                        format!(
                            "#[tideorm(hidden = ...)] references unknown field, column or attachment '{}'",
                            name
                        ),
                    )
                })
        })
        .collect()
}
