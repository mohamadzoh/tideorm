//! `#[derive(FromQueryResult)]`: a struct read from a row, each field from
//! the column of its name.

use proc_macro2::TokenStream;
use quote::quote;
use syn::ext::IdentExt;
use syn::{Data, DeriveInput, Fields};

pub fn expand(ast: &DeriveInput) -> syn::Result<TokenStream> {
    let Data::Struct(data) = &ast.data else {
        return Err(syn::Error::new_spanned(
            &ast.ident,
            "FromQueryResult only derives for a struct",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &ast.ident,
            "FromQueryResult reads columns by name, so it needs named fields",
        ));
    };
    let values = fields.named.iter().map(|field| {
        let ident = field.ident.as_ref().expect("a named field has a name");
        let column = ident.unraw().to_string();
        quote!(#ident: row.try_get(#column)?)
    });
    let name = &ast.ident;
    let (impl_generics, type_generics, where_clause) = ast.generics.split_for_impl();
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::tideorm::orm::FromQueryResult for #name #type_generics #where_clause {
            fn from_query_result(
                row: &::tideorm::orm::QueryResult,
            ) -> ::core::result::Result<Self, ::tideorm::orm::DbErr> {
                ::core::result::Result::Ok(Self { #(#values),* })
            }
        }
    })
}

#[cfg(test)]
#[path = "../../tests/unit/orm/from_query_result_tests.rs"]
mod tests;
