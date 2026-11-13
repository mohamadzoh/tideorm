use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote, quote_spanned};
use syn::spanned::Spanned;

use crate::context::BuildContext;
use crate::meta_support::has_managed_timestamp_columns;
use crate::orm::derives::{expand_column, expand_primary_key};
use crate::parse::{ModelField, RelationKind, type_string};

mod columns;
mod engine;
mod relations;

use columns::{generate_columns_impl, generate_sync_impl};
use engine::{engine_model_parts, nested_visibility};
use relations::{
    build_entity_manager_meta, build_relation_checks, build_relation_outline,
    build_relation_payload_filters,
};

pub(crate) fn generate_entity_support(ctx: &BuildContext) -> syn::Result<TokenStream2> {
    // `ModelMeta`'s checks run first: a model failing one there and one in the
    // entity module reports the `ModelMeta` error.
    let model_meta_impl = generate_model_meta_impl(ctx)?;
    let entity_module = generate_entity_module(ctx)?;
    let sync_impl = generate_sync_impl(ctx);
    let columns_impl = generate_columns_impl(ctx);
    // Item types are checked before bodies and in this order, so a field type
    // no column can hold is reported as that, ahead of the entity's own errors.
    let column_type_checks = ctx.db_fields.iter().map(|field| {
        let ty = field.stored_type(ctx.is_encrypted(field));
        quote_spanned! {field.ty.span()=>
            const _: ::core::marker::PhantomData<::tideorm::model::__ColumnType<#ty>> =
                ::core::marker::PhantomData;
        }
    });
    Ok(quote! {
        #(#column_type_checks)*
        #entity_module
        #model_meta_impl
        #sync_impl
        #columns_impl
    })
}

/// The hidden module holding the engine entity the typed CRUD paths run on.
fn generate_entity_module(ctx: &BuildContext) -> syn::Result<TokenStream2> {
    let internal_entity_mod = &ctx.internal_entity_mod;
    let pk_type = &ctx.pk_type;
    let pk_auto_increment = ctx.pk_auto_increment;
    let column_variants = &ctx.column_variants;
    let column_types = &ctx.column_types;
    let column_impls = expand_column(
        &format_ident!("Column"),
        &ctx.column_variants,
        &ctx.column_names,
    );
    let primary_key_impls = expand_primary_key(&ctx.pk_column_variants, &ctx.pk_column_names);
    let pk_column_variants = &ctx.pk_column_variants;
    let (active_model, engine_impls) = engine_model_parts(ctx);
    let vis = nested_visibility(&ctx.vis);
    let relation_checks = ctx
        .relation_fields
        .iter()
        .filter(|field| {
            field
                .relation_kind()
                .is_some_and(RelationKind::is_entity_relation)
        })
        .map(|field| build_relation_checks(ctx, field))
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(quote! {
        #[doc(hidden)]
        #[allow(non_snake_case, clippy::derivable_impls, clippy::enum_variant_names)]
        mod #internal_entity_mod {
            use super::*;
            // Deliberately NOT `use ::tideorm::orm::entity::prelude::*;`. That glob
            // and the `use super::*;` above both bring `Json` and `DateTime` into
            // scope, so any model field spelled with one of those names became an
            // ambiguous-glob error (rust-lang #114095, a future hard error) even
            // though the code was correct. The user's `super::*` stays the only glob.
            use ::tideorm::orm::entity::prelude::{
                ColumnDef, ColumnTrait, ColumnTypeTrait, PrimaryKeyTrait,
            };

            #[derive(Copy, Clone, Default, Debug)]
            #vis struct Entity;

            #[derive(Copy, Clone, Debug)]
            #vis enum Column {
                #(#column_variants),*
            }

            #[derive(Copy, Clone, Debug)]
            #vis enum PrimaryKey {
                #(#pk_column_variants),*
            }

            #column_impls

            #primary_key_impls

            impl PrimaryKeyTrait for PrimaryKey {
                type ValueType = #pk_type;
                fn auto_increment() -> bool { #pk_auto_increment }
            }

            impl ColumnTrait for Column {
                type EntityName = Entity;
                fn def(&self) -> ColumnDef {
                    match self {
                        #(Self::#column_variants => #column_types),*
                    }
                }
            }

            #(#relation_checks)*

            #active_model
        }

        // The impls naming the model sit beside it: one declared in a
        // function body cannot be named from a module inside that body.
        // Their locals are named after its fields.
        #[allow(non_snake_case)]
        const _: () = {
            use #internal_entity_mod::{ActiveModel, Column, Entity, PrimaryKey};
            #engine_impls
        };
    })
}

