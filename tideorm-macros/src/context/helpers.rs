use super::*;

use std::collections::HashSet;

pub(super) fn split_csv(value: Option<&String>) -> Option<Vec<String>> {
    value.map(|value| {
        value
            .split(',')
            .map(|part| part.trim().to_string())
            .collect()
    })
}

pub(super) fn validate_primary_key_fields(
    fields: &[ModelField],
    tokenize_enabled: bool,
) -> syn::Result<()> {
    let primary_key_fields: Vec<&ModelField> =
        fields.iter().filter(|field| field.primary_key).collect();

    if primary_key_fields.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "TideORM models require exactly one #[tideorm(primary_key)] field",
        ));
    }

    if primary_key_fields.len() > 1 {
        if tokenize_enabled {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "#[tideorm(tokenize)] requires exactly one #[tideorm(primary_key)] field",
            ));
        }

        if primary_key_fields.iter().any(|field| field.auto_increment) {
            let mut errors: Option<syn::Error> = None;
            for field in primary_key_fields
                .iter()
                .filter(|field| field.auto_increment)
            {
                let error = syn::Error::new_spanned(
                    field
                        .ident
                        .as_ref()
                        .expect("database fields must have identifiers"),
                    "composite primary keys do not support #[tideorm(auto_increment)]",
                );
                if let Some(existing) = &mut errors {
                    existing.combine(error);
                } else {
                    errors = Some(error);
                }
            }

            return Err(errors.expect("composite auto increment errors should exist"));
        }
    }

    for field in fields {
        if field.auto_increment && !field.primary_key {
            return Err(syn::Error::new_spanned(
                field
                    .ident
                    .as_ref()
                    .expect("database fields must have identifiers"),
                "#[tideorm(auto_increment)] requires #[tideorm(primary_key)] on the same field",
            ));
        }
    }

    Ok(())
}

pub(super) fn validate_relation_fields(fields: &[ModelField]) -> syn::Result<()> {
    let mut errors: Option<syn::Error> = None;

    for field in fields {
        if matches!(
            relation_wrapper_name(&field.ty),
            Some("MorphOne" | "MorphMany" | "MorphTo")
        ) && field.morph_name.is_none()
        {
            let field_ident = field
                .ident
                .as_ref()
                .expect("relation fields must have identifiers");
            let error = syn::Error::new_spanned(
                field_ident,
                "polymorphic relation fields require #[tideorm(morph_name = \"...\")]",
            );

            if let Some(existing) = &mut errors {
                existing.combine(error);
            } else {
                errors = Some(error);
            }
        }

        if field.has_many_through.is_some() {
            let mut missing = Vec::new();
            if field.pivot.is_none() {
                missing.push("pivot");
            }
            if field.foreign_key.is_none() {
                missing.push("foreign_key");
            }
            if field.related_key.is_none() {
                missing.push("related_key");
            }

            if !missing.is_empty() {
                let field_ident = field
                    .ident
                    .as_ref()
                    .expect("relation fields must have identifiers");
                let requirement_list = missing
                    .iter()
                    .map(|name| format!("#[tideorm({name} = \"...\")]"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let error = syn::Error::new_spanned(
                    field_ident,
                    format!("has_many_through relations require {}", requirement_list),
                );

                if let Some(existing) = &mut errors {
                    existing.combine(error);
                } else {
                    errors = Some(error);
                }
            }
        }
    }

    if let Some(error) = errors {
        return Err(error);
    }

    Ok(())
}

pub(super) fn resolve_encrypted_fields<'a>(
    fields: &'a [ModelField],
    requested: &[String],
) -> syn::Result<Vec<&'a ModelField>> {
    let mut resolved = Vec::new();
    let mut seen = HashSet::new();

    for requested_name in requested {
        let field = fields
            .iter()
            .find(|field| {
                let Some(ident) = field.ident.as_ref() else {
                    return false;
                };

                ident == requested_name || BuildContext::column_name(field) == *requested_name
            })
            .ok_or_else(|| {
                syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "#[tideorm(encrypted = ...)] references unknown field or column '{}'",
                        requested_name
                    ),
                )
            })?;

        validate_encrypted_field_type(field)?;

        let canonical_field_name = field
            .ident
            .as_ref()
            .expect("database fields must have identifiers")
            .to_string();
        if seen.insert(canonical_field_name) {
            resolved.push(field);
        }
    }

    Ok(resolved)
}

