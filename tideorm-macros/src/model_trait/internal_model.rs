use super::*;

pub(super) fn generate_internal_model_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let internal_entity_mod = &ctx.internal_entity_mod;
    let (insert_bindings, insert_setters) = write_conversion(ctx, Conversion::Insert);
    let pk_column_variants = &ctx.pk_column_variants;
    let name_patterns = build_name_patterns(ctx);
    let field_idents = &ctx.field_idents;
    let primary_key_condition_impl = build_primary_key_condition_impl(ctx);

    quote! {
        #[doc(hidden)]
        impl ::tideorm::internal::InternalModel for #struct_name {
            type ActiveModel = #internal_entity_mod::ActiveModel;

            fn try_into_active_model(self) -> ::tideorm::Result<Self::ActiveModel> {
                use ::tideorm::orm::ActiveValue;
                #insert_bindings
                Ok(#internal_entity_mod::ActiveModel {
                    #(#insert_setters),*
                })
            }

            fn finish_load(self, connection: &::tideorm::database::ConnectionRef) -> Self {
                let mut model = self.with_relations();
                ::tideorm::model::__finish_load(&mut model, connection);
                model
            }

            fn primary_key_columns() -> Vec<<Self::Entity as ::tideorm::orm::EntityTrait>::Column> {
                vec![#(#internal_entity_mod::Column::#pk_column_variants),*]
            }

            fn primary_key_condition(
                primary_key: &<Self as ::tideorm::model::ModelMeta>::PrimaryKey,
            ) -> ::tideorm::orm::Condition {
                #primary_key_condition_impl
            }

            fn __rebuild_relations(self) -> Self {
                self.with_relations()
            }

            // Here and in `set_field_json` an arm only
            // picks the field: the code after the `match` is emitted once per
            // model, not once per field.
            fn field_json_value(&self, field: &str) -> ::tideorm::Result<Option<::tideorm::serde_json::Value>> {
                let value = match field {
                    #(#name_patterns => ::tideorm::serde_json::to_value(&self.#field_idents),)*
                    _ => return Ok(None),
                };
                value.map(Some).map_err(|e| {
                    ::tideorm::Error::query(format!("Failed to serialize field '{}': {}", field, e))
                })
            }

            fn set_field_json(&mut self, field: &str, value: ::tideorm::serde_json::Value) -> ::tideorm::Result<bool> {
                let error = match field {
                    #(#name_patterns => match ::tideorm::serde_json::from_value(value) {
                        Ok(parsed) => {
                            self.#field_idents = parsed;
                            return Ok(true);
                        }
                        Err(error) => error,
                    },)*
                    _ => return Ok(false),
                };
                Err(::tideorm::Error::conversion(format!(
                    "Failed to set field '{}': {}",
                    field, error
                )))
            }
        }
    }
}
