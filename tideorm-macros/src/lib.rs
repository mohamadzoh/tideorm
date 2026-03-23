//! TideORM Procedural Macros
//!
//! This crate provides derive macros for TideORM models.

mod context;
mod entity_gen;
mod meta_support;
mod model_trait;
mod parse;
mod relation_gen;
mod serde_gen;
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
use parse::{ModelInput, parse_index_attributes};
use serde_gen::generate_trait_impls;
use tokenization_gen::generate_tokenizable_impl;
use validation_gen::generate_validation_impl;

#[proc_macro_derive(Model, attributes(tideorm, index, unique_index, validate))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let existing_derives = detect_existing_derives(&input.attrs);
    let (indexes, unique_indexes) = parse_index_attributes(&input.attrs);

    let model_input = match ModelInput::from_derive_input(&input) {
        Ok(value) => value,
        Err(error) => return error.write_errors().into(),
    };

    generate_model_impl(&model_input, indexes, unique_indexes, &existing_derives).into()
}

fn generate_model_impl(
    input: &ModelInput,
    indexes: Vec<parse::IndexDef>,
    unique_indexes: Vec<parse::IndexDef>,
    existing_derives: &ExistingDerives,
) -> TokenStream2 {
    match BuildContext::new(input, indexes, unique_indexes, existing_derives) {
        Ok(ctx) => {
            let entity_support = match generate_entity_support(&ctx) {
                Ok(tokens) => tokens,
                Err(error) => return error.to_compile_error(),
            };
            let model_support = generate_model_support(&ctx);
            let validation_impl = generate_validation_impl(&ctx);
            let trait_impls = generate_trait_impls(&ctx);
            let tokenizable_impl = generate_tokenizable_impl(&ctx);

            quote! {
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

    let other_attrs: Vec<_> = attrs.iter().collect();
    let inline_tide_attr = if has_inline_model_options {
        Some(quote! { #[tideorm(#model_attr)] })
    } else {
        None
    };

    Ok(quote! {
        #[derive(tideorm::Model)]
        #inline_tide_attr
        #(#other_attrs)*
        #vis struct #name #generics #fields
    })
}

#[cfg(test)]
#[path = "testing/lib_tests.rs"]
mod tests;
