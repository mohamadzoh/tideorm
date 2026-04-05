use super::*;

pub(super) fn generate_soft_delete_impl(ctx: &BuildContext) -> TokenStream2 {
    if !ctx.soft_delete_enabled {
        return quote! {};
    }

    let struct_name = &ctx.struct_name;
    let deleted_at_ident = ctx
        .soft_delete_field_ident
        .as_ref()
        .expect("soft delete field should be resolved when soft_delete is enabled");
    let deleted_at_column = ctx
        .soft_delete_column_name
        .as_ref()
        .expect("soft delete column should be resolved when soft_delete is enabled");

    quote! {
        #[::tideorm::async_trait::async_trait]
        impl ::tideorm::SoftDelete for #struct_name {
            fn deleted_at_column() -> &'static str {
                #deleted_at_column
            }

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
