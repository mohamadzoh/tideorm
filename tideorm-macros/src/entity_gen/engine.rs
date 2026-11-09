//! The engine's side of a model, written for the model's own struct.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

use crate::context::BuildContext;
use crate::orm::derives::{EngineField, expand_active_model, expand_entity, expand_model};
use crate::parse::variant_ident;

/// The engine's side of the model, with relation and `skip` fields ignored (a
/// decoded row holds their `Default`): the `ActiveModel` struct, and the
/// impls naming the model (`EntityName`, `EntityTrait`, `ModelTrait`,
/// `FromQueryResult` and the `ActiveModel` conversions).
pub(super) fn engine_model_parts(ctx: &BuildContext) -> (TokenStream2, TokenStream2) {
    let model = &ctx.struct_name;
    let variants: Vec<_> = ctx
        .fields
        .iter()
        .map(|field| field.is_column().then(|| variant_ident(field.ident())))
        .collect();
    let fields: Vec<_> = ctx
        .fields
        .iter()
        .zip(&variants)
        .map(|(field, column)| EngineField {
            ident: field.ident(),
            ty: &field.ty,
            column: column.as_ref(),
            encrypted: ctx.is_encrypted(field),
        })
        .collect();
    let (active_model, active_model_impls) =
        expand_active_model(&nested_visibility(&ctx.vis), model, &fields);
    // The table and schema are declared on both sides: the engine's entity
    // qualifies the typed CRUD statements, `ModelMeta` the ones TideORM renders.
    let entity_impls = expand_entity(model, &ctx.table_name, ctx.schema_name.as_deref());
    let model_impls = expand_model(model, &fields);
    (
        active_model,
        quote!(#entity_impls #model_impls #active_model_impls),
    )
}

/// The model's visibility, written inside the hidden module one level below
/// it: the module's types name the model, so none may be more public than it
/// (E0446).
pub(super) fn nested_visibility(vis: &syn::Visibility) -> TokenStream2 {
    match vis {
        syn::Visibility::Public(_) => quote!(pub),
        syn::Visibility::Inherited => quote!(pub(super)),
        syn::Visibility::Restricted(restricted) => {
            let path = &restricted.path;
            if path.is_ident("crate") {
                quote!(pub(crate))
            } else if path.is_ident("self") {
                quote!(pub(super))
            } else if path
                .segments
                .first()
                .is_some_and(|segment| segment.ident == "crate")
            {
                quote!(pub(in #path))
            } else {
                quote!(pub(in super::#path))
            }
        }
    }
}
