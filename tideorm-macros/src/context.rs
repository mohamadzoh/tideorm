use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Ident, Type};

mod helpers;

use crate::case::to_snake_case;
use crate::meta_support::{ExistingDerives, has_timestamp_pair, pluralize};
use crate::parse::{
    IndexDef, ModelField, ModelInput, find_db_field, parse_validation_attributes, unraw_ident,
    variant_ident,
};
use crate::relation_gen::build_relation_wiring;
use helpers::*;

pub(crate) struct BuildContext {
    pub(crate) struct_name: Ident,
    pub(crate) struct_name_str: String,
    pub(crate) table_name: String,
    /// The schema from `#[tideorm(schema = "..")]`; every statement then
    /// qualifies the table with it.
    pub(crate) schema_name: Option<String>,
    /// The soft-delete timestamp field and its column, when `soft_delete` is on.
    pub(crate) soft_delete: Option<(Ident, String)>,
    pub(crate) should_gen_debug: bool,
    pub(crate) should_gen_clone: bool,
    pub(crate) should_gen_default: bool,
    pub(crate) should_gen_serialize: bool,
    pub(crate) should_gen_deserialize: bool,
    /// Whether serializing the model and reading the JSON back reproduces
    /// every stored field: always for TideORM's own serde impls, and for a
    /// user's derive unless its attributes skip or convert a stored field.
    pub(crate) serde_round_trips: bool,
    pub(crate) hidden_attrs: Vec<String>,
    pub(crate) translatable_fields: Vec<String>,
    pub(crate) encrypted_fields: Vec<String>,
    pub(crate) encrypted_column_names: Vec<String>,
    /// `languages = ".."`; without it the configured languages apply.
    pub(crate) allowed_languages: Option<Vec<String>>,
    /// `fallback_language = ".."`; without it the configured fallback applies.
    pub(crate) fallback_language: Option<String>,
    pub(crate) has_one_files: Vec<String>,
    pub(crate) has_many_files: Vec<String>,
    pub(crate) searchable_fields: Vec<String>,
    /// `#[validate(..)]` rules, per field that declares any: columns and `skip`
    /// fields alike, since a rule only reads the field's value.
    pub(crate) validation_rules: Vec<(Ident, Vec<TokenStream2>)>,
    /// Errors in attributes the rest of the model does not depend on, reported
    /// beside the generated code rather than instead of it, so a typo in one
    /// rule is the only error and not the first of a page of missing impls.
    pub(crate) attribute_errors: Vec<syn::Error>,
    pub(crate) pk_ident: Ident,
    pub(crate) pk_idents: Vec<Ident>,
    pub(crate) pk_type: Type,
    pub(crate) pk_column_variants: Vec<Ident>,
    pub(crate) pk_column_names: Vec<String>,
    pub(crate) pk_auto_increment: bool,
    /// The persisted fields' identifiers, in declaration order.
    pub(crate) field_idents: Vec<Ident>,
    /// The persisted fields' names: what serde, `ModelMeta::field_names` and
    /// every name lookup use, so a `r#type` field is `"type"` everywhere.
    pub(crate) field_names: Vec<String>,
    pub(crate) field_types: Vec<Type>,
    /// Each persisted field's column definition.
    pub(crate) column_types: Vec<TokenStream2>,
    /// `(field name, key)` for each field the model's own `Serialize` derive
    /// writes under another key.
    pub(crate) serialized_names: Vec<(String, String)>,
    pub(crate) column_names: Vec<String>,
    pub(crate) column_variants: Vec<Ident>,
    pub(crate) timestamps_enabled: bool,
    /// The statements `__wire_relations` runs, one block per relation.
    pub(crate) relation_wiring: Vec<TokenStream2>,
    pub(crate) internal_entity_mod: Ident,
    pub(crate) columns_struct_name: Ident,
    pub(crate) index_impls: Vec<TokenStream2>,
    pub(crate) unique_index_impls: Vec<TokenStream2>,
    pub(crate) tokenize_enabled: bool,
    /// Every field of the struct, in declaration order.
    pub(crate) fields: Vec<ModelField>,
    pub(crate) relation_fields: Vec<ModelField>,
    pub(crate) db_fields: Vec<ModelField>,
}

