use super::attributes::ValueTypeAttrs;
use super::value_type_match::{
    array_type_expr, can_try_from_u64, column_type_expr, declared_column_kind,
};
use crate::parse::option_inner_type;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Field, Ident, Type, punctuated::Punctuated, spanned::Spanned, token::Comma};

#[allow(clippy::large_enum_variant)]
enum DeriveValueType {
    TupleStruct(DeriveValueTypeStruct),
    StringLike(DeriveValueTypeString),
}

struct DeriveValueTypeStruct {
    name: syn::Ident,
    ty: Type,
    column_type: TokenStream,
    column_kind: TokenStream,
    array_type: TokenStream,
    can_try_from_u64: bool,
    /// Opt-in `#[orm(try_getable_array)]`: also implement `TryGetableArray`.
    try_getable_array: bool,
}

#[derive(Default)]
struct DeriveValueTypeStructAttrs {
    column_type: Option<TokenStream>,
    array_type: Option<TokenStream>,
    try_from_u64: bool,
    try_getable_array: bool,
}

impl TryFrom<ValueTypeAttrs> for DeriveValueTypeStructAttrs {
    type Error = syn::Error;

    fn try_from(attrs: ValueTypeAttrs) -> syn::Result<Self> {
        Ok(Self {
            column_type: attrs.column_type.map(|s| s.parse()).transpose()?,
            array_type: attrs.array_type.map(|s| s.parse()).transpose()?,
            try_from_u64: attrs.try_from_u64,
            try_getable_array: attrs.try_getable_array,
        })
    }
}

struct DeriveValueTypeString {
    name: syn::Ident,
    from_str: Option<TokenStream>,
    to_str: Option<TokenStream>,
    column_type: Option<TokenStream>,
}

struct DeriveValueTypeStringAttrs {
    from_str: Option<TokenStream>,
    to_str: Option<TokenStream>,
    column_type: Option<TokenStream>,
}

impl TryFrom<ValueTypeAttrs> for DeriveValueTypeStringAttrs {
    type Error = syn::Error;

    fn try_from(attrs: ValueTypeAttrs) -> syn::Result<Self> {
        let value_type = attrs.value_type.map(|s| s.value());
        assert_eq!(value_type.as_deref(), Some("String"));

        Ok(Self {
            from_str: attrs.from_str.map(|s| s.parse()).transpose()?,
            to_str: attrs.to_str.map(|s| s.parse()).transpose()?,
            column_type: attrs.column_type.map(|s| s.parse()).transpose()?,
        })
    }
}

impl DeriveValueType {
    fn new(input: syn::DeriveInput) -> syn::Result<Self> {
        // Produce an error if the macro attributes are malformed
        let value_type_attr = ValueTypeAttrs::try_from_attributes(&input.attrs)?;

        // If some attributes were set, inspect the optional `value_type`
        let value_type = value_type_attr
            .as_ref()
            .and_then(|attr| attr.value_type.as_ref())
            .map(|s| s.value());

        // If either `value_type` is unset, or no attributes were passed, assume
        // `DeriveValueTypeStruct`. If no attrs were set, use default values.
        if value_type.is_none() {
            let value_type_attr = if let Some(value_type_attr) = value_type_attr {
                value_type_attr.try_into()?
            } else {
                DeriveValueTypeStructAttrs::default()
            };

            match input.data {
                syn::Data::Struct(syn::DataStruct {
                    fields: syn::Fields::Unnamed(syn::FieldsUnnamed { unnamed, .. }),
                    ..
                }) => {
                    return DeriveValueTypeStruct::new(input.ident, value_type_attr, unnamed)
                        .map(Self::TupleStruct);
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        input,
                        "You can only derive `DeriveValueType` on a struct with a single unnamed field, unless `value_type` is set.",
                    ));
                }
            }
        }

        let value_type_attr = value_type_attr.unwrap();
        let value_type = value_type.unwrap();

        match value_type.as_str() {
            "String" => Ok(Self::StringLike(DeriveValueTypeString::new(
                input.ident,
                value_type_attr.try_into()?,
            ))),
            _ => Err(syn::Error::new_spanned(
                input.ident,
                r#"Please specify value_type = "String""#,
            )),
        }
    }

    fn expand(&self) -> TokenStream {
        match self {
            Self::TupleStruct(s) => s.impl_value_type(),
            Self::StringLike(s) => s.impl_value_type(),
        }
    }
}

