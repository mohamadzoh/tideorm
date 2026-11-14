use proc_macro2::{Span, TokenStream, TokenTree};
use quote::{quote, quote_spanned};
use syn::{Ident, Type, TypePath};

pub fn column_type_expr(
    column_type: Option<TokenStream>,
    field_type: &Type,
    field_span: Span,
) -> TokenStream {
    match column_type {
        Some(column_type) => {
            quote_spanned! { field_span => ::tideorm::orm::prelude::ColumnType::#column_type }
        }
        None => {
            quote_spanned! { field_span => <#field_type as ::tideorm::orm::sql_builder::ValueType>::column_type() }
        }
    }
}

/// `ValueType::COLUMN_KIND` for a column type an attribute declares: the
/// kind `ColumnType::kind` gives its variant, read from the variant's name so
/// a `const` holds it even when the variant's payload is built at run time.
pub fn declared_column_kind(column_type: &TokenStream) -> TokenStream {
    let variant = column_type
        .clone()
        .into_iter()
        .find_map(|token| match token {
            TokenTree::Ident(ident) => Some(ident.to_string()),
            _ => None,
        });
    let kind = match variant.as_deref() {
        Some(
            "TinyInteger" | "SmallInteger" | "Integer" | "TinyUnsigned" | "SmallUnsigned"
            | "Unsigned",
        ) => "Integer",
        Some("BigInteger" | "BigUnsigned") => "BigInteger",
        Some("Float" | "Double") => "Float",
        Some("Uuid") => "Uuid",
        Some("Char" | "String" | "Text") => "Text",
        Some("Json" | "JsonBinary") => "Json",
        _ => "Other",
    };
    let kind = Ident::new(kind, Span::call_site());
    quote!(::tideorm::orm::sql_builder::ColumnKind::#kind)
}

fn is_type(type_path: &TypePath, ident: &str) -> bool {
    type_path.path.segments.len() == 1
        && type_path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == ident)
}

pub fn array_type_expr(
    array_type: Option<TokenStream>,
    field_type: &Type,
    field_span: Span,
) -> TokenStream {
    match array_type {
        Some(array_type) => {
            quote_spanned! { field_span => ::tideorm::orm::sql_builder::ArrayType::#array_type }
        }
        None => {
            quote_spanned! { field_span => <#field_type as ::tideorm::orm::sql_builder::ValueType>::array_type() }
        }
    }
}

pub fn can_try_from_u64(field_type: &Type) -> bool {
    let Type::Path(type_path) = field_type else {
        return false;
    };
    ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"]
        .iter()
        .any(|int| is_type(type_path, int))
}