fn generate_model_meta_impl(ctx: &BuildContext) -> syn::Result<TokenStream2> {
    let table_name = &ctx.table_name;
    let meta_schema_name = ctx
        .schema_name
        .as_ref()
        .map(|schema| quote!(fn schema_name() -> Option<&'static str> { Some(#schema) }));
    let struct_name = &ctx.struct_name;
    let pk_type = &ctx.pk_type;
    let pk_column_names = &ctx.pk_column_names;
    let column_names = &ctx.column_names;
    // An encrypted integer is stored as its payload's text.
    let integer_bounds = ctx
        .db_fields
        .iter()
        .filter(|field| field.is_integer() && !ctx.is_encrypted(field))
        .map(|field| {
            let name = field.column_name();
            let ty = field.validation_base_type();
            quote! {
                #name => Some((
                    i64::try_from(<#ty>::MIN).unwrap_or(i64::MIN),
                    i64::try_from(<#ty>::MAX).unwrap_or(i64::MAX),
                )),
            }
        });
    let field_names = &ctx.field_names;
    let hidden_attrs = &ctx.hidden_attrs;
    let flattened_fields_impl = user_serde_fields_impl(
        ctx,
        format_ident!("__flattened_fields"),
        crate::serde_names::is_flattened,
    );
    let skipped_fields_impl = user_serde_fields_impl(
        ctx,
        format_ident!("__skipped_fields"),
        crate::serde_names::is_skipped,
    );
    let translation_field = ctx.fields.iter().find(|field| {
        !ctx.translatable_fields.is_empty()
            && field
                .ident
                .as_ref()
                .is_some_and(|ident| ident == "translations")
    });
    let translation_payload_impl = translation_field.map(|field| {
        let ident = field.ident();
        quote! {
            fn __translation_payload(&self) -> ::tideorm::Result<Option<::tideorm::serde_json::Value>> {
                ::tideorm::serde_json::to_value(&self.#ident).map(Some)
                    .map_err(|error| ::tideorm::Error::internal(format!("Invalid translations: {}", error)))
            }
        }
    });
    let translatable_fields = &ctx.translatable_fields;
    let encrypted: Vec<_> = ctx
        .db_fields
        .iter()
        .filter(|field| ctx.is_encrypted(field))
        .collect();
    let has_one_files = &ctx.has_one_files;
    let has_many_files = &ctx.has_many_files;
    let searchable_fields = &ctx.searchable_fields;
    let index_impls = &ctx.index_impls;
    let unique_index_impls = &ctx.unique_index_impls;
    let morph_owner_key_impl = build_morph_owner_key_impl(ctx)?;
    let soft_delete_impl = ctx.soft_delete.as_ref().map(|(_, deleted_at_column)| {
        quote! {
            fn soft_delete_enabled() -> bool { true }
            fn deleted_at_column() -> &'static str { #deleted_at_column }
        }
    });
    // `ModelMeta::has_timestamps` has to agree with what the insert path actually does,
    // so a lone `created_at` — which is populated — reports `true` even though it is not
    // the `created_at`/`updated_at` pair that sets `ctx.timestamps_enabled`.
    let has_timestamps = ctx.timestamps_enabled || has_managed_timestamp_columns(&ctx.db_fields);
    let allowed_languages_impl = ctx.allowed_languages.as_ref().map(|languages| {
        quote! {
            fn allowed_languages() -> Vec<String> { vec![#(#languages.to_string()),*] }
        }
    });
    let fallback_language_impl = ctx.fallback_language.as_ref().map(|language| {
        quote! {
            fn fallback_language() -> String { #language.to_string() }
        }
    });
    let relation_payload_filters = build_relation_payload_filters(ctx);
    // Each method is emitted only where the model differs from its default.
    let primary_key_auto_increment_impl = ctx.pk_auto_increment.then(|| {
        quote!(
            fn primary_key_auto_increment() -> bool {
                true
            }
        )
    });
    let hidden_attributes_impl = (hidden_attrs.as_slice() != ["deleted_at"]).then(|| {
        quote!(
            fn hidden_attributes() -> Vec<&'static str> {
                vec![#(#hidden_attrs),*]
            }
        )
    });
    let relation_payload_filters_impl = (!relation_payload_filters.is_empty()).then(|| {
        quote! {
            fn relation_payload_filters() -> Vec<(&'static str, ::tideorm::model::RelationPayloadFilter)> {
                vec![#(#relation_payload_filters),*]
            }
        }
    });
    let searchable_fields_impl = (!searchable_fields.is_empty()).then(|| {
        quote!(
            fn searchable_fields() -> Vec<&'static str> {
                vec![#(#searchable_fields),*]
            }
        )
    });
    let translatable_fields_impl = (!translatable_fields.is_empty()).then(|| {
        quote!(
            fn translatable_fields() -> Vec<&'static str> {
                vec![#(#translatable_fields),*]
            }
        )
    });
    let encrypted_fields_impl = (!encrypted.is_empty()).then(|| {
        let names: Vec<_> = encrypted.iter().map(|field| field.name()).collect();
        let columns = encrypted.iter().map(|field| field.column_name());
        let types = encrypted.iter().map(|field| &field.ty);
        quote! {
            fn encrypted_fields() -> &'static [(&'static str, &'static str)] {
                &[#((#names, #columns)),*]
            }
            fn __encrypted_field_holds(field: &str, document: &::tideorm::serde_json::Value) -> bool {
                match field {
                    #(#names => <#types as ::tideorm::serde::Deserialize>::deserialize(document).is_ok(),)*
                    _ => false,
                }
            }
        }
    });
    let attached_files_impl =
        (!has_one_files.is_empty() || !has_many_files.is_empty()).then(|| {
            quote! {
                fn has_one_attached_file() -> Vec<&'static str> { vec![#(#has_one_files),*] }
                fn has_many_attached_files() -> Vec<&'static str> { vec![#(#has_many_files),*] }
            }
        });
    let has_timestamps_impl = has_timestamps.then(|| {
        quote!(
            fn has_timestamps() -> bool {
                true
            }
        )
    });
    let indexes_impl = (!index_impls.is_empty() || !unique_index_impls.is_empty()).then(|| {
        quote! {
            fn indexes() -> Vec<::tideorm::model::IndexDefinition> { vec![#(#index_impls),*] }
            fn unique_indexes() -> Vec<::tideorm::model::IndexDefinition> {
                vec![#(#unique_index_impls),*]
            }
        }
    });
    let serde_round_trips_impl = (!ctx.serde_round_trips).then(|| {
        quote! {
            fn __serde_round_trips() -> bool {
                false
            }
        }
    });
    let serialized_name_impl = (!ctx.serialized_names.is_empty()).then(|| {
        let (fields, keys): (Vec<_>, Vec<_>) = ctx.serialized_names.iter().cloned().unzip();
        quote! {
            fn serialized_name(field: &str) -> &str {
                match field {
                    #(#fields => #keys,)*
                    other => other,
                }
            }
        }
    });
    let primary_key_display_impl = build_primary_key_display_impl(ctx);
    let primary_key_is_new_impl = build_primary_key_is_new_impl(ctx);
    let entity_manager_meta_impl = build_entity_manager_meta(ctx);
    let relation_outline_impl = build_relation_outline(ctx);
    let rules_impl = ctx.project_rules.rules.as_ref().map(|rules| {
        quote! {
            fn __rules() -> &'static ::tideorm::rules::Rules {
                const RULES: ::tideorm::rules::Rules = #rules;
                &RULES
            }
        }
    });

    Ok(quote! {
        impl ::tideorm::model::ModelMeta for #struct_name {
            type PrimaryKey = #pk_type;
            fn table_name() -> &'static str { #table_name }
            #meta_schema_name
            fn primary_key_names() -> &'static [&'static str] { &[#(#pk_column_names),*] }
            #primary_key_auto_increment_impl
            fn primary_key_display(primary_key: &Self::PrimaryKey) -> String {
                #primary_key_display_impl
            }
            fn primary_key_is_new(primary_key: &Self::PrimaryKey) -> bool {
                #primary_key_is_new_impl
            }
            fn column_names() -> &'static [&'static str] { &[#(#column_names),*] }
            fn field_names() -> &'static [&'static str] { &[#(#field_names),*] }
            fn __sqlite_integer_bounds(column: &str) -> Option<(i64, i64)> {
                match Self::own_column_name(column)? {
                    #(#integer_bounds)*
                    _ => None,
                }
            }
            #hidden_attributes_impl
            #flattened_fields_impl
            #skipped_fields_impl
            #serialized_name_impl
            #serde_round_trips_impl
            #relation_payload_filters_impl
            #searchable_fields_impl
            #morph_owner_key_impl
            #translatable_fields_impl
            #translation_payload_impl
            #encrypted_fields_impl
            #allowed_languages_impl
            #fallback_language_impl
            #attached_files_impl
            #soft_delete_impl
            #has_timestamps_impl
            #indexes_impl
            #entity_manager_meta_impl
            #relation_outline_impl
            #rules_impl
        }
    })
}

/// `ModelMeta::__flattened_fields` or `__skipped_fields`: the fields whose
/// attributes `predicate` accepts, which only a model's own `Serialize` derive
/// reads; emitted when there are any.
fn user_serde_fields_impl(
    ctx: &BuildContext,
    method: syn::Ident,
    predicate: fn(&[syn::Attribute]) -> bool,
) -> Option<TokenStream2> {
    let fields: Vec<_> = ctx
        .fields
        .iter()
        .filter(|field| !ctx.should_gen_serialize && predicate(&field.attrs))
        .map(ModelField::name)
        .collect();
    (!fields.is_empty()).then(|| {
        quote! {
            fn #method() -> &'static [&'static str] {
                &[#(#fields),*]
            }
        }
    })
}

/// `ModelMeta::__morph_owner_key`: the column each `MorphOne`/`MorphMany` of
/// the model keys its children by, found by the child's `{morph}_id` column
/// and the child model's table, since two relations may share a morph name.
///
/// Two relations to one child model under one morph name that key it by
/// different columns are refused: a child row could not tell which it
/// belongs to.
fn build_morph_owner_key_impl(ctx: &BuildContext) -> syn::Result<Option<TokenStream2>> {
    let mut declared: Vec<(String, String, String)> = Vec::new();
    let mut arms = Vec::new();
    for field in ctx.relation_fields.iter().filter(|field| {
        matches!(
            field.relation_kind(),
            Some(RelationKind::MorphOne | RelationKind::MorphMany)
        )
    }) {
        let Some(morph_name) = field.morph_name.as_deref() else {
            continue;
        };
        let id_column = format!("{morph_name}_id");
        let key_column = ctx.relation_local_field(field)?.column_name();
        let Some(child) = field.related_types().into_iter().next() else {
            continue;
        };

        let child_name = quote!(#child).to_string();
        if let Some((_, _, other_key)) = declared.iter().find(|(name, morph, key)| {
            *name == child_name && morph == morph_name && *key != key_column
        }) {
            return Err(syn::Error::new_spanned(
                field.ident(),
                format!(
                    "another relation to {child_name} with morph_name = \"{morph_name}\" keys it by \
                     `{other_key}`, and this one by `{key_column}`: a {child_name} row could not tell \
                     which it belongs to; give the two relations different morph names"
                ),
            ));
        }
        declared.push((child_name, morph_name.to_string(), key_column.clone()));

        arms.push(quote! {
            if id_column == #id_column
                && (child_table.is_empty()
                    || child_table == <#child as ::tideorm::model::ModelMeta>::table_name())
            {
                return Some(#key_column);
            }
        });
    }
    if arms.is_empty() {
        return Ok(None);
    }
    Ok(Some(quote! {
        fn __morph_owner_key(id_column: &str, child_table: &str) -> Option<&'static str> {
            #(#arms)*
            None
        }
    }))
}

fn build_primary_key_display_impl(ctx: &BuildContext) -> TokenStream2 {
    let pk_column_names = &ctx.pk_column_names;
    let (bind, components) = ctx.primary_key_components();
    let displays = ctx
        .fields
        .iter()
        .filter(|field| field.primary_key)
        .zip(pk_column_names)
        .zip(components)
        .map(|((field, column), component)| {
            if type_string(&field.ty).contains("Vec<") {
                quote!(format!("{} = {:?}", #column, #component))
            } else {
                quote!(format!("{} = {}", #column, #component))
            }
        });
    quote! {
        #bind
        [#(#displays),*].join(" AND ")
    }
}

fn build_primary_key_is_new_impl(ctx: &BuildContext) -> TokenStream2 {
    if ctx.pk_column_names.len() == 1 {
        return quote!(::tideorm::model::__is_default(primary_key));
    }

    let (bind, components) = ctx.primary_key_components();
    quote! {
        #bind
        // A composite key counts as unsaved when *any* component is still at its default:
        // a partially-assigned key means the row has not been fully keyed yet.
        //
        // Requiring *every* component to be default reads better for a persisted row
        // such as `(42, "")`, but it makes the failure silent: a genuinely new row with a
        // partial key routes to `update()` and quietly affects zero rows. ORing routes it
        // to `create()`, where a real collision surfaces loudly as a duplicate-key error.
        false #(|| ::tideorm::model::__is_default(#components))*
    }
}
