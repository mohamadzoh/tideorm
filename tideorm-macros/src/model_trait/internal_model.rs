use super::*;

pub(super) fn generate_internal_model_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let internal_entity_mod = &ctx.internal_entity_mod;
    let insert_active_model_setters = &ctx.insert_active_model_setters;
    let relation_field_defaults = &ctx.relation_field_defaults;
    let relation_state_refreshes = &ctx.relation_state_refreshes;
    let pk_column_variants = &ctx.pk_column_variants;
    let field_names = &ctx.field_names;
    let column_names = &ctx.column_names;
    let column_variants = &ctx.column_variants;
    let primary_key_condition_impl = build_primary_key_condition_impl(ctx, internal_entity_mod);
    let field_json_value_arms: Vec<_> = ctx
        .field_names
        .iter()
        .zip(ctx.column_names.iter())
        .map(|(field_name, column_name)| {
            let field_name_str = field_name.to_string();
            if field_name_str == *column_name {
                quote! {
                    #field_name_str => ::serde_json::to_value(&self.#field_name)
                        .map(Some)
                        .map_err(|e| ::tideorm::Error::query(format!(
                            "Failed to serialize field '{}': {}",
                            field,
                            e
                        ))),
                }
            } else {
                quote! {
                    #field_name_str | #column_name => ::serde_json::to_value(&self.#field_name)
                        .map(Some)
                        .map_err(|e| ::tideorm::Error::query(format!(
                            "Failed to serialize field '{}': {}",
                            field,
                            e
                        ))),
                }
            }
        })
        .collect();

    quote! {
        #[doc(hidden)]
        impl ::tideorm::internal::InternalModel for #struct_name {
            type Entity = #internal_entity_mod::Entity;
            type ActiveModel = #internal_entity_mod::ActiveModel;

            fn into_active_model(self) -> Self::ActiveModel {
                use ::tideorm::orm::ActiveValue;
                #internal_entity_mod::ActiveModel {
                    #(#insert_active_model_setters),*
                }
            }

            fn from_entity_model(model: #internal_entity_mod::Model) -> Self {
                let model = Self {
                    #(#field_names: model.#field_names),*,
                    #(#relation_field_defaults),*
                }
                .with_relations();
                if ::tideorm::model::__dirty_tracking_enabled() {
                    let _ = ::tideorm::model::__remember_dirty_snapshot(&model);
                }
                model
            }

            fn to_entity_model(&self) -> <Self::Entity as ::tideorm::orm::EntityTrait>::Model {
                #internal_entity_mod::Model {
                    #(#field_names: self.#field_names.clone()),*
                }
            }

            fn column_from_str(name: &str) -> Option<<Self::Entity as ::tideorm::orm::EntityTrait>::Column> {
                match name {
                    #(#column_names | stringify!(#field_names) => Some(#internal_entity_mod::Column::#column_variants),)*
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

            fn refresh_runtime_relations_from(&mut self, previous: &Self) {
                #(#relation_state_refreshes)*
            }

            fn field_json_value(&self, field: &str) -> ::tideorm::Result<Option<::serde_json::Value>> {
                match field {
                    #(#field_json_value_arms)*
                    _ => Ok(None),
                }
            }
        }
    }
}
