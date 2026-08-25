use convert_case::{Case, Casing};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Ident, Type};

mod helpers;

use crate::meta_support::{ExistingDerives, pluralize};
use crate::parse::{
    IndexDef, ModelField, ModelInput, column_variant_ident, parse_validation_attributes,
    relation_wrapper_name, unraw_ident,
};
use crate::relation_gen::{build_relation_field_inits, build_relation_state_refreshes};
use helpers::*;

pub(crate) struct BuildContext {
    pub(crate) struct_name: Ident,
    pub(crate) struct_name_str: String,
    pub(crate) table_name: String,
    pub(crate) schema_name: String,
    pub(crate) soft_delete_enabled: bool,
    pub(crate) soft_delete_field_ident: Option<Ident>,
    pub(crate) soft_delete_column_name: Option<String>,
    pub(crate) should_gen_debug: bool,
    pub(crate) should_gen_clone: bool,
    pub(crate) should_gen_default: bool,
    pub(crate) should_gen_serialize: bool,
    pub(crate) should_gen_deserialize: bool,
    pub(crate) hidden_attrs: Vec<String>,
    pub(crate) translatable_fields: Vec<String>,
    pub(crate) encrypted_fields: Vec<String>,
    pub(crate) encrypted_column_names: Vec<String>,
    pub(crate) has_custom_languages: bool,
    pub(crate) allowed_languages: Vec<String>,
    pub(crate) has_custom_fallback: bool,
    pub(crate) fallback_language: String,
    pub(crate) has_one_files: Vec<String>,
    pub(crate) has_many_files: Vec<String>,
    pub(crate) searchable_fields: Vec<String>,
    pub(crate) validation_rules: Vec<(String, Vec<TokenStream2>)>,
    pub(crate) pk_ident: Ident,
    pub(crate) pk_idents: Vec<Ident>,
    pub(crate) pk_type: Type,
    pub(crate) field_names: Vec<Ident>,
    pub(crate) field_types: Vec<Type>,
    pub(crate) column_names: Vec<String>,
    pub(crate) column_variants: Vec<Ident>,
    pub(crate) column_type_defs: Vec<TokenStream2>,
    pub(crate) pk_column_variant: Ident,
    pub(crate) pk_column_variants: Vec<Ident>,
    pub(crate) pk_column_name: String,
    pub(crate) pk_column_names: Vec<String>,
    pub(crate) pk_auto_increment: bool,
    pub(crate) timestamps_enabled: bool,
    pub(crate) sync_column_attrs: Vec<TokenStream2>,
    pub(crate) insert_active_model_setters: Vec<TokenStream2>,
    pub(crate) relation_field_inits: Vec<TokenStream2>,
    pub(crate) relation_state_refreshes: Vec<TokenStream2>,
    pub(crate) internal_entity_mod: Ident,
    pub(crate) sea_orm_field_defs: Vec<TokenStream2>,
    pub(crate) relation_field_defaults: Vec<TokenStream2>,
    pub(crate) default_field_inits: Vec<TokenStream2>,
    pub(crate) derive_field_names: Vec<Ident>,
    pub(crate) derive_field_names_str: Vec<String>,
    pub(crate) serde_field_names: Vec<Ident>,
    pub(crate) serde_field_types: Vec<Type>,
    pub(crate) serde_field_names_str: Vec<String>,
    pub(crate) columns_struct_name: Ident,
    pub(crate) columns_struct_fields: Vec<TokenStream2>,
    pub(crate) columns_field_inits: Vec<TokenStream2>,
    pub(crate) index_impls: Vec<TokenStream2>,
    pub(crate) unique_index_impls: Vec<TokenStream2>,
    pub(crate) tokenize_enabled: bool,
    pub(crate) relation_fields: Vec<ModelField>,
    pub(crate) db_fields: Vec<ModelField>,
}

