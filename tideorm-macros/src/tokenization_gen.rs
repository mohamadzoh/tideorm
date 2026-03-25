use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

use crate::context::BuildContext;

pub(crate) fn generate_tokenizable_impl(ctx: &BuildContext) -> TokenStream2 {
    if !ctx.tokenize_enabled {
        return quote! {};
    }

    let struct_name = &ctx.struct_name;
    let struct_name_str = &ctx.struct_name_str;
    let pk_ident = &ctx.pk_ident;
    let pk_type = &ctx.pk_type;

    quote! {
        #[::tideorm::async_trait::async_trait]
        impl ::tideorm::tokenization::Tokenizable for #struct_name {
            type TokenPrimaryKey = #pk_type;

            fn token_model_name() -> &'static str {
                #struct_name_str
            }

            fn token_primary_key(&self) -> Self::TokenPrimaryKey {
                self.#pk_ident.clone()
            }

            fn tokenization_enabled() -> bool {
                true
            }

            async fn from_token(token: &str) -> ::tideorm::Result<Self> {
                let id = Self::decode_token(token)?;
                let display_id = <Self as ::tideorm::model::ModelMeta>::primary_key_display(&id);
                Self::find(id.clone())
                    .await?
                    .ok_or_else(|| ::tideorm::Error::not_found(
                        format!("{} with decoded token ID {} not found", #struct_name_str, display_id)
                    ))
            }
        }
    }
}
