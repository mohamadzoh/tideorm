use super::*;

use crate::parse::{ModelField, RelationKind};

pub(super) fn generate_entity_manager_support_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let field_idents = &ctx.field_idents;
    let field_types = &ctx.field_types;
    let name_patterns = build_name_patterns(ctx);

    let field_writer_impl = quote! {
        impl ::tideorm::entity_manager::TideEntityManagerFieldWriter for #struct_name {
            fn tide_set_field_value(
                &mut self,
                field: &str,
                value: ::tideorm::serde_json::Value,
            ) -> ::tideorm::Result<()> {
                match field {
                    #(#name_patterns => {
                        self.#field_idents = ::tideorm::serde_json::from_value::<#field_types>(value.clone())
                            .map_err(|error| ::tideorm::Error::invalid_query(format!(
                                "failed to assign entity manager value for field '{}' on {}: {}",
                                field,
                                stringify!(#struct_name),
                                error
                            )))?;
                        Ok(())
                    },)*
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
        impl ::tideorm::entity_manager::TideEntityManagerMergePersisted for #struct_name {
            fn tide_merge_persisted(&mut self, persisted: Self) {
                #(self.#field_idents = persisted.#field_idents;)*
            }
        }
    };

    let relation_database_attach_blocks = ctx
        .relation_fields
        .iter()
        .filter(|field| {
            field
                .relation_kind()
                .is_some_and(RelationKind::is_entity_relation)
        })
        .map(|field| {
            let ident = field.ident();
            quote!(self.#ident.attach_query_database(database);)
        });

    let relation_sync_blocks: Vec<TokenStream2> = ctx
        .relation_fields
        .iter()
        .filter_map(|field| build_relation_sync_block(ctx, field))
        .collect();
    // The owner's identity is the same for every relation, so it is computed once;
    // a model with nothing to sync emits no unused bindings.
    let sync_preamble = (!relation_sync_blocks.is_empty()).then(|| {
        quote! {
            let owner_table = <Self as ::tideorm::entity_manager::TideEntityManagerMeta>::tide_table_name();
            let owner_key = <Self as ::tideorm::entity_manager::TideEntityManagerMeta>::tide_pk_key(self);
        }
    });

    // Flush ordering is decided per table, so the only thing the entity manager
    // needs from a relation is which side of a foreign key each end sits on: a
    // `belongs_to` names a table this model points at, `has_one`/`has_many` name
    // tables pointing back at it. `has_many_through` contributes nothing — the
    // pivot row is written by relation sync once both sides are saved — and
    // polymorphic or self-referencing relations have no distinct target table to
    // order against.
    let mut parent_tables: Vec<TokenStream2> = Vec::new();
    let mut child_tables: Vec<TokenStream2> = Vec::new();
    for field in &ctx.relation_fields {
        let Some(related_ty) = field.related_types().into_iter().next() else {
            continue;
        };
        let table_name = quote!(<#related_ty as ::tideorm::model::ModelMeta>::table_name());

        match field.relation_kind() {
            Some(RelationKind::BelongsTo) => parent_tables.push(table_name),
            Some(RelationKind::HasOne | RelationKind::HasMany) => child_tables.push(table_name),
            _ => {}
        }
    }

    let identity_key_expr = crate::relation_gen::entity_manager_identity_key_expr(quote!(self));

    let entity_manager_impl = quote! {
        impl ::tideorm::entity_manager::TideEntityManagerMeta for #struct_name {
            fn tide_table_name() -> &'static str
            where
                Self: Sized,
            {
                <Self as ::tideorm::model::ModelMeta>::table_name()
            }

            fn tide_pk_key(&self) -> String {
                #identity_key_expr
            }

            fn tide_pk_is_new(&self) -> bool {
                let primary_key = <Self as ::tideorm::model::Model>::primary_key(self);
                <Self as ::tideorm::model::ModelMeta>::primary_key_is_new(&primary_key)
            }

            fn tide_parent_tables() -> Vec<&'static str>
            where
                Self: Sized,
            {
                vec![#(#parent_tables),*]
            }

            fn tide_child_tables() -> Vec<&'static str>
            where
                Self: Sized,
            {
                vec![#(#child_tables),*]
            }

            fn tide_attach_entity_manager_database(
                &mut self,
                database: &::tideorm::database::Database,
            ) {
                #(#relation_database_attach_blocks)*
            }
        }

        impl ::tideorm::entity_manager::TideEntityManagerSync for #struct_name {
            async fn tide_sync_entity_manager_relations<'a>(
                &'a mut self,
                entity_manager: &'a ::std::sync::Arc<::tideorm::entity_manager::EntityManager>,
            ) -> ::tideorm::Result<()> {
                #sync_preamble
                #(#relation_sync_blocks)*
                Ok(())
            }
        }

        impl #struct_name {
            pub async fn find_in_entity_manager(
                pk: <Self as ::tideorm::model::ModelMeta>::PrimaryKey,
                entity_manager: &::std::sync::Arc<::tideorm::entity_manager::EntityManager>,
            ) -> ::tideorm::Result<Option<Self>> {
                entity_manager.find::<Self>(pk).await
            }
        }
    };

    // Gated by TideORM's own `entity-manager` feature, not a `cfg` the
    // model's crate would evaluate against its own features.
    quote! {
        ::tideorm::__if_entity_manager! {
            #field_writer_impl
            #merge_impl
            #entity_manager_impl
        }
    }
}

