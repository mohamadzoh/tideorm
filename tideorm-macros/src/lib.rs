//! TideORM Procedural Macros
//!
//! This crate provides derive macros for TideORM models.
//!
//! It also carries the entity derives and attributes that TideORM
//! re-exports from `tideorm::orm`. They read `#[orm(...)]` attributes and
//! their expansions name absolute `::tideorm::orm::` paths, like the `Model`
//! derive's.

mod case;
mod context;
mod entity_gen;
mod meta_support;
mod model_trait;
mod orm;
mod parse;
mod relation_gen;
mod scope_gen;
mod serde_gen;
mod serde_names;
mod tokenization_gen;
mod validation_gen;

use darling::FromDeriveInput;
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

use context::BuildContext;
use entity_gen::generate_entity_support;
use meta_support::{ExistingDerives, detect_existing_derives};
use model_trait::generate_model_support;
use parse::{ModelInput, parse_field_index_attributes, parse_index_attributes};
use scope_gen::generate_query_scope_support;
use serde_gen::generate_trait_impls;
use tokenization_gen::generate_tokenizable_impl;
use validation_gen::generate_validation_impl;

#[proc_macro_derive(Model, attributes(tideorm, index, unique_index, validate))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let existing_derives = detect_existing_derives(&input.attrs);
    let mut indexes = parse_index_attributes(&input.attrs);
    indexes.extend(parse_field_index_attributes(&input.data));

    let model_input = match ModelInput::from_derive_input(&input) {
        Ok(value) => value,
        Err(error) => return error.write_errors().into(),
    };

    generate_model_impl(&model_input, indexes, &existing_derives).into()
}

fn generate_model_impl(
    input: &ModelInput,
    indexes: Vec<parse::IndexDef>,
    existing_derives: &ExistingDerives,
) -> TokenStream2 {
    match BuildContext::new(input, indexes, existing_derives) {
        Ok(ctx) => {
            let entity_support = match generate_entity_support(&ctx) {
                Ok(tokens) => tokens,
                Err(error) => return error.to_compile_error(),
            };
            let model_support = generate_model_support(&ctx);
            let validation_impl = generate_validation_impl(&ctx);
            let trait_impls = generate_trait_impls(&ctx);
            let tokenizable_impl = generate_tokenizable_impl(&ctx);
            let attribute_errors = ctx
                .attribute_errors
                .iter()
                .map(syn::Error::to_compile_error);

            quote! {
                #(#attribute_errors)*
                #entity_support
                #model_support
                #validation_impl
                #trait_impls
                #tokenizable_impl
            }
        }
        Err(error) => error.to_compile_error(),
    }
}

#[proc_macro_attribute]
pub fn model(attr: TokenStream, item: TokenStream) -> TokenStream {
    let model_attr = TokenStream2::from(attr);
    let input = parse_macro_input!(item as DeriveInput);

    expand_model(model_attr, input)
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

#[proc_macro_attribute]
pub fn scopes(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as syn::ItemImpl);

    generate_query_scope_support(input)
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

fn expand_model(model_attr: TokenStream2, input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let vis = &input.vis;
    let attrs = &input.attrs;
    let generics = &input.generics;

    let fields = match &input.data {
        syn::Data::Struct(data) => &data.fields,
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "#[tideorm::model] can only be applied to structs",
            ));
        }
    };

    let has_inline_model_options = !model_attr.is_empty();
    let has_explicit_tide_attr = attrs.iter().any(|attr| attr.path().is_ident("tideorm"));

    if has_inline_model_options && has_explicit_tide_attr {
        return Err(syn::Error::new_spanned(
            &input,
            "use either #[tideorm::model(...)] or a separate #[tideorm(...)] attribute, not both",
        ));
    }

    let inline_tide_attr = if has_inline_model_options {
        Some(quote! { #[tideorm(#model_attr)] })
    } else {
        None
    };
    let struct_body = struct_body_tokens(generics, fields);

    Ok(quote! {
        #[derive(tideorm::Model)]
        #inline_tide_attr
        #(#attrs)*
        #vis struct #name #struct_body
    })
}

/// Re-emit everything after the struct name: generics, fields, `where` clause
/// and — for tuple and unit structs — the trailing `;`.
///
/// `ToTokens for syn::Generics` deliberately prints only the parameter list, so
/// interpolating `#generics` alone silently drops a `where` clause and turns a
/// bounded generic model into one whose bounds no longer hold. The clause also
/// sits in a different place per struct shape: after the fields for a tuple
/// struct, before them for a braced one.
fn struct_body_tokens(generics: &syn::Generics, fields: &syn::Fields) -> TokenStream2 {
    let where_clause = &generics.where_clause;

    match fields {
        syn::Fields::Named(_) => quote! { #generics #where_clause #fields },
        syn::Fields::Unnamed(_) => quote! { #generics #fields #where_clause ; },
        syn::Fields::Unit => quote! { #generics #where_clause ; },
    }
}

// Entry points of the `tideorm::orm` derives.

#[doc = include_str!("../docs/derive_active_enum.md")]
#[proc_macro_derive(DeriveActiveEnum, attributes(orm))]
pub fn derive_active_enum(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    orm::derives::expand_derive_active_enum(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Implements `Iterable` for an enum of unit variants: `ALL` lists them in
/// declaration order.
#[proc_macro_derive(EnumIter)]
pub fn enum_iter(input: TokenStream) -> TokenStream {
    let ast = syn::parse_macro_input!(input as syn::DeriveInput);

    orm::enum_iter::expand(&ast)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Reads a struct from a raw result row, each named field from the column of
/// its name, for `Database::raw_as`. An `Option` field takes SQL `NULL`.
#[proc_macro_derive(FromQueryResult)]
pub fn derive_from_query_result(input: TokenStream) -> TokenStream {
    let ast = syn::parse_macro_input!(input as syn::DeriveInput);

    orm::from_query_result::expand(&ast)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[doc = include_str!("../docs/derive_value_type.md")]
#[proc_macro_derive(DeriveValueType, attributes(orm))]
pub fn derive_value_type(input: TokenStream) -> TokenStream {
    let derive_input = syn::parse_macro_input!(input as syn::DeriveInput);
    orm::derives::expand_derive_value_type(derive_input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[cfg(test)]
#[path = "../tests/unit/lib_tests.rs"]
mod tests;
