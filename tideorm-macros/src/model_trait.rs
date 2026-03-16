use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

use crate::context::BuildContext;
use crate::relation_gen::generate_with_relations_method;

pub(crate) fn generate_model_support(ctx: &BuildContext) -> TokenStream2 {
    let internal_model_impl = generate_internal_model_impl(ctx);
    let helper_methods_impl = generate_helper_methods_impl(ctx);
    let model_trait_impl = generate_model_trait_impl(ctx);
    quote! {
        #internal_model_impl
        #helper_methods_impl
        #model_trait_impl
    }
}

fn generate_internal_model_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let internal_entity_mod = &ctx.internal_entity_mod;
    let insert_active_model_setters = &ctx.insert_active_model_setters;
    let all_field_names = &ctx.all_field_names;
    let relation_field_defaults = &ctx.relation_field_defaults;
    let pk_column_variant = &ctx.pk_column_variant;

    quote! {
        #[doc(hidden)]
        impl ::tideorm::internal::InternalModel for #struct_name {
            type Entity = #internal_entity_mod::Entity;
            type ActiveModel = #internal_entity_mod::ActiveModel;

            fn into_active_model(self) -> Self::ActiveModel {
                use ::tideorm::sea_orm::ActiveValue;
                #internal_entity_mod::ActiveModel {
                    #(#insert_active_model_setters),*
                }
            }

            fn from_sea_model(model: #internal_entity_mod::Model) -> Self {
                Self {
                    #(#all_field_names: model.#all_field_names),*,
                    #(#relation_field_defaults),*
                }
                .with_relations()
            }

            fn primary_key_column() -> Option<<Self::Entity as ::tideorm::sea_orm::EntityTrait>::Column> {
                Some(#internal_entity_mod::Column::#pk_column_variant)
            }
        }
    }
}

fn generate_helper_methods_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let internal_entity_mod = &ctx.internal_entity_mod;
    let update_active_model_setters = &ctx.update_active_model_setters;
    let pk_ident = &ctx.pk_ident;
    let with_relations_method = generate_with_relations_method(ctx);

    quote! {
        impl #struct_name {
            #[doc(hidden)]
            fn __into_update_active_model(self) -> #internal_entity_mod::ActiveModel {
                use ::tideorm::sea_orm::ActiveValue;
                #internal_entity_mod::ActiveModel {
                    #(#update_active_model_setters),*
                }
            }

            #[doc(hidden)]
            fn __into_delete_active_model(self) -> #internal_entity_mod::ActiveModel {
                use ::tideorm::sea_orm::ActiveValue;
                #internal_entity_mod::ActiveModel {
                    #pk_ident: ActiveValue::Unchanged(self.#pk_ident),
                    ..Default::default()
                }
            }

            #with_relations_method
        }
    }
}

