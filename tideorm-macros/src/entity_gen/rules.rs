//! The model rules checked in `const` items: the ones needing a field's type,
//! which only the compiler knows, and the ones reading other models through
//! the `tideorm::rules::ModelNode` every model writes, whatever its rules. A
//! broken rule panics in its `const`, which `cargo check` reports as an error
//! at the model, field or relation breaking it.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};

use crate::context::BuildContext;
use crate::parse::{ModelField, RelationKind};

pub(super) fn generate_rule_checks(ctx: &BuildContext) -> TokenStream2 {
    let node = model_node(ctx);
    let column_checks = column_checks(ctx);
    let relation_checks = relation_checks(ctx);
    quote! {
        #node
        #(#column_checks)*
        #(#relation_checks)*
    }
}

/// The model's `ModelNode`, in a `static` its `__rules_node()` returns: a
/// `const fn`, so models whose relations lead to each other may name each
/// other's node.
fn model_node(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let name = &ctx.struct_name_str;
    let table = match &ctx.schema_name {
        Some(schema) => format!("{schema}.{}", ctx.table_name),
        None => ctx.table_name.clone(),
    };
    let owned = ctx
        .relation_fields
        .iter()
        .filter(|field| {
            matches!(
                field.relation_kind(),
                Some(
                    RelationKind::HasOne
                        | RelationKind::HasMany
                        | RelationKind::MorphOne
                        | RelationKind::MorphMany
                )
            )
        })
        .filter_map(|field| field.related_types().into_iter().next());
    let leading_columns = leading_columns(ctx);
    let indexes_foreign_keys = ctx.project_rules.limits.require_foreign_key_index;
    quote! {
        const _: () = {
            static NODE: ::tideorm::rules::ModelNode = ::tideorm::rules::ModelNode {
                id: ::tideorm::rules::__model_id(::core::concat!(
                    ::core::module_path!(), "::", #name, " ",
                    ::core::file!(), ":", ::core::line!(), ":", ::core::column!()
                )),
                name: #name,
                table: #table,
                owned: &[#(<#owned>::__rules_node()),*],
                leading_columns: &[#(#leading_columns),*],
                indexes_foreign_keys: #indexes_foreign_keys,
            };

            impl #struct_name {
                #[doc(hidden)]
                pub const fn __rules_node() -> &'static ::tideorm::rules::ModelNode {
                    &NODE
                }
            }
        };
    }
}

/// The columns the primary key and each index lead with, each also under its
/// field's name, which a relation's `foreign_key` may name instead.
fn leading_columns(ctx: &BuildContext) -> Vec<String> {
    let mut leading = Vec::new();
    let mut lead = |field: Option<&ModelField>, written: &str| match field {
        Some(field) => {
            leading.push(field.column_name());
            leading.push(field.name());
        }
        None => leading.push(written.to_string()),
    };
    if let Some(key) = ctx.db_fields.iter().find(|field| field.primary_key) {
        lead(Some(key), "");
    }
    for index in &ctx.indexes {
        if let Some((column, field)) = index.resolved_columns(&ctx.db_fields).next() {
            lead(field, column);
        }
    }
    leading.sort();
    leading.dedup();
    leading
}

/// The column rules reading a field's `ValueType::COLUMN_KIND`.
fn column_checks(ctx: &BuildContext) -> Vec<TokenStream2> {
    let limits = &ctx.project_rules.limits;
    let name = &ctx.struct_name_str;
    let kind = |field: &ModelField| {
        let ty = field.stored_type(ctx.is_encrypted(field));
        quote!(<#ty as ::tideorm::orm::sql_builder::ValueType>::COLUMN_KIND)
    };
    let mut checks = Vec::new();
    if let Some(max) = limits.max_json_columns {
        let kinds = ctx.db_fields.iter().map(kind);
        let max = max as usize;
        checks.push(fail_on(
            ctx.struct_name.span(),
            quote!(::tideorm::rules::__check_json_columns(#name, &[#(#kinds),*], #max)),
        ));
    }
    for field in &ctx.db_fields {
        let column = field.column_name();
        let kind = kind(field);
        if limits.forbid_float_columns {
            checks.push(fail_on(
                field.ident().span(),
                quote!(::tideorm::rules::__check_float_column(#name, #column, #kind)),
            ));
        }
        if field.primary_key && !limits.primary_key_types.is_empty() {
            let allowed = &limits.primary_key_types;
            checks.push(fail_on(
                field.ident().span(),
                quote!(::tideorm::rules::__check_key_type(#name, #column, #kind, &[#(#allowed),*])),
            ));
        }
    }
    checks
}

/// The relation rules: depth and cycles from this model, and that every
/// foreign key its relations name leads an index of the model holding it.
fn relation_checks(ctx: &BuildContext) -> Vec<TokenStream2> {
    let limits = &ctx.project_rules.limits;
    let struct_name = &ctx.struct_name;
    let model = ctx.struct_name.span();
    let mut checks = Vec::new();
    if limits.forbid_relation_cycles {
        checks.push(fail_on(
            model,
            quote!(::tideorm::rules::__check_relation_cycles(<#struct_name>::__rules_node())),
        ));
    }
    if let Some(max) = limits.max_relation_depth {
        let max = max as usize;
        checks.push(fail_on(
            model,
            quote!(::tideorm::rules::__check_relation_depth(<#struct_name>::__rules_node(), #max)),
        ));
    }
    for field in &ctx.relation_fields {
        for (holder, column) in foreign_keys(ctx, field) {
            checks.push(fail_on(
                field.ident().span(),
                quote!(::tideorm::rules::__check_foreign_key(<#holder>::__rules_node(), #column)),
            ));
        }
    }
    checks
}

/// A `const` running `check`, which panics at `span` with the message of the
/// rule `check` finds broken: in a `const` a panic is a compile error.
fn fail_on(span: Span, check: TokenStream2) -> TokenStream2 {
    quote_spanned! {span=>
        const _: () = if let ::core::option::Option::Some(broken) = #check {
            ::core::panic!("{}", broken.as_str())
        };
    }
}

/// Each model holding one of `field`'s foreign keys, and the key's column.
/// A polymorphic key is no foreign key, so it is not required to be indexed.
fn foreign_keys(ctx: &BuildContext, field: &ModelField) -> Vec<(TokenStream2, String)> {
    let struct_name = &ctx.struct_name;
    let types = field.related_types();
    let foreign_key = field.foreign_key.clone();
    match field.relation_kind() {
        Some(RelationKind::HasOne | RelationKind::HasMany) => types
            .first()
            .zip(foreign_key)
            .map(|(related, key)| (quote!(#related), key))
            .into_iter()
            .collect(),
        Some(RelationKind::BelongsTo) => foreign_key
            .map(|key| (quote!(#struct_name), key))
            .into_iter()
            .collect(),
        Some(RelationKind::HasManyThrough) => {
            let Some(pivot) = types.get(1) else {
                return Vec::new();
            };
            foreign_key
                .into_iter()
                .chain(field.related_key.clone())
                .map(|key| (quote!(#pivot), key))
                .collect()
        }
        Some(RelationKind::SelfRef | RelationKind::SelfRefMany) => {
            let key = foreign_key.unwrap_or_else(|| "parent_id".to_string());
            vec![(quote!(#struct_name), key)]
        }
        _ => Vec::new(),
    }
}
