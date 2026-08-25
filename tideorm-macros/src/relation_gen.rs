use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;

use crate::context::BuildContext;
use crate::parse::{ModelField, relation_generic_types, relation_wrapper_name};

pub(crate) fn build_relation_field_inits(
    ctx: &BuildContext,
    relation_fields: &[ModelField],
) -> syn::Result<Vec<TokenStream2>> {
    relation_fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| build_relation_field_init(ctx, field, ident))
        .collect()
}

pub(crate) fn build_relation_state_refreshes(
    ctx: &BuildContext,
    relation_fields: &[ModelField],
) -> syn::Result<Vec<TokenStream2>> {
    relation_fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| build_relation_state_refresh(ctx, field, ident))
        .collect()
}

/// Emit the expression that renders a model's primary key as its entity-manager
/// identity key.
///
/// `TideEntityManagerMeta::tide_pk_key` and every relation wrapper's
/// `with_owner_key` have to agree on this string, and both are emitted into
/// contexts that cannot fail — `with_relations` returns `Self`, so a
/// serialization error has nowhere to go. Panicking from generated code the
/// user never sees is close to undebuggable, so fall back to the model's own
/// primary-key rendering: it is infallible, still maps equal primary keys to
/// equal identity keys, and always contains " = ", so it can never collide with
/// a JSON-encoded key.
///
/// `model_expr` is how the surrounding context names the model: `self` inside a
/// `&self` method, `&self` where the method owns or mutably borrows it.
pub(crate) fn entity_manager_identity_key_expr(model_expr: TokenStream2) -> TokenStream2 {
    quote! {
        {
            let primary_key = <Self as ::tideorm::model::Model>::primary_key(#model_expr);
            ::tideorm::entity_manager::__pk_to_entity_manager_key(&primary_key)
                .unwrap_or_else(|_| {
                    <Self as ::tideorm::model::ModelMeta>::primary_key_display(&primary_key)
                })
        }
    }
}

pub(crate) fn generate_with_relations_method(ctx: &BuildContext) -> TokenStream2 {
    let relation_field_inits = &ctx.relation_field_inits;
    quote! {
        pub fn with_relations(mut self) -> Self {
            #(#relation_field_inits;)*
            self
        }
    }
}

fn build_relation_field_init(
    ctx: &BuildContext,
    field: &ModelField,
    ident: &Ident,
) -> syn::Result<TokenStream2> {
    match build_relation_assignment(ctx, field, ident)? {
        Some(assignment) => Ok(quote! {
            let previous = self.#ident.clone();
            #assignment
            self.#ident.preserve_runtime_state_from(&previous)
        }),
        None => Ok(quote! {
            self.#ident = Default::default()
        }),
    }
}

fn build_relation_state_refresh(
    ctx: &BuildContext,
    field: &ModelField,
    ident: &Ident,
) -> syn::Result<TokenStream2> {
    match build_relation_assignment(ctx, field, ident)? {
        Some(assignment) => Ok(quote! {
            #assignment
            self.#ident.preserve_runtime_state_from(&previous.#ident);
        }),
        None => Ok(quote! {
            self.#ident = previous.#ident.clone();
        }),
    }
}

/// Emit the `self.#ident = <constructor>;` assignment that rebuilds a relation
/// wrapper from the model's own fields. Both `with_relations` (fresh init) and
/// `refresh_runtime_relations_from` (post-serde refresh) reuse it verbatim and
/// differ only in how they preserve prior runtime state. Returns `None` for a
/// field that declares no relation, so each caller can supply its own fallback.
fn build_relation_assignment(
    ctx: &BuildContext,
    field: &ModelField,
    ident: &Ident,
) -> syn::Result<Option<TokenStream2>> {
    let relation_wrapper = relation_wrapper_name(&field.ty);
    let owner_key = entity_manager_identity_key_expr(quote!(&self));

    if field.has_one.is_some() {
        let fk = field.foreign_key.as_deref().unwrap_or("id");
        let lk = field.local_key.as_deref().unwrap_or("id");
        let lk_ident = ctx.resolve_local_key_ident(lk, ident)?;
        let related_ty = relation_generic_types(&field.ty)
            .into_iter()
            .next()
            .ok_or_else(|| {
                syn::Error::new_spanned(&field.ty, "has_one relation requires a related model type")
            })?;
        return Ok(Some(quote! {
            self.#ident = {
                #[cfg(feature = "entity-manager")]
                {
                    ::tideorm::relations::HasOne::new(#fk, #lk)
                        .with_metadata(
                            stringify!(#ident),
                            <Self as ::tideorm::model::ModelMeta>::table_name(),
                            <#related_ty as ::tideorm::model::ModelMeta>::table_name(),
                        )
                        .with_owner_key(#owner_key)
                        .with_parent_pk(::tideorm::prelude::json!(self.#lk_ident.clone()))
                }
                #[cfg(not(feature = "entity-manager"))]
                {
                    ::tideorm::relations::HasOne::new(#fk, #lk)
                        .with_parent_pk(::tideorm::prelude::json!(self.#lk_ident.clone()))
                }
            };
        }));
    }

    if field.has_many.is_some() {
        let fk = field.foreign_key.as_deref().unwrap_or("id");
        let lk = field.local_key.as_deref().unwrap_or("id");
        let lk_ident = ctx.resolve_local_key_ident(lk, ident)?;
        let related_ty = relation_generic_types(&field.ty)
            .into_iter()
            .next()
            .ok_or_else(|| {
                syn::Error::new_spanned(
                    &field.ty,
                    "has_many relation requires a related model type",
                )
            })?;
        return Ok(Some(quote! {
            self.#ident = {
                #[cfg(feature = "entity-manager")]
                {
                    ::tideorm::relations::HasMany::new(#fk, #lk)
                        .with_metadata(
                            stringify!(#ident),
                            <Self as ::tideorm::model::ModelMeta>::table_name(),
                            <#related_ty as ::tideorm::model::ModelMeta>::table_name(),
                        )
                        .with_owner_key(#owner_key)
                        .with_parent_pk(::tideorm::prelude::json!(self.#lk_ident.clone()))
                }
                #[cfg(not(feature = "entity-manager"))]
                {
                    ::tideorm::relations::HasMany::new(#fk, #lk)
                        .with_parent_pk(::tideorm::prelude::json!(self.#lk_ident.clone()))
                }
            };
        }));
    }

    if field.belongs_to.is_some() {
        let fk = field.foreign_key.as_deref().unwrap_or("id");
        let ok = field.owner_key.as_deref().unwrap_or("id");
        let fk_ident = ctx.resolve_required_db_field_ident(fk, ident)?;
        return Ok(Some(quote! {
            self.#ident = ::tideorm::relations::BelongsTo::new(#fk, #ok)
                .with_fk_value(::tideorm::prelude::json!(self.#fk_ident.clone()));
        }));
    }

    if field.has_many_through.is_some() {
        let fk = field.foreign_key.as_deref().unwrap_or("id");
        let related_key = field.related_key.as_deref().unwrap_or("id");
        let local_key = field.local_key.as_deref().unwrap_or("id");
        let related_local_key = field.owner_key.as_deref().unwrap_or("id");
        let local_key_ident = ctx.resolve_local_key_ident(local_key, ident)?;
        let pivot_table = field.pivot.as_deref().unwrap_or("");
        let related_ty = relation_generic_types(&field.ty)
            .into_iter()
            .next()
            .ok_or_else(|| {
                syn::Error::new_spanned(
                    &field.ty,
                    "has_many_through relation requires a related model type",
                )
            })?;
        return Ok(Some(quote! {
            self.#ident = {
                #[cfg(feature = "entity-manager")]
                {
                    ::tideorm::relations::HasManyThrough::new(
                        #fk,
                        #related_key,
                        #local_key,
                        #related_local_key,
                        #pivot_table,
                    )
                    .with_metadata(
                        stringify!(#ident),
                        <Self as ::tideorm::model::ModelMeta>::table_name(),
                        <#related_ty as ::tideorm::model::ModelMeta>::table_name(),
                    )
                    .with_owner_key(#owner_key)
                    .with_parent_pk(::tideorm::prelude::json!(self.#local_key_ident.clone()))
                }
                #[cfg(not(feature = "entity-manager"))]
                {
                    ::tideorm::relations::HasManyThrough::new(
                        #fk,
                        #related_key,
                        #local_key,
                        #related_local_key,
                        #pivot_table,
                    )
                    .with_parent_pk(::tideorm::prelude::json!(self.#local_key_ident.clone()))
                }
            };
        }));
    }

    if relation_wrapper == Some("MorphOne") {
        let morph_name = field.morph_name.as_deref().expect("validated morph_name");
        let local_key = field.local_key.as_deref().unwrap_or("id");
        let local_key_ident = ctx.resolve_local_key_ident(local_key, ident)?;
        return Ok(Some(quote! {
            self.#ident = ::tideorm::relations::MorphOne::new(#morph_name, #local_key)
                .with_parent(
                    ::tideorm::prelude::json!(self.#local_key_ident.clone()),
                    <Self as ::tideorm::model::ModelMeta>::table_name().to_string(),
                );
        }));
    }

    if relation_wrapper == Some("MorphMany") {
        let morph_name = field.morph_name.as_deref().expect("validated morph_name");
        let local_key = field.local_key.as_deref().unwrap_or("id");
        let local_key_ident = ctx.resolve_local_key_ident(local_key, ident)?;
        return Ok(Some(quote! {
            self.#ident = ::tideorm::relations::MorphMany::new(#morph_name, #local_key)
                .with_parent(
                    ::tideorm::prelude::json!(self.#local_key_ident.clone()),
                    <Self as ::tideorm::model::ModelMeta>::table_name().to_string(),
                );
        }));
    }

    if relation_wrapper == Some("MorphTo") {
        let morph_name = field.morph_name.as_deref().expect("validated morph_name");
        let type_column = format!("{}_type", morph_name);
        let id_column = format!("{}_id", morph_name);
        let type_ident = ctx.resolve_required_db_field_ident(&type_column, ident)?;
        let id_ident = ctx.resolve_required_db_field_ident(&id_column, ident)?;
        return Ok(Some(quote! {
            self.#ident = ::tideorm::relations::MorphTo::new(#type_column, #id_column)
                .with_values(
                    self.#type_ident.clone(),
                    ::tideorm::prelude::json!(self.#id_ident.clone()),
                );
        }));
    }

    if relation_wrapper == Some("SelfRef") {
        let foreign_key = field.foreign_key.as_deref().unwrap_or("parent_id");
        let local_key = field.local_key.as_deref().unwrap_or("id");
        let foreign_key_ident = ctx.resolve_required_db_field_ident(foreign_key, ident)?;
        return Ok(Some(quote! {
            self.#ident = ::tideorm::relations::SelfRef::new(#foreign_key, #local_key)
                .with_fk_value(::tideorm::prelude::json!(self.#foreign_key_ident.clone()));
        }));
    }

    if relation_wrapper == Some("SelfRefMany") {
        let foreign_key = field.foreign_key.as_deref().unwrap_or("parent_id");
        let local_key = field.local_key.as_deref().unwrap_or("id");
        let local_key_ident = ctx.resolve_local_key_ident(local_key, ident)?;
        return Ok(Some(quote! {
            self.#ident = ::tideorm::relations::SelfRefMany::new(#foreign_key, #local_key)
                .with_parent_pk(::tideorm::prelude::json!(self.#local_key_ident.clone()));
        }));
    }

    Ok(None)
}
