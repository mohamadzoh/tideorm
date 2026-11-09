use super::case_style::CaseStyle;
use crate::case::to_pascal_case;
use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::{Expr, Lit, LitInt, LitStr, UnOp, parse};

mod value;

struct ActiveEnum {
    ident: syn::Ident,
    enum_name: String,
    rs_type: RsType,
    db_type: DbType,
    is_string: bool,
    variants: Vec<ActiveEnumVariant>,
    variant_idents: Vec<syn::Ident>,
    variant_values: Vec<TokenStream>,
    rename_all: Option<CaseStyle>,
}

enum RsType {
    String,
    Enum,
    Other(TokenStream),
}

impl RsType {
    fn from_attr(
        ident_span: proc_macro2::Span,
        rs_type: Option<String>,
        db_type: &DbType,
    ) -> Result<Self, Error> {
        if db_type.is_enum() {
            match rs_type.as_deref() {
                None => Ok(RsType::Enum),
                Some(value) => RsType::from_database_enum_attr_value(value).ok_or_else(|| {
                    Error::TT(quote_spanned! {
                        ident_span => compile_error!("`db_type = \"Enum\"` only supports `rs_type = \"String\"` or `rs_type = \"Enum\"` (or omit `rs_type`)");
                    })
                }),
            }
        } else {
            let Some(rs_type) = rs_type else {
                return Err(Error::TT(quote_spanned! {
                    ident_span => compile_error!("Missing macro attribute `rs_type`");
                }));
            };

            if rs_type == "Enum" {
                return Err(Error::TT(quote_spanned! {
                    ident_span => compile_error!("`rs_type = \"Enum\"` requires `db_type = \"Enum\"`");
                }));
            }

            RsType::from_str(&rs_type).map_err(Error::Syn)
        }
    }

    fn from_str(value: &str) -> syn::Result<Self> {
        Ok(Self::Other(syn::parse_str::<TokenStream>(value)?))
    }

    fn from_database_enum_attr_value(value: &str) -> Option<Self> {
        match value {
            "Enum" => Some(Self::Enum),
            "String" => Some(Self::String),
            _ => None,
        }
    }
}

impl quote::ToTokens for RsType {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            RsType::String => tokens.extend(quote! { String }),
            RsType::Enum => tokens.extend(quote! { ::tideorm::orm::sql_builder::Enum }),
            RsType::Other(rs_type) => tokens.extend(rs_type.clone()),
        }
    }
}

enum DbType {
    Enum,
    Other(TokenStream),
}

impl DbType {
    fn from_attr(ident_span: proc_macro2::Span, db_type: Option<String>) -> Result<Self, Error> {
        let Some(db_type) = db_type else {
            return Err(Error::TT(quote_spanned! {
                ident_span => compile_error!("Missing macro attribute `db_type`");
            }));
        };

        DbType::from_str(&db_type).map_err(Error::Syn)
    }

    fn from_str(value: &str) -> syn::Result<Self> {
        match value {
            "Enum" => Ok(Self::Enum),
            _ => Ok(Self::Other(syn::parse_str::<TokenStream>(value)?)),
        }
    }

    fn is_enum(&self) -> bool {
        matches!(self, DbType::Enum)
    }
}

struct ActiveEnumVariant {
    ident: syn::Ident,
    string_value: Option<LitStr>,
    num_value: Option<LitInt>,
    rename: Option<CaseStyle>,
}

enum Error {
    InputNotEnum,
    Syn(syn::Error),
    TT(TokenStream),
}

