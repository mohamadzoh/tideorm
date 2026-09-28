use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

use crate::context::BuildContext;
use crate::parse::{ModelField, RelationKind};

/// For each relation, the statements that rebuild its wrapper from the
/// model's own fields and keep the runtime state of the wrapper it replaces:
/// `previous`'s when there is one, else its own.
pub(crate) fn build_relation_wiring(ctx: &BuildContext) -> syn::Result<Vec<TokenStream2>> {
    ctx.relation_fields
        .iter()
        .map(|field| {
            let ident = field.ident();
            let assignment = build_relation_assignment(ctx, field)?;
            Ok(quote! {
                let own = ::std::mem::take(&mut self.#ident);
                #assignment
                self.#ident
                    .preserve_runtime_state_from(previous.map_or(&own, |previous| &previous.#ident));
            })
        })
        .collect()
}

pub(crate) fn generate_with_relations_method(ctx: &BuildContext) -> TokenStream2 {
    let relation_wiring = &ctx.relation_wiring;
    let previous = if relation_wiring.is_empty() {
        quote!(_previous)
    } else {
        quote!(previous)
    };
    quote! {
        pub fn with_relations(mut self) -> Self {
            self.__wire_relations(None);
            self
        }

        /// Build every relation wrapper from the model's own fields, keeping
        /// the runtime state of `previous`'s, or else of the one it replaces.
        #[doc(hidden)]
        pub fn __wire_relations(&mut self, #previous: Option<&Self>) {
            #(#relation_wiring)*
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
    let local_key_name = field
        .local_key
        .as_deref()
        .unwrap_or(ctx.default_local_key());

    let relation = match kind {
        RelationKind::HasOne | RelationKind::HasMany | RelationKind::HasManyThrough => {
            let local = ctx.resolve_local_key(local_key_name, ident)?;
            let (local_key_ident, local_key) = (local.ident(), local.column_name());
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
            quote! {
                let relation = #constructor
                    .with_parent_pk(::tideorm::prelude::json!(self.#local_key_ident.clone()));
                ::tideorm::__if_entity_manager! {
                    let relation = relation
                        .with_metadata(#relation_name, <Self as ::tideorm::model::ModelMeta>::table_name())
                        .with_owner_key(::tideorm::entity_manager::__identity_key::<Self>(&self));
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
            let local = ctx.resolve_local_key(local_key_name, ident)?;
            let (local_key_ident, local_key) = (local.ident(), local.column_name());
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
            let local_key = ctx.resolve_local_key(local_key_name, ident)?.column_name();
            quote! {
                let relation = ::tideorm::relations::SelfRef::new(#foreign_key, #local_key)
                    .with_fk_value(::tideorm::prelude::json!(self.#foreign_key_ident.clone()));
            }
        }
        RelationKind::SelfRefMany => {
            let foreign_key = foreign_key.unwrap_or("parent_id");
            let local = ctx.resolve_local_key(local_key_name, ident)?;
            let (local_key_ident, local_key) = (local.ident(), local.column_name());
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
