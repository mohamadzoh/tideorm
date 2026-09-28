use super::*;

use syn::Ident;

use crate::parse::{ModelField, RelationKind};

pub(super) fn generate_eager_loader_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let struct_name_str = &ctx.struct_name_str;
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
                if models.is_empty() || relation_tree.is_empty() {
                    return Ok(());
                }

                for relation_name in relation_tree.roots() {
                    match relation_name.as_str() {
                        #(#relation_arms)*
                        _ => {
                            return Err(::tideorm::Error::query(format!(
                                "Unknown relation '{}' on {}",
                                relation_name,
                                #struct_name_str
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
    let related_types = field.related_types();
    let Some(related_ty) = related_types.first() else {
        return unsupported_relation_arm(&ctx.struct_name, &relation_name, kind);
    };
    // Resolve the related rows of every parent in one query, as `Vec<Vec<R>>` for a
    // to-many relation or `Vec<Option<R>>` for a to-one one.
    let (related, is_many) = match kind {
        RelationKind::HasManyThrough => match through_lookup(ctx, field, related_ty) {
            Some(lookup) => (lookup, true),
            None => return unsupported_relation_arm(&ctx.struct_name, &relation_name, kind),
        },
        RelationKind::HasMany
        | RelationKind::HasOne
        | RelationKind::BelongsTo
        | RelationKind::MorphOne
        | RelationKind::MorphMany => match keyed_lookup(ctx, field, related_ty, kind) {
            Some(lookup) => (
                lookup,
                matches!(kind, RelationKind::HasMany | RelationKind::MorphMany),
            ),
            None => return unsupported_relation_arm(&ctx.struct_name, &relation_name, kind),
        },
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

/// Every parent's key, as JSON, in the order of `models`.
fn parent_keys(parent_key_ident: &Ident) -> TokenStream2 {
    quote! {
        &models
            .iter()
            .map(|entry| ::tideorm::prelude::json!(entry.model.#parent_key_ident.clone()))
            .collect::<Vec<_>>()
    }
}

/// The related rows of a relation keyed by one column on each side, which
/// `relations::__eager_keyed_many`/`__eager_keyed_one` read: matched by value,
/// with a `MorphOne`/`MorphMany`'s type discriminator.
fn keyed_lookup(
    ctx: &BuildContext,
    field: &ModelField,
    related_ty: &syn::Type,
    kind: RelationKind,
) -> Option<TokenStream2> {
    let local_key = field
        .local_key
        .as_deref()
        .unwrap_or(ctx.default_local_key());
    let (parent_key_ident, related_key, morph_type) = match kind {
        RelationKind::MorphOne | RelationKind::MorphMany => {
            let morph_name = field.morph_name.as_deref()?;
            let type_column = format!("{}_type", morph_name);
            (
                ctx.resolve_local_key_ident(local_key, field.ident()).ok()?,
                format!("{}_id", morph_name),
                quote!(Some((#type_column, <Self as ::tideorm::model::ModelMeta>::table_name()))),
            )
        }
        RelationKind::BelongsTo => (
            ctx.resolve_required_db_field_ident(field.foreign_key.as_deref()?, field.ident())
                .ok()?,
            field.owner_key.as_deref().unwrap_or("id").to_string(),
            quote!(None),
        ),
        _ => (
            ctx.resolve_local_key_ident(local_key, field.ident()).ok()?,
            field.foreign_key.clone()?,
            quote!(None),
        ),
    };
    let lookup = if matches!(kind, RelationKind::HasMany | RelationKind::MorphMany) {
        quote!(__eager_keyed_many)
    } else {
        quote!(__eager_keyed_one)
    };
    let parent_keys = parent_keys(&parent_key_ident);

    Some(quote! {
        ::tideorm::relations::#lookup::<#related_ty>(#parent_keys, #related_key, #morph_type).await?
    })
}

/// The related rows of a `HasManyThrough`, which `relations::__eager_through`
/// reads through the pivot.
fn through_lookup(
    ctx: &BuildContext,
    field: &ModelField,
    related_ty: &syn::Type,
) -> Option<TokenStream2> {
    let pivot_ty = field.related_types().get(1)?.clone();
    let local_key = field
        .local_key
        .as_deref()
        .unwrap_or(ctx.default_local_key());
    let parent_key_ident = ctx.resolve_local_key_ident(local_key, field.ident()).ok()?;
    let foreign_key = field.foreign_key.as_deref()?;
    let related_key = field.related_key.as_deref()?;
    let related_local_key = field.owner_key.as_deref().unwrap_or("id");
    let parent_keys = parent_keys(&parent_key_ident);

    Some(quote! {
        ::tideorm::relations::__eager_through::<#related_ty, #pivot_ty>(
            #parent_keys,
            #foreign_key,
            #related_key,
            #related_local_key,
        )
        .await?
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
