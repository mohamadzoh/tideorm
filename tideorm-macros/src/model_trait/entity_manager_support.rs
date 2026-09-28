use super::*;

use crate::parse::{ModelField, RelationKind};

pub(super) fn generate_entity_manager_support_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let field_idents = &ctx.field_idents;

    let merge_impl = quote! {
        impl ::tideorm::entity_manager::TideEntityManagerMergePersisted for #struct_name {
            fn tide_merge_persisted(&mut self, persisted: Self) {
                #(self.#field_idents = persisted.#field_idents;)*
            }
        }
    };

    // Every relation of a model a manager loaded queries its database, the
    // polymorphic and self-referencing ones too.
    let relation_database_attach_blocks = ctx.relation_fields.iter().map(|field| {
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
            let owner_key = ::tideorm::entity_manager::__identity_key::<Self>(self);
            let owner = ::tideorm::entity_manager::__SyncOwner {
                entity_manager,
                table: <Self as ::tideorm::model::ModelMeta>::table_name(),
                key: &owner_key,
            };
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
    // Rows of one table are ordered by the foreign keys that join them: each
    // relation's `(related table, foreign key, referenced column)`; the
    // runtime keeps the ones whose related table is this model's own.
    let mut row_references: Vec<TokenStream2> = Vec::new();
    for field in &ctx.relation_fields {
        let Some(related_ty) = field.related_types().into_iter().next() else {
            continue;
        };
        let table_name = quote!(<#related_ty as ::tideorm::model::ModelMeta>::table_name());
        let foreign_key = field.foreign_key.as_deref();
        let local_key = field
            .local_key
            .as_deref()
            .unwrap_or(ctx.default_local_key());

        match field.relation_kind() {
            Some(RelationKind::BelongsTo) => {
                let owner_key = field.owner_key.as_deref().unwrap_or("id");
                if let Some(foreign_key) = foreign_key {
                    row_references.push(quote!((#table_name, #foreign_key, #owner_key)));
                }
                parent_tables.push(table_name);
            }
            Some(RelationKind::HasOne | RelationKind::HasMany) => {
                if let Some(foreign_key) = foreign_key {
                    row_references.push(quote!((#table_name, #foreign_key, #local_key)));
                }
                child_tables.push(table_name);
            }
            Some(RelationKind::SelfRef | RelationKind::SelfRefMany) => {
                let foreign_key = foreign_key.unwrap_or("parent_id");
                row_references.push(quote!((
                    <Self as ::tideorm::model::ModelMeta>::table_name(),
                    #foreign_key,
                    #local_key
                )));
            }
            _ => {}
        }
    }
    let row_references_impl = (!row_references.is_empty()).then(|| {
        quote! {
            fn tide_row_references() -> Vec<(&'static str, &'static str, &'static str)>
            where
                Self: Sized,
            {
                vec![#(#row_references),*]
            }
        }
    });

    let entity_manager_impl = quote! {
        impl ::tideorm::entity_manager::TideEntityManagerMeta for #struct_name {
            fn tide_table_name() -> &'static str
            where
                Self: Sized,
            {
                <Self as ::tideorm::model::ModelMeta>::table_name()
            }

            fn tide_pk_key(&self) -> String {
                ::tideorm::entity_manager::__identity_key(self)
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

            #row_references_impl

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

    };

    // Gated by TideORM's own `entity-manager` feature, not a `cfg` the
    // model's crate would evaluate against its own features.
    quote! {
        ::tideorm::__if_entity_manager! {
            #merge_impl
            #entity_manager_impl
        }
    }
}

/// The flush-time sync of one loaded `has_many`/`has_one`/`has_many_through`
/// relation, which the runtime runs: persist changed children, then delete (or
/// detach) the ones the relation no longer holds and record what it holds now.
fn build_relation_sync_block(ctx: &BuildContext, field: &ModelField) -> Option<TokenStream2> {
    let ident = field.ident();
    let local_key = field
        .local_key
        .as_deref()
        .unwrap_or(ctx.default_local_key());
    let owner_value = |ident: &syn::Ident| {
        let local_key_ident = ctx
            .resolve_local_key_ident(local_key, ident)
            .expect("relation local keys resolve when the context is built");
        quote!(::tideorm::serde_json::to_value(self.#local_key_ident.clone())?)
    };

    Some(match field.relation_kind()? {
        RelationKind::HasMany => {
            let owner_value = owner_value(ident);
            quote! {
                ::tideorm::entity_manager::__sync_has_many(&owner, &mut self.#ident, #owner_value).await?;
            }
        }
        RelationKind::HasOne => {
            let owner_value = owner_value(ident);
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