impl BuildContext {
    pub(crate) fn new(
        input: &ModelInput,
        indexes: Vec<IndexDef>,
        unique_indexes: Vec<IndexDef>,
        existing_derives: &ExistingDerives,
    ) -> syn::Result<Self> {
        let struct_name = input.ident.clone();
        let struct_name_str = unraw_ident(&struct_name);
        let table_name = input
            .table
            .clone()
            .unwrap_or_else(|| pluralize(&struct_name_str.to_case(Case::Snake)));
        let schema_name = input.schema.clone().unwrap_or_else(|| "public".to_string());
        // `deleted_at_column` only means anything alongside `soft_delete`; without it the
        // model silently compiles with hard-delete semantics and the override is dropped.
        if input.deleted_at_column.is_some() && !input.soft_delete {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "#[tideorm(deleted_at_column = \"...\")] has no effect without #[tideorm(soft_delete)]; \
                 add soft_delete, or drop deleted_at_column",
            ));
        }
        let soft_delete_key = input.deleted_at_column.as_deref().unwrap_or("deleted_at");
        let should_gen_debug =
            !input.skip_derives && !input.skip_debug && !existing_derives.has_debug;
        let should_gen_clone =
            !input.skip_derives && !input.skip_clone && !existing_derives.has_clone;
        let should_gen_default =
            !input.skip_derives && !input.skip_default && !existing_derives.has_default;
        let should_gen_serialize =
            !input.skip_derives && !input.skip_serialize && !existing_derives.has_serialize;
        let should_gen_deserialize =
            !input.skip_derives && !input.skip_deserialize && !existing_derives.has_deserialize;
        let hidden_attrs =
            split_csv(input.hidden.as_ref()).unwrap_or_else(|| vec!["deleted_at".to_string()]);
        let translatable_fields = split_csv(input.translatable.as_ref()).unwrap_or_default();
        let encrypted = split_csv(input.encrypted.as_ref()).unwrap_or_default();
        let has_custom_languages = input.languages.is_some();
        let allowed_languages = split_csv(input.languages.as_ref()).unwrap_or_default();
        let has_custom_fallback = input.fallback_language.is_some();
        let fallback_language = input.fallback_language.clone().unwrap_or_default();
        let has_one_files = split_csv(input.has_one_files.as_ref()).unwrap_or_default();
        let has_many_files = split_csv(input.has_many_files.as_ref()).unwrap_or_default();
        let searchable_fields = split_csv(input.searchable.as_ref()).unwrap_or_default();

        let fields = match &input.data {
            darling::ast::Data::Struct(fields) => fields,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input.ident,
                    "Model can only be derived for structs with named fields",
                ));
            }
        };

        let db_fields: Vec<ModelField> = fields
            .iter()
            .filter(|field| !field.skip && !field.is_relation() && !field.is_relation_type())
            .cloned()
            .collect();
        let relation_fields: Vec<ModelField> = fields
            .iter()
            .filter(|field| field.is_relation() || field.is_relation_type())
            .cloned()
            .collect();

        validate_primary_key_fields(&input.ident, &db_fields, input.tokenize)?;
        validate_relation_fields(&relation_fields)?;
        validate_index_definitions(&indexes, &unique_indexes, &db_fields)?;

        let resolved_encrypted_fields =
            resolve_encrypted_fields(&input.ident, &db_fields, &encrypted)?;
        let encrypted_fields = resolved_encrypted_fields
            .iter()
            .filter_map(|field| field.ident.as_ref())
            .map(unraw_ident)
            .collect();
        let encrypted_column_names = resolved_encrypted_fields
            .iter()
            .map(|field| Self::column_name(field))
            .collect();

        let validation_rules = db_fields
            .iter()
            .filter_map(|field| {
                let field_name = unraw_ident(field.ident.as_ref()?);
                Some((field_name, field))
            })
            .map(|(field_name, field)| {
                parse_validation_attributes(&field_name, field).map(|rules| (field_name, rules))
            })
            .collect::<syn::Result<Vec<_>>>()?
            .into_iter()
            .filter(|(_, rules)| !rules.is_empty())
            .collect();

        let pk_fields: Vec<_> = db_fields.iter().filter(|field| field.primary_key).collect();
        let pk_field = pk_fields.first().copied();
        let pk_ident = pk_field
            .and_then(|field| field.ident.as_ref())
            .cloned()
            .unwrap_or_else(|| format_ident!("id"));
        let pk_idents: Vec<_> = pk_fields
            .iter()
            .filter_map(|field| field.ident.as_ref().cloned())
            .collect();
        let pk_types: Vec<_> = pk_fields.iter().map(|field| field.ty.clone()).collect();
        let pk_type = if pk_types.len() == 1 {
            pk_types[0].clone()
        } else {
            syn::parse2(quote!((#(#pk_types),*)))?
        };

        let field_names: Vec<_> = db_fields
            .iter()
            .filter_map(|field| field.ident.as_ref().cloned())
            .collect();
        let field_types: Vec<_> = db_fields.iter().map(|field| field.ty.clone()).collect();
        let column_names: Vec<_> = db_fields.iter().map(Self::column_name).collect();
        let column_variants: Vec<_> = db_fields
            .iter()
            .filter_map(|field| field.ident.as_ref())
            .map(column_variant_ident)
            .collect();
        let column_type_defs = db_fields
            .iter()
            .filter_map(|field| {
                let variant = column_variant_ident(field.ident.as_ref()?);
                let col_type_expr = field.column_type_expr();
                Some(quote!(Self::#variant => #col_type_expr))
            })
            .collect();
        let pk_column_variants: Vec<_> = pk_idents.iter().map(column_variant_ident).collect();
        let pk_column_variant = pk_column_variants
            .first()
            .cloned()
            .unwrap_or_else(|| format_ident!("Id"));
        let pk_column_names: Vec<_> = pk_fields
            .iter()
            .map(|field| Self::column_name(field))
            .collect();
        let pk_column_name = pk_column_names
            .first()
            .cloned()
            .unwrap_or_else(|| unraw_ident(&pk_ident).to_case(Case::Snake));
        let pk_auto_increment =
            pk_fields.len() == 1 && pk_field.map(|field| field.auto_increment).unwrap_or(false);
        let (soft_delete_field_ident, soft_delete_column_name) = if input.soft_delete {
            let field = db_fields
                .iter()
                .find(|field| {
                    field.ident.as_ref().is_some_and(|ident| ident == soft_delete_key)
                        || Self::column_name(field) == soft_delete_key
                })
                .ok_or_else(|| {
                    syn::Error::new_spanned(
                        &input.ident,
                        format!(
                            "soft_delete requires a field or column named '{}'; set #[tideorm(deleted_at_column = \"...\")] to override",
                            soft_delete_key
                        ),
                    )
                })?;

            let ty = &field.ty;
            let ty_str = quote!(#ty).to_string().replace(' ', "");
            let is_supported_type = matches!(
                ty_str.as_str(),
                "Option<DateTime<Utc>>"
                    | "Option<DateTime<chrono::Utc>>"
                    | "Option<chrono::DateTime<Utc>>"
                    | "Option<chrono::DateTime<chrono::Utc>>"
            );

            if !is_supported_type {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    "soft_delete field must have type Option<chrono::DateTime<chrono::Utc>>",
                ));
            }

            (field.ident.clone(), Some(Self::column_name(field)))
        } else {
            (None, None)
        };
        let timestamps_enabled = input.timestamps || has_timestamp_pair(&db_fields);
        let sync_column_attrs = build_sync_column_attrs(&db_fields);
        let insert_active_model_setters = build_insert_active_model_setters(&db_fields);
        let internal_entity_mod = format_ident!(
            "__tideorm_internal_{}",
            struct_name_str.to_case(Case::Snake)
        );
        let sea_orm_field_defs = build_sea_orm_field_defs(&db_fields);
        let relation_field_defaults = build_relation_field_defaults(&relation_fields);
        let default_field_inits = fields
            .iter()
            .filter_map(|field| field.ident.as_ref())
            .map(|ident| quote!(#ident: Default::default()))
            .collect();
        let derive_field_names: Vec<_> = fields
            .iter()
            .filter_map(|field| field.ident.as_ref().cloned())
            .collect();
        let derive_field_names_str = derive_field_names.iter().map(unraw_ident).collect();
        let serde_field_names: Vec<_> = fields
            .iter()
            .filter_map(|field| field.ident.as_ref().cloned())
            .collect();
        let serde_field_types = fields.iter().map(|field| field.ty.clone()).collect();
        let serde_field_names_str = serde_field_names.iter().map(unraw_ident).collect();
        let columns_struct_name = format_ident!("{}Columns", struct_name);
        let columns_struct_fields = build_columns_struct_fields(&db_fields);
        let columns_field_inits = build_columns_field_inits(&db_fields);
        let index_impls = build_index_impls(&table_name, &indexes, false);
        let unique_index_impls = build_index_impls(&table_name, &unique_indexes, true);

        let mut ctx = Self {
            struct_name,
            struct_name_str,
            table_name,
            schema_name,
            soft_delete_enabled: input.soft_delete,
            soft_delete_field_ident,
            soft_delete_column_name,
            should_gen_debug,
            should_gen_clone,
            should_gen_default,
            should_gen_serialize,
            should_gen_deserialize,
            hidden_attrs,
            translatable_fields,
            encrypted_fields,
            encrypted_column_names,
            has_custom_languages,
            allowed_languages,
            has_custom_fallback,
            fallback_language,
            has_one_files,
            has_many_files,
            searchable_fields,
            validation_rules,
            pk_ident,
            pk_idents,
            pk_type,
            field_names,
            field_types,
            column_names,
            column_variants,
            column_type_defs,
            pk_column_variant,
            pk_column_variants,
            pk_column_name,
            pk_column_names,
            pk_auto_increment,
            timestamps_enabled,
            sync_column_attrs,
            insert_active_model_setters,
            relation_field_inits: Vec::new(),
            relation_state_refreshes: Vec::new(),
            internal_entity_mod,
            sea_orm_field_defs,
            relation_field_defaults,
            default_field_inits,
            derive_field_names,
            derive_field_names_str,
            serde_field_names,
            serde_field_types,
            serde_field_names_str,
            columns_struct_name,
            columns_struct_fields,
            columns_field_inits,
            index_impls,
            unique_index_impls,
            tokenize_enabled: input.tokenize,
            relation_fields: relation_fields.clone(),
            db_fields,
        };
        ctx.relation_field_inits = build_relation_field_inits(&ctx, &relation_fields)?;
        ctx.relation_state_refreshes = build_relation_state_refreshes(&ctx, &relation_fields)?;
        Ok(ctx)
    }

    pub(crate) fn allowed_languages_impl(&self) -> TokenStream2 {
        if self.has_custom_languages {
            let allowed_languages = &self.allowed_languages;
            quote!(vec![#(#allowed_languages.to_string()),*])
        } else {
            quote!(::tideorm::config::Config::get_languages())
        }
    }

    pub(crate) fn fallback_language_impl(&self) -> TokenStream2 {
        if self.has_custom_fallback {
            let fallback_language = &self.fallback_language;
            quote!(#fallback_language.to_string())
        } else {
            quote!(::tideorm::config::Config::get_fallback_language())
        }
    }

    pub(crate) fn resolve_required_db_field_ident(
        &self,
        key: &str,
        relation_ident: &Ident,
    ) -> syn::Result<Ident> {
        self.db_fields
            .iter()
            .find_map(|field| {
                let ident = field.ident.as_ref()?;
                let column_name = Self::column_name(field);
                if ident == key || column_name == key {
                    Some(ident.clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                syn::Error::new_spanned(
                    relation_ident,
                    format!("relation references unknown field or column '{}'", key),
                )
            })
    }

    pub(crate) fn resolve_local_key_ident(
        &self,
        key: &str,
        relation_ident: &Ident,
    ) -> syn::Result<Ident> {
        if let Some(ident) = self.db_fields.iter().find_map(|field| {
            let ident = field.ident.as_ref()?;
            let column_name = Self::column_name(field);
            if ident == key || column_name == key {
                Some(ident.clone())
            } else {
                None
            }
        }) {
            Ok(ident)
        } else if key == "id" {
            if self.pk_idents.len() == 1 {
                Ok(self.pk_ident.clone())
            } else {
                Err(syn::Error::new_spanned(
                    relation_ident,
                    "composite primary keys require an explicit relation local_key; implicit 'id' is ambiguous",
                ))
            }
        } else {
            Err(syn::Error::new_spanned(
                relation_ident,
                format!("relation references unknown local_key '{}'", key),
            ))
        }
    }

    pub(crate) fn column_name(field: &ModelField) -> String {
        field.column.clone().unwrap_or_else(|| {
            field
                .ident
                .as_ref()
                .map(|ident| unraw_ident(ident).to_case(Case::Snake))
                .unwrap_or_default()
        })
    }
}

/// Rejects malformed `#[index(..)]` / `#[unique_index(..)]` attributes and index
/// definitions that reference columns the model does not declare.
fn validate_index_definitions(
    indexes: &[IndexDef],
    unique_indexes: &[IndexDef],
    db_fields: &[ModelField],
) -> syn::Result<()> {
    let mut errors: Option<syn::Error> = None;

    for index in indexes.iter().chain(unique_indexes) {
        if let Some(error) = &index.error {
            combine_error(&mut errors, error.clone());
            continue;
        }

        for column in &index.columns {
            if !column_exists(db_fields, column) {
                combine_error(&mut errors, unknown_column_error(index, column));
            }
        }
    }

    match errors {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn column_exists(db_fields: &[ModelField], column: &str) -> bool {
    db_fields.iter().any(|field| {
        let matches_field = match field.ident.as_ref() {
            Some(ident) => unraw_ident(ident) == column,
            None => false,
        };
        matches_field || BuildContext::column_name(field) == column
    })
}

fn unknown_column_error(index: &IndexDef, column: &str) -> syn::Error {
    let attribute = index.attribute_name();
    let message = format!("#[{attribute}(..)] references unknown field or column '{column}'");
    syn::Error::new(index.span, message)
}

fn combine_error(errors: &mut Option<syn::Error>, error: syn::Error) {
    match errors {
        Some(existing) => existing.combine(error),
        None => *errors = Some(error),
    }
}
