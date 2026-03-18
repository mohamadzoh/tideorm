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

    let other_attrs: Vec<_> = attrs
        .iter()
        .filter(|attr| !attr.path().is_ident("derive"))
        .collect();
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
mod tests {
    use super::*;
    use quote::quote;
    use syn::{Type, parse_quote};

    use crate::parse::ModelField;

    fn normalize_tokens(tokens: &str) -> String {
        tokens.chars().filter(|ch| !ch.is_whitespace()).collect()
    }

    fn field_with_type(ty: Type) -> ModelField {
        ModelField {
            ident: Some(parse_quote!(field)),
            ty,
            attrs: vec![],
            primary_key: false,
            auto_increment: false,
            column: None,
            nullable: false,
            default: None,
            skip: false,
            timestamp: false,
            has_one: None,
            has_many: None,
            belongs_to: None,
            has_many_through: None,
            foreign_key: None,
            owner_key: None,
            local_key: None,
            pivot: None,
            related_key: None,
            morph_name: None,
        }
    }

    #[test]
    fn detects_direct_relation_wrappers() {
        for ty in [
            parse_quote!(HasOne<User>),
            parse_quote!(::tideorm::relations::HasMany<Post>),
            parse_quote!(BelongsTo<Account>),
            parse_quote!(HasManyThrough<Role, UserRole>),
            parse_quote!(MorphTo<Commentable>),
            parse_quote!(MorphOne<Image>),
            parse_quote!(MorphMany<Tag>),
            parse_quote!(SelfRef<Employee>),
            parse_quote!(SelfRefMany<Employee>),
        ] {
            assert!(field_with_type(ty).is_relation_type());
        }
    }

    #[test]
    fn detects_wrapped_relation_wrappers() {
        assert!(field_with_type(parse_quote!(Option<HasOne<User>>)).is_relation_type());
        assert!(field_with_type(parse_quote!(Box<::tideorm::MorphMany<Tag>>)).is_relation_type());
    }

    #[test]
    fn does_not_match_non_relation_names_by_substring() {
        assert!(!field_with_type(parse_quote!(HasOneCount)).is_relation_type());
        assert!(!field_with_type(parse_quote!(MyBelongsToMetadata)).is_relation_type());
        assert!(!field_with_type(parse_quote!(Vec<HasManyLabel>)).is_relation_type());
        assert!(!field_with_type(parse_quote!(String)).is_relation_type());
    }

    #[test]
    fn model_attribute_accepts_inline_table_options() {
        let input: DeriveInput = parse_quote! {
            pub struct User {
                pub id: i64,
            }
        };

        let expanded = expand_model(quote!(table = "users", soft_delete), input)
            .expect("inline model attribute should expand successfully")
            .to_string();
        let normalized = normalize_tokens(&expanded);

        assert!(normalized.contains("#[derive(tideorm::Model)]"));
        assert!(normalized.contains("#[tideorm(table=\"users\",soft_delete)]"));
    }

    #[test]
    fn model_attribute_preserves_stacked_tideorm_attribute() {
        let input: DeriveInput = parse_quote! {
            #[tideorm(table = "users")]
            pub struct User {
                pub id: i64,
            }
        };

        let expanded = expand_model(TokenStream2::new(), input)
            .expect("stacked tideorm syntax should still expand successfully")
            .to_string();
        let normalized = normalize_tokens(&expanded);

        assert!(normalized.contains("#[tideorm(table=\"users\")]"));
    }

    #[test]
    fn model_attribute_rejects_mixed_inline_and_stacked_options() {
        let input: DeriveInput = parse_quote! {
            #[tideorm(table = "users")]
            pub struct User {
                pub id: i64,
            }
        };

        let error = expand_model(quote!(table = "users"), input)
            .expect_err("mixed syntax should be rejected")
            .to_string();

        assert!(error.contains("use either #[tideorm::model(...)] or a separate #[tideorm(...)]"));
    }
}
