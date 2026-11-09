use super::*;

use crate::parse::{ModelField, RelationKind};

pub(super) fn generate_entity_manager_support_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let field_idents = &ctx.field_idents;
    let relation_idents: Vec<_> = ctx
        .relation_fields
        .iter()
        .map(|field| field.ident())
        .collect();

    // A model without relations keeps the trait's defaults.
    let relation_copy_impl = (!relation_idents.is_empty()).then(|| {
        quote! {
            fn tide_relations_from(&mut self, source: &Self) {
                #(self.#relation_idents = ::std::clone::Clone::clone(&source.#relation_idents);)*
            }

            fn tide_relations_match(&self, other: &Self) -> bool {
                #(::tideorm::serde_json::to_value(&self.#relation_idents).ok()
                    == ::tideorm::serde_json::to_value(&other.#relation_idents).ok())&&*
            }
        }
    });
    let merge_impl = quote! {
        impl ::tideorm::entity_manager::TideEntityManagerMergePersisted for #struct_name {
            fn tide_merge_persisted(&mut self, persisted: Self) {
                #(self.#field_idents = persisted.#field_idents;)*
            }

            #relation_copy_impl
        }
    };

    let relation_sync_blocks: Vec<TokenStream2> = ctx
        .relation_fields
        .iter()
        .filter_map(|field| build_relation_sync_block(ctx, field))
        .collect();
    // The owner's identity is the same for every relation, so it is computed once;
    // a model with nothing to sync emits no unused bindings.
    let sync_preamble = (!relation_sync_blocks.is_empty()).then(|| {
        quote! {
            let owner = ::tideorm::entity_manager::__SyncOwner {
                entity_manager,
                table: <Self as ::tideorm::model::ModelMeta>::table_name(),
            };
        }
    });

    let sync_impl = quote! {
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
    };

    // Gated by TideORM's own `entity-manager` feature, not a `cfg` the
    // model's crate would evaluate against its own features.
    quote! {
        ::tideorm::__if_entity_manager! {
            #merge_impl
            #sync_impl
        }
    }
}

/// The flush-time sync of one loaded `has_many`/`has_one`/`has_many_through`
/// relation, which the runtime runs: persist changed children, then delete (or
/// detach) the ones the relation no longer holds and record what it holds now.
fn build_relation_sync_block(ctx: &BuildContext, field: &ModelField) -> Option<TokenStream2> {
    let ident = field.ident();
    let owner_value = || {
        let local_key_ident = ctx
            .relation_local_ident(field)
            .expect("relation local keys resolve when the context is built");
        quote!(::tideorm::DbValue::from(::std::clone::Clone::clone(&self.#local_key_ident)))
    };

    Some(match field.relation_kind()? {
        RelationKind::HasMany => {
            let owner_value = owner_value();
            quote! {
                ::tideorm::entity_manager::__sync_has_many(&owner, &mut self.#ident, #owner_value).await?;
            }
        }
        RelationKind::HasOne => {
            let owner_value = owner_value();
            quote! {
                ::tideorm::entity_manager::__sync_has_one(&owner, &mut self.#ident, #owner_value).await?;
            }
        }
        RelationKind::HasManyThrough => quote! {
            ::tideorm::entity_manager::__sync_has_many_through(&owner, &mut self.#ident).await?;
        },
        _ => return None,
    })
}
