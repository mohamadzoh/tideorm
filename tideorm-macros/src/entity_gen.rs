use convert_case::{Case, Casing};
use proc_macro2::TokenStream as TokenStream2;
use quote::format_ident;
use quote::quote;

use crate::context::BuildContext;
use crate::parse::relation_generic_types;

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
    let struct_name = &ctx.struct_name;
    let pk_column_variants = &ctx.pk_column_variants;
    let pk_type = &ctx.pk_type;
    let pk_column_names = &ctx.pk_column_names;
    let pk_auto_increment = ctx.pk_auto_increment;
    let column_type_defs = &ctx.column_type_defs;
    let column_variants = &ctx.column_variants;
    let sea_orm_field_defs = &ctx.sea_orm_field_defs;
    let hidden_attrs = &ctx.hidden_attrs;
    let translatable_fields = &ctx.translatable_fields;
    let has_one_files = &ctx.has_one_files;
    let has_many_files = &ctx.has_many_files;
    let searchable_fields = &ctx.searchable_fields;
    let column_names = &ctx.column_names;
    let field_names = &ctx.field_names;
    let index_impls = &ctx.index_impls;
    let unique_index_impls = &ctx.unique_index_impls;
    let soft_delete_enabled = ctx.soft_delete_enabled;
    let deleted_at_column_impl = if soft_delete_enabled {
        quote! {
            fn deleted_at_column() -> &'static str {
                <Self as ::tideorm::SoftDelete>::deleted_at_column()
            }
        }
    } else {
        quote! {}
    };
    let timestamps_enabled = ctx.timestamps_enabled;
    let allowed_languages_impl = ctx.allowed_languages_impl();
    let fallback_language_impl = ctx.fallback_language_impl();
    let relation_variants = build_relation_variants(ctx);
    let relation_defs = build_relation_defs(ctx)?;
    let related_impls = build_related_impls(ctx)?;
    let primary_key_display_impl = build_primary_key_display_impl(ctx);
    let primary_key_is_new_impl = build_primary_key_is_new_impl(ctx);
    let relation_trait_impl = if relation_variants.is_empty() {
        quote! {
            impl RelationTrait for Relation {
                fn def(&self) -> RelationDef {
                    match *self {}
                }
            }
        }
    } else {
        quote! {
            impl RelationTrait for Relation {
                fn def(&self) -> RelationDef {
                    match self {
                        #(#relation_defs),*
                    }
                }
            }
        }
    };

    Ok(quote! {
        #[doc(hidden)]
        #[allow(non_snake_case, dead_code, unused_imports, clippy::derivable_impls, clippy::enum_variant_names, clippy::redundant_closure)]
        mod #internal_entity_mod {
            use super::*;
            use ::tideorm::sea_orm as sea_orm;
            use ::tideorm::sea_orm::entity::prelude::*;
            use ::tideorm::sea_orm::{ActiveValue, DeriveActiveModel, DeriveEntity, DeriveModel};

            #[derive(Copy, Clone, Default, Debug, DeriveEntity)]
            pub struct Entity;

            impl EntityName for Entity {
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
                #(#column_variants),*
            }

            #[derive(Copy, Clone, Debug, EnumIter, DerivePrimaryKey)]
            pub enum PrimaryKey {
                #(#pk_column_variants),*
            }

            impl PrimaryKeyTrait for PrimaryKey {
                type ValueType = #pk_type;
                fn auto_increment() -> bool { #pk_auto_increment }
            }

            impl ColumnTrait for Column {
                type EntityName = Entity;
                fn def(&self) -> ColumnDef {
                    match self {
                        #(#column_type_defs),*
                    }
                }
            }

            #[derive(Copy, Clone, Debug, EnumIter)]
            pub enum Relation {
                #(#relation_variants),*
            }

            #relation_trait_impl

            #(#related_impls)*

            impl ActiveModelBehavior for ActiveModel {}
        }

        impl ::tideorm::model::ModelMeta for #struct_name {
            type PrimaryKey = #pk_type;
            fn table_name() -> &'static str { #table_name }
            fn primary_key_names() -> &'static [&'static str] { &[#(#pk_column_names),*] }
            fn primary_key_auto_increment() -> bool { #pk_auto_increment }
            fn primary_key_display(primary_key: &Self::PrimaryKey) -> String {
                #primary_key_display_impl
            }
            fn primary_key_is_new(primary_key: &Self::PrimaryKey) -> bool {
                #primary_key_is_new_impl
            }
            fn column_names() -> &'static [&'static str] { &[#(#column_names),*] }
            fn field_names() -> &'static [&'static str] { &[#(stringify!(#field_names)),*] }
            fn hidden_attributes() -> Vec<&'static str> { vec![#(#hidden_attrs),*] }
            fn searchable_fields() -> Vec<&'static str> { vec![#(#searchable_fields),*] }
            fn translatable_fields() -> Vec<&'static str> { vec![#(#translatable_fields),*] }
            fn allowed_languages() -> Vec<String> { #allowed_languages_impl }
            fn fallback_language() -> String { #fallback_language_impl }
            fn has_one_attached_file() -> Vec<&'static str> { vec![#(#has_one_files),*] }
            fn has_many_attached_files() -> Vec<&'static str> { vec![#(#has_many_files),*] }
            fn soft_delete_enabled() -> bool { #soft_delete_enabled }
            #deleted_at_column_impl
            fn has_timestamps() -> bool { #timestamps_enabled }
            fn indexes() -> Vec<::tideorm::model::IndexDefinition> { vec![#(#index_impls),*] }
            fn unique_indexes() -> Vec<::tideorm::model::IndexDefinition> { vec![#(#unique_index_impls),*] }
        }
    })
}

fn build_primary_key_display_impl(ctx: &BuildContext) -> TokenStream2 {
    if ctx.pk_column_names.len() == 1 {
        let pk_column_name = &ctx.pk_column_name;
        return quote! {
            format!("{} = {}", #pk_column_name, primary_key)
        };
    }

    let bindings: Vec<_> = (0..ctx.pk_column_names.len())
        .map(|index| format_ident!("pk_{index}"))
        .collect();
    let pk_column_names = &ctx.pk_column_names;

    quote! {
        let (#(#bindings),*) = primary_key.clone();
        vec![#(format!("{} = {}", #pk_column_names, #bindings)),*].join(" AND ")
    }
}

fn build_primary_key_is_new_impl(ctx: &BuildContext) -> TokenStream2 {
    if ctx.pk_column_names.len() == 1 {
        return quote! {
            fn __tideorm_is_default<T>(value: &T) -> bool
            where
                T: ::std::default::Default + ::std::cmp::PartialEq,
            {
                value == &T::default()
            }

            __tideorm_is_default(primary_key)
        };
    }

    let bindings: Vec<_> = (0..ctx.pk_column_names.len())
        .map(|index| format_ident!("pk_{index}"))
        .collect();

    quote! {
        fn __tideorm_is_default<T>(value: &T) -> bool
        where
            T: ::std::default::Default + ::std::cmp::PartialEq,
        {
            value == &T::default()
        }

        let (#(#bindings),*) = primary_key.clone();
        false #(|| __tideorm_is_default(&#bindings))*
    }
}

fn build_relation_variants(ctx: &BuildContext) -> Vec<syn::Ident> {
    ctx.relation_fields
        .iter()
        .filter_map(|field| field.ident.as_ref())
        .map(|ident| format_ident!("{}", ident.to_string().to_case(Case::Pascal)))
        .collect()
}

fn build_relation_defs(ctx: &BuildContext) -> syn::Result<Vec<TokenStream2>> {
    ctx.relation_fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| -> syn::Result<TokenStream2> {
            let variant = format_ident!("{}", ident.to_string().to_case(Case::Pascal));
            let related_types = relation_generic_types(&field.ty);
            let related_ty = related_types.first().cloned().ok_or_else(|| {
                syn::Error::new_spanned(&field.ty, "relation field must specify a related model type")
            })?;

            if field.has_many_through.is_some() {
                let pivot_ty = related_types.get(1).cloned().ok_or_else(|| {
                    syn::Error::new_spanned(&field.ty, "has_many_through relations must specify both related and pivot model types")
                })?;
                let pivot_entity = build_related_entity_value(&pivot_ty);
                let local_ident = ctx.resolve_local_key_ident(
                    field.local_key.as_deref().unwrap_or("id"),
                    ident,
                )?;
                let local_column_variant = format_ident!("{}", local_ident.to_string().to_case(Case::Pascal));
                let foreign_key = field.foreign_key.as_deref().unwrap_or("id");
                let pivot_error = format!(
                    "many-to-many relation '{}' references an unknown pivot foreign key '{}'",
                    ident, foreign_key
                );
                let pivot_assert = compile_time_column_assert(&pivot_ty, foreign_key, &pivot_error);

                return Ok(quote! {
                    Self::#variant => {
                        #pivot_assert
                        let mut relation: RelationDef = Entity::belongs_to(#pivot_entity)
                            .from(Column::#local_column_variant)
                            .to(<#pivot_ty as ::tideorm::internal::InternalModel>::column_from_str(#foreign_key)
                                .unwrap_or_else(|| unreachable!(#pivot_error)))
                            .into();
                        relation.rel_type = ::tideorm::sea_orm::RelationType::HasMany;
                        relation
                    }
                });
            }

            let local_key = if field.belongs_to.is_some() {
                field.foreign_key.as_deref().unwrap_or("id")
            } else {
                field.local_key.as_deref().unwrap_or("id")
            };
            let remote_key = if field.belongs_to.is_some() {
                field.owner_key.as_deref().unwrap_or("id")
            } else {
                field.foreign_key.as_deref().unwrap_or("id")
            };
            let local_ident = if field.belongs_to.is_some() {
                ctx.resolve_required_db_field_ident(local_key, ident)?
            } else {
                ctx.resolve_local_key_ident(local_key, ident)?
            };
            let local_column_variant = format_ident!("{}", local_ident.to_string().to_case(Case::Pascal));
            let relation_type = if field.has_many.is_some() {
                quote!(::tideorm::sea_orm::RelationType::HasMany)
            } else {
                quote!(::tideorm::sea_orm::RelationType::HasOne)
            };
            let remote_error = format!(
                "relation '{}' references an unknown remote column '{}'",
                ident, remote_key
            );
            let remote_assert = compile_time_column_assert(&related_ty, remote_key, &remote_error);

            Ok(quote! {
                Self::#variant => {
                    #remote_assert
                    let mut relation: RelationDef = Entity::belongs_to(<<#related_ty as ::tideorm::internal::InternalModel>::Entity as Default>::default())
                        .from(Column::#local_column_variant)
                        .to(<#related_ty as ::tideorm::internal::InternalModel>::column_from_str(#remote_key)
                            .unwrap_or_else(|| unreachable!(#remote_error)))
                        .into();
                    relation.rel_type = #relation_type;
                    relation
                }
            })
        })
        .collect()
}

fn build_related_entity_value(ty: &syn::Type) -> TokenStream2 {
    quote!(<<#ty as ::tideorm::internal::InternalModel>::Entity as Default>::default())
}

fn build_related_impls(ctx: &BuildContext) -> syn::Result<Vec<TokenStream2>> {
    ctx.relation_fields
        .iter()
        .filter(|field| field.is_relation())
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| -> syn::Result<TokenStream2> {
            let related_types = relation_generic_types(&field.ty);
            let related_ty = related_types.first().cloned().ok_or_else(|| {
                syn::Error::new_spanned(&field.ty, "relation field must specify a related model type")
            })?;
            let related_entity = build_related_entity_value(&related_ty);

            if field.has_many_through.is_some() {
                let pivot_ty = related_types.get(1).cloned().ok_or_else(|| {
                    syn::Error::new_spanned(&field.ty, "has_many_through relations must specify both related and pivot model types")
                })?;
                let pivot_entity = build_related_entity_value(&pivot_ty);
                let local_ident = ctx
                    .resolve_local_key_ident(field.local_key.as_deref().unwrap_or("id"), ident)?;
                let local_column_variant = format_ident!("{}", local_ident.to_string().to_case(Case::Pascal));
                let foreign_key = field.foreign_key.as_deref().unwrap_or("id");
                let related_key = field.related_key.as_deref().unwrap_or("id");
                let related_local_key = field.owner_key.as_deref().unwrap_or("id");
                let pivot_related_error = format!(
                    "many-to-many relation '{}' references an unknown pivot related column '{}'",
                    ident, related_key
                );
                let related_column_error = format!(
                    "many-to-many relation '{}' references an unknown related column '{}'",
                    ident, related_local_key
                );
                let pivot_foreign_error = format!(
                    "many-to-many relation '{}' references an unknown pivot foreign key '{}'",
                    ident, foreign_key
                );
                let pivot_related_assert = compile_time_column_assert(&pivot_ty, related_key, &pivot_related_error);
                let related_column_assert = compile_time_column_assert(&related_ty, related_local_key, &related_column_error);
                let pivot_foreign_assert = compile_time_column_assert(&pivot_ty, foreign_key, &pivot_foreign_error);

                return Ok(quote! {
                    impl ::tideorm::sea_orm::Related<<#related_ty as ::tideorm::internal::InternalModel>::Entity> for Entity {
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
                            #pivot_foreign_assert
                            let mut relation: RelationDef = Entity::belongs_to(#pivot_entity)
                                .from(Column::#local_column_variant)
                                .to(<#pivot_ty as ::tideorm::internal::InternalModel>::column_from_str(#foreign_key)
                                    .unwrap_or_else(|| unreachable!(#pivot_foreign_error)))
                                .into();
                            relation.rel_type = ::tideorm::sea_orm::RelationType::HasMany;
                            Some(relation)
                        }
                    }
                });
            }

            let local_key = if field.belongs_to.is_some() {
                field.foreign_key.as_deref().unwrap_or("id")
            } else {
                field.local_key.as_deref().unwrap_or("id")
            };
            let remote_key = if field.belongs_to.is_some() {
                field.owner_key.as_deref().unwrap_or("id")
            } else {
                field.foreign_key.as_deref().unwrap_or("id")
            };
            let local_ident = if field.belongs_to.is_some() {
                ctx.resolve_required_db_field_ident(local_key, ident)?
            } else {
                ctx.resolve_local_key_ident(local_key, ident)?
            };
            let local_column_variant = format_ident!("{}", local_ident.to_string().to_case(Case::Pascal));
            let relation_type = if field.has_many.is_some() {
                quote!(::tideorm::sea_orm::RelationType::HasMany)
            } else {
                quote!(::tideorm::sea_orm::RelationType::HasOne)
            };
            let remote_error = format!(
                "relation '{}' references an unknown remote column '{}'",
                ident, remote_key
            );
            let remote_assert = compile_time_column_assert(&related_ty, remote_key, &remote_error);

            Ok(quote! {
                impl ::tideorm::sea_orm::Related<<#related_ty as ::tideorm::internal::InternalModel>::Entity> for Entity {
                    fn to() -> RelationDef {
                        #remote_assert
                        let mut relation: RelationDef = Entity::belongs_to(#related_entity)
                            .from(Column::#local_column_variant)
                            .to(<#related_ty as ::tideorm::internal::InternalModel>::column_from_str(#remote_key)
                                .unwrap_or_else(|| unreachable!(#remote_error)))
                            .into();
                        relation.rel_type = #relation_type;
                        relation
                    }
                }
            })
        })
        .collect()
}

fn compile_time_column_assert(ty: &syn::Type, column: &str, message: &str) -> TokenStream2 {
    quote! {
        const _: () = {
            if !<#ty>::__has_column_name(#column) {
                panic!(#message);
            }
        };
    }
}

fn generate_sync_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let table_name = &ctx.table_name;
    let schema_name = &ctx.schema_name;
    let field_types = &ctx.field_types;
    let column_names = &ctx.column_names;
    let pk_column_names = &ctx.pk_column_names;
    let sync_column_attrs = &ctx.sync_column_attrs;

    quote! {
        impl #struct_name {
            #[doc(hidden)]
            pub fn __get_sync_schema() -> ::tideorm::sync::ModelSchema {
                use ::tideorm::sync::{ColumnDef, ModelSchema, normalize_rust_type};
                let mut schema = ModelSchema::new(#table_name)
                    .schema(#schema_name)
                    .primary_keys(vec![#(#pk_column_names.to_string()),*]);
                #(
                    {
                        let rust_type = normalize_rust_type(stringify!(#field_types));
                        let mut col = ColumnDef::new(#column_names, rust_type);
                        #sync_column_attrs
                        schema = schema.column(col);
                    }
                )*
                schema
            }

            #[doc(hidden)]
            #[inline]
            pub fn __register_for_sync() {
                ::tideorm::sync::SyncRegistry::register_schema(Self::__get_sync_schema());
            }
        }

        impl ::tideorm::sync::SyncModel for #struct_name {
            fn sync_schema() -> ::tideorm::sync::ModelSchema {
                Self::__get_sync_schema()
            }
        }
    }
}

fn generate_columns_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let columns_struct_name = &ctx.columns_struct_name;
    let columns_struct_fields = &ctx.columns_struct_fields;
    let columns_field_inits = &ctx.columns_field_inits;

    quote! {
        #[allow(non_camel_case_types)]
        #[derive(Clone)]
        pub struct #columns_struct_name {
            #(#columns_struct_fields),*
        }

        impl #struct_name {
            #[allow(non_upper_case_globals)]
            pub const columns: #columns_struct_name = #columns_struct_name {
                #(#columns_field_inits),*
            };
        }
    }
}
