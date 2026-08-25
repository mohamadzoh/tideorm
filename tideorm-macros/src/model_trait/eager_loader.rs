use super::*;

use syn::Ident;

use crate::parse::{ModelField, relation_wrapper_name};

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

/// Build the related model's base `find()` with its own soft-delete scope applied.
///
/// Eager loading must hide trashed rows exactly like `HasMany::load()` does, so the
/// generated expression mirrors the runtime's `scoped_find` helper. Trait methods are
/// called through fully qualified paths so no extra imports leak into the generated
/// function body.
fn scoped_related_find(related_ty: &syn::Type) -> TokenStream2 {
    quote! {
        {
            let __tideorm_related_find =
                <<#related_ty as ::tideorm::internal::InternalModel>::Entity as EntityTrait>::find();
            let __tideorm_deleted_at =
                if <#related_ty as ::tideorm::model::ModelMeta>::soft_delete_enabled() {
                    <#related_ty as ::tideorm::internal::InternalModel>::column_from_str(
                        <#related_ty as ::tideorm::model::ModelMeta>::deleted_at_column(),
                    )
                } else {
                    None
                };
            match __tideorm_deleted_at {
                Some(__tideorm_column) => ::tideorm::orm::QueryFilter::filter(
                    __tideorm_related_find,
                    ::tideorm::orm::ColumnTrait::is_null(&__tideorm_column),
                ),
                None => __tideorm_related_find,
            }
        }
    }
}

/// Resolve the next relation level for a `Vec<Vec<Related>>` collected across
/// every parent, then redistribute it into the original groups.
///
/// Recursing per parent would be N+1, so the whole collection is flattened,
/// loaded once, and split back apart by the recorded group sizes. Operates on a
/// `mut grouped` binding and a `nested` `Option<&RelationTree>`.
fn nested_many_redistribution(related_ty: &syn::Type) -> TokenStream2 {
    quote! {
        if let Some(nested_tree) = nested {
            let mut group_sizes: Vec<usize> = Vec::with_capacity(grouped.len());
            let mut flattened: Vec<::tideorm::relations::WithRelations<#related_ty>> =
                Vec::new();
            for group in grouped {
                group_sizes.push(group.len());
                flattened.extend(
                    group.into_iter().map(::tideorm::relations::WithRelations::new),
                );
            }

            <#related_ty as ::tideorm::relations::EagerLoadModel>::__eager_load(
                &mut flattened,
                nested_tree,
            )
            .await?;

            let mut restored = flattened
                .into_iter()
                .map(::tideorm::relations::WithRelations::into_inner);
            grouped = Vec::with_capacity(group_sizes.len());
            for group_size in group_sizes {
                grouped.push(restored.by_ref().take(group_size).collect());
            }
        }
    }
}

/// The single-related counterpart of [`nested_many_redistribution`]: gather the
/// present children from every parent, resolve the next level with one pass,
/// then put them back in their original slots. Operates on a
/// `mut related_models: Vec<Option<Related>>` binding.
fn nested_one_redistribution(related_ty: &syn::Type) -> TokenStream2 {
    quote! {
        if let Some(nested_tree) = nested {
            let mut slots: Vec<bool> = Vec::with_capacity(related_models.len());
            let mut flattened: Vec<::tideorm::relations::WithRelations<#related_ty>> =
                Vec::new();
            for related_model in related_models {
                match related_model {
                    Some(model) => {
                        slots.push(true);
                        flattened.push(::tideorm::relations::WithRelations::new(model));
                    }
                    None => slots.push(false),
                }
            }

            <#related_ty as ::tideorm::relations::EagerLoadModel>::__eager_load(
                &mut flattened,
                nested_tree,
            )
            .await?;

            let mut restored = flattened
                .into_iter()
                .map(::tideorm::relations::WithRelations::into_inner);
            related_models = Vec::with_capacity(slots.len());
            for filled in slots {
                if filled {
                    related_models.push(restored.next());
                } else {
                    related_models.push(None);
                }
            }
        }
    }
}

