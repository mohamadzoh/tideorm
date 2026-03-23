use convert_case::{Case, Casing};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Ident, Type};

use crate::meta_support::{ExistingDerives, pluralize};
use crate::parse::{IndexDef, ModelField, ModelInput, parse_validation_attributes};
use crate::relation_gen::{build_relation_field_inits, build_relation_state_refreshes};

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
    pub(crate) update_active_model_setters: Vec<TokenStream2>,
    pub(crate) relation_field_inits: Vec<TokenStream2>,
    pub(crate) relation_state_refreshes: Vec<TokenStream2>,
    pub(crate) internal_entity_mod: Ident,
    pub(crate) sea_orm_field_defs: Vec<TokenStream2>,
    pub(crate) all_field_names: Vec<Ident>,
    pub(crate) relation_field_defaults: Vec<TokenStream2>,
    pub(crate) default_field_inits: Vec<TokenStream2>,
    pub(crate) derive_field_names: Vec<Ident>,
    pub(crate) derive_field_names_str: Vec<String>,
    pub(crate) serde_field_names: Vec<Ident>,
    pub(crate) serde_field_names_str: Vec<String>,
    pub(crate) columns_struct_name: Ident,
    pub(crate) columns_struct_fields: Vec<TokenStream2>,
    pub(crate) columns_field_inits: Vec<TokenStream2>,
    pub(crate) index_impls: Vec<TokenStream2>,
    pub(crate) unique_index_impls: Vec<TokenStream2>,
    pub(crate) tokenize_enabled: bool,
    pub(crate) relation_fields: Vec<ModelField>,
    db_fields: Vec<ModelField>,
}

impl BuildContext {
    pub(crate) fn new(
        input: &ModelInput,
        indexes: Vec<IndexDef>,
        unique_indexes: Vec<IndexDef>,
        existing_derives: &ExistingDerives,
    ) -> syn::Result<Self> {
        let struct_name = input.ident.clone();
        let struct_name_str = struct_name.to_string();
        let table_name = input
            .table
            .clone()
            .unwrap_or_else(|| pluralize(&struct_name_str.to_case(Case::Snake)));
        let schema_name = input.schema.clone().unwrap_or_else(|| "public".to_string());
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

        validate_primary_key_fields(&db_fields, input.tokenize)?;
        validate_relation_fields(&relation_fields)?;

        let validation_rules = db_fields
            .iter()
            .filter_map(|field| {
                let field_name = field.ident.as_ref()?.to_string();
                let rules = parse_validation_attributes(&field_name, &field.attrs);
                (!rules.is_empty()).then_some((field_name, rules))
            })
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
            .map(|ident| format_ident!("{}", ident.to_string().to_case(Case::Pascal)))
            .collect();
        let column_type_defs = db_fields
            .iter()
            .filter_map(|field| {
                let variant = format_ident!(
                    "{}",
                    field.ident.as_ref()?.to_string().to_case(Case::Pascal)
                );
                let col_type_expr = field.column_type_expr();
                Some(quote!(Self::#variant => #col_type_expr))
            })
            .collect();
        let pk_column_variants: Vec<_> = pk_idents
            .iter()
            .map(|ident| format_ident!("{}", ident.to_string().to_case(Case::Pascal)))
            .collect();
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
            .unwrap_or_else(|| pk_ident.to_string().to_case(Case::Snake));
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
        let update_active_model_setters = build_update_active_model_setters(&db_fields);
        let internal_entity_mod =
            format_ident!("__tideorm_internal_{}", struct_name_str.to_lowercase());
        let sea_orm_field_defs = build_sea_orm_field_defs(&db_fields);
        let all_field_names = field_names.clone();
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
        let derive_field_names_str = derive_field_names.iter().map(ToString::to_string).collect();
        let serde_field_names = field_names.clone();
        let serde_field_names_str = serde_field_names.iter().map(ToString::to_string).collect();
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
            update_active_model_setters,
            relation_field_inits: Vec::new(),
            relation_state_refreshes: Vec::new(),
            internal_entity_mod,
            sea_orm_field_defs,
            all_field_names,
            relation_field_defaults,
            default_field_inits,
            derive_field_names,
            derive_field_names_str,
            serde_field_names,
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

    fn column_name(field: &ModelField) -> String {
        field.column.clone().unwrap_or_else(|| {
            field
                .ident
                .as_ref()
                .map(|ident| ident.to_string().to_case(Case::Snake))
                .unwrap_or_default()
        })
    }
}

fn split_csv(value: Option<&String>) -> Option<Vec<String>> {
    value.map(|value| {
        value
            .split(',')
            .map(|part| part.trim().to_string())
            .collect()
    })
}

fn validate_primary_key_fields(fields: &[ModelField], tokenize_enabled: bool) -> syn::Result<()> {
    let primary_key_fields: Vec<&ModelField> =
        fields.iter().filter(|field| field.primary_key).collect();

    if primary_key_fields.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "TideORM models require exactly one #[tideorm(primary_key)] field",
        ));
    }