/// The fields a user-supplied `Serialize` derive writes under another key.
fn renamed_fields(input: &ModelInput, fields: &[ModelField]) -> Vec<(String, String)> {
    let rule = crate::serde_names::rename_all(&input.attrs);
    fields
        .iter()
        .filter_map(|field| {
            let name = field.name();
            let key = crate::serde_names::field_key(&field.attrs, &name, rule)?;
            (key != name).then_some((name, key))
        })
        .collect()
}

impl BuildContext {
    /// The key `field` is serialized under.
    pub(crate) fn serialized_key(&self, field: &ModelField) -> String {
        let name = field.name();
        self.serialized_names
            .iter()
            .find(|(field_name, _)| *field_name == name)
            .map_or(name, |(_, key)| key.clone())
    }

    pub(crate) fn new(
        input: &ModelInput,
        indexes: Vec<IndexDef>,
        existing_derives: &ExistingDerives,
    ) -> syn::Result<Self> {
        let struct_name = input.ident.clone();
        let struct_name_str = unraw_ident(&struct_name);
        let table_name = input
            .table
            .clone()
            .unwrap_or_else(|| pluralize(&to_snake_case(&struct_name_str)));
        let schema_name = input.schema.clone();
        // `deleted_at_column` only means anything alongside `soft_delete`; without it the
        // model silently compiles with hard-delete semantics and the override is dropped.
        if input.deleted_at_column.is_some() && !input.soft_delete {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "#[tideorm(deleted_at_column = \"...\")] has no effect without #[tideorm(soft_delete)]; \
                 add soft_delete, or drop deleted_at_column",
            ));
        }
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
        let list = |attribute: &str, value: Option<&String>| {
            split_csv(&input.ident, attribute, value).map(Option::unwrap_or_default)
        };
        let translatable = list("translatable", input.translatable.as_ref())?;
        let encrypted = list("encrypted", input.encrypted.as_ref())?;
        let has_one_files = list("has_one_files", input.has_one_files.as_ref())?;
        let has_many_files = list("has_many_files", input.has_many_files.as_ref())?;
        let searchable = list("searchable", input.searchable.as_ref())?;

        let fields: Vec<ModelField> = match &input.data {
            darling::ast::Data::Struct(fields) => fields.iter().cloned().collect(),
            _ => {
                return Err(syn::Error::new_spanned(
                    &input.ident,
                    "Model can only be derived for structs with named fields",
                ));
            }
        };

        let db_fields: Vec<ModelField> = fields
            .iter()
            .filter(|field| field.is_column())
            .cloned()
            .collect();
        let relation_fields: Vec<ModelField> = fields
            .iter()
            .filter(|field| field.relation_kind().is_some())
            .cloned()
            .collect();

        validate_primary_key_fields(&input.ident, &db_fields, input.tokenize)?;
        validate_relation_fields(&fields)?;
        validate_index_definitions(&indexes, &db_fields)?;

        let resolved_encrypted_fields =
            resolve_encrypted_fields(&input.ident, &db_fields, &encrypted)?;
        let encrypted_fields = resolved_encrypted_fields
            .iter()
            .map(|field| field.name())
            .collect();
        let encrypted_column_names = resolved_encrypted_fields
            .iter()
            .map(|field| field.column_name())
            .collect();
        let field_names_of = |attribute: &str, names: &[String]| {
            resolve_field_list(&input.ident, attribute, &db_fields, names)
                .map(|fields| fields.into_iter().map(ModelField::name).collect::<Vec<_>>())
        };
        let translatable_fields = field_names_of("translatable", &translatable)?;
        let searchable_fields = field_names_of("searchable", &searchable)?;

        let mut validation_rules = Vec::new();
        let mut attribute_errors = Vec::new();
        for field in &fields {
            match parse_validation_attributes(field) {
                Ok(rules) if !rules.is_empty() => {
                    validation_rules.push((field.ident().clone(), rules));
                }
                Ok(_) => {}
                Err(error) => attribute_errors.push(error),
            }
        }

        let pk_fields: Vec<&ModelField> =
            db_fields.iter().filter(|field| field.primary_key).collect();
        let pk_idents: Vec<Ident> = pk_fields
            .iter()
            .map(|field| field.ident().clone())
            .collect();
        let pk_ident = pk_idents[0].clone();
        let pk_type = if let [pk_field] = pk_fields.as_slice() {
            pk_field.ty.clone()
        } else {
            let pk_types = pk_fields.iter().map(|field| &field.ty);
            syn::parse2(quote!((#(#pk_types),*)))?
        };
        let pk_column_variants = pk_idents.iter().map(variant_ident).collect();
        let pk_column_names = pk_fields.iter().map(|field| field.column_name()).collect();
        let pk_auto_increment =
            matches!(pk_fields.as_slice(), [pk_field] if pk_field.auto_increment);

        let soft_delete = if input.soft_delete {
            let key = input.deleted_at_column.as_deref().unwrap_or("deleted_at");
            Some(resolve_soft_delete_field(&input.ident, &db_fields, key)?)
        } else {
            None
        };
        let timestamps_enabled = input.timestamps || has_timestamp_pair(&db_fields);
        let internal_entity_mod =
            format_ident!("__tideorm_internal_{}", to_snake_case(&struct_name_str));
        let index_impls = build_index_impls(&table_name, &indexes, false, &db_fields);
        let unique_index_impls = build_index_impls(&table_name, &indexes, true, &db_fields);
        let hidden_attrs = resolve_hidden_fields(
            &input.ident,
            &fields,
            &has_one_files
                .iter()
                .chain(&has_many_files)
                .collect::<Vec<_>>(),
            split_csv(&input.ident, "hidden", input.hidden.as_ref())?,
            soft_delete.as_ref().map(|(field, _)| field),
        )?;

        let mut ctx = Self {
            columns_struct_name: format_ident!("{}Columns", struct_name),
            struct_name,
            struct_name_str,
            table_name,
            schema_name,
            soft_delete,
            should_gen_debug,
            should_gen_clone,
            should_gen_default,
            should_gen_serialize,
            should_gen_deserialize,
            serde_round_trips: {
                let derived_serialize = should_gen_serialize || existing_derives.has_serialize;
                let derived_deserialize =
                    should_gen_deserialize || existing_derives.has_deserialize;
                let field_attrs = || db_fields.iter().map(|field| field.attrs.as_slice());
                // TideORM's generated half reads and writes field names, so
                // against a user derive of the other half any rename loses the
                // renamed keys; with both derived, only a split one does.
                let user_derives_both = !should_gen_serialize && !should_gen_deserialize;
                derived_serialize
                    && derived_deserialize
                    && ((should_gen_serialize && should_gen_deserialize)
                        || (crate::serde_names::round_trips(&input.attrs, field_attrs())
                            && !crate::serde_names::renames(
                                &input.attrs,
                                field_attrs(),
                                user_derives_both,
                            )))
            },
            hidden_attrs,
            translatable_fields,
            encrypted_fields,
            encrypted_column_names,
            allowed_languages: split_csv(&input.ident, "languages", input.languages.as_ref())?,
            fallback_language: input.fallback_language.clone(),
            has_one_files,
            has_many_files,
            searchable_fields,
            validation_rules,
            attribute_errors,
            pk_ident,
            pk_idents,
            pk_type,
            pk_column_variants,
            pk_column_names,
            pk_auto_increment,
            field_idents: db_fields
                .iter()
                .map(|field| field.ident().clone())
                .collect(),
            field_names: db_fields.iter().map(ModelField::name).collect(),
            field_types: db_fields.iter().map(|field| field.ty.clone()).collect(),
            column_types: db_fields
                .iter()
                .map(ModelField::column_type_expr)
                .collect::<syn::Result<_>>()?,
            serialized_names: if should_gen_serialize {
                Vec::new()
            } else {
                renamed_fields(input, &fields)
            },
            column_names: db_fields.iter().map(ModelField::column_name).collect(),
            column_variants: db_fields
                .iter()
                .map(|field| variant_ident(field.ident()))
                .collect(),
            timestamps_enabled,
            relation_wiring: Vec::new(),
            internal_entity_mod,
            index_impls,
            unique_index_impls,
            tokenize_enabled: input.tokenize,
            fields,
            relation_fields,
            db_fields,
        };
        ctx.relation_wiring = build_relation_wiring(&ctx)?;
        Ok(ctx)
    }

    /// `(field, type)` for each persisted field whose integer type a backend's
    /// driver cannot store and read back.
    pub(crate) fn driver_limited_fields(&self) -> Vec<(String, &'static str)> {
        self.db_fields
            .iter()
            .filter_map(|field| Some((field.name(), field.driver_limited_integer()?)))
            .collect()
    }

    /// A reference to each component of the `primary_key: &Self::PrimaryKey`
    /// in scope: `primary_key` itself for a single key, else `pk_0`, `pk_1`,
    /// .., which the returned statement binds from the tuple.
    pub(crate) fn primary_key_components(&self) -> (TokenStream2, Vec<TokenStream2>) {
        if let [_] = self.pk_idents.as_slice() {
            return (TokenStream2::new(), vec![quote!(primary_key)]);
        }
        let bindings: Vec<Ident> = (0..self.pk_idents.len())
            .map(|index| format_ident!("pk_{index}"))
            .collect();
        (
            quote!(let (#(#bindings),*) = primary_key;),
            bindings.iter().map(|binding| quote!(#binding)).collect(),
        )
    }

    /// Whether `field` is one of the model's `#[tideorm(encrypted = ..)]` columns.
    pub(crate) fn is_encrypted(&self, field: &ModelField) -> bool {
        self.encrypted_fields.contains(&field.name())
    }

    pub(crate) fn resolve_required_db_field_ident(
        &self,
        key: &str,
        relation_ident: &Ident,
    ) -> syn::Result<Ident> {
        find_db_field(&self.db_fields, key)
            .map(|field| field.ident().clone())
            .ok_or_else(|| {
                syn::Error::new_spanned(
                    relation_ident,
                    format!("relation references unknown field or column '{}'", key),
                )
            })
    }

    /// The column a relation's own side keys by when it names no
    /// `local_key`: the primary key's, which need not be called `id`.
    pub(crate) fn default_local_key(&self) -> &str {
        match self.pk_column_names.as_slice() {
            [column] => column,
            _ => "id",
        }
    }

    /// The ident of the field a relation's `local_key` names.
    pub(crate) fn resolve_local_key_ident(
        &self,
        key: &str,
        relation_ident: &Ident,
    ) -> syn::Result<Ident> {
        self.resolve_local_key(key, relation_ident)
            .map(|field| field.ident().clone())
    }

    /// The field a relation's `local_key` names: a field or column, or `id`
    /// for the primary key, whatever it is called.
    pub(crate) fn resolve_local_key(
        &self,
        key: &str,
        relation_ident: &Ident,
    ) -> syn::Result<&ModelField> {
        if let Some(field) = find_db_field(&self.db_fields, key) {
            return Ok(field);
        }

        if key != "id" {
            return Err(syn::Error::new_spanned(
                relation_ident,
                format!("relation references unknown local_key '{}'", key),
            ));
        }

        match self.db_fields.iter().find(|field| field.primary_key) {
            Some(field) if self.pk_idents.len() == 1 => Ok(field),
            _ => Err(syn::Error::new_spanned(
                relation_ident,
                "composite primary keys require an explicit relation local_key; implicit 'id' is ambiguous",
            )),
        }
    }
}
