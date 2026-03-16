use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

use crate::context::BuildContext;

pub(crate) fn generate_entity_support(ctx: &BuildContext) -> TokenStream2 {
    let base_impl = generate_base_impl(ctx);
    let sync_impl = generate_sync_impl(ctx);
    let columns_impl = generate_columns_impl(ctx);
    quote! {
        #base_impl
        #sync_impl
        #columns_impl
    }
}

fn generate_base_impl(ctx: &BuildContext) -> TokenStream2 {
    let internal_entity_mod = &ctx.internal_entity_mod;
    let table_name = &ctx.table_name;
    let struct_name = &ctx.struct_name;
    let pk_column_variant = &ctx.pk_column_variant;
    let pk_type = &ctx.pk_type;
    let pk_auto_increment = ctx.pk_auto_increment;
    let pk_ident = &ctx.pk_ident;
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
    let timestamps_enabled = ctx.timestamps_enabled;
    let allowed_languages_impl = ctx.allowed_languages_impl();
    let fallback_language_impl = ctx.fallback_language_impl();

    quote! {
        #[doc(hidden)]
        #[allow(non_snake_case, dead_code, unused_imports, clippy::derivable_impls, clippy::enum_variant_names, clippy::redundant_closure)]
        mod #internal_entity_mod {
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
                #pk_column_variant
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

            #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
            pub enum Relation {}

            impl ActiveModelBehavior for ActiveModel {}
        }

        impl ::tideorm::model::ModelMeta for #struct_name {
            type PrimaryKey = #pk_type;
            fn table_name() -> &'static str { #table_name }
            fn primary_key_name() -> &'static str { stringify!(#pk_ident) }
            fn primary_key_auto_increment() -> bool { #pk_auto_increment }
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
            fn has_timestamps() -> bool { #timestamps_enabled }
            fn indexes() -> Vec<::tideorm::model::IndexDefinition> { vec![#(#index_impls),*] }
            fn unique_indexes() -> Vec<::tideorm::model::IndexDefinition> { vec![#(#unique_index_impls),*] }
        }
    }
}

fn generate_sync_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let table_name = &ctx.table_name;
    let schema_name = &ctx.schema_name;
    let field_types = &ctx.field_types;
    let column_names = &ctx.column_names;
    let sync_column_attrs = &ctx.sync_column_attrs;

    quote! {
        impl #struct_name {
            #[doc(hidden)]
            pub fn __get_sync_schema() -> ::tideorm::sync::ModelSchema {
                use ::tideorm::sync::{ColumnDef, ModelSchema, normalize_rust_type};
                let mut schema = ModelSchema::new(#table_name).schema(#schema_name);
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