impl ActiveEnum {
    // Nested `if let`s, which clippy would collapse into let chains.
    #[allow(clippy::collapsible_if)]
    fn new(input: syn::DeriveInput) -> Result<Self, Error> {
        let ident_span = input.ident.span();
        let ident = input.ident;

        let mut enum_name = to_pascal_case(&ident.to_string());
        let mut rs_type = None;
        let mut db_type = None;
        let mut rename_all = None;

        input
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("orm"))
            .try_for_each(|attr| {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("rs_type") {
                        let litstr: LitStr = meta.value()?.parse()?;
                        rs_type = Some(litstr.value());
                    } else if meta.path.is_ident("db_type") {
                        let litstr: LitStr = meta.value()?.parse()?;
                        db_type = Some(litstr.value());
                    } else if meta.path.is_ident("enum_name") {
                        let litstr: LitStr = meta.value()?.parse()?;
                        enum_name = litstr.value();
                    } else if meta.path.is_ident("rename_all") {
                        rename_all = Some((&meta).try_into()?);
                    } else {
                        return Err(meta.error(format!(
                            "Unknown attribute parameter found: {:?}",
                            meta.path.get_ident()
                        )));
                    }
                    Ok(())
                })
                .map_err(Error::Syn)
            })?;

        let db_type = DbType::from_attr(ident_span, db_type)?;
        let rs_type = RsType::from_attr(ident_span, rs_type, &db_type)?;

        let syn::Data::Enum(syn::DataEnum {
            variants: variant_vec,
            ..
        }) = input.data
        else {
            return Err(Error::InputNotEnum);
        };

        let mut is_string = rename_all.is_some();
        let mut is_int = false;
        let mut variants = Vec::new();

        for variant in variant_vec {
            let variant_span = variant.ident.span();
            let mut string_value = None;
            let mut num_value = None;
            let mut rename_rule = None;

            for attr in variant.attrs.iter() {
                if !attr.path().is_ident("orm") {
                    continue;
                }
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("string_value") {
                        is_string = true;
                        string_value = Some(meta.value()?.parse::<LitStr>()?);
                    } else if meta.path.is_ident("num_value") {
                        is_int = true;
                        num_value = Some(meta.value()?.parse::<LitInt>()?);
                    } else if meta.path.is_ident("rename") {
                        is_string = true;
                        rename_rule = Some((&meta).try_into()?);
                    } else {
                        return Err(meta.error(format!(
                            "Unknown attribute parameter found: {:?}",
                            meta.path.get_ident()
                        )));
                    }

                    Ok(())
                })
                .map_err(Error::Syn)?;
            }

            if is_string && is_int {
                return Err(Error::TT(quote_spanned! {
                    ident_span => compile_error!("All enum variants should specify the same `*_value` macro attribute, either `string_value` or `num_value` but not both");
                }));
            }

            if string_value.is_none() && num_value.is_none() && rename_rule.or(rename_all).is_none()
            {
                match variant.discriminant {
                    Some((_, Expr::Lit(exprlit))) => {
                        if let Lit::Int(litint) = exprlit.lit {
                            is_int = true;
                            num_value = Some(litint);
                        } else {
                            return Err(Error::TT(quote_spanned! {
                                variant_span => compile_error!("Enum variant discriminant is not an integer");
                            }));
                        }
                    }
                    //rust doesn't provide negative variants in enums as a single LitInt, this workarounds that
                    Some((_, Expr::Unary(exprnlit))) => {
                        if let UnOp::Neg(_) = exprnlit.op {
                            if let Expr::Lit(exprlit) = *exprnlit.expr {
                                if let Lit::Int(litint) = exprlit.lit {
                                    let negative_token = quote! { -#litint };
                                    let litint = parse(negative_token.into()).unwrap();

                                    is_int = true;
                                    num_value = Some(litint);
                                }
                            }
                        } else {
                            return Err(Error::TT(quote_spanned! {
                                variant_span => compile_error!("Only - token is supported in enum variants, not ! and *");
                            }));
                        }
                    }
                    _ => {
                        return Err(Error::TT(quote_spanned! {
                            variant_span => compile_error!("Missing macro attribute, either `string_value`, `num_value` or `rename` should be specified or specify repr[X] and have a value for every entry");
                        }));
                    }
                }
            }

            variants.push(ActiveEnumVariant {
                ident: variant.ident,
                string_value,
                num_value,
                rename: rename_rule,
            });
        }

        if db_type.is_enum() && is_int {
            return Err(Error::TT(quote_spanned! {
                ident_span => compile_error!("`db_type = \"Enum\"` does not support `num_value` or numeric discriminants");
            }));
        }

        let variant_idents: Vec<syn::Ident> = variants
            .iter()
            .map(|variant| variant.ident.clone())
            .collect();

        let variant_values: Vec<TokenStream> = variants
            .iter()
            .map(|variant| {
                let variant_span = variant.ident.span();

                if let Some(string_value) = &variant.string_value {
                    let string = string_value.value();
                    Ok(quote! { #string })
                } else if let Some(num_value) = &variant.num_value {
                    Ok(quote! { #num_value })
                } else if let Some(rename_rule) = variant.rename.or(rename_all) {
                    let variant_ident = rename_rule.apply(&variant.ident.to_string());
                    Ok(quote! { #variant_ident })
                } else {
                    Err(Error::TT(quote_spanned! {
                        variant_span => compile_error!("Missing macro attribute, either `string_value`, `num_value` or `rename_all` should be specified");
                    }))
                }
            })
            .collect::<Result<_, _>>()?;

        Ok(Self {
            ident,
            enum_name,
            rs_type,
            db_type,
            is_string,
            variants,
            variant_idents,
            variant_values,
            rename_all,
        })
    }

    fn generate_enum_impls(&self) -> bool {
        self.db_type.is_enum() && matches!(self.rs_type, RsType::Enum)
    }

    /// The variants' string values: given, or the variant renamed.
    fn string_values(&self) -> Vec<String> {
        let rename_all = self.rename_all;
        self.variants
            .iter()
            .filter_map(|variant| {
                variant
                    .string_value
                    .as_ref()
                    .map(|string_value| string_value.value())
                    .or_else(|| {
                        variant
                            .rename
                            .map(|rename| rename.apply(&variant.ident.to_string()))
                    })
                    .or_else(|| rename_all.map(|rule| rule.apply(&variant.ident.to_string())))
            })
            .collect()
    }

    fn expand(&self) -> TokenStream {
        let Self {
            ident, enum_name, ..
        } = self;

        let enum_name_iden = format_ident!("{}Enum", ident);

        let not_u8_impl = if cfg!(feature = "postgres") {
            quote!(
                #[automatically_derived]
                impl ::tideorm::orm::sql_builder::postgres_array::NotU8 for #ident {}
            )
        } else {
            quote!()
        };

        let value_type_impl = self.value_type_impl();
        let convert_impls = self.convert_impls();
        let nullable_impl = self.nullable_impl();
        let impl_try_getable_array = self.try_getable_array_impl();
        let active_enum_impl = self.active_enum_impl();
        let try_getable_impl = self.try_getable_impl();
        let postgres_impl = self.postgres_impl();

        quote!(
            #[doc = " Generated by tideorm-macros"]
            #[derive(Debug, Clone, PartialEq, Eq)]
            pub struct #enum_name_iden;

            #[automatically_derived]
            impl ::tideorm::orm::Iden for #enum_name_iden {
                fn unquoted(&self) -> &str {
                    #enum_name
                }
            }

            #active_enum_impl

            #impl_try_getable_array

            #convert_impls

            #try_getable_impl

            #postgres_impl

            #value_type_impl

            #nullable_impl

            #[automatically_derived]
            impl ::tideorm::orm::IntoActiveValue<#ident> for #ident {
                fn into_active_value(self) -> ::tideorm::orm::ActiveValue<#ident> {
                    ::tideorm::orm::ActiveValue::set(self)
                }
            }

            #not_u8_impl
        )
    }
}

pub fn expand_derive_active_enum(input: syn::DeriveInput) -> syn::Result<TokenStream> {
    let ident_span = input.ident.span();

    match ActiveEnum::new(input) {
        Ok(model) => Ok(model.expand()),
        Err(Error::InputNotEnum) => Ok(quote_spanned! {
            ident_span => compile_error!("you can only derive ActiveEnum on enums");
        }),
        Err(Error::TT(token_stream)) => Ok(token_stream),
        Err(Error::Syn(e)) => Err(e),
    }
}
