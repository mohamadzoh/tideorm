//! What a model's relation fields generate beside the wrappers: the checks
//! of their keys, the hidden-attribute filters of their payloads, and what an
//! entity manager reads them through.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::Type;

use crate::context::BuildContext;
use crate::parse::{ModelField, RelationKind};

/// Per-relation hidden-attribute filters for `ModelMeta::relation_payload_filters`.
///
/// An eager-loaded relation payload is untagged JSON by the time `to_json` sees it,
/// so the only way it can be filtered by the *target* model's hidden list is for the
/// derive — which does know the target type — to hand over a function pointer to it.
///
/// `MorphTo` is skipped: its generic parameter carries no `Model` bound because the
/// target is chosen at runtime, so there is no single type to capture. Those payloads
/// keep falling back to the owning model's hidden list.
pub(super) fn build_relation_payload_filters(ctx: &BuildContext) -> Vec<TokenStream2> {
    ctx.relation_fields
        .iter()
        .filter(|field| field.relation_kind() != Some(RelationKind::MorphTo))
        .filter_map(|field| {
            let target = field.related_types().into_iter().next()?;
            let key = ctx.serialized_key(field);
            Some(quote! {
                (
                    #key,
                    <#target as ::tideorm::model::ModelMeta>::__strip_hidden_payload
                        as ::tideorm::model::RelationPayloadFilter
                )
            })
        })
        .collect()
}

fn missing_related_type(field: &ModelField) -> syn::Error {
    syn::Error::new_spanned(
        &field.ty,
        "relation field must specify a related model type",
    )
}

fn missing_pivot_type(field: &ModelField) -> syn::Error {
    syn::Error::new_spanned(
        &field.ty,
        "has_many_through relations must specify both related and pivot model types",
    )
}

/// The checks one entity relation's keys must pass: its local key is a field
/// of this model (a compile error naming the relation otherwise), and the
/// columns it names on the related model, or on the pivot and the related
/// model for `HasManyThrough`, exist there (a `const` assertion, since only
/// those models know their columns).
pub(super) fn build_relation_checks(
    ctx: &BuildContext,
    field: &ModelField,
) -> syn::Result<TokenStream2> {
    let ident = field.ident();
    let name = field.name();
    let column_assert = |ty: &Type, column: &str, message: &str| {
        compile_time_column_assert(ty, column, message, ident.span())
    };
    let related_types = field.related_types();
    let related_ty = related_types
        .first()
        .ok_or_else(|| missing_related_type(field))?;
    let foreign_key = field
        .foreign_key
        .as_deref()
        .expect("validated relation foreign_key");

    match field.relation_kind() {
        Some(RelationKind::HasManyThrough) => {
            let pivot_ty = related_types
                .get(1)
                .ok_or_else(|| missing_pivot_type(field))?;
            ctx.relation_local_ident(field)?;
            let related_key = field.related_key.as_deref().expect("validated related_key");
            let related_local_key = field.owner_key.as_deref().unwrap_or("id");
            let pivot_foreign = column_assert(
                pivot_ty,
                foreign_key,
                &format!(
                    "many-to-many relation '{name}' references an unknown pivot foreign key '{foreign_key}'"
                ),
            );
            let pivot_related = column_assert(
                pivot_ty,
                related_key,
                &format!(
                    "many-to-many relation '{name}' references an unknown pivot related column '{related_key}'"
                ),
            );
            let related_column = column_assert(
                related_ty,
                related_local_key,
                &format!(
                    "many-to-many relation '{name}' references an unknown related column '{related_local_key}'"
                ),
            );
            Ok(quote! {
                #pivot_foreign
                #pivot_related
                #related_column
            })
        }
        Some(RelationKind::BelongsTo) => {
            ctx.resolve_required_db_field_ident(foreign_key, ident)?;
            let owner_key = field.owner_key.as_deref().unwrap_or("id");
            Ok(column_assert(
                related_ty,
                owner_key,
                &remote_column_error(&name, owner_key),
            ))
        }
        _ => {
            ctx.relation_local_ident(field)?;
            Ok(column_assert(
                related_ty,
                foreign_key,
                &remote_column_error(&name, foreign_key),
            ))
        }
    }
}

fn remote_column_error(relation: &str, column: &str) -> String {
    format!(
        "relation '{}' references an unknown remote column '{}'",
        relation, column
    )
}

