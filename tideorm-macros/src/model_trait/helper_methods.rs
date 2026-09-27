use super::*;

pub(super) fn generate_helper_methods_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let internal_entity_mod = &ctx.internal_entity_mod;
    let update_setters = column_initializers(ctx, Conversion::Update);
    let column_name_checks =
        ctx.field_names
            .iter()
            .zip(&ctx.column_names)
            .flat_map(|(field_name, column_name)| {
                let field_check = quote!(::tideorm::model::__str_eq(name, #field_name));
                let column_check = (field_name != column_name)
                    .then(|| quote!(::tideorm::model::__str_eq(name, #column_name)));
                std::iter::once(field_check).chain(column_check)
            });
    let with_relations_method = generate_with_relations_method(ctx);

    quote! {
        impl #struct_name {
            // Relation wiring asserts at compile time that a foreign/owner key names a
            // real column *on the related model*, and that related model routinely lives
            // in another crate. The accessor therefore has to be reachable from outside
            // its defining crate; `pub(crate)` made every cross-crate relation fail with
            // E0624. It stays `#[doc(hidden)]` so it is not part of the documented API.
            #[doc(hidden)]
            pub const fn __has_column_name(name: &str) -> bool {
                #(#column_name_checks)||*
            }

            #[doc(hidden)]
            fn __into_update_active_model(self) -> ::tideorm::Result<#internal_entity_mod::ActiveModel> {
                use ::tideorm::orm::ActiveValue;
                Ok(#internal_entity_mod::ActiveModel {
                    #(#update_setters),*
                })
            }

            #with_relations_method
        }
    }
}
