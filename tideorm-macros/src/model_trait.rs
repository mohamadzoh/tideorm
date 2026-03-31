use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};

use crate::context::BuildContext;
use crate::parse::relation_generic_types;
use crate::relation_gen::generate_with_relations_method;

pub(crate) fn generate_model_support(ctx: &BuildContext) -> TokenStream2 {
    let internal_model_impl = generate_internal_model_impl(ctx);
    let helper_methods_impl = generate_helper_methods_impl(ctx);
    let model_trait_impl = generate_model_trait_impl(ctx);
    let soft_delete_impl = generate_soft_delete_impl(ctx);
    let eager_loader_impl = generate_eager_loader_impl(ctx);
    let entity_manager_support_impl = generate_entity_manager_support_impl(ctx);
    quote! {
        #internal_model_impl
        #helper_methods_impl
        #model_trait_impl
        #soft_delete_impl
        #eager_loader_impl
        #entity_manager_support_impl
    }
}

fn build_primary_key_value_impl(ctx: &BuildContext) -> TokenStream2 {
    if ctx.pk_idents.len() == 1 {
        let pk_ident = &ctx.pk_ident;
        quote!(self.#pk_ident.clone())
    } else {
        let pk_idents = &ctx.pk_idents;
        quote!((#(self.#pk_idents.clone()),*))
    }
}

fn build_primary_key_condition_impl(
    ctx: &BuildContext,
    internal_entity_mod: &syn::Ident,
) -> TokenStream2 {
    if ctx.pk_column_variants.len() == 1 {
        let pk_column_variant = &ctx.pk_column_variant;
        return quote! {
            use ::tideorm::orm::ColumnTrait;
            ::tideorm::orm::Condition::all()
                .add(#internal_entity_mod::Column::#pk_column_variant.eq(primary_key.clone()))
        };
    }

    let bindings: Vec<_> = (0..ctx.pk_column_variants.len())
        .map(|index| format_ident!("pk_{index}"))
        .collect();
    let pk_column_variants = &ctx.pk_column_variants;

    quote! {
        use ::tideorm::orm::ColumnTrait;
        let (#(#bindings),*) = primary_key.clone();
        ::tideorm::orm::Condition::all()
            #(.add(#internal_entity_mod::Column::#pk_column_variants.eq(#bindings)))*
    }
}

fn build_pk_conflict_check(ctx: &BuildContext) -> TokenStream2 {
    let pk_column_names = &ctx.pk_column_names;
    quote! {
        conflict_cols.iter().any(|column| matches!(column.as_str(), #(#pk_column_names)|*))
    }
}

fn build_pk_exclusion_check(ctx: &BuildContext, value_ident: TokenStream2) -> TokenStream2 {
    let pk_column_names = &ctx.pk_column_names;
    quote! {
        matches!(#value_ident.as_str(), #(#pk_column_names)|*)
    }
}

fn generate_entity_manager_support_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let field_writer_arms: Vec<_> = ctx
        .field_names
        .iter()
        .zip(ctx.field_types.iter())
        .zip(ctx.column_names.iter())
        .map(|((field_ident, field_ty), column_name)| {
            let field_name = field_ident.to_string();
            quote! {
                #field_name | #column_name => {
                    self.#field_ident = ::serde_json::from_value::<#field_ty>(value.clone())
                        .map_err(|error| ::tideorm::Error::invalid_query(format!(
                            "failed to assign entity manager value for field '{}' on {}: {}",
                            field,
                            stringify!(#struct_name),
                            error
                        )))?;
                    Ok(())
                }
            }
        })
        .collect();
    let relation_field_names: Vec<_> = ctx
        .relation_fields
        .iter()
        .filter_map(|field| field.ident.as_ref())
        .map(|ident| ident.to_string())
        .collect();
    let merge_persisted_assignments: Vec<_> = ctx
        .field_names
        .iter()
        .filter(|field_ident| {
            let field_name = field_ident.to_string();
            !relation_field_names.iter().any(|relation_name| relation_name == &field_name)
        })
        .map(|field_ident| {
            quote! {
                self.#field_ident = persisted.#field_ident;
            }
        })
        .collect();

    let field_writer_impl = quote! {
        #[cfg(feature = "entity-manager")]
        impl ::tideorm::entity_manager::TideEntityManagerFieldWriter for #struct_name {
            fn tide_set_field_value(
                &mut self,
                field: &str,
                value: ::serde_json::Value,
            ) -> ::tideorm::Result<()> {
                match field {
                    #(#field_writer_arms,)*
                    _ => Err(::tideorm::Error::invalid_query(format!(
                        "field '{}' on {} is not an entity-manager-writable column",
                        field,
                        stringify!(#struct_name)
                    ))),
                }
            }
        }
    };

    let merge_impl = quote! {
        #[cfg(feature = "entity-manager")]
        impl ::tideorm::entity_manager::TideEntityManagerMergePersisted for #struct_name {
            fn tide_merge_persisted(&mut self, persisted: Self) {
                #(#merge_persisted_assignments)*
            }
        }
    };
    let relation_sync_blocks: Vec<_> = ctx
        .relation_fields
        .iter()
        .filter_map(|field| {
            let ident = field.ident.as_ref()?;

            if field.has_many.is_some() {
                let related_ty = relation_generic_types(&field.ty).into_iter().next()?;
                let foreign_key = field.foreign_key.as_deref().unwrap_or("id");
                let local_key = field.local_key.as_deref().unwrap_or("id");
                let local_key_ident = ctx
                    .resolve_local_key_ident(local_key, ident)
                    .expect("validated has_many local key should resolve");

                return Some(quote! {
                    if self.#ident.is_loaded() {
                        let owner_table = <Self as ::tideorm::entity_manager::TideEntityManagerMeta>::tide_table_name();
                        let owner_key = <Self as ::tideorm::entity_manager::TideEntityManagerMeta>::tide_pk_key(self);
                        let relation_name = stringify!(#ident);
                        let current_keys = self.#ident.current_keys()?;
                        let to_delete = entity_manager
                            .deletions::<#related_ty>(owner_table, &owner_key, relation_name, &current_keys)
                            .await;

                        if !to_delete.is_empty() {
                            for deleted_key in &to_delete {
                                if let Some(deleted) = entity_manager.get_by_entity_manager_key::<#related_ty>(deleted_key) {
                                    ::tideorm::entity_manager::__with_entity_manager_db(
                                        entity_manager,
                                        <#related_ty as ::tideorm::model::Model>::delete(deleted),
                                    )
                                    .await?;
                                }

                                entity_manager.remove_by_entity_manager_key::<#related_ty>(deleted_key);
                            }
                        }

                        let child_fk_value = ::serde_json::to_value(self.#local_key_ident.clone())?;
                        let mut updated_keys = Vec::new();

                        if let Some(items) = self.#ident.as_mut() {
                            updated_keys.reserve(items.len());

                            for item in items.iter_mut() {
                                <#related_ty as ::tideorm::entity_manager::TideEntityManagerFieldWriter>::tide_set_field_value(
                                    item,
                                    #foreign_key,
                                    child_fk_value.clone(),
                                )?;

                                let existing_key = ::tideorm::entity_manager::__model_entity_manager_key(item)?;
                                let should_persist = match existing_key.as_deref() {
                                    Some(existing_key) => match entity_manager.get_by_entity_manager_key::<#related_ty>(existing_key) {
                                        Some(cached) => {
                                            ::serde_json::to_value(&*item)?
                                                != ::serde_json::to_value(&cached)?
                                        }
                                        None => true,
                                    },
                                    None => true,
                                };

                                if !should_persist {
                                    if let Some(existing_key) = existing_key {
                                        updated_keys.push(existing_key);
                                    }

                                    <#related_ty as ::tideorm::entity_manager::TideEntityManagerSync>::tide_sync_entity_manager_relations(item, entity_manager).await?;
                                    entity_manager.put(item.clone());
                                    continue;
                                }

                                let saved = ::tideorm::entity_manager::save_with_entity_manager(item, entity_manager).await?;

                                if let Some(saved_key) = ::tideorm::entity_manager::__model_entity_manager_key(&saved)? {
                                    updated_keys.push(saved_key);
                                }

                                *item = saved.clone();
                                entity_manager.put(saved);
                            }
                        }

                        entity_manager
                            .snapshot::<#related_ty>(owner_table, &owner_key, relation_name, &updated_keys)
                            .await;
                    }
                });
            }

            if field.has_one.is_some() {
                let related_ty = relation_generic_types(&field.ty).into_iter().next()?;
                let foreign_key = field.foreign_key.as_deref().unwrap_or("id");
                let local_key = field.local_key.as_deref().unwrap_or("id");
                let local_key_ident = ctx
                    .resolve_local_key_ident(local_key, ident)
                    .expect("validated has_one local key should resolve");

                return Some(quote! {
                    if self.#ident.is_loaded() {
                        let owner_table = <Self as ::tideorm::entity_manager::TideEntityManagerMeta>::tide_table_name();
                        let owner_key = <Self as ::tideorm::entity_manager::TideEntityManagerMeta>::tide_pk_key(self);
                        let relation_name = stringify!(#ident);
                        let child_fk_value = ::serde_json::to_value(self.#local_key_ident.clone())?;
                        let mut updated_keys = Vec::new();

                        if let Some(item) = self.#ident.as_mut() {
                            <#related_ty as ::tideorm::entity_manager::TideEntityManagerFieldWriter>::tide_set_field_value(
                                item,
                                #foreign_key,
                                child_fk_value,
                            )?;

                            let existing_key = ::tideorm::entity_manager::__model_entity_manager_key(item)?;
                            let should_persist = match existing_key.as_deref() {
                                Some(existing_key) => match entity_manager.get_by_entity_manager_key::<#related_ty>(existing_key) {
                                    Some(cached) => {
                                        ::serde_json::to_value(&*item)?
                                            != ::serde_json::to_value(&cached)?
                                    }
                                    None => true,
                                },
                                None => true,
                            };

                            if !should_persist {
                                if let Some(existing_key) = existing_key {
                                    updated_keys.push(existing_key);
                                }

                                <#related_ty as ::tideorm::entity_manager::TideEntityManagerSync>::tide_sync_entity_manager_relations(item, entity_manager).await?;
                                entity_manager.put(item.clone());
                            } else {
                                let saved = ::tideorm::entity_manager::save_with_entity_manager(item, entity_manager).await?;

                                if let Some(saved_key) = ::tideorm::entity_manager::__model_entity_manager_key(&saved)? {
                                    updated_keys.push(saved_key);
                                }

                                *item = saved.clone();
                                entity_manager.put(saved);
                            }
                        }

                        let to_delete = entity_manager
                            .deletions::<#related_ty>(owner_table, &owner_key, relation_name, &updated_keys)
                            .await;

                        if !to_delete.is_empty() {
                            for deleted_key in &to_delete {
                                if let Some(deleted) = entity_manager.get_by_entity_manager_key::<#related_ty>(deleted_key) {
                                    ::tideorm::entity_manager::__with_entity_manager_db(
                                        entity_manager,
                                        <#related_ty as ::tideorm::model::Model>::delete(deleted),
                                    )
                                    .await?;
                                }

                                entity_manager.remove_by_entity_manager_key::<#related_ty>(deleted_key);
                            }
                        }

                        entity_manager
                            .snapshot::<#related_ty>(owner_table, &owner_key, relation_name, &updated_keys)
                            .await;
                    }
                });
            }

            if field.has_many_through.is_some() {
                let mut relation_types = relation_generic_types(&field.ty).into_iter();
                let related_ty = relation_types.next()?;
                let related_local_key = field.owner_key.as_deref().unwrap_or("id");

                return Some(quote! {
                    if self.#ident.is_loaded() {
                        let owner_table = <Self as ::tideorm::entity_manager::TideEntityManagerMeta>::tide_table_name();
                        let owner_key = <Self as ::tideorm::entity_manager::TideEntityManagerMeta>::tide_pk_key(self);
                        let relation_name = stringify!(#ident);
                        let mut updated_keys = Vec::new();
                        let mut related_values = ::std::collections::HashMap::<String, ::serde_json::Value>::new();

                        if let Some(items) = self.#ident.as_mut() {
                            updated_keys.reserve(items.len());

                            for item in items.iter_mut() {
                                let existing_key = ::tideorm::entity_manager::__model_entity_manager_key(item)?;
                                let should_persist = match existing_key.as_deref() {
                                    Some(existing_key) => match entity_manager.get_by_entity_manager_key::<#related_ty>(existing_key) {
                                        Some(cached) => {
                                            ::serde_json::to_value(&*item)?
                                                != ::serde_json::to_value(&cached)?
                                        }
                                        None => true,
                                    },
                                    None => true,
                                };

                                if !should_persist {
                                    <#related_ty as ::tideorm::entity_manager::TideEntityManagerSync>::tide_sync_entity_manager_relations(item, entity_manager).await?;
                                    entity_manager.put(item.clone());
                                } else {
                                    let saved = ::tideorm::entity_manager::save_with_entity_manager(item, entity_manager).await?;
                                    *item = saved.clone();
                                    entity_manager.put(saved);
                                }

                                let current_key = ::tideorm::entity_manager::__model_entity_manager_key(item)?
                                    .ok_or_else(|| ::tideorm::Error::invalid_query(format!(
                                        "{} relation '{}' requires persisted related keys after save",
                                        stringify!(#struct_name),
                                        relation_name,
                                    )))?;
                                let related_value = <#related_ty as ::tideorm::internal::InternalModel>::field_json_value(
                                    item,
                                    #related_local_key,
                                )?
                                .ok_or_else(|| ::tideorm::Error::invalid_query(format!(
                                    "{} relation '{}' could not read related key '{}' from saved model",
                                    stringify!(#struct_name),
                                    relation_name,
                                    #related_local_key,
                                )))?;

                                related_values.insert(current_key.clone(), related_value);
                                updated_keys.push(current_key);
                            }
                        }

                        let to_detach = entity_manager
                            .deletions::<#related_ty>(owner_table, &owner_key, relation_name, &updated_keys)
                            .await;
                        for deleted_key in &to_detach {
                            if let Some(deleted) = entity_manager.get_by_entity_manager_key::<#related_ty>(deleted_key) {
                                if let Some(related_value) = <#related_ty as ::tideorm::internal::InternalModel>::field_json_value(
                                    &deleted,
                                    #related_local_key,
                                )? {
                                    self.#ident.detach(related_value).await?;
                                }
                            }
                        }

                        let to_attach = entity_manager
                            .additions::<#related_ty>(owner_table, &owner_key, relation_name, &updated_keys)
                            .await;
                        for attach_key in &to_attach {
                            if let Some(related_value) = related_values.get(attach_key) {
                                self.#ident.attach(related_value.clone()).await?;
                            }
                        }

                        entity_manager
                            .snapshot::<#related_ty>(owner_table, &owner_key, relation_name, &updated_keys)
                            .await;
                    }
                });
            }

            None
        })
        .collect();

    let entity_manager_impl = quote! {
        #[cfg(feature = "entity-manager")]
        impl ::tideorm::entity_manager::TideEntityManagerMeta for #struct_name {
            fn tide_table_name() -> &'static str
            where
                Self: Sized,
            {
                <Self as ::tideorm::model::ModelMeta>::table_name()
            }

            fn tide_pk_key(&self) -> String {
                ::tideorm::entity_manager::__pk_to_entity_manager_key(
                    &<Self as ::tideorm::model::Model>::primary_key(self),
                )
                .expect("entity manager primary key should serialize")
            }
        }

        #[cfg(feature = "entity-manager")]
        impl ::tideorm::entity_manager::TideEntityManagerSync for #struct_name {
            fn tide_sync_entity_manager_relations<'a>(
                &'a mut self,
                entity_manager: &'a ::std::sync::Arc<::tideorm::entity_manager::EntityManager>,
            ) -> ::std::pin::Pin<
                Box<dyn ::std::future::Future<Output = ::tideorm::Result<()>> + Send + 'a>,
            > {
                Box::pin(async move {
                    #(#relation_sync_blocks)*
                    Ok(())
                })
            }
        }

        #[cfg(feature = "entity-manager")]
        impl #struct_name {
            pub async fn find_in_entity_manager(
                pk: <Self as ::tideorm::model::ModelMeta>::PrimaryKey,
                entity_manager: &::std::sync::Arc<::tideorm::entity_manager::EntityManager>,
            ) -> ::tideorm::Result<Option<Self>> {
                entity_manager.find::<Self>(pk).await
            }
        }
    };

    quote! {
        #field_writer_impl
        #merge_impl
        #entity_manager_impl
    }
}

