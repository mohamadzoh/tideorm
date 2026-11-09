use super::*;

impl ActiveEnum {
    pub(super) fn to_value_impl(&self) -> TokenStream {
        let enum_name = &self.enum_name;
        let variant_idents = &self.variant_idents;
        let variant_values = &self.variant_values;

        if self.generate_enum_impls() {
            quote! {
                let value = match self {
                    #( Self::#variant_idents => #variant_values, )*
                };
                ::tideorm::orm::sql_builder::Enum {
                    type_name: #enum_name.into(),
                    value: value.into(),
                }
            }
        } else {
            quote! {
                match self {
                    #( Self::#variant_idents => #variant_values, )*
                }
                .to_owned()
            }
        }
    }

    pub(super) fn value_type_try_from_impl(&self) -> TokenStream {
        if self.generate_enum_impls() {
            quote! {
                use ::tideorm::orm::sql_builder::{OptionEnum, Value, ValueTypeErr};

                match v {
                    Value::Enum(value) => match value {
                        OptionEnum::Some(value) => <Self as ::tideorm::orm::ActiveEnum>::try_from_value(value.as_ref())
                            .map_err(|_| ValueTypeErr),
                        OptionEnum::None(_) => Err(ValueTypeErr),
                    },
                    _ => Err(ValueTypeErr),
                }
            }
        } else {
            quote! {
                use ::tideorm::orm::sql_builder::{ValueType, ValueTypeErr};

                let value = <<Self as ::tideorm::orm::ActiveEnum>::Value as ValueType>::try_from(v)?;
                <Self as ::tideorm::orm::ActiveEnum>::try_from_value(&value).map_err(|_| ValueTypeErr)
            }
        }
    }

