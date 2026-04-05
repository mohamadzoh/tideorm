use super::*;

pub(super) fn generate_helper_methods_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let internal_entity_mod = &ctx.internal_entity_mod;
    let update_active_model_setters = &ctx.update_active_model_setters;
    let pk_idents = &ctx.pk_idents;
    let table_name = &ctx.table_name;
    let field_names = &ctx.field_names;
    let column_names = &ctx.column_names;
    let with_relations_method = generate_with_relations_method(ctx);
    let delete_active_model_init = if ctx.field_names.len() == ctx.pk_idents.len() {
        quote! {
            #internal_entity_mod::ActiveModel {
                #(#pk_idents: ActiveValue::Unchanged(self.#pk_idents)),*
            }
        }
    } else {
        quote! {
            #internal_entity_mod::ActiveModel {
                #(#pk_idents: ActiveValue::Unchanged(self.#pk_idents)),*,
                ..Default::default()
            }
        }
    };

    quote! {
        impl #struct_name {
            #[doc(hidden)]
            pub(crate) const fn __column_name_eq(left: &str, right: &str) -> bool {
                let left_bytes = left.as_bytes();
                let right_bytes = right.as_bytes();

                if left_bytes.len() != right_bytes.len() {
                    return false;
                }

                let mut index = 0;
                while index < left_bytes.len() {
                    if left_bytes[index] != right_bytes[index] {
                        return false;
                    }
                    index += 1;
                }

                true
            }

            #[doc(hidden)]
            pub(crate) const fn __has_column_name(name: &str) -> bool {
                #(
                    if Self::__column_name_eq(name, #column_names) || Self::__column_name_eq(name, stringify!(#field_names)) {
                        return true;
                    }
                )*

                false
            }

            #[doc(hidden)]
            fn __base_error_context() -> ::tideorm::error::ErrorContext {
                ::tideorm::error::ErrorContext::new().table(#table_name)
            }

            #[doc(hidden)]
            fn __primary_key_error_context(
                primary_key: &<Self as ::tideorm::model::ModelMeta>::PrimaryKey,
            ) -> ::tideorm::error::ErrorContext {
                let condition = <Self as ::tideorm::model::ModelMeta>::primary_key_display(primary_key);
                Self::__base_error_context()
                    .condition(condition.clone())
                    .operator_chain(condition)
            }

            #[doc(hidden)]
            fn __into_update_active_model(self) -> #internal_entity_mod::ActiveModel {
                use ::tideorm::orm::ActiveValue;
                #internal_entity_mod::ActiveModel {
                    #(#update_active_model_setters),*
                }
            }

            #[doc(hidden)]
            fn __into_delete_active_model(self) -> #internal_entity_mod::ActiveModel {
                use ::tideorm::orm::ActiveValue;
                #delete_active_model_init
            }

            #with_relations_method
        }
    }
}
