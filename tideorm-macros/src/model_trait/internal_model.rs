use super::*;

use std::collections::HashSet;

use crate::meta_support::auto_timestamp_value;
use crate::parse::unraw_ident;

pub(super) fn generate_internal_model_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let struct_name_str = &ctx.struct_name_str;
    let internal_entity_mod = &ctx.internal_entity_mod;
    // Models without encrypted fields reuse the shared setters built with the context;
    // encrypted models need the infallible path to leave those columns unset instead of
    // writing plaintext (see `build_unencrypted_insert_active_model_setters`).
    let insert_active_model_setters = if ctx.encrypted_fields.is_empty() {
        ctx.insert_active_model_setters.clone()
    } else {
        build_unencrypted_insert_active_model_setters(ctx)
    };
    let try_insert_active_model_setters = build_try_insert_active_model_setters(ctx);
    let relation_field_defaults = &ctx.relation_field_defaults;
    let skipped_field_defaults = build_skipped_field_defaults(ctx);
    let relation_state_refreshes = &ctx.relation_state_refreshes;
    let pk_column_variants = &ctx.pk_column_variants;
    let field_names = &ctx.field_names;
    let column_names = &ctx.column_names;
    let column_variants = &ctx.column_variants;
    let try_from_entity_model_fields = build_try_from_entity_model_fields(ctx);
    let try_to_entity_model_fields = build_try_to_entity_model_fields(ctx);
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
    let from_entity_model_body = if ctx.encrypted_fields.is_empty() {
        quote! {
            let model = Self {
                #(#field_names: model.#field_names,)*
                #(#relation_field_defaults,)*
                #(#skipped_field_defaults,)*
            }
            .with_relations();
            if ::tideorm::model::__dirty_tracking_enabled() {
                let _ = ::tideorm::model::__remember_dirty_snapshot(&model);
            }
            model
        }
    } else {
        // The infallible path cannot decrypt, and handing back a ciphertext string as if
        // it were plaintext silently corrupts the next write. Route through the fallible
        // twin instead of quietly disagreeing with it.
        quote! {
            <Self as ::tideorm::internal::InternalModel>::try_from_entity_model(model)
                .unwrap_or_else(|error| panic!(
                    "{}::from_entity_model cannot decrypt encrypted fields ({}); use try_from_entity_model instead",
                    #struct_name_str,
                    error
                ))
        }
    };

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

            fn try_into_active_model(self) -> ::tideorm::Result<Self::ActiveModel> {
                use ::tideorm::orm::ActiveValue;

                Ok(#internal_entity_mod::ActiveModel {
                    #(#try_insert_active_model_setters),*
                })
            }

            fn from_entity_model(model: #internal_entity_mod::Model) -> Self {
                #from_entity_model_body
            }

            fn try_from_entity_model(model: #internal_entity_mod::Model) -> ::tideorm::Result<Self> {
                let model = Self {
                    #(#try_from_entity_model_fields,)*
                    #(#relation_field_defaults,)*
                    #(#skipped_field_defaults,)*
                }
                .with_relations();
                if ::tideorm::model::__dirty_tracking_enabled() {
                    let _ = ::tideorm::model::__remember_dirty_snapshot(&model);
                }
                Ok(model)
            }

            fn to_entity_model(&self) -> <Self::Entity as ::tideorm::orm::EntityTrait>::Model {
                #internal_entity_mod::Model {
                    #(#field_names: self.#field_names.clone()),*
                }
            }

            fn try_to_entity_model(&self) -> ::tideorm::Result<<Self::Entity as ::tideorm::orm::EntityTrait>::Model> {
                Ok(#internal_entity_mod::Model {
                    #(#try_to_entity_model_fields),*
                })
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

fn build_try_insert_active_model_setters(ctx: &BuildContext) -> Vec<TokenStream2> {
    let table_name = &ctx.table_name;

    ctx.db_fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| {
            let column_name = BuildContext::column_name(field);
            let field_name = unraw_ident(ident);
            if field.primary_key && field.auto_increment {
                quote!(#ident: ActiveValue::NotSet)
            } else if let Some(value) = auto_timestamp_value(field) {
                quote!(#ident: ActiveValue::Set(#value))
            } else if ctx.encrypted_fields.contains(&field_name) {
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

/// Insert setters for the infallible `into_active_model` path of a model that declares
/// encrypted fields.
///
/// That path has no way to report an encryption failure, so rather than persisting the
/// plaintext it leaves the encrypted columns `NotSet`. Callers that need to write them
/// go through `try_into_active_model`, which encrypts and surfaces failures as `Err`.
fn build_unencrypted_insert_active_model_setters(ctx: &BuildContext) -> Vec<TokenStream2> {
    ctx.db_fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| {
            if field.primary_key && field.auto_increment {
                quote!(#ident: ActiveValue::NotSet)
            } else if let Some(value) = auto_timestamp_value(field) {
                quote!(#ident: ActiveValue::Set(#value))
            } else if ctx.encrypted_fields.contains(&unraw_ident(ident)) {
                quote!(#ident: ActiveValue::NotSet)
            } else {
                quote!(#ident: ActiveValue::Set(self.#ident))
            }
        })
        .collect()
}

/// Defaults for fields the user marked `#[tideorm(skip)]`.
///
/// Skipped fields stay on the user's struct but are neither persisted columns nor
/// relations, so they appear in no other generated field list. Without an entry here the
/// generated `Self { .. }` literals are incomplete and every such model fails to compile.
fn build_skipped_field_defaults(ctx: &BuildContext) -> Vec<TokenStream2> {
    let mut generated: HashSet<String> = ctx.field_names.iter().map(ToString::to_string).collect();
    generated.extend(
        ctx.relation_fields
            .iter()
            .filter_map(|field| field.ident.as_ref())
            .map(ToString::to_string),
    );

    ctx.derive_field_names
        .iter()
        .filter(|ident| !generated.contains(&ident.to_string()))
        .map(|ident| quote!(#ident: Default::default()))
        .collect()
}

fn build_try_from_entity_model_fields(ctx: &BuildContext) -> Vec<TokenStream2> {
    let table_name = &ctx.table_name;

    ctx.db_fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| {
            let column_name = BuildContext::column_name(field);
            let field_name = unraw_ident(ident);
            if ctx
                .encrypted_fields
                .iter()
                .any(|value| value == &field_name)
            {
                quote!(
                    #ident: ::tideorm::model::__decrypt_model_field(
                        model.#ident,
                        #table_name,
                        stringify!(#ident),
                        #column_name,
                    )?
                )
            } else {
                quote!(#ident: model.#ident)
            }
        })
        .collect()
}

fn build_try_to_entity_model_fields(ctx: &BuildContext) -> Vec<TokenStream2> {
    let table_name = &ctx.table_name;

    ctx.db_fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| {
            let column_name = BuildContext::column_name(field);
            let field_name = unraw_ident(ident);
            if ctx
                .encrypted_fields
                .iter()
                .any(|value| value == &field_name)
            {
                quote!(
                    #ident: ::tideorm::model::__encrypt_model_field(
                        self.#ident.clone(),
                        #table_name,
                        stringify!(#ident),
                        #column_name,
                    )?
                )
            } else {
                quote!(#ident: self.#ident.clone())
            }
        })
        .collect()
}
