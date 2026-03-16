use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};

use crate::context::BuildContext;

pub(crate) fn generate_trait_impls(ctx: &BuildContext) -> TokenStream2 {
    let default_impl = generate_default_impl(ctx);
    let debug_impl = generate_debug_impl(ctx);
    let clone_impl = generate_clone_impl(ctx);
    let serialize_impl = generate_serialize_impl(ctx);
    let deserialize_impl = generate_deserialize_impl(ctx);
    quote! {
        #default_impl
        #debug_impl
        #clone_impl
        #serialize_impl
        #deserialize_impl
    }
}

fn generate_default_impl(ctx: &BuildContext) -> TokenStream2 {
    if !ctx.should_gen_default {
        return quote! {};
    }
    let struct_name = &ctx.struct_name;
    let default_field_inits = &ctx.default_field_inits;
    quote! {
        impl ::std::default::Default for #struct_name {
            fn default() -> Self {
                Self { #(#default_field_inits),* }
            }
        }
    }
}

fn generate_debug_impl(ctx: &BuildContext) -> TokenStream2 {
    if !ctx.should_gen_debug {
        return quote! {};
    }
    let struct_name = &ctx.struct_name;
    let derive_field_names = &ctx.derive_field_names;
    let derive_field_names_str = &ctx.derive_field_names_str;
    quote! {
        impl ::std::fmt::Debug for #struct_name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.debug_struct(stringify!(#struct_name))
                    #(.field(#derive_field_names_str, &self.#derive_field_names))*
                    .finish()
            }
        }
    }
}

fn generate_clone_impl(ctx: &BuildContext) -> TokenStream2 {
    if !ctx.should_gen_clone {
        return quote! {};
    }
    let struct_name = &ctx.struct_name;
    let derive_field_names = &ctx.derive_field_names;
    quote! {
        impl ::std::clone::Clone for #struct_name {
            fn clone(&self) -> Self {
                Self { #(#derive_field_names: self.#derive_field_names.clone()),* }
            }
        }
    }
}

fn generate_serialize_impl(ctx: &BuildContext) -> TokenStream2 {
    if !ctx.should_gen_serialize {
        return quote! {};
    }
    let struct_name = &ctx.struct_name;
    let serde_field_names = &ctx.serde_field_names;
    let serde_field_names_str = &ctx.serde_field_names_str;
    let field_count = serde_field_names.len();
    quote! {
        impl ::serde::Serialize for #struct_name {
            fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
            where
                S: ::serde::Serializer,
            {
                use ::serde::ser::SerializeStruct;
                let mut state = serializer.serialize_struct(stringify!(#struct_name), #field_count)?;
                #(state.serialize_field(#serde_field_names_str, &self.#serde_field_names)?;)*
                state.end()
            }
        }
    }
}

fn generate_deserialize_impl(ctx: &BuildContext) -> TokenStream2 {
    if !ctx.should_gen_deserialize {
        return quote! {};
    }
    let struct_name = &ctx.struct_name;
    let serde_field_names = &ctx.serde_field_names;
    let serde_field_names_str = &ctx.serde_field_names_str;
    let relation_field_defaults = &ctx.relation_field_defaults;
    let field_count = serde_field_names.len();
    let field_indices: Vec<_> = (0..field_count).collect();
    let field_names_upper: Vec<_> = serde_field_names
        .iter()
        .map(|ident| format_ident!("__field_{}", ident))
        .collect();

    quote! {
        impl<'de> ::serde::Deserialize<'de> for #struct_name {
            fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                #[allow(non_camel_case_types)]
                enum __Field { #(#field_names_upper,)* __ignore }

                struct __FieldVisitor;
                impl<'de> ::serde::de::Visitor<'de> for __FieldVisitor {
                    type Value = __Field;
                    fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                        formatter.write_str("field identifier")
                    }
                    fn visit_str<E>(self, value: &str) -> ::std::result::Result<__Field, E>
                    where
                        E: ::serde::de::Error,
                    {
                        match value {
                            #(#serde_field_names_str => Ok(__Field::#field_names_upper),)*
                            _ => Ok(__Field::__ignore),
                        }
                    }
                }

                impl<'de> ::serde::Deserialize<'de> for __Field {
                    fn deserialize<D>(deserializer: D) -> ::std::result::Result<__Field, D::Error>
                    where
                        D: ::serde::Deserializer<'de>,
                    {
                        deserializer.deserialize_identifier(__FieldVisitor)
                    }
                }

                struct __Visitor;
                impl<'de> ::serde::de::Visitor<'de> for __Visitor {
                    type Value = #struct_name;
                    fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                        formatter.write_str(concat!("struct ", stringify!(#struct_name)))
                    }
                    fn visit_map<A>(self, mut map: A) -> ::std::result::Result<#struct_name, A::Error>
                    where
                        A: ::serde::de::MapAccess<'de>,
                    {
                        #(let mut #field_names_upper: Option<_> = None;)*
                        while let Some(key) = map.next_key()? {
                            match key {
                                #(__Field::#field_names_upper => {
                                    if #field_names_upper.is_some() {
                                        return Err(::serde::de::Error::duplicate_field(#serde_field_names_str));
                                    }
                                    #field_names_upper = Some(map.next_value()?);
                                })*
                                __Field::__ignore => {
                                    let _ = map.next_value::<::serde::de::IgnoredAny>()?;
                                }
                            }
                        }
                        Ok(#struct_name {
                            #(#serde_field_names: #field_names_upper.unwrap_or_default(),)*
                            #(#relation_field_defaults,)*
                        })
                    }
                    fn visit_seq<A>(self, mut seq: A) -> ::std::result::Result<#struct_name, A::Error>
                    where
                        A: ::serde::de::SeqAccess<'de>,
                    {
                        #(let #field_names_upper = seq.next_element()?.ok_or_else(|| ::serde::de::Error::invalid_length(#field_indices, &self))?;)*
                        Ok(#struct_name {
                            #(#serde_field_names: #field_names_upper,)*
                            #(#relation_field_defaults,)*
                        })
                    }
                }

                const FIELDS: &'static [&'static str] = &[#(#serde_field_names_str),*];
                deserializer.deserialize_struct(stringify!(#struct_name), FIELDS, __Visitor)
            }
        }
    }
}
