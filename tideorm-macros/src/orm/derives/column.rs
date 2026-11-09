use proc_macro2::{Ident, TokenStream};
use quote::quote;

use super::is_static_iden;
use crate::orm::enum_iter::iterable;

/// A column enum's `IdenStatic`, `Iden` and `Iterable`: each variant stands
/// for its column name.
pub(crate) fn expand_column(ident: &Ident, variants: &[Ident], names: &[String]) -> TokenStream {
    let quoted = names.iter().all(|name| is_static_iden(name)).then(|| {
        quote! {
            fn quoted(&self) -> std::borrow::Cow<'static, str> {
                std::borrow::Cow::Borrowed(::tideorm::orm::IdenStatic::as_str(self))
            }
        }
    });
    let iterable = iterable(ident, variants);

    quote!(
        #[automatically_derived]
        impl ::tideorm::orm::IdenStatic for #ident {
            fn as_str(&self) -> &'static str {
                match self {
                    #(Self::#variants => #names),*
                }
            }
        }

        #[automatically_derived]
        impl ::tideorm::orm::Iden for #ident {
            #quoted

            fn unquoted(&self) -> &str {
                ::tideorm::orm::IdenStatic::as_str(self)
            }
        }

        #iterable
    )
}

/// The `PrimaryKey` enum's impls: [`expand_column`]'s, and its conversions
/// from and to the `Column` variant of the same name.
pub(crate) fn expand_primary_key(variants: &[Ident], names: &[String]) -> TokenStream {
    let ident = Ident::new("PrimaryKey", proc_macro2::Span::call_site());
    let column_impls = expand_column(&ident, variants, names);

    quote!(
        #column_impls

        #[automatically_derived]
        impl ::tideorm::orm::PrimaryKeyToColumn for PrimaryKey {
            type Column = Column;

            fn into_column(self) -> Self::Column {
                match self {
                    #(Self::#variants => Self::Column::#variants,)*
                }
            }

            fn from_column(col: Self::Column) -> Option<Self> {
                match col {
                    #(Self::Column::#variants => Some(Self::#variants),)*
                    _ => None,
                }
            }
        }
    )
}