/// The flush-time sync of one loaded `has_many`/`has_one`/`has_many_through`
/// relation: persist changed children, then delete (or detach) the ones the
/// relation no longer holds and record what it holds now.
fn build_relation_sync_block(ctx: &BuildContext, field: &ModelField) -> Option<TokenStream2> {
    let ident = field.ident();
    let relation_name = field.name();
    let related_ty = field.related_types().into_iter().next()?;
    let local_key = field
        .local_key
        .as_deref()
        .unwrap_or(ctx.default_local_key());
    let struct_name = &ctx.struct_name;

    let sync = match field.relation_kind()? {
        RelationKind::HasMany => {
            let foreign_key = field
                .foreign_key
                .as_deref()
                .expect("validated relation foreign_key");
            let local_key_ident = ctx
                .resolve_local_key_ident(local_key, ident)
                .expect("relation local keys resolve when the context is built");
            quote! {
                let current_keys = self.#ident.current_keys()?;
                ::tideorm::entity_manager::__delete_detached_entities::<#related_ty>(
                    entity_manager, owner_table, &owner_key, #relation_name, &current_keys,
                )
                .await?;

                let child_fk_value = ::tideorm::serde_json::to_value(self.#local_key_ident.clone())?;
                let mut updated_keys = Vec::new();
                if let Some(items) = self.#ident.as_mut() {
                    updated_keys.reserve(items.len());
                    for item in items.iter_mut() {
                        <#related_ty as ::tideorm::entity_manager::TideEntityManagerFieldWriter>::tide_set_field_value(
                            item,
                            #foreign_key,
                            child_fk_value.clone(),
                        )?;
                        updated_keys.extend(
                            ::tideorm::entity_manager::__sync_related_entity(item, entity_manager).await?,
                        );
                    }
                }
            }
        }
        RelationKind::HasOne => {
            let foreign_key = field
                .foreign_key
                .as_deref()
                .expect("validated relation foreign_key");
            let local_key_ident = ctx
                .resolve_local_key_ident(local_key, ident)
                .expect("relation local keys resolve when the context is built");
            quote! {
                // The row the relation no longer holds goes first, as for a
                // `has_many`: a unique foreign key refuses the new row beside it.
                let current_keys: Vec<String> = match self.#ident.as_mut() {
                    Some(item) => ::tideorm::entity_manager::__model_entity_manager_key(&*item)?
                        .into_iter()
                        .collect(),
                    None => Vec::new(),
                };
                ::tideorm::entity_manager::__delete_detached_entities::<#related_ty>(
                    entity_manager, owner_table, &owner_key, #relation_name, &current_keys,
                )
                .await?;

                let child_fk_value = ::tideorm::serde_json::to_value(self.#local_key_ident.clone())?;
                let mut updated_keys = Vec::new();
                if let Some(item) = self.#ident.as_mut() {
                    <#related_ty as ::tideorm::entity_manager::TideEntityManagerFieldWriter>::tide_set_field_value(
                        item,
                        #foreign_key,
                        child_fk_value,
                    )?;
                    updated_keys.extend(
                        ::tideorm::entity_manager::__sync_related_entity(item, entity_manager).await?,
                    );
                }
            }
        }
        RelationKind::HasManyThrough => {
            let related_local_key = field.owner_key.as_deref().unwrap_or("id");
            quote! {
                let mut updated_keys = Vec::new();
                let mut related_values = ::std::collections::HashMap::<String, ::tideorm::serde_json::Value>::new();

                if let Some(items) = self.#ident.as_mut() {
                    updated_keys.reserve(items.len());

                    for item in items.iter_mut() {
                        let current_key = ::tideorm::entity_manager::__sync_related_entity(item, entity_manager)
                            .await?
                            .ok_or_else(|| ::tideorm::Error::invalid_query(format!(
                                "{} relation '{}' requires persisted related keys after save",
                                stringify!(#struct_name),
                                #relation_name,
                            )))?;
                        let related_value = <#related_ty as ::tideorm::internal::InternalModel>::field_json_value(
                            item,
                            #related_local_key,
                        )?
                        .ok_or_else(|| ::tideorm::Error::invalid_query(format!(
                            "{} relation '{}' could not read related key '{}' from saved model",
                            stringify!(#struct_name),
                            #relation_name,
                            #related_local_key,
                        )))?;

                        related_values.insert(current_key.clone(), related_value);
                        updated_keys.push(current_key);
                    }
                }

                let to_detach = entity_manager
                    .deletions::<#related_ty>(owner_table, &owner_key, #relation_name, &updated_keys);
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
                    .additions::<#related_ty>(owner_table, &owner_key, #relation_name, &updated_keys);
                for attach_key in &to_attach {
                    if let Some(related_value) = related_values.get(attach_key) {
                        self.#ident.attach(related_value.clone()).await?;
                    }
                }
            }
        }
        _ => return None,
    };

    Some(quote! {
        if self.#ident.is_loaded() {
            #sync
            entity_manager
                .snapshot::<#related_ty>(owner_table, &owner_key, #relation_name, &updated_keys);
        }
    })
}
