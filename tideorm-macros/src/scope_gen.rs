use proc_macro2::{Group, TokenStream as TokenStream2, TokenTree};
use quote::{ToTokens, format_ident, quote};
use syn::punctuated::Punctuated;
use syn::{
    Attribute, FnArg, GenericArgument, ImplItem, ImplItemFn, ItemImpl, Meta, Pat, PatIdent,
    PathArguments, ReturnType, Token, Type, TypePath,
};

/// Every `pub` method with a receiver that `QueryBuilder` defines itself:
/// in its inherent impls, and through `condition_methods!` and
/// `conditional_methods!`, whatever feature gates it. Sorted.
///
/// Method calls try a type's own methods before a trait's taking `self`
/// the same way, so a scope named after one of them is never called (or,
/// against a `&self` method, hides it). The macro cannot see TideORM's
/// types, so the names are listed here; a unit test compares the list with
/// TideORM's source.
pub(crate) const QUERY_BUILDER_METHODS: &[&str] = &[
    "aggregates",
    "apply",
    "avg",
    "begin_or",
    "build_sql_preview",
    "cache",
    "cache_with_key",
    "chunk",
    "consolidate",
    "count",
    "count_distinct",
    "debug",
    "delete",
    "delete_all",
    "dense_rank",
    "distinct",
    "exists",
    "find",
    "find_or_fail",
    "first",
    "first_or_fail",
    "first_value",
    "force_delete",
    "get",
    "get_as",
    "get_json",
    "group_by",
    "has_any_related",
    "has_no_related",
    "has_no_related_at_all",
    "has_related",
    "having",
    "inner_join",
    "inner_join_as",
    "lag",
    "latest",
    "lead",
    "left_join",
    "left_join_as",
    "limit",
    "lock_for_update",
    "max",
    "min",
    "no_cache",
    "ntile",
    "offset",
    "oldest",
    "only_trashed",
    "or_where",
    "or_where_array_contained_by",
    "or_where_array_contains",
    "or_where_array_overlaps",
    "or_where_between",
    "or_where_column_eq",
    "or_where_column_gt",
    "or_where_column_gte",
    "or_where_column_lt",
    "or_where_column_lte",
    "or_where_column_ne",
    "or_where_contains",
    "or_where_ends_with",
    "or_where_eq",
    "or_where_gt",
    "or_where_gte",
    "or_where_in",
    "or_where_json_contained_by",
    "or_where_json_contains",
    "or_where_json_key_exists",
    "or_where_json_key_not_exists",
    "or_where_json_path_exists",
    "or_where_json_path_not_exists",
    "or_where_like",
    "or_where_lt",
    "or_where_lte",
    "or_where_not",
    "or_where_not_between",
    "or_where_not_in",
    "or_where_not_like",
    "or_where_not_null",
    "or_where_null",
    "or_where_raw",
    "or_where_raw_with",
    "or_where_starts_with",
    "order_asc",
    "order_by",
    "order_by_raw",
    "order_desc",
    "page",
    "paginate",
    "pluck",
    "rank",
    "reorder",
    "restore",
    "right_join",
    "right_join_as",
    "row_number",
    "running_sum",
    "select",
    "select_also_linked",
    "select_raw",
    "select_subquery",
    "select_with_linked",
    "soft_delete",
    "sort",
    "sum",
    "to_subquery_sql",
    "union",
    "union_all",
    "union_all_raw",
    "union_raw",
    "update_all",
    "validate",
    "value",
    "when",
    "when_some",
    "where_array_contained_by",
    "where_array_contains",
    "where_array_overlaps",
    "where_between",
    "where_col",
    "where_column_eq",
    "where_column_gt",
    "where_column_gte",
    "where_column_lt",
    "where_column_lte",
    "where_column_ne",
    "where_contains",
    "where_doesnt_have",
    "where_ends_with",
    "where_eq",
    "where_exists",
    "where_gt",
    "where_gte",
    "where_has",
    "where_in",
    "where_in_subquery",
    "where_json_contained_by",
    "where_json_contains",
    "where_json_key_exists",
    "where_json_key_not_exists",
    "where_json_path_exists",
    "where_json_path_not_exists",
    "where_like",
    "where_lt",
    "where_lte",
    "where_not",
    "where_not_between",
    "where_not_exists",
    "where_not_in",
    "where_not_in_subquery",
    "where_not_like",
    "where_not_null",
    "where_null",
    "where_raw",
    "where_raw_with",
    "where_starts_with",
    "window",
    "with",
    "with_cte",
    "with_many",
    "with_query",
    "with_recursive_cte",
    "with_trashed",
];

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

    let cfg_attrs = conditional_attributes(&item_impl.attrs)?;
    let model_ty = &*item_impl.self_ty;
    let model_ident = model_ident(model_ty)?;
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

        let generated = generate_scope_method(method, model_ty)?;
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

        #(#cfg_attrs)*
        pub trait #trait_ident {
            #(#trait_methods)*
        }

        #(#cfg_attrs)*
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
        || !matches!(method.sig.safety, syn::Safety::Default)
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

    let method_ident = &method.sig.ident;
    let name = crate::parse::unraw_ident(method_ident);
    if QUERY_BUILDER_METHODS.binary_search(&name.as_str()).is_ok() {
        return Err(syn::Error::new_spanned(
            method_ident,
            format!(
                "a scope cannot be named `{name}`: QueryBuilder has its own `{name}` method, \
                 and `query.{name}(..)` cannot call both; rename the scope"
            ),
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

        // The trait method has no body, where a `mut` binding is an error, and
        // in the trait `Self` is the query builder, not the model.
        let attrs = &pat_type.attrs;
        let ty = replace_self(pat_type.ty.to_token_stream(), model_ty);
        forwarded_inputs.push(quote! { #(#attrs)* #arg_ident: #ty });
        let cfg_attrs = conditional_attributes(attrs)?;
        forwarded_args.push(quote! { #(#cfg_attrs)* #arg_ident });
    }

    let cfg_attrs = conditional_attributes(&method.attrs)?;
    let signature = quote! {
        #(#cfg_attrs)*
        #[must_use]
        fn #method_ident(self #(, #forwarded_inputs)*) -> ::tideorm::QueryBuilder<#model_ty>
    };
    Ok(GeneratedScopeMethod {
        trait_method: quote!(#signature;),
        extension_method: quote! {
            #signature {
                #model_ty::#method_ident(self #(, #forwarded_args)*)
            }
        },
    })
}

// Only copy compilation gates: other attributes (such as `inline`) may be
// invalid on a bodyless trait method or a forwarded argument expression.
fn conditional_attributes(attrs: &[Attribute]) -> syn::Result<Vec<TokenStream2>> {
    attrs.iter().try_fold(Vec::new(), |mut result, attr| {
        if let Some(meta) = conditional_meta(&attr.meta)? {
            result.push(quote! { #[#meta] });
        }
        Ok(result)
    })
}

fn conditional_meta(meta: &Meta) -> syn::Result<Option<Meta>> {
    if meta.path().is_ident("cfg") {
        return Ok(Some(meta.clone()));
    }
    if !meta.path().is_ident("cfg_attr") {
        return Ok(None);
    }
    let Meta::List(list) = meta else {
        return Err(syn::Error::new_spanned(meta, "expected cfg_attr arguments"));
    };
    let mut args = list
        .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?
        .into_iter();
    let predicate = args
        .next()
        .ok_or_else(|| syn::Error::new_spanned(meta, "expected cfg_attr predicate"))?;
    let mut gates = Vec::new();
    for arg in args {
        if let Some(gate) = conditional_meta(&arg)? {
            gates.push(gate);
        }
    }
    if gates.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            syn::parse_quote! { cfg_attr(#predicate, #(#gates),*) },
        ))
    }
}

/// `tokens` with every `Self` spelled as `model_ty`.
fn replace_self(tokens: TokenStream2, model_ty: &Type) -> TokenStream2 {
    tokens
        .into_iter()
        .map(|tree| match tree {
            TokenTree::Ident(ident) if ident == "Self" => quote!(#model_ty),
            TokenTree::Group(group) => {
                let mut replaced =
                    Group::new(group.delimiter(), replace_self(group.stream(), model_ty));
                replaced.set_span(group.span());
                TokenStream2::from(TokenTree::Group(replaced))
            }
            other => TokenStream2::from(other),
        })
        .collect()
}

fn model_ident(model_ty: &Type) -> syn::Result<&syn::Ident> {
    let Type::Path(TypePath {
        qself: None, path, ..
    }) = model_ty
    else {
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
    let Type::Path(TypePath {
        qself: None, path, ..
    }) = ty
    else {
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
    if matches!(lhs, Type::Path(TypePath { qself: None, path, .. }) if path.is_ident("Self")) {
        return true;
    }

    lhs == rhs
}
