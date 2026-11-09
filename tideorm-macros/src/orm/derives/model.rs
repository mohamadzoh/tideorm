use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::ext::IdentExt;

use super::EngineField;

/// The model's `FromQueryResult` and `ModelTrait`: a row decodes into its
/// columns, the other fields take their `Default`. An encrypted column is read
/// as text and decrypted into its field's own type, so a decoded model never
/// holds ciphertext; `ModelTrait` reads and sets that plaintext.
pub(crate) fn expand_model(model: &Ident, fields: &[EngineField]) -> TokenStream {
    let row = Ident::new("row", Span::mixed_site());
    let column = quote!(<<Self as ::tideorm::orm::ModelTrait>::Entity as ::tideorm::orm::entity::EntityTrait>::Column);
    let entity_column = quote!(<Self::Entity as ::tideorm::orm::entity::EntityTrait>::Column);
    let mut readers = Vec::new();
    let mut values = Vec::new();
    let mut gets = Vec::new();
    let mut value_types = Vec::new();
    let mut sets = Vec::new();
    for EngineField {
        ident,
        ty,
        column: variant,
        encrypted,
    } in fields
    {
        let Some(variant) = variant else {
            values.push(quote!(#ident: Default::default()));
            continue;
        };
        let name = quote!(::tideorm::orm::IdenStatic::as_str(&#column::#variant));
        let arm = quote!(#entity_column::#variant =>);
        if *encrypted {
            let field_name = ident.unraw().to_string();
            readers.push(quote! {
                let #ident = ::tideorm::orm::__read_model_field::<::std::string::String>(#row, #name)?;
            });
            values.push(quote! {
                #ident: ::tideorm::model::__decrypt_model_field::<Self, #ty>(#ident, #field_name, #name)?
            });
            gets.push(quote!(#arm ::tideorm::model::__encrypted_field_value(&self.#ident),));
            value_types.push(quote!(#arm <::std::string::String as ::tideorm::orm::sql_builder::ValueType>::array_type(),));
            sets.push(
                quote!(#arm self.#ident = ::tideorm::model::__encrypted_field_from_value(v)?,),
            );
        } else {
            readers.push(quote! {
                let #ident = ::tideorm::orm::__read_model_field::<#ty>(#row, #name)?;
            });
            values.push(quote! {
                #ident: #ident.ok_or_else(|| ::tideorm::orm::DbErr::Type(format!(
                    "Missing value for column '{}'",
                    #name
                )))?
            });
            gets.push(quote!(#arm self.#ident.clone().into(),));
            value_types
                .push(quote!(#arm <#ty as ::tideorm::orm::sql_builder::ValueType>::array_type(),));
            sets.push(quote!(#arm self.#ident = ::tideorm::orm::sql_builder::ValueType::try_from(v).map_err(|e| ::tideorm::orm::DbErr::Type(e.to_string()))?,));
        }
    }
    let missing_field_msg = format!("field does not exist on {model}");

    quote!(
        #[automatically_derived]
        impl ::tideorm::orm::FromQueryResult for #model {
            fn from_query_result(#row: &::tideorm::orm::QueryResult) -> std::result::Result<Self, ::tideorm::orm::DbErr> {
                #(#readers)*

                Ok(Self {
                    #(#values),*
                })
            }
        }

        #[automatically_derived]
        impl ::tideorm::orm::ModelTrait for #model {
            type Entity = Entity;

            fn get(&self, c: #entity_column) -> ::tideorm::orm::Value {
                match c {
                    #(#gets)*
                }
            }

            fn get_value_type(c: #entity_column) -> ::tideorm::orm::sql_builder::ArrayType {
                match c {
                    #(#value_types)*
                }
            }

            fn try_set(&mut self, c: #entity_column, v: ::tideorm::orm::Value) -> ::core::result::Result<(), ::tideorm::orm::DbErr> {
                match c {
                    #(#sets)*
                    _ => return Err(::tideorm::orm::DbErr::Type(#missing_field_msg.to_owned())),
                }
                Ok(())
            }
        }
    )
}
