use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;

use crate::context::BuildContext;
use crate::parse::ModelField;

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
    if field.has_one.is_some() {
        let fk = field.foreign_key.as_deref().unwrap_or("id");
        let lk = field.local_key.as_deref().unwrap_or("id");
        let lk_ident = ctx.resolve_local_key_ident(lk, ident)?;
        return Ok(quote! {
            self.#ident = ::tideorm::relations::HasOne::new(#fk, #lk)
                .with_parent_pk(::serde_json::json!(self.#lk_ident.clone()))
        });
    }

    if field.has_many.is_some() {
        let fk = field.foreign_key.as_deref().unwrap_or("id");
        let lk = field.local_key.as_deref().unwrap_or("id");
        let lk_ident = ctx.resolve_local_key_ident(lk, ident)?;
        return Ok(quote! {
            self.#ident = ::tideorm::relations::HasMany::new(#fk, #lk)
                .with_parent_pk(::serde_json::json!(self.#lk_ident.clone()))
        });
    }

    if field.belongs_to.is_some() {
        let fk = field.foreign_key.as_deref().unwrap_or("id");
        let ok = field.owner_key.as_deref().unwrap_or("id");
        let fk_ident = ctx.resolve_required_db_field_ident(fk, ident)?;
        return Ok(quote! {
            self.#ident = ::tideorm::relations::BelongsTo::new(#fk, #ok)
                .with_fk_value(::serde_json::json!(self.#fk_ident.clone()))
        });
    }

    if field.has_many_through.is_some() {
        let fk = field.foreign_key.as_deref().unwrap_or("id");
        let related_key = field.related_key.as_deref().unwrap_or("id");
        let local_key = field.local_key.as_deref().unwrap_or("id");
        let related_local_key = field.owner_key.as_deref().unwrap_or("id");
        let local_key_ident = ctx.resolve_local_key_ident(local_key, ident)?;
        let pivot_table = field.pivot.as_deref().unwrap_or("");
        return Ok(quote! {
            self.#ident = ::tideorm::relations::HasManyThrough::new(
                #fk,
                #related_key,
                #local_key,
                #related_local_key,
                #pivot_table,
            )
            .with_parent_pk(::serde_json::json!(self.#local_key_ident.clone()))
        });
    }
    Ok(quote! {
        self.#ident = Default::default()
    })
}
