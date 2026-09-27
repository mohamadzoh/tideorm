use super::*;

pub(super) fn generate_internal_model_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let internal_entity_mod = &ctx.internal_entity_mod;
    let insert_setters = column_initializers(ctx, Conversion::Insert { encrypt: false });
    let from_entity_fields = column_initializers(ctx, Conversion::FromEntity);
    let to_entity_fields = column_initializers(ctx, Conversion::ToEntity { encrypt: false });
    // Relation wrappers and `#[tideorm(skip)]` fields have no column to read.
    let non_column_defaults = ctx
        .fields
        .iter()
        .filter(|field| !field.is_column())
        .map(|field| {
            let ident = field.ident();
            quote!(#ident: Default::default())
        });
    let relation_state_refreshes = &ctx.relation_state_refreshes;
    let pk_column_variants = &ctx.pk_column_variants;
    let name_patterns = build_name_patterns(ctx);
    let column_variants = &ctx.column_variants;
    let field_idents = &ctx.field_idents;
    let primary_key_condition_impl = build_primary_key_condition_impl(ctx);
    // The trait's defaults bridge the fallible conversions to the plaintext ones,
    // which is exactly right until a column is encrypted.
    let encrypted_conversions = (!ctx.encrypted_fields.is_empty()).then(|| {
        let insert_setters = column_initializers(ctx, Conversion::Insert { encrypt: true });
        let to_entity_fields = column_initializers(ctx, Conversion::ToEntity { encrypt: true });
        quote! {
            fn try_into_active_model(self) -> ::tideorm::Result<Self::ActiveModel> {
                use ::tideorm::orm::ActiveValue;
                Ok(#internal_entity_mod::ActiveModel {
                    #(#insert_setters),*
                })
            }

            fn try_to_entity_model(&self) -> ::tideorm::Result<<Self::Entity as ::tideorm::orm::EntityTrait>::Model> {
                Ok(#internal_entity_mod::Model {
                    #(#to_entity_fields),*
                })
            }
        }
    });

    quote! {
        #[doc(hidden)]
        impl ::tideorm::internal::InternalModel for #struct_name {
            type Entity = #internal_entity_mod::Entity;
            type ActiveModel = #internal_entity_mod::ActiveModel;

            fn into_active_model(self) -> Self::ActiveModel {
                use ::tideorm::orm::ActiveValue;
                #internal_entity_mod::ActiveModel {
                    #(#insert_setters),*
                }
            }

            #encrypted_conversions

            fn try_from_entity_model(model: #internal_entity_mod::Model) -> ::tideorm::Result<Self> {
                let model = Self {
                    #(#from_entity_fields,)*
                    #(#non_column_defaults,)*
                }
                .with_relations();
                ::tideorm::model::__remember_dirty_snapshot(&model);
                Ok(model)
            }

            fn to_entity_model(&self) -> <Self::Entity as ::tideorm::orm::EntityTrait>::Model {
                #internal_entity_mod::Model {
                    #(#to_entity_fields),*
                }
            }

            fn column_from_str(name: &str) -> Option<<Self::Entity as ::tideorm::orm::EntityTrait>::Column> {
                match name {
                    #(#name_patterns => Some(#internal_entity_mod::Column::#column_variants),)*
                    _ => None,
                }
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

            fn refresh_runtime_relations_from(&mut self, previous: &Self) {
                #(#relation_state_refreshes)*
            }

            fn field_json_value(&self, field: &str) -> ::tideorm::Result<Option<::tideorm::serde_json::Value>> {
                match field {
                    #(#name_patterns => ::tideorm::serde_json::to_value(&self.#field_idents)
                        .map(Some)
                        .map_err(|e| ::tideorm::Error::query(format!(
                            "Failed to serialize field '{}': {}",
                            field,
                            e
                        ))),)*
                    _ => Ok(None),
                }
            }

            fn set_field_json(&mut self, field: &str, value: ::tideorm::serde_json::Value) -> ::tideorm::Result<bool> {
                match field {
                    #(#name_patterns => {
                        self.#field_idents = ::tideorm::serde_json::from_value(value).map_err(|e| {
                            ::tideorm::Error::conversion(format!(
                                "Failed to set field '{}': {}",
                                field,
                                e
                            ))
                        })?;
                        Ok(true)
                    })*
                    _ => Ok(false),
                }
            }
        }
    }
}