impl DeriveValueTypeStruct {
    fn new(
        name: Ident,
        attrs: DeriveValueTypeStructAttrs,
        fields: Punctuated<Field, Comma>,
    ) -> syn::Result<Self> {
        let Some(field) = fields.into_iter().next() else {
            return Err(syn::Error::new_spanned(
                name,
                "You can only derive `DeriveValueType` on tuple struct with 1 inner value",
            ));
        };

        let field_span = field.span();
        let ty = field.ty;
        let option_inner = option_inner_type(&ty);
        let field_type = option_inner.unwrap_or(&ty);

        let column_kind = attrs.column_type.as_ref().map_or_else(
            || quote!(<#field_type as ::tideorm::orm::sql_builder::ValueType>::COLUMN_KIND),
            declared_column_kind,
        );
        let column_type = column_type_expr(attrs.column_type, field_type, field_span);
        let array_type = array_type_expr(attrs.array_type, field_type, field_span);
        let can_try_from_u64 =
            attrs.try_from_u64 || (option_inner.is_none() && can_try_from_u64(field_type));

        Ok(Self {
            name,
            ty,
            column_type,
            column_kind,
            array_type,
            can_try_from_u64,
            try_getable_array: attrs.try_getable_array,
        })
    }

    fn impl_value_type(&self) -> TokenStream {
        let name = &self.name;
        let field_type = &self.ty;
        let column_type = &self.column_type;
        let array_type = &self.array_type;

        let try_from_u64_impl = if self.can_try_from_u64 {
            quote!(
                #[automatically_derived]
                impl ::tideorm::orm::TryFromU64 for #name {
                    fn try_from_u64(n: u64) -> ::core::result::Result<Self, ::tideorm::orm::DbErr> {
                        use std::convert::TryInto;
                        Ok(Self(n.try_into().map_err(|e| ::tideorm::orm::DbErr::TryIntoErr {
                            from: stringify!(u64),
                            into: stringify!(#name),
                            source: std::sync::Arc::new(e),
                        })?))
                    }
                }
            )
        } else {
            quote!()
        };

        let impl_try_getable_array = if cfg!(feature = "postgres") && self.try_getable_array {
            quote!(
                #[automatically_derived]
                impl ::tideorm::orm::TryGetableArray for #name {
                    fn try_get_by<I: ::tideorm::orm::ColIdx>(res: &::tideorm::orm::QueryResult, index: I)
                        -> std::result::Result<Vec<Self>, ::tideorm::orm::TryGetError> {
                        Ok(<Vec<#field_type> as ::tideorm::orm::TryGetable>::try_get_by(res, index)?
                            .into_iter()
                            .map(#name)
                            .collect())
                    }
                }
            )
        } else {
            quote!()
        };

        let value = Ident::new("v", Span::call_site());
        let value_type = value_type_impl(
            name,
            &value,
            quote!(<#field_type as ::tideorm::orm::sql_builder::ValueType>::try_from(#value).map(|v| #name(v))),
            array_type,
            column_type,
            &self.column_kind,
        );
        let shared = shared_impls(name);

        quote!(
            #[automatically_derived]
            impl std::convert::From<#name> for ::tideorm::orm::Value {
                fn from(source: #name) -> Self {
                    source.0.into()
                }
            }

            #[automatically_derived]
            impl ::tideorm::orm::TryGetable for #name {
                fn try_get_by<I: ::tideorm::orm::ColIdx>(res: &::tideorm::orm::QueryResult, idx: I)
                    -> std::result::Result<Self, ::tideorm::orm::TryGetError> {
                    <#field_type as ::tideorm::orm::TryGetable>::try_get_by(res, idx).map(|v| #name(v))
                }
            }

            #value_type

            #[automatically_derived]
            impl ::tideorm::orm::sql_builder::Nullable for #name {
                fn null() -> ::tideorm::orm::Value {
                    <#field_type as ::tideorm::orm::sql_builder::Nullable>::null()
                }
            }

            #shared

            #try_from_u64_impl

            #impl_try_getable_array
        )
    }
}

impl DeriveValueTypeString {
    fn new(name: Ident, attrs: DeriveValueTypeStringAttrs) -> Self {
        Self {
            name,
            from_str: attrs.from_str,
            to_str: attrs.to_str,
            column_type: attrs.column_type,
        }
    }

