use super::*;

pub(super) fn build_primary_key_value_impl(ctx: &BuildContext) -> TokenStream2 {
    if ctx.pk_idents.len() == 1 {
        let pk_ident = &ctx.pk_ident;
        quote!(self.#pk_ident.clone())
    } else {
        let pk_idents = &ctx.pk_idents;
        quote!((#(self.#pk_idents.clone()),*))
    }
}

pub(super) fn build_primary_key_condition_impl(
    ctx: &BuildContext,
    internal_entity_mod: &syn::Ident,
) -> TokenStream2 {
    if ctx.pk_column_variants.len() == 1 {
        let pk_column_variant = &ctx.pk_column_variant;
        return quote! {
            use ::tideorm::orm::ColumnTrait;
            ::tideorm::orm::Condition::all()
                .add(#internal_entity_mod::Column::#pk_column_variant.eq(primary_key.clone()))
        };
    }

    let bindings: Vec<_> = (0..ctx.pk_column_variants.len())
        .map(|index| format_ident!("pk_{index}"))
        .collect();
    let pk_column_variants = &ctx.pk_column_variants;

    quote! {
        use ::tideorm::orm::ColumnTrait;
        let (#(#bindings),*) = primary_key.clone();
        ::tideorm::orm::Condition::all()
            #(.add(#internal_entity_mod::Column::#pk_column_variants.eq(#bindings)))*
    }
}

pub(super) fn build_pk_conflict_check(ctx: &BuildContext) -> TokenStream2 {
    let pk_column_names = &ctx.pk_column_names;
    quote! {
        conflict_cols.iter().any(|column| matches!(column.as_str(), #(#pk_column_names)|*))
    }
}

pub(super) fn build_pk_exclusion_check(
    ctx: &BuildContext,
    value_ident: TokenStream2,
) -> TokenStream2 {
    let pk_column_names = &ctx.pk_column_names;
    quote! {
        matches!(#value_ident.as_str(), #(#pk_column_names)|*)
    }
}
