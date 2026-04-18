use super::*;

pub(super) fn generate_eager_loader_impl(ctx: &BuildContext) -> TokenStream2 {
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
                    .map(|entry| entry.model.try_to_entity_model())
                    .collect::<::tideorm::Result<Vec<_>>>()?;
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
                                .map(<#related_ty as ::tideorm::internal::InternalModel>::try_from_entity_model)
                                .collect::<::tideorm::Result<Vec<_>>>()?;

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
                                    let model = <#related_ty as ::tideorm::internal::InternalModel>::try_from_entity_model(model)?;
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
