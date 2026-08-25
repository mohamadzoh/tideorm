//! TideORM Procedural Macros
//!
//! This crate provides derive macros for TideORM models.

mod context;
mod entity_gen;
mod meta_support;
mod model_trait;
mod parse;
mod relation_gen;
mod scope_gen;
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
use scope_gen::generate_query_scope_support;
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

    let other_attrs: Vec<_> = attrs.iter().collect();
    let inline_tide_attr = if has_inline_model_options {
        Some(quote! { #[tideorm(#model_attr)] })
    } else {
        None
    };
    let struct_body = struct_body_tokens(generics, fields);

    Ok(quote! {
        #[derive(tideorm::Model)]
        #inline_tide_attr
        #(#other_attrs)*
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

#[cfg(test)]
#[path = "../tests/unit/lib_tests.rs"]
mod tests;

/// Regression tests for codegen defects that are only visible in the emitted
/// tokens (cross-crate visibility, `where` clauses, duplicate match patterns).
///
/// These live inline rather than in `tests/unit/lib_tests.rs` only because the
/// change that introduced them was scoped to `src/`; move them across when the
/// two files are next touched together.
#[cfg(test)]
mod codegen_regression_tests {
    use super::*;

    use darling::FromDeriveInput;
    use proc_macro2::{Delimiter, Group};
    use quote::quote;
    use syn::{DeriveInput, Type, parse_quote};

    use crate::meta_support::detect_existing_derives;
    use crate::parse::ModelInput;
    use crate::serde_gen::is_option_type;

    fn normalize(tokens: &str) -> String {
        tokens.chars().filter(|ch| !ch.is_whitespace()).collect()
    }

    fn expand(input: DeriveInput) -> String {
        let existing_derives = detect_existing_derives(&input.attrs);
        let model_input = ModelInput::from_derive_input(&input).expect("model input should parse");
        let expanded = generate_model_impl(&model_input, vec![], vec![], &existing_derives);

        normalize(&expanded.to_string())
    }

    #[test]
    fn model_attribute_keeps_generic_where_clause() {
        let input: DeriveInput = parse_quote! {
            pub struct Wrapper<T>
            where
                T: Clone,
            {
                pub id: i64,
                pub payload: T,
            }
        };

        let expanded = expand_model(quote!(table = "wrappers"), input)
            .expect("generic model should expand")
            .to_string();

        assert!(normalize(&expanded).contains("structWrapper<T>whereT:Clone"));
    }

    #[test]
    fn model_attribute_keeps_tuple_and_unit_struct_shapes() {
        let tuple: DeriveInput = parse_quote! {
            pub struct Wrapper<T>(pub T) where T: Clone;
        };
        let expanded = expand_model(TokenStream2::new(), tuple)
            .expect("tuple struct should expand")
            .to_string();
        assert!(normalize(&expanded).contains("structWrapper<T>(pubT)whereT:Clone;"));

        let unit: DeriveInput = parse_quote! {
            pub struct Marker;
        };
        let expanded = expand_model(TokenStream2::new(), unit)
            .expect("unit struct should expand")
            .to_string();
        assert!(normalize(&expanded).contains("structMarker;"));
    }

    #[test]
    fn option_detection_sees_through_macro_group_wrappers() {
        let grouped = Group::new(Delimiter::None, quote!(Option<String>));
        let ty: Type = syn::parse2(quote!(#grouped)).expect("grouped type should parse");

        assert!(matches!(ty, Type::Group(_)));
        assert!(is_option_type(&ty));
        assert!(!is_option_type(&parse_quote!(String)));
    }

    #[test]
    fn relation_column_assertion_accessor_is_reachable_across_crates() {
        let expanded = expand(parse_quote! {
            struct User {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
            }
        });

        assert!(expanded.contains("pubconstfn__has_column_name"));
        assert!(!expanded.contains("pub(crate)constfn__has_column_name"));
    }

    #[test]
    fn entity_manager_field_writer_does_not_repeat_identical_patterns() {
        let expanded = expand(parse_quote! {
            struct Customer {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                #[tideorm(column = "customer_phone")]
                phone: String,
            }
        });

        assert!(!expanded.contains("\"id\"|\"id\""));
        assert!(expanded.contains("\"phone\"|\"customer_phone\"=>"));
    }

    #[test]
    fn entity_manager_pk_key_falls_back_instead_of_panicking() {
        let expanded = expand(parse_quote! {
            struct User {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
            }
        });

        assert!(expanded.contains("__pk_to_entity_manager_key(&primary_key).unwrap_or_else("));
        assert!(!expanded.contains("entitymanagerprimarykeyshouldserialize"));
    }

    #[test]
    fn relations_without_an_eager_path_name_the_limitation() {
        let expanded = expand(parse_quote! {
            struct Node {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                children: SelfRefMany<Node>,
            }
        });

        assert!(expanded.contains("hasnoeagerpathforSelfRefManyrelations"));
    }

    /// The tokenization accessors live on `Tokenizable` alone.
    ///
    /// `ModelMeta` used to declare `tokenization_enabled`/`token_encoder`/`token_decoder`
    /// too, which made `User::tokenization_enabled()` an E0034 ambiguity whenever the
    /// prelude put both traits in scope. 0.10 removed them from `ModelMeta`, so the
    /// inherent shims that used to out-resolve the ambiguity are gone as well — emitting
    /// them now would shadow the real `Tokenizable` methods for no reason.
    #[test]
    fn tokenized_models_do_not_emit_tokenization_shims() {
        let expanded = expand(parse_quote! {
            #[tideorm(tokenize)]
            struct Session {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
            }
        });

        assert!(expanded.contains("Tokenizable"));
        assert!(!expanded.contains("Tokenizable>::tokenization_enabled()"));
        assert!(!expanded.contains("Tokenizable>::token_encoder()"));
        assert!(!expanded.contains("Tokenizable>::token_decoder()"));
    }
}
