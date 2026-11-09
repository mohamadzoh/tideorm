use proc_macro2::{Ident, TokenStream};
use quote::quote;

use super::is_static_iden;

/// The `Entity` unit struct's impls: its table, and the model, active model,
/// column and primary-key types beside it.
pub(crate) fn expand_entity(
    model: &Ident,
    table_name: &str,
    schema_name: Option<&str>,
) -> TokenStream {
    let schema_name = match schema_name {
        Some(schema) => quote!(Some(#schema)),
        None => quote!(None),
    };
    let quoted = is_static_iden(table_name).then(|| {
        quote! {
            fn quoted(&self) -> std::borrow::Cow<'static, str> {
                std::borrow::Cow::Borrowed(#table_name)
            }
        }
    });

    quote!(
        #[automatically_derived]
        impl ::tideorm::orm::entity::EntityName for Entity {
            fn schema_name(&self) -> Option<&str> {
                #schema_name
            }

            fn table_name(&self) -> &'static str {
                #table_name
            }
        }

        #[automatically_derived]
        impl ::tideorm::orm::entity::EntityTrait for Entity {
            type Model = #model;

            type ActiveModel = ActiveModel;

            type Column = Column;

            type PrimaryKey = PrimaryKey;
        }

        #[automatically_derived]
        impl ::tideorm::orm::Iden for Entity {
            #quoted

            fn unquoted(&self) -> &str {
                #table_name
            }
        }

        #[automatically_derived]
        impl ::tideorm::orm::IdenStatic for Entity {
            fn as_str(&self) -> &'static str {
                #table_name
            }
        }
    )
}