/// Arm for a relation `with(..)` cannot resolve.
///
/// These relations are declared, wired and lazily loadable, so falling into the
/// generic "Unknown relation" catch-all reads like a typo. Name the limitation
/// and point at the lazy path instead. Every declared relation gets an arm, so
/// the catch-all is only ever reached by a genuinely unknown name.
fn unsupported_relation_arm(
    struct_name: &Ident,
    relation_name: &str,
    wrapper: Option<&str>,
) -> TokenStream2 {
    let message = format!(
        "Relation '{}' on {} cannot be eager loaded: `with(..)` has no eager path for {} relations; load it lazily with `model.{}.load().await`",
        relation_name,
        struct_name,
        wrapper.unwrap_or("this kind of"),
        relation_name
    );

    quote! {
        #relation_name => {
            return Err(::tideorm::Error::query(#message));
        }
    }
}

/// Arm for `MorphOne`/`MorphMany`, which SeaORM's `LoaderTrait` cannot express
/// because the join carries a type discriminator alongside the key.
fn morph_relation_arm(
    ctx: &BuildContext,
    field: &ModelField,
    ident: &Ident,
    related_ty: &syn::Type,
    is_many: bool,
) -> Option<TokenStream2> {
    let relation_name = ident.to_string();
    let morph_name = field.morph_name.as_deref()?;
    let type_column = format!("{}_type", morph_name);
    let id_column = format!("{}_id", morph_name);
    let local_key = field.local_key.as_deref().unwrap_or("id");
    let local_key_ident = ctx.resolve_local_key_ident(local_key, ident).ok()?;

    let lookup = quote! {
        let nested = relation_tree.get_nested(#relation_name);
        let parent_keys: Vec<_> = models
            .iter()
            .map(|entry| ::tideorm::prelude::json!(entry.model.#local_key_ident.clone()))
            .collect();
        let by_key = <#related_ty as ::tideorm::relations::EagerLoadModel>::__load_grouped_by_key(
            &parent_keys,
            #id_column,
            Some((#type_column, <Self as ::tideorm::model::ModelMeta>::table_name())),
        )
        .await?;
    };

    if is_many {
        let redistribute = nested_many_redistribution(related_ty);
        return Some(quote! {
            #relation_name => {
                #lookup

                let mut grouped: Vec<Vec<#related_ty>> = parent_keys
                    .iter()
                    .map(|key| by_key.get(&key.to_string()).cloned().unwrap_or_default())
                    .collect();

                #redistribute

                for (entry, related_models) in models.iter_mut().zip(grouped.into_iter()) {
                    entry.set_relation(#relation_name, &related_models)?;
                    entry.model.#ident.set_cached(related_models);
                }
            }
        });
    }

    let redistribute = nested_one_redistribution(related_ty);
    Some(quote! {
        #relation_name => {
            #lookup

            let mut related_models: Vec<Option<#related_ty>> = parent_keys
                .iter()
                .map(|key| by_key.get(&key.to_string()).and_then(|group| group.first().cloned()))
                .collect();

            #redistribute

            for (entry, related_model) in models.iter_mut().zip(related_models.into_iter()) {
                entry.set_relation(#relation_name, &related_model)?;
                entry.model.#ident.set_cached(related_model);
            }
        }
    })
}

fn build_eager_relation_arms(ctx: &BuildContext) -> Vec<TokenStream2> {
    let struct_name = &ctx.struct_name;

    ctx.relation_fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| {
            let wrapper = relation_wrapper_name(&field.ty);
            let name = ident.to_string();

            build_supported_relation_arm(ctx, field, ident, wrapper)
                .unwrap_or_else(|| unsupported_relation_arm(struct_name, &name, wrapper))
        })
        .collect()
}

/// The `with(..)` arm for one relation field, or `None` when this relation has no
/// eager path and the caller should emit `unsupported_relation_arm` instead.
fn build_supported_relation_arm(
    ctx: &BuildContext,
    field: &ModelField,
    ident: &Ident,
    wrapper: Option<&str>,
) -> Option<TokenStream2> {
    // `MorphTo` resolves a different target type per row and the self-referencing
    // wrappers keep no cache slot to fill, so neither has an eager form.
    if matches!(wrapper, Some("MorphTo" | "SelfRef" | "SelfRefMany")) {
        return None;
    }

    let relation_name = ident.to_string();
    let related_types = relation_generic_types(&field.ty);
    let related_ty = related_types.first()?.clone();

    match wrapper {
        Some("MorphMany") => return morph_relation_arm(ctx, field, ident, &related_ty, true),
        Some("MorphOne") => return morph_relation_arm(ctx, field, ident, &related_ty, false),
        _ => {}
    }

    let scoped_find = scoped_related_find(&related_ty);

    if field.has_many.is_some() || field.has_many_through.is_some() {
        let redistribute = nested_many_redistribution(&related_ty);
        return Some(quote! {
            #relation_name => {
                let nested = relation_tree.get_nested(#relation_name);
                let loaded = match &connection {
                    ::tideorm::database::ConnectionRef::Database(conn) => entity_models
                        .load_many(#scoped_find, conn.connection())
                        .await,
                    ::tideorm::database::ConnectionRef::Transaction(tx) => entity_models
                        .load_many(#scoped_find, tx.as_ref())
                        .await,
                }
                    .map_err(|error| ::tideorm::Error::query(error.to_string()))?;

                let mut grouped: Vec<Vec<#related_ty>> = loaded
                    .into_iter()
                    .map(|related_models| {
                        related_models
                            .into_iter()
                            .map(<#related_ty as ::tideorm::internal::InternalModel>::try_from_entity_model)
                            .collect::<::tideorm::Result<Vec<_>>>()
                    })
                    .collect::<::tideorm::Result<Vec<_>>>()?;

                #redistribute

                for (entry, related_models) in models.iter_mut().zip(grouped.into_iter()) {
                    entry.set_relation(#relation_name, &related_models)?;
                    entry.model.#ident.set_cached(related_models);
                }
            }
        });
    }

    if field.has_one.is_some() || field.belongs_to.is_some() {
        let redistribute = nested_one_redistribution(&related_ty);
        return Some(quote! {
            #relation_name => {
                let nested = relation_tree.get_nested(#relation_name);
                let loaded = match &connection {
                    ::tideorm::database::ConnectionRef::Database(conn) => entity_models
                        .load_one(#scoped_find, conn.connection())
                        .await,
                    ::tideorm::database::ConnectionRef::Transaction(tx) => entity_models
                        .load_one(#scoped_find, tx.as_ref())
                        .await,
                }
                    .map_err(|error| ::tideorm::Error::query(error.to_string()))?;

                let mut related_models: Vec<Option<#related_ty>> = loaded
                    .into_iter()
                    .map(|related_model| {
                        related_model
                            .map(<#related_ty as ::tideorm::internal::InternalModel>::try_from_entity_model)
                            .transpose()
                    })
                    .collect::<::tideorm::Result<Vec<_>>>()?;

                #redistribute

                for (entry, related_model) in models.iter_mut().zip(related_models.into_iter()) {
                    entry.set_relation(#relation_name, &related_model)?;
                    entry.model.#ident.set_cached(related_model);
                }
            }
        });
    }

    None
}
