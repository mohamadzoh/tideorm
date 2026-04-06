use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    FnArg, GenericArgument, ImplItem, ImplItemFn, ItemImpl, Pat, PatIdent, PathArguments,
    ReturnType, Type, TypePath,
};

pub(crate) fn generate_query_scope_support(item_impl: ItemImpl) -> syn::Result<TokenStream2> {
    if item_impl.trait_.is_some() {
        return Err(syn::Error::new_spanned(
            &item_impl.self_ty,
            "#[tideorm::scopes] can only be applied to inherent impl blocks",
        ));
    }

    if !item_impl.generics.params.is_empty() || item_impl.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &item_impl.generics,
            "#[tideorm::scopes] does not support generic impl blocks",
        ));
    }

    let model_ty = (*item_impl.self_ty).clone();
    let model_ident = model_ident(&model_ty)?;
    let trait_ident = format_ident!("{}QueryScopes", model_ident);

    let mut trait_methods = Vec::new();
    let mut extension_methods = Vec::new();

    for item in &item_impl.items {
        let method = match item {
            ImplItem::Fn(method) => method,
            _ => {
                return Err(syn::Error::new_spanned(
                    item,
                    "#[tideorm::scopes] impl blocks may only contain scope methods",
                ));
            }
        };

        let generated = generate_scope_method(method, &model_ty)?;
        trait_methods.push(generated.trait_method);
        extension_methods.push(generated.extension_method);
    }

    if trait_methods.is_empty() {
        return Err(syn::Error::new_spanned(
            &item_impl.self_ty,
            "#[tideorm::scopes] requires at least one scope method",
        ));
    }

    Ok(quote! {
        #item_impl

        pub trait #trait_ident {
            #(#trait_methods)*
        }

        impl #trait_ident for ::tideorm::QueryBuilder<#model_ty> {
            #(#extension_methods)*
        }
    })
}

struct GeneratedScopeMethod {
    trait_method: TokenStream2,
    extension_method: TokenStream2,
}

fn generate_scope_method(
    method: &ImplItemFn,
    model_ty: &Type,
) -> syn::Result<GeneratedScopeMethod> {
    if method.sig.receiver().is_some() {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "scope methods must be associated functions whose first argument is QueryBuilder<Self>",
        ));
    }

    if method.sig.asyncness.is_some()
        || method.sig.constness.is_some()
        || method.sig.unsafety.is_some()
        || method.sig.abi.is_some()
    {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "scope methods must be plain synchronous Rust functions",
        ));
    }

    if !method.sig.generics.params.is_empty() || method.sig.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &method.sig.generics,
            "scope methods do not support generic parameters",
        ));
    }

    let mut inputs = method.sig.inputs.iter();
    let query_input = inputs.next().ok_or_else(|| {
        syn::Error::new_spanned(
            &method.sig,
            "scope methods must accept QueryBuilder<Self> as their first argument",
        )
    })?;

    match query_input {
        FnArg::Typed(pat_type) if is_query_builder_type(&pat_type.ty, model_ty) => {}
        FnArg::Typed(pat_type) => {
            return Err(syn::Error::new_spanned(
                &pat_type.ty,
                "scope methods must accept QueryBuilder<Self> as their first argument",
            ));
        }
        FnArg::Receiver(receiver) => {
            return Err(syn::Error::new_spanned(
                receiver,
                "scope methods may not take a self receiver",
            ));
        }
    }

    match &method.sig.output {
        ReturnType::Type(_, ty) if is_query_builder_type(ty, model_ty) => {}
        ReturnType::Type(_, ty) => {
            return Err(syn::Error::new_spanned(
                ty,
                "scope methods must return QueryBuilder<Self>",
            ));
        }
        ReturnType::Default => {
            return Err(syn::Error::new_spanned(
                &method.sig.output,
                "scope methods must return QueryBuilder<Self>",
            ));
        }
    }

    let mut forwarded_inputs = Vec::new();
    let mut forwarded_args = Vec::new();

    for input in inputs {
        let FnArg::Typed(pat_type) = input else {
            continue;
        };

        let arg_ident = match pat_type.pat.as_ref() {
            Pat::Ident(PatIdent { ident, .. }) => ident,
            _ => {
                return Err(syn::Error::new_spanned(
                    &pat_type.pat,
                    "scope parameters after the query argument must use simple identifiers",
                ));
            }
        };

        forwarded_inputs.push(pat_type.clone());
        forwarded_args.push(quote! { #arg_ident });
    }

    let method_ident = &method.sig.ident;

    Ok(GeneratedScopeMethod {
        trait_method: quote! {
            #[must_use]
            fn #method_ident(self #(, #forwarded_inputs)*) -> ::tideorm::QueryBuilder<#model_ty>;
        },
        extension_method: quote! {
            #[must_use]
            fn #method_ident(self #(, #forwarded_inputs)*) -> ::tideorm::QueryBuilder<#model_ty> {
                #model_ty::#method_ident(self #(, #forwarded_args)*)
            }
        },
    })
}

fn model_ident(model_ty: &Type) -> syn::Result<&syn::Ident> {
    let Type::Path(TypePath { qself: None, path }) = model_ty else {
        return Err(syn::Error::new_spanned(
            model_ty,
            "#[tideorm::scopes] requires a concrete model type",
        ));
    };

    path.segments
        .last()
        .map(|segment| &segment.ident)
        .ok_or_else(|| {
            syn::Error::new_spanned(
                model_ty,
                "#[tideorm::scopes] requires a concrete model type",
            )
        })
}

fn is_query_builder_type(ty: &Type, model_ty: &Type) -> bool {
    let Type::Path(TypePath { qself: None, path }) = ty else {
        return false;
    };

    let Some(segment) = path.segments.last() else {
        return false;
    };

    if segment.ident != "QueryBuilder" {
        return false;
    }

    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };

    if arguments.args.len() != 1 {
        return false;
    }

    matches!(arguments.args.first(), Some(GenericArgument::Type(argument_ty)) if same_model_type(argument_ty, model_ty))
}

fn same_model_type(lhs: &Type, rhs: &Type) -> bool {
    if matches!(lhs, Type::Path(TypePath { qself: None, path }) if path.is_ident("Self")) {
        return true;
    }

    lhs == rhs
}