fn validate_encrypted_field_type(field: &ModelField) -> syn::Result<()> {
    let ty = normalized_type_name(&field.ty);
    if is_supported_encrypted_field_type(&ty) {
        return Ok(());
    }

    Err(syn::Error::new_spanned(
        &field.ty,
        "#[tideorm(encrypted = ...)] only supports String/Text fields and Option<String>/Option<Text> fields",
    ))
}

fn normalized_type_name(ty: &Type) -> String {
    quote!(#ty)
        .to_string()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

fn is_supported_encrypted_field_type(ty: &str) -> bool {
    let inner = strip_option_type(ty).unwrap_or(ty);
    matches_string_like_type(inner)
}

fn strip_option_type(ty: &str) -> Option<&str> {
    [
        "Option<",
        "std::option::Option<",
        "::std::option::Option<",
        "core::option::Option<",
        "::core::option::Option<",
    ]
    .into_iter()
    .find_map(|prefix| ty.strip_prefix(prefix)?.strip_suffix('>'))
}

fn matches_string_like_type(ty: &str) -> bool {
    matches!(
        ty,
        "String"
            | "std::string::String"
            | "::std::string::String"
            | "alloc::string::String"
            | "::alloc::string::String"
            | "Text"
            | "::tideorm::types::Text"
            | "tideorm::types::Text"
    ) || ty.ends_with("::String")
        || ty.ends_with("::Text")
}

pub(super) fn has_timestamp_pair(fields: &[ModelField]) -> bool {
    let has_created_at = fields
        .iter()
        .any(|field| matches_timestamp_field(field, "created_at"));
    let has_updated_at = fields
        .iter()
        .any(|field| matches_timestamp_field(field, "updated_at"));
    has_created_at && has_updated_at
}

fn matches_timestamp_field(field: &ModelField, expected: &str) -> bool {
    field.ident.as_ref().is_some_and(|ident| ident == expected)
        || BuildContext::column_name(field) == expected
}

pub(super) fn build_sync_column_attrs(fields: &[ModelField]) -> Vec<TokenStream2> {
    fields
        .iter()
        .map(|field| {
            let mut attrs = Vec::new();
            if field.primary_key {
                attrs.push(quote!(col = col.primary_key();));
            }
            if field.auto_increment {
                attrs.push(quote!(col = col.auto_increment();));
            }
            let ty = &field.ty;
            let ty_str = quote!(#ty).to_string();
            if !field.nullable && !ty_str.contains("Option") {
                attrs.push(quote!(col = col.not_null();));
            }
            if let Some(default) = &field.default {
                attrs.push(quote!(col = col.default(#default);));
            }
            quote!(#(#attrs)*)
        })
        .collect()
}

pub(super) fn build_insert_active_model_setters(fields: &[ModelField]) -> Vec<TokenStream2> {
    fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| {
            if field.primary_key && field.auto_increment {
                quote!(#ident: ActiveValue::NotSet)
            } else if matches_timestamp_field(field, "created_at")
                || matches_timestamp_field(field, "updated_at")
            {
                quote!(#ident: ActiveValue::Set(::tideorm::chrono::Utc::now()))
            } else {
                quote!(#ident: ActiveValue::Set(self.#ident))
            }
        })
        .collect()
}

pub(super) fn build_sea_orm_field_defs(fields: &[ModelField]) -> Vec<TokenStream2> {
    fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| {
            let ty = &field.ty;
            let column_name = BuildContext::column_name(field);
            let mut attrs = Vec::new();
            if field.primary_key {
                attrs.push(quote!(primary_key));
            }
            if field.auto_increment {
                attrs.push(quote!(auto_increment));
            }
            attrs.push(quote!(column_name = #column_name));
            quote!(#[sea_orm(#(#attrs),*)] pub #ident: #ty)
        })
        .collect()
}

pub(super) fn build_relation_field_defaults(fields: &[ModelField]) -> Vec<TokenStream2> {
    fields
        .iter()
        .filter_map(|field| field.ident.as_ref())
        .map(|ident| quote!(#ident: Default::default()))
        .collect()
}

pub(super) fn build_columns_struct_fields(fields: &[ModelField]) -> Vec<TokenStream2> {
    fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (ident, &field.ty)))
        .map(|(ident, ty)| quote!(pub #ident: ::tideorm::columns::Column<#ty>))
        .collect()
}

pub(super) fn build_columns_field_inits(fields: &[ModelField]) -> Vec<TokenStream2> {
    fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| {
            let column_name = BuildContext::column_name(field);
            quote!(#ident: ::tideorm::columns::Column::new(#column_name))
        })
        .collect()
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
