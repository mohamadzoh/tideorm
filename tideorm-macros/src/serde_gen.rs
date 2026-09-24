use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};

use crate::context::BuildContext;
use crate::meta_support::auto_timestamp_value;
use crate::parse::{ModelField, RelationKind, is_optional_type};

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
    let field_idents = ctx.fields.iter().map(ModelField::ident);
    quote! {
        impl ::std::default::Default for #struct_name {
            fn default() -> Self {
                Self { #(#field_idents: Default::default()),* }.with_relations()
            }
        }
    }
}

fn generate_debug_impl(ctx: &BuildContext) -> TokenStream2 {
    if !ctx.should_gen_debug {
        return quote! {};
    }
    let struct_name = &ctx.struct_name;
    let field_idents = ctx.fields.iter().map(ModelField::ident);
    let field_names = ctx.fields.iter().map(ModelField::name);
    quote! {
        impl ::std::fmt::Debug for #struct_name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.debug_struct(stringify!(#struct_name))
                    #(.field(#field_names, &self.#field_idents))*
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
    let field_idents: Vec<_> = ctx.fields.iter().map(ModelField::ident).collect();
    quote! {
        impl ::std::clone::Clone for #struct_name {
            fn clone(&self) -> Self {
                Self { #(#field_idents: self.#field_idents.clone()),* }
            }
        }
    }
}

fn generate_serialize_impl(ctx: &BuildContext) -> TokenStream2 {
    if !ctx.should_gen_serialize {
        return quote! {};
    }
    let struct_name = &ctx.struct_name;
    // Relation wrappers are serialized only once something is cached in them;
    // `MorphTo` keeps no cache, so it is never serialized.
    let (relation_fields, other_fields): (Vec<&ModelField>, Vec<&ModelField>) = ctx
        .fields
        .iter()
        .filter(|field| field.relation_kind() != Some(RelationKind::MorphTo))
        .partition(|field| field.relation_kind().is_some());
    let relation_idents: Vec<_> = relation_fields.iter().map(|field| field.ident()).collect();
    let relation_names = relation_fields.iter().map(|field| field.name());
    let other_idents = other_fields.iter().map(|field| field.ident());
    let other_names = other_fields.iter().map(|field| field.name());
    let base_field_count = other_fields.len();
    quote! {
        impl ::tideorm::serde::Serialize for #struct_name {
            fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
            where
                S: ::tideorm::serde::Serializer,
            {
                use ::tideorm::serde::ser::SerializeStruct;
                let relation_field_count = 0usize #( + usize::from(self.#relation_idents.get_cached().is_some()))*;
                let mut state = serializer.serialize_struct(
                    stringify!(#struct_name),
                    #base_field_count + relation_field_count,
                )?;
                #(state.serialize_field(#other_names, &self.#other_idents)?;)*
                #(if self.#relation_idents.get_cached().is_some() {
                    state.serialize_field(#relation_names, &self.#relation_idents)?;
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
    let field_idents: Vec<_> = ctx.fields.iter().map(ModelField::ident).collect();
    let field_names: Vec<_> = ctx.fields.iter().map(ModelField::name).collect();
    let temp_idents: Vec<_> = field_idents
        .iter()
        .map(|ident| format_ident!("__field_{}", ident))
        .collect();
    // Optional fields, relation wrappers, skipped fields, an auto-increment key
    // and the managed timestamps may be left out of the input; everything else
    // is required.
    let field_defaults: Vec<bool> = ctx
        .fields
        .iter()
        .map(|field| {
            is_optional_type(&field.ty)
                || field.relation_kind().is_some()
                || field.skip
                || (ctx.pk_auto_increment && ctx.pk_idents.contains(field.ident()))
                || auto_timestamp_value(field).is_some()
        })
        .collect();
    let field_resolutions = field_idents
        .iter()
        .zip(&field_names)
        .zip(&temp_idents)
        .zip(&field_defaults)
        .map(|(((field_ident, field_name), temp_ident), use_default)| {
            if *use_default {
                quote!(#field_ident: #temp_ident.unwrap_or_default())
            } else {
                quote!(
                    #field_ident: #temp_ident.ok_or_else(|| ::tideorm::serde::de::Error::missing_field(#field_name))?
                )
            }
        });
    let seq_field_resolutions = temp_idents.iter().zip(&field_defaults).enumerate().map(
        |(field_index, (temp_ident, use_default))| {
            if *use_default {
                quote!(let #temp_ident = seq.next_element()?.unwrap_or_default();)
            } else {
                quote!(
                    let #temp_ident = seq
                        .next_element()?
                        .ok_or_else(|| ::tideorm::serde::de::Error::invalid_length(#field_index, &self))?;
                )
            }
        },
    );

    quote! {
        impl<'de> ::tideorm::serde::Deserialize<'de> for #struct_name {
            fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
            where
                D: ::tideorm::serde::Deserializer<'de>,
            {
                #[allow(non_camel_case_types)]
                enum __Field { #(#temp_idents,)* __ignore }

                struct __FieldVisitor;
                impl<'de> ::tideorm::serde::de::Visitor<'de> for __FieldVisitor {
                    type Value = __Field;
                    fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                        formatter.write_str("field identifier")
                    }
                    fn visit_str<E>(self, value: &str) -> ::std::result::Result<__Field, E>
                    where
                        E: ::tideorm::serde::de::Error,
                    {
                        match value {
                            #(#field_names => Ok(__Field::#temp_idents),)*
                            _ => Ok(__Field::__ignore),
                        }
                    }
                }

                impl<'de> ::tideorm::serde::Deserialize<'de> for __Field {
                    fn deserialize<D>(deserializer: D) -> ::std::result::Result<__Field, D::Error>
                    where
                        D: ::tideorm::serde::Deserializer<'de>,
                    {
                        deserializer.deserialize_identifier(__FieldVisitor)
                    }
                }

                struct __Visitor;
                impl<'de> ::tideorm::serde::de::Visitor<'de> for __Visitor {
                    type Value = #struct_name;
                    fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                        formatter.write_str(concat!("struct ", stringify!(#struct_name)))
                    }
                    fn visit_map<A>(self, mut map: A) -> ::std::result::Result<#struct_name, A::Error>
                    where
                        A: ::tideorm::serde::de::MapAccess<'de>,
                    {
                        #(let mut #temp_idents: Option<_> = None;)*
                        while let Some(key) = map.next_key()? {
                            match key {
                                #(__Field::#temp_idents => {
                                    if #temp_idents.is_some() {
                                        return Err(::tideorm::serde::de::Error::duplicate_field(#field_names));
                                    }
                                    #temp_idents = Some(map.next_value()?);
                                })*
                                __Field::__ignore => {
                                    let _ = map.next_value::<::tideorm::serde::de::IgnoredAny>()?;
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
                        A: ::tideorm::serde::de::SeqAccess<'de>,
                    {
                        #(#seq_field_resolutions)*
                        let model = #struct_name {
                            #(#field_idents: #temp_idents,)*
                        };
                        Ok(model.with_relations())
                    }
                }

                const FIELDS: &'static [&'static str] = &[#(#field_names),*];
                deserializer.deserialize_struct(stringify!(#struct_name), FIELDS, __Visitor)
            }
        }
    }
}