fn compile_time_column_assert(ty: &Type, column: &str, message: &str, span: Span) -> TokenStream2 {
    quote_spanned! {span=>
        const _: () = assert!(<#ty>::__has_column_name(#column), #message);
    }
}

/// The `ModelMeta` methods a load and an entity manager reach a model's
/// relations through: attaching every relation to a database, and, for the
/// manager, the tables and keys its flush orders rows by and dropping what
/// they loaded. `None` for a model without relations, which keeps the
/// defaults.
///
/// Flush order is decided per table, so all a relation tells the flush is
/// which side of a foreign key each end sits on: a `belongs_to` names a table
/// this model points at, `has_one`/`has_many` name tables pointing back at it.
/// `has_many_through` adds none, since relation sync writes the pivot row once
/// both sides are saved, and polymorphic or self-referencing relations have no
/// distinct table to order against. Rows of one table are ordered by each
/// relation's `(related table, foreign key, referenced column)`; the runtime
/// keeps the ones whose related table is this model's own.
pub(super) fn build_entity_manager_meta(ctx: &BuildContext) -> Option<TokenStream2> {
    if ctx.relation_fields.is_empty() {
        return None;
    }
    let mut parent_tables = Vec::new();
    let mut child_tables = Vec::new();
    let mut row_references = Vec::new();
    for field in &ctx.relation_fields {
        let Some(related_ty) = field.related_types().into_iter().next() else {
            continue;
        };
        let table_name = quote!(<#related_ty as ::tideorm::model::ModelMeta>::table_name());
        let foreign_key = field.foreign_key.as_deref();
        let local_key = || {
            ctx.relation_local_field(field)
                .expect("relation local keys resolve when the context is built")
                .column_name()
        };

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
                    let local_key = local_key();
                    row_references.push(quote!((#table_name, #foreign_key, #local_key)));
                }
                child_tables.push(table_name);
            }
            Some(RelationKind::SelfRef | RelationKind::SelfRefMany) => {
                let foreign_key = foreign_key.unwrap_or("parent_id");
                let local_key = local_key();
                row_references.push(quote!((
                    <Self as ::tideorm::model::ModelMeta>::table_name(),
                    #foreign_key,
                    #local_key
                )));
            }
            _ => {}
        }
    }
    let relation_idents: Vec<_> = ctx
        .relation_fields
        .iter()
        .map(|field| field.ident())
        .collect();

    // The entity-manager items are gated by TideORM's own `entity-manager`
    // feature, not a `cfg` the model's crate would evaluate against its own
    // features.
    Some(quote! {
        // Every relation of a loaded model queries the pool it was loaded
        // from, or the database of the manager that holds it, the
        // polymorphic and self-referencing ones too.
        fn __attach_query_database(&mut self, database: &::tideorm::database::Database) {
            #(self.#relation_idents.attach_query_database(database);)*
        }

        ::tideorm::__if_entity_manager! {
            fn __parent_tables() -> Vec<&'static str> {
                vec![#(#parent_tables),*]
            }

            fn __child_tables() -> Vec<&'static str> {
                vec![#(#child_tables),*]
            }

            fn __row_references() -> Vec<(&'static str, &'static str, &'static str)> {
                vec![#(#row_references),*]
            }

            fn __unload_relations(&mut self) {
                #(self.#relation_idents = ::std::default::Default::default();)*
                self.__wire_relations();
            }
        }
    })
}

/// The `ModelMeta::__relations` the project rules read a model's relations
/// through: each one's related model, whether this model owns the related rows
/// (the edges relation depth follows), and the columns holding its foreign
/// keys. `None` for a model without relations, which keeps the default.
pub(super) fn build_relation_outline(ctx: &BuildContext) -> Option<TokenStream2> {
    if ctx.relation_fields.is_empty() {
        return None;
    }
    let outline = quote!(::tideorm::rules::RelationOutline);
    let outlines = ctx.relation_fields.iter().filter_map(|field| {
        let name = field.name();
        let types = field.related_types();
        let foreign_key = field.foreign_key.as_deref();
        Some(match field.relation_kind()? {
            RelationKind::HasOne | RelationKind::HasMany => {
                let (related, foreign_key) = (types.first()?, foreign_key?);
                quote!(#outline::owning::<#related>(#name).keyed::<#related>(#foreign_key))
            }
            // A polymorphic key is no foreign key, so it is not required to be indexed.
            RelationKind::MorphOne | RelationKind::MorphMany => {
                let related = types.first()?;
                quote!(#outline::owning::<#related>(#name))
            }
            RelationKind::BelongsTo => {
                let (related, foreign_key) = (types.first()?, foreign_key?);
                quote!(#outline::referencing::<#related>(#name).keyed::<Self>(#foreign_key))
            }
            RelationKind::HasManyThrough => {
                let (related, pivot) = (types.first()?, types.get(1)?);
                let (foreign_key, related_key) = (foreign_key?, field.related_key.as_deref()?);
                quote! {
                    #outline::referencing::<#related>(#name)
                        .keyed::<#pivot>(#foreign_key)
                        .keyed::<#pivot>(#related_key)
                }
            }
            RelationKind::SelfRef | RelationKind::SelfRefMany => {
                let foreign_key = foreign_key.unwrap_or("parent_id");
                quote!(#outline::referencing::<Self>(#name).keyed::<Self>(#foreign_key))
            }
            RelationKind::MorphTo => quote!(#outline::morph_to(#name)),
        })
    });
    Some(quote! {
        fn __relations() -> Vec<::tideorm::rules::RelationOutline> {
            vec![#(#outlines),*]
        }
    })
}