    pub(super) fn nullable_impl(&self) -> TokenStream {
        let ident = &self.ident;
        let enum_name = &self.enum_name;
        let nullable_value_impl = if self.generate_enum_impls() {
            quote! {
                use ::tideorm::orm::sql_builder::{OptionEnum, Value};
                Value::Enum(OptionEnum::None(#enum_name.into()))
            }
        } else {
            quote! {
                use ::tideorm::orm::sql_builder;
                <<Self as ::tideorm::orm::ActiveEnum>::Value as sql_builder::Nullable>::null()
            }
        };

        quote! {
            #[automatically_derived]
            impl ::tideorm::orm::sql_builder::Nullable for #ident {
                fn null() -> ::tideorm::orm::sql_builder::Value {
                    #nullable_value_impl
                }
            }
        }
    }

    pub(super) fn value_type_impl(&self) -> TokenStream {
        let ident = &self.ident;
        let value_type_try_from_impl = self.value_type_try_from_impl();
        let enum_name = &self.enum_name;

        let type_name_impl = quote! { stringify!(#ident).to_owned() };

        let value_type_array_type = if self.generate_enum_impls() {
            quote! {
                ::tideorm::orm::sql_builder::ArrayType::Enum(Box::new(#enum_name.into()))
            }
        } else {
            quote! {
                <<Self as ::tideorm::orm::ActiveEnum>::Value as ::tideorm::orm::sql_builder::ValueType>::array_type()
            }
        };

        let enum_type_name = if self.db_type.is_enum() {
            quote! { Some(#enum_name) }
        } else {
            quote! { None }
        };

        quote! {
            #[automatically_derived]
            impl ::tideorm::orm::sql_builder::ValueType for #ident {
                fn try_from(v: ::tideorm::orm::sql_builder::Value) -> std::result::Result<Self, ::tideorm::orm::sql_builder::ValueTypeErr> {
                    #value_type_try_from_impl
                }

                fn type_name() -> String {
                    #type_name_impl
                }

                fn array_type() -> ::tideorm::orm::sql_builder::ArrayType {
                    #value_type_array_type
                }

                fn column_type() -> ::tideorm::orm::sql_builder::ColumnType {
                    <Self as ::tideorm::orm::ActiveEnum>::db_type()
                        .get_column_type()
                        .to_owned()
                        .into()
                }

                fn enum_type_name() -> Option<&'static str> {
                    #enum_type_name
                }
            }
        }
    }

    pub(super) fn try_getable_impl(&self) -> TokenStream {
        let ident = &self.ident;
        let postgres_try_get = if cfg!(feature = "postgres") && self.db_type.is_enum() {
            quote! {
                if let Some(result) = res.try_get_from_postgres::<Self, I>(idx) {
                    return result;
                }
            }
        } else {
            quote!()
        };
        let try_get_by_impl = {
            let enum_name = &self.enum_name;
            if self.generate_enum_impls() {
                quote! {
                    #postgres_try_get
                    let value: String = <String as ::tideorm::orm::TryGetable>::try_get_by(res, idx)?;
                    let value = ::tideorm::orm::sql_builder::Enum {
                        type_name: #enum_name.into(),
                        value: value.into(),
                    };
                    <Self as ::tideorm::orm::ActiveEnum>::try_from_value(&value)
                        .map_err(::tideorm::orm::TryGetError::DbErr)
                }
            } else {
                quote! {
                    #postgres_try_get
                    let value = <<Self as ::tideorm::orm::ActiveEnum>::Value as ::tideorm::orm::TryGetable>::try_get_by(res, idx)?;
                    <Self as ::tideorm::orm::ActiveEnum>::try_from_value(&value)
                        .map_err(::tideorm::orm::TryGetError::DbErr)
                }
            }
        };

        quote! {
            #[automatically_derived]
            impl ::tideorm::orm::TryGetable for #ident {
                fn try_get_by<I: ::tideorm::orm::ColIdx>(res: &::tideorm::orm::QueryResult, idx: I) -> std::result::Result<Self, ::tideorm::orm::TryGetError> {
                    #try_get_by_impl
                }
            }
        }
    }

    pub(super) fn active_enum_impl(&self) -> TokenStream {
        let ident = &self.ident;
        let enum_name_iden = format_ident!("{}Enum", ident);
        let rs_type = &self.rs_type;
        let variant_idents = &self.variant_idents;
        let variant_values = &self.variant_values;
        let to_value_body = self.to_value_impl();
        let column_type = {
            match &self.db_type {
                DbType::Enum => {
                    let values = self.string_values();
                    quote! {
                        Enum {
                            name: <Self as ::tideorm::orm::ActiveEnum>::name(),
                            variants: ::std::vec![
                                #(::tideorm::orm::sql_builder::DynIden::from(#values)),*
                            ],
                        }
                    }
                }
                DbType::Other(db_type) => db_type.clone(),
            }
        };

        let val = if self.generate_enum_impls() {
            quote! { v.value.as_ref() }
        } else if self.is_string {
            quote! { <<Self as ::tideorm::orm::ActiveEnum>::Value as std::convert::AsRef<str>>::as_ref(v) }
        } else {
            quote! { v }
        };

        quote! {
            #[automatically_derived]
            impl ::tideorm::orm::ActiveEnum for #ident {
                type Value = #rs_type;

                type ValueVec = Vec<#rs_type>;

                fn name() -> ::tideorm::orm::sql_builder::DynIden {
                    #enum_name_iden.into()
                }

                fn to_value(&self) -> <Self as ::tideorm::orm::ActiveEnum>::Value {
                    #to_value_body
                }

                fn try_from_value(v: &<Self as ::tideorm::orm::ActiveEnum>::Value) -> std::result::Result<Self, ::tideorm::orm::DbErr> {
                    match #val {
                        #( #variant_values => Ok(Self::#variant_idents), )*
                        _ => Err(::tideorm::orm::DbErr::Type(format!(
                            "unexpected value for {} enum: {}",
                            stringify!(#ident),
                            #val
                        ))),
                    }
                }

                fn db_type() -> ::tideorm::orm::ColumnDef {
                    ::tideorm::orm::prelude::ColumnTypeTrait::def(::tideorm::orm::ColumnType::#column_type)
                }
            }
        }
    }

    pub(super) fn convert_impls(&self) -> TokenStream {
        let ident = &self.ident;
        let variant_idents = &self.variant_idents;
        let variant_values = &self.variant_values;

        let try_from_str_impl = if self.db_type.is_enum() {
            quote! {
                #[automatically_derived]
                impl std::convert::TryFrom<&str> for #ident {
                    type Error = ::tideorm::orm::DbErr;

                    fn try_from(source: &str) -> std::result::Result<Self, ::tideorm::orm::DbErr> {
                        match source {
                            #( #variant_values => Ok(Self::#variant_idents), )*
                            _ => Err(::tideorm::orm::DbErr::Type(format!(
                                "unexpected value for {} enum: {}",
                                stringify!(#ident),
                                source
                            ))),
                        }
                    }
                }
            }
        } else {
            quote!()
        };

        if self.generate_enum_impls() {
            let enum_name = &self.enum_name;

            quote! {
                #[automatically_derived]
                impl std::convert::From<#ident> for ::tideorm::orm::sql_builder::Enum {
                    fn from(source: #ident) -> Self {
                        let value = match source {
                            #( #ident::#variant_idents => #variant_values, )*
                        };
                        Self {
                            type_name: #enum_name.into(),
                            value: value.into(),
                        }
                    }
                }

                #[automatically_derived]
                impl std::convert::From<#ident> for ::tideorm::orm::sql_builder::Value {
                    fn from(source: #ident) -> Self {
                        let enum_value = ::tideorm::orm::sql_builder::Enum::from(source);
                        ::tideorm::orm::sql_builder::Value::from(enum_value)
                    }
                }

                #try_from_str_impl
            }
        } else {
            quote! {
                #[automatically_derived]
                impl std::convert::From<#ident> for ::tideorm::orm::sql_builder::Value {
                    fn from(source: #ident) -> Self {
                        <#ident as ::tideorm::orm::ActiveEnum>::to_value(&source).into()
                    }
                }

                #try_from_str_impl
            }
        }
    }

    pub(super) fn postgres_impl(&self) -> TokenStream {
        if !cfg!(feature = "postgres") || !self.db_type.is_enum() {
            return quote!();
        }

        let ident = &self.ident;
        let enum_name = &self.enum_name;
        let ident_s = ident.to_string();
        let variant_idents = &self.variant_idents;
        let variant_values = &self.variant_values;

        quote! {
            #[automatically_derived]
            impl ::tideorm::orm::driver::Type<::tideorm::orm::driver::Postgres> for #ident {
                fn type_info() -> ::tideorm::orm::driver::postgres::PgTypeInfo {
                    ::tideorm::orm::driver::postgres::PgTypeInfo::with_name(#enum_name)
                }
            }

            #[automatically_derived]
            impl<'r> ::tideorm::orm::driver::decode::Decode<'r, ::tideorm::orm::driver::Postgres> for #ident {
                fn decode(
                    value: ::tideorm::orm::driver::postgres::PgValueRef<'r>,
                ) -> std::result::Result<
                    Self,
                    std::boxed::Box<
                        dyn std::error::Error + 'static + std::marker::Send + std::marker::Sync,
                    >,
                > {
                    let value = <&'r str as ::tideorm::orm::driver::decode::Decode<
                        'r,
                        ::tideorm::orm::driver::Postgres,
                    >>::decode(value)?;

                    match value {
                        #( #variant_values => Ok(Self::#variant_idents), )*
                        _ => Err(format!("invalid value {:?} for enum {}", value, #ident_s).into()),
                    }
                }
            }

            #[automatically_derived]
            impl ::tideorm::orm::driver::postgres::PgHasArrayType for #ident {
                fn array_type_info() -> ::tideorm::orm::driver::postgres::PgTypeInfo {
                    ::tideorm::orm::driver::postgres::PgTypeInfo::array_of(#enum_name)
                }
            }
        }
    }

    pub(super) fn try_getable_array_impl(&self) -> TokenStream {
        let ident = &self.ident;

        if cfg!(feature = "postgres") {
            let postgres_try_get = if self.db_type.is_enum() {
                quote! {
                    if let Some(result) = res.try_get_from_postgres::<Vec<Self>, I>(index) {
                        return result;
                    }
                }
            } else {
                quote!()
            };
            quote!(
                #[automatically_derived]
                impl ::tideorm::orm::TryGetableArray for #ident {
                    fn try_get_by<I: ::tideorm::orm::ColIdx>(res: &::tideorm::orm::QueryResult, index: I) -> std::result::Result<Vec<Self>, ::tideorm::orm::TryGetError> {
                        #postgres_try_get
                        <<Self as ::tideorm::orm::ActiveEnum>::Value as ::tideorm::orm::ActiveEnumValue>::try_get_vec_by(res, index)?
                            .into_iter()
                            .map(|value| <Self as ::tideorm::orm::ActiveEnum>::try_from_value(&value).map_err(Into::into))
                            .collect()
                    }
                }
            )
        } else {
            quote!()
        }
    }
}
