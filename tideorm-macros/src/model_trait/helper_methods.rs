use super::*;

pub(super) fn generate_helper_methods_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let internal_entity_mod = &ctx.internal_entity_mod;
    let try_update_active_model_setters = build_try_update_active_model_setters(ctx);
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
            fn __into_update_active_model(self) -> ::tideorm::Result<#internal_entity_mod::ActiveModel> {
                use ::tideorm::orm::ActiveValue;
                Ok(#internal_entity_mod::ActiveModel {
                    #(#try_update_active_model_setters),*
                })
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

fn build_try_update_active_model_setters(ctx: &BuildContext) -> Vec<TokenStream2> {
    let table_name = &ctx.table_name;

    ctx.db_fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| {
            let column_name = BuildContext::column_name(field);
            let field_name = ident.to_string();

            if field.primary_key {
                quote!(#ident: ActiveValue::Unchanged(self.#ident))
            } else if field_name == "updated_at" || column_name == "updated_at" {
                quote!(#ident: ActiveValue::Set(::tideorm::chrono::Utc::now()))
            } else if ctx
                .encrypted_fields
                .iter()
                .any(|value| value == &field_name)
            {
                quote!(
                    #ident: ActiveValue::Set(::tideorm::model::__encrypt_model_field(
                        self.#ident,
                        #table_name,
                        stringify!(#ident),
                        #column_name,
                    )?)
                )
            } else {
                quote!(#ident: ActiveValue::Set(self.#ident))
            }
        })
        .collect()
}
