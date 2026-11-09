//! `#[derive(EnumIter)]`: an enum's variants as `Iterable::ALL`.

use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Fields};

pub fn expand(ast: &DeriveInput) -> syn::Result<TokenStream> {
    let Data::Enum(data) = &ast.data else {
        return Err(syn::Error::new_spanned(
            &ast.ident,
            "EnumIter only derives for an enum",
        ));
    };
    if !ast.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &ast.generics,
            "EnumIter does not derive for a generic enum",
        ));
    }
    let variants = data
        .variants
        .iter()
        .map(|variant| match variant.fields {
            Fields::Unit => Ok(variant.ident.clone()),
            _ => Err(syn::Error::new_spanned(
                variant,
                "EnumIter lists unit variants only",
            )),
        })
        .collect::<syn::Result<Vec<_>>>()?;
    Ok(iterable(&ast.ident, &variants))
}

/// `Iterable` for the enum `name` of unit `variants`.
pub(crate) fn iterable(name: &Ident, variants: &[Ident]) -> TokenStream {
    quote! {
        #[automatically_derived]
        impl ::tideorm::orm::Iterable for #name {
            const ALL: &'static [Self] = &[#(#name::#variants),*];
        }
    }
}
