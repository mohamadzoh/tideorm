//! Schema sync and typed columns.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};

use crate::context::BuildContext;
use crate::parse::is_optional_type;

pub(super) fn generate_sync_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let table_name = &ctx.table_name;
    let schema_call = ctx
        .schema_name
        .as_ref()
        .map(|schema| quote!(.schema(#schema)));
    let pk_column_names = &ctx.pk_column_names;
    let columns = ctx.db_fields.iter().map(|field| {
        let column_name = field.column_name();
        let ty = field.stored_type(ctx.is_encrypted(field));
        let primary_key = field.primary_key.then(|| quote!(.primary_key()));
        let auto_increment = field.auto_increment.then(|| quote!(.auto_increment()));
        let not_null =
            (!field.nullable && !is_optional_type(&field.ty)).then(|| quote!(.not_null()));
        let default = field
            .default
            .as_ref()
            .map(|default| quote!(.default(#default)));
        quote! {
            ::tideorm::sync::ColumnDef::new(
                #column_name,
                <#ty as ::tideorm::orm::sql_builder::ValueType>::column_type()
            )
                #primary_key #auto_increment #not_null #default
        }
    });

    let forbids_raw_sql = ctx.project_rules.forbids_raw_sql;
    // Read by the build, so editing the rules recompiles the model.
    let rules_file = ctx.project_rules.file.as_ref().map(|file| {
        quote!(
            const _: &[u8] = include_bytes!(#file);
        )
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
                schema_name: <#struct_name as ::tideorm::model::ModelMeta>::schema_name,
                column_names: <#struct_name as ::tideorm::model::ModelMeta>::column_names,
                encrypted_column: ::tideorm::model::__encrypted_column::<#struct_name>,
                forbids_raw_sql: #forbids_raw_sql,
            }
        }
        #rules_file
    }
}

pub(super) fn generate_columns_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let columns_struct_name = &ctx.columns_struct_name;
    let field_idents = &ctx.field_idents;
    let field_types = &ctx.field_types;
    let column_names = &ctx.column_names;
    let table_name = &ctx.table_name;
    let schema = ctx
        .schema_name
        .as_ref()
        .map_or_else(|| quote!(None), |schema| quote!(Some(#schema)));

    // A field's constant is its name, `r#` removed, in upper case.
    let constant_ident = |name: &str| format_ident!("{}", name.to_uppercase());
    let constant_idents: Vec<_> = ctx
        .field_names
        .iter()
        .map(|name| constant_ident(name))
        .collect();
    let relation_constants = ctx.relation_fields.iter().map(|field| {
        let name = field.name();
        let ident = constant_ident(&name);
        quote! {
            #[doc = concat!("The `", #name, "` relation for eager loading.")]
            pub const #ident: &'static str = #name;
        }
    });

    quote! {
        // Its fields are the model's, which may be `camelCase`.
        #[allow(non_camel_case_types, non_snake_case)]
        #[derive(Clone)]
        pub struct #columns_struct_name {
            #(pub #field_idents: ::tideorm::columns::Column<#field_types>),*
        }

        impl #struct_name {
            #(
                #[doc = concat!("Typed reference to the `", #column_names, "` column.")]
                pub const #constant_idents: ::tideorm::columns::Column<#field_types> =
                    ::tideorm::columns::Column::of_schema(#schema, #table_name, #column_names);
            )*
            #(#relation_constants)*

            #[allow(non_upper_case_globals)]
            pub const columns: #columns_struct_name = #columns_struct_name {
                #(#field_idents: Self::#constant_idents),*
            };
        }
    }
}
