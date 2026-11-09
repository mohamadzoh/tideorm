use proc_macro2::{Span, TokenStream};
use quote::quote_spanned;
use syn::{Type, TypePath};

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