fn generate_model_trait_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let internal_entity_mod = &ctx.internal_entity_mod;
    let pk_ident = &ctx.pk_ident;
    let table_name = &ctx.table_name;
    let pk_column_name = &ctx.pk_column_name;
    let pk_auto_increment = ctx.pk_auto_increment;
    let column_names = &ctx.column_names;
    let field_names = &ctx.field_names;

    quote! {
        #[::tideorm::async_trait::async_trait]
        impl ::tideorm::model::Model for #struct_name {
            fn primary_key(&self) -> Self::PrimaryKey {
                self.#pk_ident.clone()
            }

            async fn find(id: Self::PrimaryKey) -> ::tideorm::Result<Option<Self>> {
                use ::tideorm::internal::InternalModel;
                use ::tideorm::sea_orm::EntityTrait;
                let result = #internal_entity_mod::Entity::find_by_id(id)
                    .one(::tideorm::require_db()?.__internal_connection())
                    .await
                    .map_err(|e| ::tideorm::Error::query(e.to_string()))?;
                Ok(result.map(|model| <Self as InternalModel>::from_sea_model(model)))
            }

            async fn destroy(id: Self::PrimaryKey) -> ::tideorm::Result<u64> {
                use ::tideorm::sea_orm::EntityTrait;
                let result = #internal_entity_mod::Entity::delete_by_id(id)
                    .exec(::tideorm::require_db()?.__internal_connection())
                    .await
                    .map_err(|e| ::tideorm::Error::query(e.to_string()))?;
                Ok(result.rows_affected)
            }

            async fn create(model: Self) -> ::tideorm::Result<Self> { model.save().await }

            async fn delete(self) -> ::tideorm::Result<u64> {
                use ::tideorm::sea_orm::ActiveModelTrait;
                let active = self.__into_delete_active_model();
                let result = active.delete(::tideorm::require_db()?.__internal_connection())
                    .await
                    .map_err(|e| ::tideorm::Error::query(e.to_string()))?;
                Ok(result.rows_affected)
            }

            async fn save(self) -> ::tideorm::Result<Self> {
                use ::tideorm::internal::InternalModel;
                use ::tideorm::sea_orm::ActiveModelTrait;
                let active = <Self as InternalModel>::into_active_model(self);
                let result = active.insert(::tideorm::require_db()?.__internal_connection())
                    .await
                    .map_err(|e| ::tideorm::Error::query(e.to_string()))?;
                Ok(<Self as InternalModel>::from_sea_model(result))
            }

            async fn update(self) -> ::tideorm::Result<Self> {
                use ::tideorm::internal::InternalModel;
                use ::tideorm::sea_orm::ActiveModelTrait;
                let active = self.__into_update_active_model();
                let result = active.update(::tideorm::require_db()?.__internal_connection())
                    .await
                    .map_err(|e| ::tideorm::Error::query(e.to_string()))?;
                Ok(<Self as InternalModel>::from_sea_model(result))
            }

            async fn insert_or_update(model: Self, conflict_columns: Vec<&str>) -> ::tideorm::Result<Self> {
                let cols: Vec<String> = conflict_columns.into_iter().map(|value| value.to_string()).collect();
                let builder = ::tideorm::model::OnConflictBuilder::new(cols);
                Self::__insert_with_conflict(model, builder).await
            }

            async fn __insert_with_conflict(
                model: Self,
                builder: ::tideorm::model::OnConflictBuilder<Self>,
            ) -> ::tideorm::Result<Self> {
                use ::tideorm::Database;
                use ::tideorm::internal::InternalModel;
                use serde_json::json;

                let all_columns: Vec<&str> = vec![#(#column_names),*];
                let model_clone = model.clone();
                let all_values: Vec<serde_json::Value> = vec![#(json!(model_clone.#field_names)),*];
                let include_pk = builder.conflict_columns.contains(&#pk_column_name.to_string()) || !#pk_auto_increment;
                let (columns, values): (Vec<&str>, Vec<serde_json::Value>) = all_columns
                    .iter()
                    .zip(all_values.into_iter())
                    .filter(|(column, _)| !(**column == #pk_column_name && #pk_auto_increment && !include_pk))
                    .map(|(column, value)| (*column, value))
                    .unzip();

                let column_list = columns.iter().map(|column| format!("\"{}\"", column)).collect::<Vec<_>>().join(", ");
                let value_list = values.iter()
                    .map(|value| match value {
                        serde_json::Value::Null => "NULL".to_string(),
                        serde_json::Value::Bool(value) => value.to_string(),
                        serde_json::Value::Number(value) => value.to_string(),
                        serde_json::Value::String(value) => format!("'{}'", value.replace('\'', "''")),
                        _ => format!("'{}'", serde_json::to_string(value).unwrap_or_default().replace('\'', "''")),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let conflict_cols = builder.conflict_columns;
                let conflict_list = conflict_cols.iter().map(|column| format!("\"{}\"", column)).collect::<Vec<_>>().join(", ");
                let update_cols: Vec<String> = if let Some(cols) = builder.update_columns {
                    cols
                } else if let Some(exclude) = builder.exclude_columns {
                    columns.iter().filter(|column| !exclude.contains(&column.to_string())).map(|column| column.to_string()).collect()
                } else {
                    columns.iter().filter(|column| {
                        let column = column.to_string();
                        !conflict_cols.contains(&column) && column != #pk_column_name
                    }).map(|column| column.to_string()).collect()
                };
                let update_list = update_cols.iter().map(|column| format!("\"{}\" = EXCLUDED.\"{}\"", column, column)).collect::<Vec<_>>().join(", ");
                let sql = format!(
                    "INSERT INTO \"{}\" ({}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET {} RETURNING *",
                    #table_name, column_list, value_list, conflict_list, update_list
                );
                let results: Vec<Self> = ::tideorm::Database::raw(&sql).await?;
                results.into_iter().next().ok_or_else(|| ::tideorm::Error::query("INSERT ... ON CONFLICT returned no rows".to_string()))
            }
        }
    }
}
