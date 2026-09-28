use std::collections::HashSet;

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Type;

use crate::context::BuildContext;
use crate::meta_support::has_managed_timestamp_columns;
use crate::parse::{ModelField, RelationKind, is_optional_type, type_string, variant_ident};

pub(crate) fn generate_entity_support(ctx: &BuildContext) -> syn::Result<TokenStream2> {
    let base_impl = generate_base_impl(ctx)?;
    let sync_impl = generate_sync_impl(ctx);
    let columns_impl = generate_columns_impl(ctx);
    Ok(quote! {
        #base_impl
        #sync_impl
        #columns_impl
    })
}

fn generate_base_impl(ctx: &BuildContext) -> syn::Result<TokenStream2> {
    let internal_entity_mod = &ctx.internal_entity_mod;
    let table_name = &ctx.table_name;
    // Declared on both sides: the engine's entity qualifies the typed CRUD
    // statements, `ModelMeta` the ones TideORM renders itself.
    let entity_schema_name = ctx
        .schema_name
        .as_ref()
        .map(|schema| quote!(fn schema_name(&self) -> Option<&str> { Some(#schema) }));
    let meta_schema_name = ctx
        .schema_name
        .as_ref()
        .map(|schema| quote!(fn schema_name() -> Option<&'static str> { Some(#schema) }));
    let struct_name = &ctx.struct_name;
    let pk_type = &ctx.pk_type;
    let pk_column_names = &ctx.pk_column_names;
    let pk_column_variants = &ctx.pk_column_variants;
    let pk_auto_increment = ctx.pk_auto_increment;
    let column_names = &ctx.column_names;
    let column_variants = &ctx.column_variants;
    let column_types = &ctx.column_types;
    let sea_orm_field_defs = ctx.db_fields.iter().map(sea_orm_field_def);
    let field_names = &ctx.field_names;
    let hidden_attrs = &ctx.hidden_attrs;
    let translatable_fields = &ctx.translatable_fields;
    let encrypted_fields = &ctx.encrypted_fields;
    let encrypted_column_names = &ctx.encrypted_column_names;
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
    let primary_key_auto_increment_impl = pk_auto_increment.then(|| {
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
    let encrypted_fields_impl = (!encrypted_fields.is_empty()).then(|| {
        quote! {
            fn encrypted_fields() -> Vec<&'static str> { vec![#(#encrypted_fields),*] }
            fn encrypted_column_names() -> Vec<&'static str> { vec![#(#encrypted_column_names),*] }
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
    let driver_limited_fields = ctx.driver_limited_fields();
    let driver_limited_fields_impl = (!driver_limited_fields.is_empty()).then(|| {
        let (fields, types): (Vec<_>, Vec<_>) = driver_limited_fields.into_iter().unzip();
        quote! {
            fn driver_limited_fields() -> &'static [(&'static str, &'static str)] {
                &[#((#fields, #types)),*]
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
    let entity_relations: Vec<&ModelField> = ctx
        .relation_fields
        .iter()
        .filter(|field| {
            field
                .relation_kind()
                .is_some_and(RelationKind::is_entity_relation)
        })
        .collect();
    let relation_variants = entity_relations
        .iter()
        .map(|field| variant_ident(field.ident()));
    let relation_defs = entity_relations
        .iter()
        .map(|field| build_relation_def(ctx, field))
        .collect::<syn::Result<Vec<_>>>()?;
    let related_impls = build_related_impls(&entity_relations)?;
    let relation_def_body = if relation_defs.is_empty() {
        quote!(match *self {})
    } else {
        quote!(match self { #(#relation_defs),* })
    };
    let primary_key_display_impl = build_primary_key_display_impl(ctx);
    let primary_key_is_new_impl = build_primary_key_is_new_impl(ctx);

    Ok(quote! {
        #[doc(hidden)]
        #[allow(non_snake_case, clippy::derivable_impls, clippy::enum_variant_names, clippy::redundant_closure)]
        mod #internal_entity_mod {
            use super::*;
            // The engine's derives write a bare `Result<_, DbErr>`, which the glob
            // above would resolve to `tideorm::Result` in a module that imports it.
            use ::core::result::Result;
            use ::tideorm::orm as sea_orm;
            // Deliberately NOT `use ::tideorm::orm::entity::prelude::*;`. That glob
            // and the `use super::*;` above both bring `Json` and `DateTime` into
            // scope, so any model field spelled with one of those names became an
            // ambiguous-glob error (rust-lang #114095, a future hard error) even
            // though the code was correct. The user's `super::*` stays the only glob.
            use ::tideorm::orm::entity::prelude::{
                ActiveModelBehavior, ColumnDef, ColumnTrait, ColumnTypeTrait, DeriveColumn,
                DerivePrimaryKey, EntityName, EntityTrait, EnumIter, PrimaryKeyTrait,
                RelationDef, RelationTrait,
            };
            use ::tideorm::orm::{DeriveActiveModel, DeriveEntity, DeriveModel};

            #[derive(Copy, Clone, Default, Debug, DeriveEntity)]
            pub struct Entity;

            impl EntityName for Entity {
                #entity_schema_name
                fn table_name(&self) -> &'static str {
                    #table_name
                }
            }

            #[derive(Clone, Debug, PartialEq, DeriveModel, DeriveActiveModel)]
            pub struct Model {
                #(#sea_orm_field_defs),*
            }

            #[derive(Copy, Clone, Debug, EnumIter, DeriveColumn)]
            pub enum Column {
                #(
                    #[sea_orm(column_name = #column_names)]
                    #column_variants
                ),*
            }

            #[derive(Copy, Clone, Debug, EnumIter, DerivePrimaryKey)]
            pub enum PrimaryKey {
                #(
                    #[sea_orm(column_name = #pk_column_names)]
                    #pk_column_variants
                ),*
            }

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

            #[derive(Copy, Clone, Debug, EnumIter)]
            pub enum Relation {
                #(#relation_variants),*
            }

            impl RelationTrait for Relation {
                fn def(&self) -> RelationDef {
                    #relation_def_body
                }
            }

            #(#related_impls)*

            impl ActiveModelBehavior for ActiveModel {}
        }

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
            #hidden_attributes_impl
            #serialized_name_impl
            #serde_round_trips_impl
            #relation_payload_filters_impl
            #searchable_fields_impl
            #morph_owner_key_impl
            #translatable_fields_impl
            #encrypted_fields_impl
            #driver_limited_fields_impl
            #allowed_languages_impl
            #fallback_language_impl
            #attached_files_impl
            #soft_delete_impl
            #has_timestamps_impl
            #indexes_impl
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
        let local_key = field
            .local_key
            .as_deref()
            .unwrap_or(ctx.default_local_key());
        // `local_key = "id"` names the primary key, whatever it is called.
        let key_column = ctx
            .resolve_local_key(local_key, field.ident())?
            .column_name();
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

fn sea_orm_field_def(field: &ModelField) -> TokenStream2 {
    let ident = field.ident();
    let ty = &field.ty;
    let column_name = field.column_name();
    let primary_key = field.primary_key.then(|| quote!(primary_key,));
    let auto_increment = field.auto_increment.then(|| quote!(auto_increment,));
    // SeaORM's derives would name the `Column` variant themselves, splitting
    // digits their own way (`s3key` is `S3key` to them, `S3Key` here).
    let enum_name = variant_ident(ident).to_string();
    quote!(#[sea_orm(#primary_key #auto_increment column_name = #column_name, enum_name = #enum_name)] pub #ident: #ty)
}

fn build_primary_key_display_impl(ctx: &BuildContext) -> TokenStream2 {
    let pk_column_names = &ctx.pk_column_names;
    let (bind, components) = ctx.primary_key_components();
    quote! {
        #bind
        [#(format!("{} = {}", #pk_column_names, #components)),*].join(" AND ")
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

/// Per-relation hidden-attribute filters for `ModelMeta::relation_payload_filters`.
///
/// An eager-loaded relation payload is untagged JSON by the time `to_json` sees it,
/// so the only way it can be filtered by the *target* model's hidden list is for the
/// derive — which does know the target type — to hand over a function pointer to it.
///
/// `MorphTo` is skipped: its generic parameter carries no `Model` bound because the
/// target is chosen at runtime, so there is no single type to capture. Those payloads
/// keep falling back to the owning model's hidden list.
fn build_relation_payload_filters(ctx: &BuildContext) -> Vec<TokenStream2> {
    ctx.relation_fields
        .iter()
        .filter(|field| field.relation_kind() != Some(RelationKind::MorphTo))
        .filter_map(|field| {
            let target = field.related_types().into_iter().next()?;
            let key = ctx.serialized_key(field);
            Some(quote! {
                (
                    #key,
                    <#target as ::tideorm::model::ModelMeta>::__strip_hidden_payload
                        as ::tideorm::model::RelationPayloadFilter
                )
            })
        })
        .collect()
}

fn missing_related_type(field: &ModelField) -> syn::Error {
    syn::Error::new_spanned(
        &field.ty,
        "relation field must specify a related model type",
    )
}

fn missing_pivot_type(field: &ModelField) -> syn::Error {
    syn::Error::new_spanned(
        &field.ty,
        "has_many_through relations must specify both related and pivot model types",
    )
}

/// The `RelationTrait::def` arm for one entity relation: the join from this model
/// to the related model, or to the pivot for `HasManyThrough`.
fn build_relation_def(ctx: &BuildContext, field: &ModelField) -> syn::Result<TokenStream2> {
    let ident = field.ident();
    let name = field.name();
    let related_types = field.related_types();
    let related_ty = related_types
        .first()
        .ok_or_else(|| missing_related_type(field))?;
    let foreign_key = field
        .foreign_key
        .as_deref()
        .expect("validated relation foreign_key");
    let local_key = field
        .local_key
        .as_deref()
        .unwrap_or(ctx.default_local_key());

    let (target, local_ident, remote_key, relation_type, error) = match field.relation_kind() {
        Some(RelationKind::HasManyThrough) => (
            related_types
                .get(1)
                .ok_or_else(|| missing_pivot_type(field))?,
            ctx.resolve_local_key_ident(local_key, ident)?,
            foreign_key,
            quote!(HasMany),
            format!(
                "many-to-many relation '{}' references an unknown pivot foreign key '{}'",
                name, foreign_key
            ),
        ),
        Some(RelationKind::BelongsTo) => {
            let owner_key = field.owner_key.as_deref().unwrap_or("id");
            (
                related_ty,
                ctx.resolve_required_db_field_ident(foreign_key, ident)?,
                owner_key,
                quote!(HasOne),
                remote_column_error(&name, owner_key),
            )
        }
        kind => (
            related_ty,
            ctx.resolve_local_key_ident(local_key, ident)?,
            foreign_key,
            if kind == Some(RelationKind::HasMany) {
                quote!(HasMany)
            } else {
                quote!(HasOne)
            },
            remote_column_error(&name, foreign_key),
        ),
    };

    let variant = variant_ident(ident);
    let local_column = variant_ident(&local_ident);
    let remote_assert = compile_time_column_assert(target, remote_key, &error);
    let target_entity = related_entity_value(target);
    Ok(quote! {
        Self::#variant => {
            #remote_assert
            let mut relation: RelationDef = Entity::belongs_to(#target_entity)
                .from(Column::#local_column)
                .to(<#target as ::tideorm::internal::InternalModel>::column_from_str(#remote_key)
                    .unwrap_or_else(|| unreachable!(#error)))
                .into();
            relation.rel_type = ::tideorm::orm::RelationType::#relation_type;
            relation
        }
    })
}

fn remote_column_error(relation: &str, column: &str) -> String {
    format!(
        "relation '{}' references an unknown remote column '{}'",
        relation, column
    )
}

fn related_entity_value(ty: &Type) -> TokenStream2 {
    quote!(<<#ty as ::tideorm::internal::InternalModel>::Entity as Default>::default())
}

/// `Related` impls, one per related entity.
///
/// `to()` and `via()` return the `RelationTrait::def` arm — as SeaORM's own
/// `DeriveRelated` does — so each relation's columns are resolved and checked in
/// exactly one place. Only `HasManyThrough::to()` builds a join of its own, from
/// the pivot to the related model, which no `def` arm describes.
///
/// Rust allows one `Related<X>` impl per entity pair, so a second relation to
/// the same model (an `author` and an `editor`, both `BelongsTo<User>`) gets
/// none. TideORM reads no relation through `Related`: every `with(..)` and
/// `load()` goes by the field's own keys, so both relations load their own
/// rows. Each field keeps its `Relation` variant and `def` arm, and its
/// column checks.
fn build_related_impls(relations: &[&ModelField]) -> syn::Result<Vec<TokenStream2>> {
    let mut related_entities = HashSet::new();
    let mut impls = Vec::new();

    for field in relations {
        let ident = field.ident();
        let related_types = field.related_types();
        let related_ty = related_types
            .first()
            .ok_or_else(|| missing_related_type(field))?;
        let first_to_target = related_entities.insert(type_string(related_ty));

        let variant = variant_ident(ident);
        let related_entity_ty = quote!(<#related_ty as ::tideorm::internal::InternalModel>::Entity);

        if field.relation_kind() != Some(RelationKind::HasManyThrough) {
            if !first_to_target {
                continue;
            }
            impls.push(quote! {
                impl ::tideorm::orm::Related<#related_entity_ty> for Entity {
                    fn to() -> RelationDef {
                        Relation::#variant.def()
                    }
                }
            });
            continue;
        }

        let pivot_ty = related_types
            .get(1)
            .ok_or_else(|| missing_pivot_type(field))?;
        let name = field.name();
        let related_key = field.related_key.as_deref().expect("validated related_key");
        let related_local_key = field.owner_key.as_deref().unwrap_or("id");
        let pivot_related_error = format!(
            "many-to-many relation '{}' references an unknown pivot related column '{}'",
            name, related_key
        );
        let related_column_error = format!(
            "many-to-many relation '{}' references an unknown related column '{}'",
            name, related_local_key
        );
        let pivot_related_assert =
            compile_time_column_assert(pivot_ty, related_key, &pivot_related_error);
        let related_column_assert =
            compile_time_column_assert(related_ty, related_local_key, &related_column_error);
        let related_entity = related_entity_value(related_ty);

        if !first_to_target {
            impls.push(quote! {
                #pivot_related_assert
                #related_column_assert
            });
            continue;
        }

        impls.push(quote! {
            impl ::tideorm::orm::Related<#related_entity_ty> for Entity {
                fn to() -> RelationDef {
                    #pivot_related_assert
                    #related_column_assert
                    <#pivot_ty as ::tideorm::internal::InternalModel>::Entity::belongs_to(#related_entity)
                        .from(<#pivot_ty as ::tideorm::internal::InternalModel>::column_from_str(#related_key)
                            .unwrap_or_else(|| unreachable!(#pivot_related_error)))
                        .to(<#related_ty as ::tideorm::internal::InternalModel>::column_from_str(#related_local_key)
                            .unwrap_or_else(|| unreachable!(#related_column_error)))
                        .into()
                }

                fn via() -> Option<RelationDef> {
                    Some(Relation::#variant.def())
                }
            }
        });
    }

    Ok(impls)
}

fn compile_time_column_assert(ty: &Type, column: &str, message: &str) -> TokenStream2 {
    quote! {
        const _: () = assert!(<#ty>::__has_column_name(#column), #message);
    }
}

fn generate_sync_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let table_name = &ctx.table_name;
    let schema_call = ctx
        .schema_name
        .as_ref()
        .map(|schema| quote!(.schema(#schema)));
    let pk_column_names = &ctx.pk_column_names;
    let columns = ctx.db_fields.iter().map(|field| {
        let column_name = field.column_name();
        let rust_type = type_string(&field.ty);
        let primary_key = field.primary_key.then(|| quote!(.primary_key()));
        let auto_increment = field.auto_increment.then(|| quote!(.auto_increment()));
        let not_null =
            (!field.nullable && !is_optional_type(&field.ty)).then(|| quote!(.not_null()));
        let default = field
            .default
            .as_ref()
            .map(|default| quote!(.default(#default)));
        quote! {
            ::tideorm::sync::ColumnDef::new(#column_name, #rust_type)
                #primary_key #auto_increment #not_null #default
        }
    });

    quote! {
        impl #struct_name {
            #[doc(hidden)]
            pub fn __get_sync_schema() -> ::tideorm::sync::ModelSchema {
                ::tideorm::sync::ModelSchema::new(#table_name)
                    #schema_call
                    .primary_keys(vec![#(#pk_column_names.to_string()),*])
                    .indexes(
                        <Self as ::tideorm::model::ModelMeta>::indexes()
                            .into_iter()
                            .chain(<Self as ::tideorm::model::ModelMeta>::unique_indexes())
                            .collect(),
                    )
                    #(.column(#columns))*
            }
        }

        impl ::tideorm::sync::SyncModel for #struct_name {
            fn sync_schema() -> ::tideorm::sync::ModelSchema {
                Self::__get_sync_schema()
            }
        }

        ::tideorm::inventory::submit! {
            ::tideorm::sync::CompiledModelRegistration {
                source_path: file!(),
                sync_schema: #struct_name::__get_sync_schema,
                table_name: <#struct_name as ::tideorm::model::ModelMeta>::table_name,
                column_type: ::tideorm::internal::__column_type_of::<#struct_name>,
            }
        }
    }
}

fn generate_columns_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let columns_struct_name = &ctx.columns_struct_name;
    let field_idents = &ctx.field_idents;
    let field_types = &ctx.field_types;
    let column_names = &ctx.column_names;
    let table_name = &ctx.table_name;

    quote! {
        #[allow(non_camel_case_types)]
        #[derive(Clone)]
        pub struct #columns_struct_name {
            #(pub #field_idents: ::tideorm::columns::Column<#field_types>),*
        }

        impl #struct_name {
            #[allow(non_upper_case_globals)]
            pub const columns: #columns_struct_name = #columns_struct_name {
                #(#field_idents: ::tideorm::columns::Column::of(#table_name, #column_names)),*
            };
        }
    }
}
