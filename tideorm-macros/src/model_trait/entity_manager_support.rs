use super::*;

pub(super) fn generate_entity_manager_support_impl(ctx: &BuildContext) -> TokenStream2 {
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
            !relation_field_names
                .iter()
                .any(|relation_name| relation_name == &field_name)
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
    let relation_database_attach_blocks: Vec<_> = ctx
        .relation_fields
        .iter()
        .filter_map(|field| {
            let ident = field.ident.as_ref()?;
            if field.has_one.is_some()
                || field.has_many.is_some()
                || field.belongs_to.is_some()
                || field.has_many_through.is_some()
            {
                return Some(quote! {
                    self.#ident.attach_query_database(database);
                });
            }

            None
        })
        .collect();
    // Every relation-sync arm decides whether a loaded child differs from its
    // entity-manager-cached copy the same way; emit that check from one place.
    let should_persist_fragment = |related_ty: &syn::Type| {
        quote! {
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
                let should_persist_check = should_persist_fragment(&related_ty);

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

                                #should_persist_check

                                if !should_persist {
                                    if let Some(existing_key) = existing_key {
                                        updated_keys.push(existing_key);
                                    }

                                    <#related_ty as ::tideorm::entity_manager::TideEntityManagerSync>::tide_sync_entity_manager_relations(item, entity_manager).await?;
                                    entity_manager.put(item.clone());
                                    continue;
                                }

                                let saved = ::tideorm::entity_manager::__save_with_entity_manager_in_scope(item, entity_manager).await?;

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
                let should_persist_check = should_persist_fragment(&related_ty);

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

                            #should_persist_check

                            if !should_persist {
                                if let Some(existing_key) = existing_key {
                                    updated_keys.push(existing_key);
                                }

                                <#related_ty as ::tideorm::entity_manager::TideEntityManagerSync>::tide_sync_entity_manager_relations(item, entity_manager).await?;
                                entity_manager.put(item.clone());
                            } else {
                                let saved = ::tideorm::entity_manager::__save_with_entity_manager_in_scope(item, entity_manager).await?;

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
                let should_persist_check = should_persist_fragment(&related_ty);

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
                                #should_persist_check

                                if !should_persist {
                                    <#related_ty as ::tideorm::entity_manager::TideEntityManagerSync>::tide_sync_entity_manager_relations(item, entity_manager).await?;
                                    entity_manager.put(item.clone());
                                } else {
                                    let saved = ::tideorm::entity_manager::__save_with_entity_manager_in_scope(item, entity_manager).await?;
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

            fn tide_attach_entity_manager_database(
                &mut self,
                database: &::tideorm::database::Database,
            ) {
                #(#relation_database_attach_blocks)*
            }
        }

        #[cfg(feature = "entity-manager")]
        impl ::tideorm::entity_manager::TideEntityManagerSync for #struct_name {
            async fn tide_sync_entity_manager_relations<'a>(
                &'a mut self,
                entity_manager: &'a ::std::sync::Arc<::tideorm::entity_manager::EntityManager>,
            ) -> ::tideorm::Result<()> {
                #(#relation_sync_blocks)*
                Ok(())
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
