use super::*;

pub(super) fn generate_soft_delete_impl(ctx: &BuildContext) -> TokenStream2 {
    let Some((deleted_at_ident, _)) = &ctx.soft_delete else {
        return quote! {};
    };
    let struct_name = &ctx.struct_name;

    quote! {
        #[::tideorm::async_trait::async_trait]
        impl ::tideorm::SoftDelete for #struct_name {
            fn deleted_at(&self) -> Option<::tideorm::chrono::DateTime<::tideorm::chrono::Utc>> {
                self.#deleted_at_ident.clone()
            }

            fn set_deleted_at(
                &mut self,
                timestamp: Option<::tideorm::chrono::DateTime<::tideorm::chrono::Utc>>,
            ) {
                self.#deleted_at_ident = timestamp;
            }
        }
    }
}
