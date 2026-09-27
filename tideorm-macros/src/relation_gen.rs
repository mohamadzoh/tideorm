use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

use crate::context::BuildContext;
use crate::parse::{ModelField, RelationKind};

pub(crate) fn build_relation_field_inits(ctx: &BuildContext) -> syn::Result<Vec<TokenStream2>> {
    ctx.relation_fields
        .iter()
        .map(|field| {
            let ident = field.ident();
            let assignment = build_relation_assignment(ctx, field)?;
            Ok(quote! {
                let previous = self.#ident.clone();
                #assignment
                self.#ident.preserve_runtime_state_from(&previous);
            })
        })
        .collect()
}

pub(crate) fn build_relation_state_refreshes(ctx: &BuildContext) -> syn::Result<Vec<TokenStream2>> {
    ctx.relation_fields
        .iter()
        .map(|field| {
            let ident = field.ident();
            let assignment = build_relation_assignment(ctx, field)?;
            Ok(quote! {
                #assignment
                self.#ident.preserve_runtime_state_from(&previous.#ident);
            })
        })
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
            #(#relation_field_inits)*
            self
        }
    }
}

/// Emit the statements that rebuild a relation wrapper from the model's own
/// fields and assign it to `self.#ident`. Both `with_relations` (fresh init)
/// and `refresh_runtime_relations_from` (post-serde refresh) reuse them and
/// differ only in how they preserve prior runtime state.
fn build_relation_assignment(ctx: &BuildContext, field: &ModelField) -> syn::Result<TokenStream2> {
    let ident = field.ident();
    let kind = field
        .relation_kind()
        .expect("relation fields have a relation wrapper type");
    let foreign_key = field.foreign_key.as_deref();
    let local_key = field
        .local_key
        .as_deref()
        .unwrap_or(ctx.default_local_key());

    let relation = match kind {
        RelationKind::HasOne | RelationKind::HasMany | RelationKind::HasManyThrough => {
            let local_key_ident = ctx.resolve_local_key_ident(local_key, ident)?;
            let foreign_key = foreign_key.expect("validated relation foreign_key");
            let constructor = match kind {
                RelationKind::HasOne => {
                    quote!(::tideorm::relations::HasOne::new(#foreign_key, #local_key))
                }
                RelationKind::HasMany => {
                    quote!(::tideorm::relations::HasMany::new(#foreign_key, #local_key))
                }
                _ => {
                    let related_key = field.related_key.as_deref().expect("validated related_key");
                    let related_local_key = field.owner_key.as_deref().unwrap_or("id");
                    let pivot_table = field.pivot.as_deref().expect("validated pivot");
                    quote!(::tideorm::relations::HasManyThrough::new(
                        #foreign_key,
                        #related_key,
                        #local_key,
                        #related_local_key,
                        #pivot_table,
                    ))
                }
            };
            let relation_name = field.name();
            let owner_key = entity_manager_identity_key_expr(quote!(&self));
            quote! {
                let relation = #constructor
                    .with_parent_pk(::tideorm::prelude::json!(self.#local_key_ident.clone()));
                ::tideorm::__if_entity_manager! {
                    let relation = relation
                        .with_metadata(#relation_name, <Self as ::tideorm::model::ModelMeta>::table_name())
                        .with_owner_key(#owner_key);
                }
            }
        }
        RelationKind::BelongsTo => {
            let foreign_key = foreign_key.expect("validated relation foreign_key");
            let owner_key = field.owner_key.as_deref().unwrap_or("id");
            let foreign_key_ident = ctx.resolve_required_db_field_ident(foreign_key, ident)?;
            quote! {
                let relation = ::tideorm::relations::BelongsTo::new(#foreign_key, #owner_key)
                    .with_fk_value(::tideorm::prelude::json!(self.#foreign_key_ident.clone()));
            }
        }
        RelationKind::MorphOne | RelationKind::MorphMany => {
            let morph_name = field.morph_name.as_deref().expect("validated morph_name");
            let local_key_ident = ctx.resolve_local_key_ident(local_key, ident)?;
            let wrapper = if kind == RelationKind::MorphOne {
                quote!(MorphOne)
            } else {
                quote!(MorphMany)
            };
            quote! {
                let relation = ::tideorm::relations::#wrapper::new(#morph_name, #local_key)
                    .with_parent(
                        ::tideorm::prelude::json!(self.#local_key_ident.clone()),
                        <Self as ::tideorm::model::ModelMeta>::table_name().to_string(),
                    );
            }
        }
        RelationKind::MorphTo => {
            let morph_name = field.morph_name.as_deref().expect("validated morph_name");
            let type_column = format!("{}_type", morph_name);
            let id_column = format!("{}_id", morph_name);
            let type_ident = ctx.resolve_required_db_field_ident(&type_column, ident)?;
            let id_ident = ctx.resolve_required_db_field_ident(&id_column, ident)?;
            quote! {
                let relation = ::tideorm::relations::MorphTo::new(#type_column, #id_column)
                    .__on::<Self>()
                    .with_values(
                        self.#type_ident.clone(),
                        ::tideorm::prelude::json!(self.#id_ident.clone()),
                    );
            }
        }
        RelationKind::SelfRef => {
            let foreign_key = foreign_key.unwrap_or("parent_id");
            let foreign_key_ident = ctx.resolve_required_db_field_ident(foreign_key, ident)?;
            // The parent is looked up by this column, so a typo is caught here
            // rather than as an unknown column at the first load.
            ctx.resolve_local_key_ident(local_key, ident)?;
            quote! {
                let relation = ::tideorm::relations::SelfRef::new(#foreign_key, #local_key)
                    .with_fk_value(::tideorm::prelude::json!(self.#foreign_key_ident.clone()));
            }
        }
        RelationKind::SelfRefMany => {
            let foreign_key = foreign_key.unwrap_or("parent_id");
            let local_key_ident = ctx.resolve_local_key_ident(local_key, ident)?;
            quote! {
                let relation = ::tideorm::relations::SelfRefMany::new(#foreign_key, #local_key)
                    .with_parent_pk(::tideorm::prelude::json!(self.#local_key_ident.clone()));
            }
        }
    };

    Ok(quote! {
        #relation
        self.#ident = relation;
    })
}
