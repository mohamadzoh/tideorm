use super::*;

use syn::Ident;

use crate::parse::{ModelField, RelationKind};

/// Parents resolved per eager-load query. One `IN` list for every parent would
/// exceed the bind-parameter limit (32,766 on SQLite, 65,535 elsewhere) on a
/// large result, so the parents are loaded in chunks of this size.
const EAGER_LOAD_CHUNK: usize = 5_000;

pub(super) fn generate_eager_loader_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let relation_arms = ctx
        .relation_fields
        .iter()
        .map(|field| build_relation_arm(ctx, field));

    quote! {
        #[::tideorm::async_trait::async_trait]
        impl ::tideorm::relations::EagerLoadModel for #struct_name {
            async fn __eager_load(
                models: &mut [::tideorm::relations::WithRelations<Self>],
                relation_tree: &::tideorm::relations::RelationTree,
            ) -> ::tideorm::Result<()> {
                use ::tideorm::internal::InternalModel;
                use ::tideorm::orm::LoaderTrait;

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

/// The `with(..)` arm for one relation field.
///
/// Every declared relation gets an arm, so the "Unknown relation" catch-all is
/// only ever reached by a genuinely unknown name.
fn build_relation_arm(ctx: &BuildContext, field: &ModelField) -> TokenStream2 {
    let relation_name = field.name();
    let kind = field
        .relation_kind()
        .expect("relation fields have a relation wrapper type");
    let related_ty = match field.related_types().into_iter().next() {
        Some(related_ty) => related_ty,
        None => return unsupported_relation_arm(&ctx.struct_name, &relation_name, kind),
    };

    // Resolve the related rows of every parent in one query, as `Vec<Vec<R>>` for a
    // to-many relation or `Vec<Option<R>>` for a to-one one.
    let (related, is_many) = match kind {
        RelationKind::HasMany | RelationKind::HasManyThrough => (
            quote! {
                {
                    let mut related = Vec::with_capacity(entity_models.len());
                    for parents in entity_models.chunks(#EAGER_LOAD_CHUNK) {
                        related.extend(
                            parents
                                .load_many(::tideorm::internal::scoped_find::<#related_ty>(), &connection.executor())
                                .await
                                .map_err(::tideorm::Error::from)?,
                        );
                    }
                    related
                }
                    .into_iter()
                    .map(|related_models| {
                        related_models
                            .into_iter()
                            .map(<#related_ty as InternalModel>::try_from_entity_model)
                            .collect::<::tideorm::Result<Vec<_>>>()
                    })
                    .collect::<::tideorm::Result<Vec<_>>>()?
            },
            true,
        ),
        RelationKind::HasOne | RelationKind::BelongsTo => (
            quote! {
                {
                    let mut related = Vec::with_capacity(entity_models.len());
                    for parents in entity_models.chunks(#EAGER_LOAD_CHUNK) {
                        related.extend(
                            parents
                                .load_one(::tideorm::internal::scoped_find::<#related_ty>(), &connection.executor())
                                .await
                                .map_err(::tideorm::Error::from)?,
                        );
                    }
                    related
                }
                    .into_iter()
                    .map(|related_model| {
                        related_model
                            .map(<#related_ty as InternalModel>::try_from_entity_model)
                            .transpose()
                    })
                    .collect::<::tideorm::Result<Vec<_>>>()?
            },
            false,
        ),
        RelationKind::MorphOne | RelationKind::MorphMany => {
            match morph_lookup(ctx, field, &related_ty, kind == RelationKind::MorphMany) {
                Some(lookup) => (lookup, kind == RelationKind::MorphMany),
                None => return unsupported_relation_arm(&ctx.struct_name, &relation_name, kind),
            }
        }
        // `MorphTo` resolves a different target type per row, and a self-referencing
        // relation nests to any depth, which no fixed set of batched queries covers.
        RelationKind::MorphTo | RelationKind::SelfRef | RelationKind::SelfRefMany => {
            return unsupported_relation_arm(&ctx.struct_name, &relation_name, kind);
        }
    };

    let resolve_nested = if is_many {
        quote!(::tideorm::relations::__eager_load_nested_many)
    } else {
        quote!(::tideorm::relations::__eager_load_nested_one)
    };
    let ident = field.ident();

    quote! {
        #relation_name => {
            let related = #resolve_nested(#related, relation_tree.get_nested(#relation_name)).await?;
            for (entry, related) in models.iter_mut().zip(related) {
                entry.set_relation(#relation_name, &related)?;
                entry.model.#ident.set_cached(related);
            }
        }
    }
}

/// The related rows of a `MorphOne`/`MorphMany`, which SeaORM's `LoaderTrait`
/// cannot express because the join carries a type discriminator alongside the key.
fn morph_lookup(
    ctx: &BuildContext,
    field: &ModelField,
    related_ty: &syn::Type,
    is_many: bool,
) -> Option<TokenStream2> {
    let morph_name = field.morph_name.as_deref()?;
    let type_column = format!("{}_type", morph_name);
    let id_column = format!("{}_id", morph_name);
    let local_key = field.local_key.as_deref().unwrap_or("id");
    let local_key_ident = ctx.resolve_local_key_ident(local_key, field.ident()).ok()?;
    let pick = if is_many {
        quote!(.cloned().unwrap_or_default())
    } else {
        quote!(.and_then(|group| group.first().cloned()))
    };

    Some(quote! {
        {
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
            parent_keys
                .iter()
                .map(|key| by_key.get(&key.to_string()) #pick)
                .collect::<Vec<_>>()
        }
    })
}

/// Arm for a relation `with(..)` cannot resolve.
///
/// These relations are declared, wired and lazily loadable, so falling into the
/// generic "Unknown relation" catch-all reads like a typo. Name the limitation
/// and point at the lazy path instead.
fn unsupported_relation_arm(
    struct_name: &Ident,
    relation_name: &str,
    kind: RelationKind,
) -> TokenStream2 {
    let message = format!(
        "Relation '{}' on {} cannot be eager loaded: `with(..)` has no eager path for {} relations; load it lazily with `model.{}.load().await`",
        relation_name,
        struct_name,
        kind.wrapper(),
        relation_name
    );

    quote! {
        #relation_name => {
            return Err(::tideorm::Error::query(#message));
        }
    }
}