fn generate_soft_delete_impl(ctx: &BuildContext) -> TokenStream2 {
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

fn generate_internal_model_impl(ctx: &BuildContext) -> TokenStream2 {
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
                Self {
                    #(#field_names: model.#field_names),*,
                    #(#relation_field_defaults),*
                }
                .with_relations()
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

fn generate_helper_methods_impl(ctx: &BuildContext) -> TokenStream2 {
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

fn generate_model_trait_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let internal_entity_mod = &ctx.internal_entity_mod;
    let primary_key_impl = build_primary_key_value_impl(ctx);
    let table_name = &ctx.table_name;
    let pk_auto_increment = ctx.pk_auto_increment;
    let column_names = &ctx.column_names;
    let column_variants = &ctx.column_variants;
    let field_names = &ctx.field_names;
    let pk_contains_conflict_check = build_pk_conflict_check(ctx);
    let pk_exclusion_check = build_pk_exclusion_check(ctx, quote!(column));

    quote! {
        #[::tideorm::async_trait::async_trait]
        impl ::tideorm::model::Model for #struct_name {
            fn primary_key(&self) -> Self::PrimaryKey {
                #primary_key_impl
            }

            async fn find(id: Self::PrimaryKey) -> ::tideorm::Result<Option<Self>> {
                use ::tideorm::database::Connection;
                use ::tideorm::internal::InternalModel;
                use ::tideorm::orm::{EntityTrait, QueryFilter};
                let error_context = Self::__primary_key_error_context(&id)
                    .query(format!("find({})", <Self as ::tideorm::model::ModelMeta>::primary_key_display(&id)));
                let result = ::tideorm::profiling::__profile_future(async move {
                    let connection = ::tideorm::database::__current_connection()
                        .map_err(|error| ::tideorm::orm::OrmError::Custom(error.to_string()))?;
                    match connection {
                        ::tideorm::database::ConnectionRef::Database(conn) => {
                            #internal_entity_mod::Entity::find()
                                .filter(<Self as InternalModel>::primary_key_condition(&id))
                                .one(conn.connection())
                                .await
                        }
                        ::tideorm::database::ConnectionRef::Transaction(tx) => {
                            #internal_entity_mod::Entity::find()
                                .filter(<Self as InternalModel>::primary_key_condition(&id))
                                .one(tx.as_ref())
                                .await
                        }
                    }
                })
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                Ok(result.map(|model| <Self as InternalModel>::from_entity_model(model)))
            }

            async fn find_with(
                id: Self::PrimaryKey,
                db: &::tideorm::database::Database,
            ) -> ::tideorm::Result<Option<Self>> {
                use ::tideorm::database::Connection;
                use ::tideorm::internal::InternalModel;
                use ::tideorm::orm::{EntityTrait, QueryFilter};
                let error_context = Self::__primary_key_error_context(&id)
                    .query(format!("find({})", <Self as ::tideorm::model::ModelMeta>::primary_key_display(&id)));
                let result = ::tideorm::profiling::__profile_future(async move {
                    let connection = db
                        .__get_connection()
                        .map_err(|error| ::tideorm::orm::OrmError::Custom(error.to_string()))?;
                    match connection {
                        ::tideorm::database::ConnectionRef::Database(conn) => {
                            #internal_entity_mod::Entity::find()
                                .filter(<Self as InternalModel>::primary_key_condition(&id))
                                .one(conn.connection())
                                .await
                        }
                        ::tideorm::database::ConnectionRef::Transaction(tx) => {
                            #internal_entity_mod::Entity::find()
                                .filter(<Self as InternalModel>::primary_key_condition(&id))
                                .one(tx.as_ref())
                                .await
                        }
                    }
                })
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                Ok(result.map(|model| <Self as InternalModel>::from_entity_model(model)))
            }

            async fn destroy(id: Self::PrimaryKey) -> ::tideorm::Result<u64> {
                use ::tideorm::database::Connection;
                use ::tideorm::orm::{EntityTrait, QueryFilter};
                let error_context = Self::__primary_key_error_context(&id)
                    .query(format!("destroy({})", <Self as ::tideorm::model::ModelMeta>::primary_key_display(&id)));
                let result = ::tideorm::profiling::__profile_future(async move {
                    let connection = ::tideorm::database::__current_connection()
                        .map_err(|error| ::tideorm::orm::OrmError::Custom(error.to_string()))?;
                    match connection {
                        ::tideorm::database::ConnectionRef::Database(conn) => {
                            #internal_entity_mod::Entity::delete_many()
                                .filter(<Self as ::tideorm::internal::InternalModel>::primary_key_condition(&id))
                                .exec(conn.connection())
                                .await
                        }
                        ::tideorm::database::ConnectionRef::Transaction(tx) => {
                            #internal_entity_mod::Entity::delete_many()
                                .filter(<Self as ::tideorm::internal::InternalModel>::primary_key_condition(&id))
                                .exec(tx.as_ref())
                                .await
                        }
                    }
                })
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                if result.rows_affected > 0 {
                    ::tideorm::QueryCache::global().invalidate_model(#table_name);
                }
                Ok(result.rows_affected)
            }

            async fn create(model: Self) -> ::tideorm::Result<Self> {
                use ::tideorm::database::Connection;
                use ::tideorm::callbacks::{
                    AfterCreateDispatch, AfterValidationDispatch, BeforeCreateOnlyDispatch,
                    BeforeSaveDispatch, BeforeValidationDispatch,
                };
                use ::tideorm::internal::InternalModel;
                use ::tideorm::orm::ActiveModelTrait;
                use ::tideorm::validation::Validate;
                let mut model = model;
                (&mut model).run_before_validation()?;
                ::tideorm::validation::Validate::validate(&model)
                    .map_err(::tideorm::Error::from)?;
                (&model).run_after_validation()?;
                (&mut model).run_before_save()?;
                (&mut model).run_before_create_only()?;
                let error_context = Self::__base_error_context().query(format!("insert into {}", #table_name));
                let active = <Self as InternalModel>::into_active_model(model);
                let result = ::tideorm::profiling::__profile_future(
                    async move {
                        let connection = ::tideorm::database::__current_connection()
                            .map_err(|error| ::tideorm::orm::OrmError::Custom(error.to_string()))?;
                        match connection {
                            ::tideorm::database::ConnectionRef::Database(conn) => active.insert(conn.connection()).await,
                            ::tideorm::database::ConnectionRef::Transaction(tx) => active.insert(tx.as_ref()).await,
                        }
                    }
                )
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                let model = <Self as InternalModel>::from_entity_model(result);
                ::tideorm::QueryCache::global().invalidate_model(#table_name);
                (&model).run_after_create()?;
                Ok(model)
            }

            async fn delete(self) -> ::tideorm::Result<u64> {
                use ::tideorm::database::Connection;
                use ::tideorm::orm::ActiveModelTrait;
                use ::tideorm::callbacks::{AfterDeleteDispatch, BeforeDeleteDispatch};
                let model = self;
                (&model).run_before_delete()?;
                let primary_key = model.primary_key();
                let error_context = Self::__primary_key_error_context(&primary_key)
                    .query(format!("delete where {}", <Self as ::tideorm::model::ModelMeta>::primary_key_display(&primary_key)));
                let active = model.clone().__into_delete_active_model();
                let result = ::tideorm::profiling::__profile_future(async move {
                    let connection = ::tideorm::database::__current_connection()
                        .map_err(|error| ::tideorm::orm::OrmError::Custom(error.to_string()))?;
                    match connection {
                        ::tideorm::database::ConnectionRef::Database(conn) => active.delete(conn.connection()).await,
                        ::tideorm::database::ConnectionRef::Transaction(tx) => active.delete(tx.as_ref()).await,
                    }
                })
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                if result.rows_affected > 0 {
                    ::tideorm::QueryCache::global().invalidate_model(#table_name);
                }
                (&model).run_after_delete()?;
                Ok(result.rows_affected)
            }

            async fn save(self) -> ::tideorm::Result<Self> {
                if self.is_new() {
                    Self::create(self).await
                } else {
                    let primary_key = self.primary_key();
                    if <Self as ::tideorm::model::ModelMeta>::primary_key_auto_increment()
                        && <Self as ::tideorm::model::ModelMeta>::primary_key_names().len() == 1
                    {
                        self.update().await
                    } else if Self::exists(primary_key).await? {
                        self.update().await
                    } else {
                        Self::create(self).await
                    }
                }
            }

            async fn update(self) -> ::tideorm::Result<Self> {
                use ::tideorm::database::Connection;
                use ::tideorm::callbacks::{
                    AfterUpdateDispatch, AfterValidationDispatch, BeforeSaveDispatch,
                    BeforeUpdateOnlyDispatch, BeforeValidationDispatch,
                };
                use ::tideorm::internal::InternalModel;
                use ::tideorm::orm::ActiveModelTrait;
                use ::tideorm::validation::Validate;
                let mut model = self;
                (&mut model).run_before_validation()?;
                ::tideorm::validation::Validate::validate(&model)
                    .map_err(::tideorm::Error::from)?;
                (&model).run_after_validation()?;
                (&mut model).run_before_save()?;
                (&mut model).run_before_update_only()?;
                let primary_key = model.primary_key();
                let error_context = Self::__primary_key_error_context(&primary_key)
                    .query(format!("update where {}", <Self as ::tideorm::model::ModelMeta>::primary_key_display(&primary_key)));
                let active = model.__into_update_active_model();
                let result = ::tideorm::profiling::__profile_future(
                    async move {
                        let connection = ::tideorm::database::__current_connection()
                            .map_err(|error| ::tideorm::orm::OrmError::Custom(error.to_string()))?;
                        match connection {
                            ::tideorm::database::ConnectionRef::Database(conn) => active.update(conn.connection()).await,
                            ::tideorm::database::ConnectionRef::Transaction(tx) => active.update(tx.as_ref()).await,
                        }
                    }
                )
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                let model = <Self as InternalModel>::from_entity_model(result);
                ::tideorm::QueryCache::global().invalidate_model(#table_name);
                (&model).run_after_update()?;
                Ok(model)
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
                use ::tideorm::database::Connection;
                use ::tideorm::internal::InternalModel;
                use ::tideorm::orm::{ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter};
                use ::tideorm::orm::sea_query::OnConflict;

                let model_for_lookup = model.clone();
                let conflict_cols = builder.conflict_columns;
                let include_pk = #pk_contains_conflict_check || !#pk_auto_increment;
                let insertable_columns: Vec<&str> = vec![#(#column_names),*]
                    .into_iter()
                    .filter(|column| !(*column == <Self as ::tideorm::model::ModelMeta>::primary_key_name() && #pk_auto_increment && !include_pk))
                    .collect();
                let conflict_columns: Vec<_> = conflict_cols
                    .iter()
                    .map(|column| {
                        Self::column_from_str(column).ok_or_else(|| {
                            ::tideorm::Error::invalid_query(format!(
                                "unknown conflict column '{}' for {}",
                                column,
                                #table_name
                            ))
                        })
                    })
                    .collect::<::tideorm::Result<Vec<_>>>()?;
                let update_cols: Vec<String> = if let Some(cols) = builder.update_columns {
                    cols
                } else if let Some(exclude) = builder.exclude_columns {
                    insertable_columns
                        .iter()
                        .filter(|column| !exclude.contains(&column.to_string()))
                        .map(|column| column.to_string())
                        .collect()
                } else {
                    insertable_columns.iter().filter(|column| {
                        let column = column.to_string();
                        !conflict_cols.contains(&column) && !#pk_exclusion_check
                    }).map(|column| column.to_string()).collect()
                };
                let update_columns: Vec<_> = update_cols
                    .iter()
                    .map(|column| {
                        Self::column_from_str(column).ok_or_else(|| {
                            ::tideorm::Error::invalid_query(format!(
                                "unknown update column '{}' for {}",
                                column,
                                #table_name
                            ))
                        })
                    })
                    .collect::<::tideorm::Result<Vec<_>>>()?;

                let on_conflict = if update_columns.is_empty() {
                    OnConflict::columns(conflict_columns.iter().cloned())
                        .do_nothing()
                        .to_owned()
                } else {
                    OnConflict::columns(conflict_columns.iter().cloned())
                        .update_columns(update_columns.iter().cloned())
                        .to_owned()
                };

                let sql = format!(
                    "insert_or_update into {} on conflict ({})",
                    #table_name,
                    conflict_cols.join(", ")
                );
                let error_context = Self::__base_error_context().query(sql.clone());

                let entity_model = model.to_entity_model();
                let active_model = if include_pk {
                    entity_model.into_active_model()
                } else {
                    <Self as InternalModel>::into_active_model(model)
                };

                ::tideorm::profiling::__profile_future(async move {
                    let connection = ::tideorm::database::__current_connection()
                        .map_err(|error| ::tideorm::orm::OrmError::Custom(error.to_string()))?;
                    match connection {
                        ::tideorm::database::ConnectionRef::Database(conn) => {
                            #internal_entity_mod::Entity::insert(active_model)
                                .on_conflict(on_conflict)
                                .exec(conn.connection())
                                .await
                        }
                        ::tideorm::database::ConnectionRef::Transaction(tx) => {
                            #internal_entity_mod::Entity::insert(active_model)
                                .on_conflict(on_conflict)
                                .exec(tx.as_ref())
                                .await
                        }
                    }
                })
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context.clone()))?;
                ::tideorm::QueryCache::global().invalidate_model(#table_name);

                let mut finder = #internal_entity_mod::Entity::find();
                for conflict_column in &conflict_cols {
                    finder = match conflict_column.as_str() {
                        #(#column_names | stringify!(#field_names) => finder.filter(#internal_entity_mod::Column::#column_variants.eq(model_for_lookup.#field_names.clone())),)*
                        _ => {
                            return Err(::tideorm::Error::invalid_query(format!(
                                "unknown conflict column '{}' for {}",
                                conflict_column,
                                #table_name
                            )));
                        }
                    };
                }

                let result = match ::tideorm::database::__current_connection()? {
                    ::tideorm::database::ConnectionRef::Database(conn) => finder.one(conn.connection()).await,
                    ::tideorm::database::ConnectionRef::Transaction(tx) => finder.one(tx.as_ref()).await,
                }
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;

                result
                    .map(<Self as InternalModel>::from_entity_model)
                    .ok_or_else(|| ::tideorm::Error::query("upsert completed but no matching row could be reloaded".to_string()))
            }
        }
    }
}

fn generate_eager_loader_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let relation_arms = build_eager_relation_arms(ctx);

    quote! {
        #[::tideorm::async_trait::async_trait]
        impl ::tideorm::relations::EagerLoadModel for #struct_name {
            async fn __eager_load(
                models: &mut [::tideorm::relations::WithRelations<Self>],
                relation_tree: &::tideorm::relations::RelationTree,
            ) -> ::tideorm::Result<()> {
                use ::tideorm::database::Connection;
                use ::tideorm::internal::InternalModel;
                use ::tideorm::orm::{EntityTrait, LoaderTrait};

                if models.is_empty() || relation_tree.is_empty() {
                    return Ok(());
                }

                let entity_models: Vec<_> = models
                    .iter()
                    .map(|entry| entry.model.to_entity_model())
                    .collect();
                let connection = ::tideorm::database::__current_connection()?;

                for relation_name in relation_tree.roots() {
                    match relation_name.as_str() {
                        #(#relation_arms)*
                        _ => {
                            return Err(::tideorm::Error::query(format!(
                                "Unknown relation '{}' on {}",
                                relation_name,
                                stringify!(#struct_name)
                            )));
                        }
                    }
                }

                Ok(())
            }
        }
    }
}

fn build_eager_relation_arms(ctx: &BuildContext) -> Vec<TokenStream2> {
    ctx.relation_fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .filter_map(|(field, ident)| {
            let relation_name = ident.to_string();
            let related_types = relation_generic_types(&field.ty);
            let related_ty = related_types.first()?.clone();

            if field.has_many.is_some() || field.has_many_through.is_some() {
                return Some(quote! {
                    #relation_name => {
                        let nested = relation_tree.get_nested(#relation_name);
                        let loaded = match &connection {
                            ::tideorm::database::ConnectionRef::Database(conn) => entity_models
                                .load_many(<<#related_ty as ::tideorm::internal::InternalModel>::Entity as EntityTrait>::find(), conn.connection())
                                .await,
                            ::tideorm::database::ConnectionRef::Transaction(tx) => entity_models
                                .load_many(<<#related_ty as ::tideorm::internal::InternalModel>::Entity as EntityTrait>::find(), tx.as_ref())
                                .await,
                        }
                            .map_err(|error| ::tideorm::Error::query(error.to_string()))?;

                        for (entry, related_models) in models.iter_mut().zip(loaded.into_iter()) {
                            let mut related_models: Vec<#related_ty> = related_models
                                .into_iter()
                                .map(<#related_ty as ::tideorm::internal::InternalModel>::from_entity_model)
                                .collect();

                            if let Some(nested_tree) = nested {
                                let mut wrapped_related: Vec<::tideorm::relations::WithRelations<#related_ty>> = related_models
                                    .into_iter()
                                    .map(::tideorm::relations::WithRelations::new)
                                    .collect();
                                <#related_ty as ::tideorm::relations::EagerLoadModel>::__eager_load(
                                    &mut wrapped_related,
                                    nested_tree,
                                )
                                .await?;
                                related_models = wrapped_related
                                    .into_iter()
                                    .map(::tideorm::relations::WithRelations::into_inner)
                                    .collect();
                            }

                            entry.model.#ident.set_cached(related_models);
                        }
                    }
                });
            }

            if field.has_one.is_some() || field.belongs_to.is_some() {
                return Some(quote! {
                    #relation_name => {
                        let nested = relation_tree.get_nested(#relation_name);
                        let loaded = match &connection {
                            ::tideorm::database::ConnectionRef::Database(conn) => entity_models
                                .load_one(<<#related_ty as ::tideorm::internal::InternalModel>::Entity as EntityTrait>::find(), conn.connection())
                                .await,
                            ::tideorm::database::ConnectionRef::Transaction(tx) => entity_models
                                .load_one(<<#related_ty as ::tideorm::internal::InternalModel>::Entity as EntityTrait>::find(), tx.as_ref())
                                .await,
                        }
                            .map_err(|error| ::tideorm::Error::query(error.to_string()))?;

                        for (entry, related_model) in models.iter_mut().zip(loaded.into_iter()) {
                            let related_model = match related_model {
                                Some(model) => {
                                    let model = <#related_ty as ::tideorm::internal::InternalModel>::from_entity_model(model);
                                    if let Some(nested_tree) = nested {
                                        let mut wrapped_related = vec![::tideorm::relations::WithRelations::new(model)];
                                        <#related_ty as ::tideorm::relations::EagerLoadModel>::__eager_load(
                                            &mut wrapped_related,
                                            nested_tree,
                                        )
                                        .await?;
                                        wrapped_related
                                            .into_iter()
                                            .next()
                                            .map(::tideorm::relations::WithRelations::into_inner)
                                    } else {
                                        Some(model)
                                    }
                                }
                                None => None,
                            };

                            entry.model.#ident.set_cached(related_model);
                        }
                    }
                });
            }

            None
        })
        .collect()
}
