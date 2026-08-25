use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::Type;

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
                Self { #(#default_field_inits),* }.with_relations()
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
    let relation_field_idents: Vec<_> = ctx
        .serde_field_names
        .iter()
        .filter(|field_ident| {
            ctx.relation_fields.iter().any(|field| {
                field
                    .ident
                    .as_ref()
                    .is_some_and(|ident| ident == *field_ident)
            })
        })
        .cloned()
        .collect();
    let relation_field_names_str: Vec<_> = ctx
        .serde_field_names
        .iter()
        .zip(ctx.serde_field_names_str.iter())
        .filter(|(field_ident, _)| {
            ctx.relation_fields.iter().any(|field| {
                field
                    .ident
                    .as_ref()
                    .is_some_and(|ident| ident == *field_ident)
            })
        })
        .map(|(_, field_name)| field_name.clone())
        .collect();
    let non_relation_field_names: Vec<_> = ctx
        .serde_field_names
        .iter()
        .filter(|field_ident| {
            !ctx.relation_fields.iter().any(|field| {
                field
                    .ident
                    .as_ref()
                    .is_some_and(|ident| ident == *field_ident)
            })
        })
        .cloned()
        .collect();
    let non_relation_field_names_str: Vec<_> = ctx
        .serde_field_names
        .iter()
        .zip(ctx.serde_field_names_str.iter())
        .filter(|(field_ident, _)| {
            !ctx.relation_fields.iter().any(|field| {
                field
                    .ident
                    .as_ref()
                    .is_some_and(|ident| ident == *field_ident)
            })
        })
        .map(|(_, field_name)| field_name.clone())
        .collect();
    let base_field_count = non_relation_field_names.len();
    quote! {
        impl ::serde::Serialize for #struct_name {
            fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
            where
                S: ::serde::Serializer,
            {
                use ::serde::ser::SerializeStruct;
                let relation_field_count = 0usize #( + usize::from(self.#relation_field_idents.get_cached().is_some()))*;
                let mut state = serializer.serialize_struct(
                    stringify!(#struct_name),
                    #base_field_count + relation_field_count,
                )?;
                #(state.serialize_field(#non_relation_field_names_str, &self.#non_relation_field_names)?;)*
                #(if self.#relation_field_idents.get_cached().is_some() {
                    state.serialize_field(#relation_field_names_str, &self.#relation_field_idents)?;
                })*
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
    let serde_field_types = &ctx.serde_field_types;
    let serde_field_names_str = &ctx.serde_field_names_str;
    let field_count = serde_field_names.len();
    let field_indices: Vec<_> = (0..field_count).collect();
    let field_names_upper: Vec<_> = serde_field_names
        .iter()
        .map(|ident| format_ident!("__field_{}", ident))
        .collect();
    let field_defaults: Vec<_> = serde_field_names
        .iter()
        .zip(serde_field_types.iter())
        .map(|(field_ident, field_ty)| {
            let is_option = is_option_type(field_ty);
            let is_auto_increment_primary_key = ctx.pk_auto_increment
                && ctx.pk_idents.iter().any(|pk_ident| pk_ident == field_ident);
            let is_relation_field = ctx.relation_fields.iter().any(|field| {
                field
                    .ident
                    .as_ref()
                    .is_some_and(|ident| ident == field_ident)
            });

            is_option || is_auto_increment_primary_key || is_relation_field
        })
        .collect();
    let field_resolutions: Vec<_> = serde_field_names
        .iter()
        .zip(serde_field_names_str.iter())
        .zip(field_names_upper.iter())
        .zip(field_defaults.iter())
        .map(|(((field_ident, field_name_str), temp_ident), use_default)| {
            if *use_default {
                quote!(#field_ident: #temp_ident.unwrap_or_default())
            } else {
                quote!(
                    #field_ident: #temp_ident.ok_or_else(|| ::serde::de::Error::missing_field(#field_name_str))?
                )
            }
        })
        .collect();
    let seq_field_resolutions: Vec<_> = field_names_upper
        .iter()
        .zip(field_indices.iter())
        .zip(field_defaults.iter())
        .map(|((temp_ident, field_index), use_default)| {
            if *use_default {
                quote!(let #temp_ident = seq.next_element()?.unwrap_or_default();)
            } else {
                quote!(
                    let #temp_ident = seq
                        .next_element()?
                        .ok_or_else(|| ::serde::de::Error::invalid_length(#field_index, &self))?;
                )
            }
        })
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
                        let model = #struct_name {
                            #(#field_resolutions,)*
                        };
                        Ok(model.with_relations())
                    }
                    fn visit_seq<A>(self, mut seq: A) -> ::std::result::Result<#struct_name, A::Error>
                    where
                        A: ::serde::de::SeqAccess<'de>,
                    {
                        #(#seq_field_resolutions)*
                        let model = #struct_name {
                            #(#serde_field_names: #field_names_upper,)*
                        };
                        Ok(model.with_relations())
                    }
                }

                const FIELDS: &'static [&'static str] = &[#(#serde_field_names_str),*];
                deserializer.deserialize_struct(stringify!(#struct_name), FIELDS, __Visitor)
            }
        }
    }
}

/// Whether a field type is an `Option<..>`, and therefore may be omitted from the
/// generated `Deserialize`.
///
/// Macro-substituted types arrive wrapped in an invisible `Type::Group`, so the
/// same wrappers `parse::option_inner_type` unwraps when it looks for the `Option`
/// payload have to be unwrapped here too — otherwise an `Option` field reached
/// through a macro variable is generated as a *required* field.
pub(crate) fn is_option_type(ty: &Type) -> bool {
    match ty {
        Type::Group(group) => is_option_type(&group.elem),
        Type::Paren(paren) => is_option_type(&paren.elem),
        Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Option"),
        _ => false,
    }
}