    if primary_key_fields.len() > 1 {
        if tokenize_enabled {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "#[tideorm(tokenize)] requires exactly one #[tideorm(primary_key)] field",
            ));
        }

        if primary_key_fields.iter().any(|field| field.auto_increment) {
            let mut errors: Option<syn::Error> = None;
            for field in primary_key_fields
                .iter()
                .filter(|field| field.auto_increment)
            {
                let error = syn::Error::new_spanned(
                    field
                        .ident
                        .as_ref()
                        .expect("database fields must have identifiers"),
                    "composite primary keys do not support #[tideorm(auto_increment)]",
                );
                if let Some(existing) = &mut errors {
                    existing.combine(error);
                } else {
                    errors = Some(error);
                }
            }

            return Err(errors.expect("composite auto increment errors should exist"));
        }
    }

    for field in fields {
        if field.auto_increment && !field.primary_key {
            return Err(syn::Error::new_spanned(
                field
                    .ident
                    .as_ref()
                    .expect("database fields must have identifiers"),
                "#[tideorm(auto_increment)] requires #[tideorm(primary_key)] on the same field",
            ));
        }
    }

    Ok(())
}

fn validate_relation_fields(fields: &[ModelField]) -> syn::Result<()> {
    let mut errors: Option<syn::Error> = None;

    for field in fields {
        if field.has_many_through.is_some() {
            let mut missing = Vec::new();
            if field.pivot.is_none() {
                missing.push("pivot");
            }
            if field.foreign_key.is_none() {
                missing.push("foreign_key");
            }
            if field.related_key.is_none() {
                missing.push("related_key");
            }

            if !missing.is_empty() {
                let field_ident = field
                    .ident
                    .as_ref()
                    .expect("relation fields must have identifiers");
                let requirement_list = missing
                    .iter()
                    .map(|name| format!("#[tideorm({name} = \"...\")]"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let error = syn::Error::new_spanned(
                    field_ident,
                    format!("has_many_through relations require {}", requirement_list),
                );

                if let Some(existing) = &mut errors {
                    existing.combine(error);
                } else {
                    errors = Some(error);
                }
            }
        }
    }

    if let Some(error) = errors {
        return Err(error);
    }

    Ok(())
}

fn has_timestamp_pair(fields: &[ModelField]) -> bool {
    let has_created_at = fields.iter().any(|field| {
        field
            .ident
            .as_ref()
            .map(|ident| ident == "created_at")
            .unwrap_or(false)
    });
    let has_updated_at = fields.iter().any(|field| {
        field
            .ident
            .as_ref()
            .map(|ident| ident == "updated_at")
            .unwrap_or(false)
    });
    has_created_at && has_updated_at
}

fn build_sync_column_attrs(fields: &[ModelField]) -> Vec<TokenStream2> {
    fields
        .iter()
        .map(|field| {
            let mut attrs = Vec::new();
            if field.primary_key {
                attrs.push(quote!(col = col.primary_key();));
            }
            if field.auto_increment {
                attrs.push(quote!(col = col.auto_increment();));
            }
            let ty = &field.ty;
            let ty_str = quote!(#ty).to_string();
            if !field.nullable && !ty_str.contains("Option") {
                attrs.push(quote!(col = col.not_null();));
            }
            if let Some(default) = &field.default {
                attrs.push(quote!(col = col.default(#default);));
            }
            quote!(#(#attrs)*)
        })
        .collect()
}

fn build_insert_active_model_setters(fields: &[ModelField]) -> Vec<TokenStream2> {
    fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| {
            let field_name = ident.to_string();
            if field.primary_key && field.auto_increment {
                quote!(#ident: ActiveValue::NotSet)
            } else if field_name == "created_at" || field_name == "updated_at" {
                quote!(#ident: ActiveValue::Set(::tideorm::chrono::Utc::now()))
            } else {
                quote!(#ident: ActiveValue::Set(self.#ident))
            }
        })
        .collect()
}

fn build_update_active_model_setters(fields: &[ModelField]) -> Vec<TokenStream2> {
    fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| {
            let field_name = ident.to_string();
            if field.primary_key {
                quote!(#ident: ActiveValue::Unchanged(self.#ident))
            } else if field_name == "updated_at" {
                quote!(#ident: ActiveValue::Set(::tideorm::chrono::Utc::now()))
            } else {
                quote!(#ident: ActiveValue::Set(self.#ident))
            }
        })
        .collect()
}

fn build_sea_orm_field_defs(fields: &[ModelField]) -> Vec<TokenStream2> {
    fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| {
            let ty = &field.ty;
            let column_name = BuildContext::column_name(field);
            let mut attrs = Vec::new();
            if field.primary_key {
                attrs.push(quote!(primary_key));
            }
            if field.auto_increment {
                attrs.push(quote!(auto_increment));
            }
            attrs.push(quote!(column_name = #column_name));
            quote!(#[sea_orm(#(#attrs),*)] pub #ident: #ty)
        })
        .collect()
}

fn build_relation_field_defaults(fields: &[ModelField]) -> Vec<TokenStream2> {
    fields
        .iter()
        .filter_map(|field| field.ident.as_ref())
        .map(|ident| quote!(#ident: Default::default()))
        .collect()
}

fn build_columns_struct_fields(fields: &[ModelField]) -> Vec<TokenStream2> {
    fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (ident, &field.ty)))
        .map(|(ident, ty)| quote!(pub #ident: ::tideorm::columns::Column<#ty>))
        .collect()
}

fn build_columns_field_inits(fields: &[ModelField]) -> Vec<TokenStream2> {
    fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| {
            let column_name = BuildContext::column_name(field);
            quote!(#ident: ::tideorm::columns::Column::new(#column_name))
        })
        .collect()
}

fn build_index_impls(table_name: &str, indexes: &[IndexDef], unique: bool) -> Vec<TokenStream2> {
    indexes
        .iter()
        .map(|index| {
            let name = index.get_name(table_name);
            let columns = &index.columns;
            quote!(::tideorm::model::IndexDefinition::new(#name, vec![#(#columns.to_string()),*], #unique))
        })
        .collect()
}