    fn impl_value_type(&self) -> TokenStream {
        let name = &self.name;
        let from_str = match &self.from_str {
            Some(from_str) => from_str,
            None => &quote!(std::str::FromStr::from_str),
        };
        let to_str = match &self.to_str {
            Some(to_str) => to_str,
            None => &quote!(std::string::ToString::to_string),
        };
        let column_type = match &self.column_type {
            Some(column_type) => column_type,
            None => &quote!(String(::tideorm::orm::sql_builder::StringLen::None)),
        };
        let source = Ident::new("source", Span::mixed_site());
        let res = Ident::new("res", Span::mixed_site());
        let idx = Ident::new("idx", Span::mixed_site());
        let string = Ident::new("string", Span::mixed_site());
        let err = Ident::new("err", Span::mixed_site());
        let value = Ident::new("v", Span::mixed_site());

        let value_type = value_type_impl(
            name,
            &value,
            quote!(
                let #string = <String as ::tideorm::orm::sql_builder::ValueType>::try_from(#value)?;
                #from_str(&#string).map_err(|_| ::tideorm::orm::sql_builder::ValueTypeErr)
            ),
            &quote!(::tideorm::orm::sql_builder::ArrayType::String),
            &quote!(::tideorm::orm::sql_builder::ColumnType::#column_type),
            &declared_column_kind(column_type),
        );
        let shared = shared_impls(name);

        quote!(
            #[automatically_derived]
            impl std::convert::From<#name> for ::tideorm::orm::Value {
                fn from(#source: #name) -> Self {
                    #to_str(&#source).into()
                }
            }

            #[automatically_derived]
            impl ::tideorm::orm::TryGetable for #name {
                fn try_get_by<I: ::tideorm::orm::ColIdx>(#res: &::tideorm::orm::QueryResult, #idx: I)
                    -> std::result::Result<Self, ::tideorm::orm::TryGetError> {
                    let #string = String::try_get_by(#res, #idx)?;
                    #from_str(&#string).map_err(|#err| {
                        ::tideorm::orm::TryGetError::DbErr(::tideorm::orm::DbErr::TryIntoErr {
                            from: "String",
                            into: stringify!(#name),
                            source: std::sync::Arc::new(#err),
                        })
                    })
                }
            }

            #value_type

            #[automatically_derived]
            impl ::tideorm::orm::sql_builder::Nullable for #name {
                fn null() -> ::tideorm::orm::Value {
                    ::tideorm::orm::Value::String(None)
                }
            }

            #shared
        )
    }
}

/// The `ValueType` impl of `name`, whose `try_from(value)` runs `try_from`.
fn value_type_impl(
    name: &Ident,
    value: &Ident,
    try_from: TokenStream,
    array_type: &TokenStream,
    column_type: &TokenStream,
    column_kind: &TokenStream,
) -> TokenStream {
    quote!(
        #[automatically_derived]
        impl ::tideorm::orm::sql_builder::ValueType for #name {
            fn try_from(#value: ::tideorm::orm::Value) -> std::result::Result<Self, ::tideorm::orm::sql_builder::ValueTypeErr> {
                #try_from
            }

            fn type_name() -> std::string::String {
                stringify!(#name).to_owned()
            }

            fn array_type() -> ::tideorm::orm::sql_builder::ArrayType {
                #array_type
            }

            fn column_type() -> ::tideorm::orm::sql_builder::ColumnType {
                #column_type
            }

            const COLUMN_KIND: ::tideorm::orm::sql_builder::ColumnKind = #column_kind;
        }
    )
}

/// The impls both kinds of value type emit alike: `IntoActiveValue`, and on
/// PostgreSQL `NotU8`, which lets a `Vec` of it be an array.
fn shared_impls(name: &Ident) -> TokenStream {
    let not_u8 = if cfg!(feature = "postgres") {
        quote!(
            #[automatically_derived]
            impl ::tideorm::orm::sql_builder::postgres_array::NotU8 for #name {}
        )
    } else {
        quote!()
    };
    quote!(
        #[automatically_derived]
        impl ::tideorm::orm::IntoActiveValue<#name> for #name {
            fn into_active_value(self) -> ::tideorm::orm::ActiveValue<#name> {
                ::tideorm::orm::ActiveValue::Set(self)
            }
        }

        #not_u8
    )
}

pub fn expand_derive_value_type(input: syn::DeriveInput) -> syn::Result<TokenStream> {
    Ok(DeriveValueType::new(input)?.expand())
}
