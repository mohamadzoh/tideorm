//! TideORM Procedural Macros
//!
//! This crate provides derive macros for TideORM models.
//! Users should never need to import this crate directly - it's re-exported from `tideorm`.

use darling::{ast::Data, FromDeriveInput, FromField};
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, DeriveInput, Ident, Type, Attribute, Meta};
use convert_case::{Case, Casing};

/// Field-level attributes for model fields
#[derive(Debug, FromField)]
#[darling(attributes(tide), forward_attrs(validate))]
#[allow(dead_code)] // Some fields reserved for future use
struct ModelField {
    ident: Option<Ident>,
    ty: Type,
    attrs: Vec<syn::Attribute>,
    
    /// Mark as primary key
    #[darling(default)]
    primary_key: bool,
    
    /// Auto-increment (for primary keys)
    #[darling(default)]
    auto_increment: bool,
    
    /// Column name override
    #[darling(default)]
    column: Option<String>,
    
    /// Nullable field
    #[darling(default)]
    nullable: bool,
    
    /// Default value expression
    #[darling(default)]
    default: Option<String>,
    
    /// Skip this field in queries
    #[darling(default)]
    skip: bool,
    
    /// This field is a timestamp (created_at, updated_at)
    #[darling(default)]
    timestamp: bool,
}

/// Index definition parsed from #[index(...)] attribute
#[derive(Debug, Clone)]
struct IndexDef {
    name: Option<String>,
    columns: Vec<String>,
    unique: bool,
}

impl IndexDef {
    /// Parse from a simple string like "email" or "first_name,last_name"
    fn from_columns(columns: &str, unique: bool) -> Self {
        Self {
            name: None,
            columns: columns.split(',').map(|s| s.trim().to_string()).collect(),
            unique,
        }
    }
    
    /// Parse from named format like (name = "idx_email", columns = "email")
    fn from_named(name: String, columns: &str, unique: bool) -> Self {
        Self {
            name: Some(name),
            columns: columns.split(',').map(|s| s.trim().to_string()).collect(),
            unique,
        }
    }
    
    /// Generate the final index name
    fn get_name(&self, table_name: &str) -> String {
        if let Some(ref name) = self.name {
            name.clone()
        } else {
            let prefix = if self.unique { "uidx" } else { "idx" };
            let col_part = self.columns.join("_");
            format!("{}_{}_{}", prefix, table_name, col_part)
        }
    }
}

/// Parse #[index(...)] and #[unique_index(...)] attributes from the input
fn parse_index_attributes(attrs: &[Attribute]) -> (Vec<IndexDef>, Vec<IndexDef>) {
    let mut indexes = Vec::new();
    let mut unique_indexes = Vec::new();
    
    for attr in attrs {
        let is_index = attr.path().is_ident("index");
        let is_unique_index = attr.path().is_ident("unique_index");
        
        if !is_index && !is_unique_index {
            continue;
        }
        
        let unique = is_unique_index;
        
        // Parse the attribute based on its structure
        match &attr.meta {
            // #[index("email")] or #[index("first_name,last_name")]
            Meta::List(list) => {
                let tokens = list.tokens.to_string();
                
                // Check if it's named format: (name = "...", columns = "...")
                if tokens.contains("name") && tokens.contains("columns") {
                    let mut name = None;
                    let mut columns = None;
                    
                    // Parse the nested meta
                    let _ = attr.parse_nested_meta(|nested| {
                        if nested.path.is_ident("name") {
                            let value: syn::LitStr = nested.value()?.parse()?;
                            name = Some(value.value());
                        } else if nested.path.is_ident("columns") {
                            let value: syn::LitStr = nested.value()?.parse()?;
                            columns = Some(value.value());
                        }
                        Ok(())
                    });
                    
                    if let Some(cols) = columns {
                        let idx = if let Some(n) = name {
                            IndexDef::from_named(n, &cols, unique)
                        } else {
                            IndexDef::from_columns(&cols, unique)
                        };
                        if unique {
                            unique_indexes.push(idx);
                        } else {
                            indexes.push(idx);
                        }
                    }
                } else {
                    // Simple format: #[index("email")]
                    // Extract the string from tokens
                    let clean = tokens.trim().trim_matches('"');
                    if !clean.is_empty() {
                        let idx = IndexDef::from_columns(clean, unique);
                        if unique {
                            unique_indexes.push(idx);
                        } else {
                            indexes.push(idx);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    
    (indexes, unique_indexes)
}

/// Parse #[validate(...)] attributes from field attributes
fn parse_validation_attributes(_field_name: &str, attrs: &[Attribute]) -> Vec<proc_macro2::TokenStream> {
    let mut rules = Vec::new();
    
    for attr in attrs {
        if !attr.path().is_ident("validate") {
            continue;
        }
        
        match &attr.meta {
            Meta::List(list) => {
                let tokens = list.tokens.to_string();
                
                // Parse validation rules
                // Examples: #[validate(email)], #[validate(min_length = 3, max_length = 100)]
                for part in tokens.split(',') {
                    let part = part.trim();
                    
                    if part == "required" {
                        rules.push(quote! { ::tideorm::validation::ValidationRule::Required });
                    } else if part == "email" {
                        rules.push(quote! { ::tideorm::validation::ValidationRule::Email });
                    } else if part == "url" {
                        rules.push(quote! { ::tideorm::validation::ValidationRule::Url });
                    } else if part == "alpha" {
                        rules.push(quote! { ::tideorm::validation::ValidationRule::Alpha });
                    } else if part == "alphanumeric" {
                        rules.push(quote! { ::tideorm::validation::ValidationRule::Alphanumeric });
                    } else if part == "numeric" {
                        rules.push(quote! { ::tideorm::validation::ValidationRule::Numeric });
                    } else if part == "uuid" {
                        rules.push(quote! { ::tideorm::validation::ValidationRule::Uuid });
                    } else if part.starts_with("min_length") {
                        if let Some(val) = extract_value(part, "min_length") {
                            if let Ok(n) = val.parse::<usize>() {
                                rules.push(quote! { ::tideorm::validation::ValidationRule::MinLength(#n) });
                            }
                        }
                    } else if part.starts_with("max_length") {
                        if let Some(val) = extract_value(part, "max_length") {
                            if let Ok(n) = val.parse::<usize>() {
                                rules.push(quote! { ::tideorm::validation::ValidationRule::MaxLength(#n) });
                            }
                        }
                    } else if part.starts_with("length") && !part.contains("min_") && !part.contains("max_") {
                        if let Some(val) = extract_value(part, "length") {
                            if let Ok(n) = val.parse::<usize>() {
                                rules.push(quote! { ::tideorm::validation::ValidationRule::Length(#n) });
                            }
                        }
                    } else if part.starts_with("min") && !part.contains("length") {
                        if let Some(val) = extract_value(part, "min") {
                            if let Ok(n) = val.parse::<f64>() {
                                rules.push(quote! { ::tideorm::validation::ValidationRule::Min(#n) });
                            }
                        }
                    } else if part.starts_with("max") && !part.contains("length") {
                        if let Some(val) = extract_value(part, "max") {
                            if let Ok(n) = val.parse::<f64>() {
                                rules.push(quote! { ::tideorm::validation::ValidationRule::Max(#n) });
                            }
                        }
                    } else if part.starts_with("range") {
                        // range = "1..100" or range(1, 100)
                        if let Some(val) = extract_value(part, "range") {
                            let val = val.trim_matches('"');
                            if val.contains("..") {
                                let parts: Vec<&str> = val.split("..").collect();
                                if parts.len() == 2 {
                                    if let (Ok(min), Ok(max)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                                        rules.push(quote! { ::tideorm::validation::ValidationRule::Range(#min, #max) });
                                    }
                                }
                            }
                        }
                    } else if part.starts_with("regex") {
                        if let Some(val) = extract_value(part, "regex") {
                            let pattern = val.trim_matches('"');
                            rules.push(quote! { ::tideorm::validation::ValidationRule::Regex(#pattern.to_string()) });
                        }
                    } else if part.starts_with("custom") {
                        if let Some(val) = extract_value(part, "custom") {
                            let msg = val.trim_matches('"');
                            rules.push(quote! { ::tideorm::validation::ValidationRule::Custom(#msg.to_string()) });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    
    rules
}

/// Extract value from "key = value" or "key(value)" format
fn extract_value(input: &str, key: &str) -> Option<String> {
    let input = input.trim();
    
    // Try "key = value" format
    if let Some(pos) = input.find('=') {
        let k = input[..pos].trim();
        if k == key {
            return Some(input[pos + 1..].trim().to_string());
        }
    }
    
    // Try "key(value)" format
    if input.starts_with(key) && input.contains('(') && input.ends_with(')') {
        let start = input.find('(').unwrap() + 1;
        let end = input.len() - 1;
        return Some(input[start..end].trim().to_string());
    }
    
    None
}

/// Struct-level attributes for the model
#[derive(Debug, FromDeriveInput)]
#[darling(attributes(tide), supports(struct_named))]
#[allow(dead_code)] // Some fields reserved for future use
struct ModelInput {
    ident: Ident,
    data: Data<(), ModelField>,
    
    /// Table name override (defaults to snake_case plural)
    #[darling(default)]
    table: Option<String>,
    
    /// Schema name (for postgres)
    #[darling(default)]
    schema: Option<String>,
    
    /// Enable soft deletes
    #[darling(default)]
    soft_delete: bool,
    
    /// Enable timestamps (created_at, updated_at)
    #[darling(default)]
    timestamps: bool,
    
    /// Hidden attributes (not exposed in JSON)
    #[darling(default)]
    hidden: Option<String>,
    
    /// Translatable fields (for i18n)
    #[darling(default)]
    translatable: Option<String>,
    
    /// Allowed languages for translations
    #[darling(default)]
    languages: Option<String>,
    
    /// Fallback language
    #[darling(default)]
    fallback_language: Option<String>,
    
    /// HasOne file attachments
    #[darling(default)]
    has_one_files: Option<String>,
    
    /// HasMany file attachments
    #[darling(default)]
    has_many_files: Option<String>,
    
    /// Searchable fields
    #[darling(default)]
    searchable: Option<String>,
}

/// Derive macro for TideORM models
///
/// # Example
/// ```ignore
/// use tideorm::prelude::*;
///
/// #[derive(Model, Clone, Debug)]
/// #[tide(table = "users")]
/// #[index("email")]
/// #[index("active")]
/// #[index(name = "idx_name_status", columns = "name,status")]
/// #[unique_index("email")]
/// pub struct User {
///     #[tide(primary_key, auto_increment)]
///     pub id: i64,
///     
///     #[validate(email)]
///     pub email: String,
///     
///     #[validate(min_length = 2, max_length = 100)]
///     pub name: String,
///     
///     #[tide(nullable)]
///     pub bio: Option<String>,
/// }
/// ```
#[proc_macro_derive(Model, attributes(tide, index, unique_index, validate))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    
    // Parse index attributes separately
    let (indexes, unique_indexes) = parse_index_attributes(&input.attrs);
    
    let model_input = match ModelInput::from_derive_input(&input) {
        Ok(v) => v,
        Err(e) => return e.write_errors().into(),
    };
    
    let expanded = generate_model_impl(&model_input, indexes, unique_indexes);
    TokenStream::from(expanded)
}

fn generate_model_impl(input: &ModelInput, indexes: Vec<IndexDef>, unique_indexes: Vec<IndexDef>) -> proc_macro2::TokenStream {
    let struct_name = &input.ident;
    let table_name = input.table.clone().unwrap_or_else(|| {
        // Convert StructName to table_name (snake_case, pluralized)
        let name = struct_name.to_string().to_case(Case::Snake);
        pluralize(&name)
    });
    
    // Schema name for sync
    let schema_name = input.schema.clone().unwrap_or_else(|| "public".to_string());
    
    // Soft delete
    let soft_delete_enabled = input.soft_delete;
    
    // Parse hidden attributes
    let hidden_attrs: Vec<String> = input.hidden.as_ref()
        .map(|s| s.split(',').map(|v| v.trim().to_string()).collect())
        .unwrap_or_else(|| vec!["deleted_at".to_string()]);
    
    // Parse translatable fields
    let translatable_fields: Vec<String> = input.translatable.as_ref()
        .map(|s| s.split(',').map(|v| v.trim().to_string()).collect())
        .unwrap_or_default();
    
    // Parse allowed languages (only if explicitly specified)
    let has_custom_languages = input.languages.is_some();
    let allowed_languages: Vec<String> = input.languages.as_ref()
        .map(|s| s.split(',').map(|v| v.trim().to_string()).collect())
        .unwrap_or_default();
    
    // Fallback language (only if explicitly specified)
    let has_custom_fallback = input.fallback_language.is_some();
    let fallback_language = input.fallback_language.clone().unwrap_or_default();
    
    // Parse hasOne file relations
    let has_one_files: Vec<String> = input.has_one_files.as_ref()
        .map(|s| s.split(',').map(|v| v.trim().to_string()).collect())
        .unwrap_or_default();
    
    // Parse hasMany file relations
    let has_many_files: Vec<String> = input.has_many_files.as_ref()
        .map(|s| s.split(',').map(|v| v.trim().to_string()).collect())
        .unwrap_or_default();
    
    // Parse searchable fields
    let searchable_fields: Vec<String> = input.searchable.as_ref()
        .map(|s| s.split(',').map(|v| v.trim().to_string()).collect())
        .unwrap_or_default();
    
    // Generate index implementation code from parsed IndexDef structs
    let index_impls: Vec<_> = indexes.iter().map(|idx| {
        let name = idx.get_name(&table_name);
        let columns = &idx.columns;
        quote! {
            ::tideorm::model::IndexDefinition::new(
                #name,
                vec![#(#columns.to_string()),*],
                false
            )
        }
    }).collect();
    
    let unique_index_impls: Vec<_> = unique_indexes.iter().map(|idx| {
        let name = idx.get_name(&table_name);
        let columns = &idx.columns;
        quote! {
            ::tideorm::model::IndexDefinition::new(
                #name,
                vec![#(#columns.to_string()),*],
                true
            )
        }
    }).collect();
    
    let fields = match &input.data {
        Data::Struct(fields) => fields,
        _ => panic!("Model can only be derived for structs"),
    };
    
    // Parse validation rules from #[validate(...)] attributes on fields
    let validation_rules: Vec<_> = fields
        .iter()
        .filter(|f| !f.skip)
        .filter_map(|f| {
            let field_name = f.ident.as_ref()?.to_string();
            let rules = parse_validation_attributes(&field_name, &f.attrs);
            if rules.is_empty() {
                None
            } else {
                Some((field_name, rules))
            }
        })
        .collect();
    
    // Find primary key field
    let pk_field = fields.iter().find(|f| f.primary_key);
    let pk_ident = pk_field
        .and_then(|f| f.ident.as_ref())
        .cloned()
        .unwrap_or_else(|| format_ident!("id"));
    let pk_type = pk_field
        .map(|f| &f.ty)
        .cloned()
        .unwrap_or_else(|| syn::parse_quote!(i64));
    
    // Generate column names and field mappings
    let field_names: Vec<_> = fields
        .iter()
        .filter(|f| !f.skip)
        .filter_map(|f| f.ident.as_ref())
        .collect();
    
    let field_types: Vec<_> = fields
        .iter()
        .filter(|f| !f.skip)
        .map(|f| &f.ty)
        .collect();
    
    let column_names: Vec<_> = fields
        .iter()
        .filter(|f| !f.skip)
        .map(|f| {
            f.column.clone().unwrap_or_else(|| {
                f.ident
                    .as_ref()
                    .map(|i| i.to_string().to_case(Case::Snake))
                    .unwrap_or_default()
            })
        })
        .collect();
    
    // Generate insert column/field lists (excluding auto-increment PK)
    // These were used for regular inserts but upserts now use all columns
    let _insert_field_names: Vec<_> = fields
        .iter()
        .filter(|f| !f.skip && !(f.primary_key && f.auto_increment))
        .filter_map(|f| f.ident.as_ref())
        .collect();
    
    let _insert_column_names: Vec<_> = fields
        .iter()
        .filter(|f| !f.skip && !(f.primary_key && f.auto_increment))
        .map(|f| {
            f.column.clone().unwrap_or_else(|| {
                f.ident
                    .as_ref()
                    .map(|i| i.to_string().to_case(Case::Snake))
                    .unwrap_or_default()
            })
        })
        .collect();
    
    // Generate column enum variants (PascalCase)
    let column_variants: Vec<_> = fields
        .iter()
        .filter(|f| !f.skip)
        .filter_map(|f| f.ident.as_ref())
        .map(|i| format_ident!("{}", i.to_string().to_case(Case::Pascal)))
        .collect();
    
    // Primary key info
    let pk_column_variant = format_ident!("{}", pk_ident.to_string().to_case(Case::Pascal));
    let pk_column_name = pk_field
        .and_then(|f| f.column.clone())
        .unwrap_or_else(|| pk_ident.to_string().to_case(Case::Snake));
    let pk_auto_increment = pk_field.map(|f| f.auto_increment).unwrap_or(false);
    
    // Detect timestamp fields - either explicitly enabled via #[tide(timestamps)] 
    // or auto-detected by field names
    let has_created_at = fields.iter().any(|f| {
        f.ident.as_ref().map(|i| i.to_string() == "created_at").unwrap_or(false)
    });
    let has_updated_at = fields.iter().any(|f| {
        f.ident.as_ref().map(|i| i.to_string() == "updated_at").unwrap_or(false)
    });
    
    // timestamps enabled if: explicitly set via #[tide(timestamps)] OR both fields exist
    let timestamps_enabled = input.timestamps || (has_created_at && has_updated_at);
    
    // Generate sync column attribute setters (primary_key, auto_increment, not_null)
    let sync_column_attrs: Vec<_> = fields
        .iter()
        .filter(|f| !f.skip)
        .map(|f| {
            let mut attrs = Vec::new();
            
            if f.primary_key {
                attrs.push(quote! { col = col.primary_key(); });
            }
            if f.auto_increment {
                attrs.push(quote! { col = col.auto_increment(); });
            }
            // Check if type is not Option (meaning NOT NULL)
            let ty_str = quote!(#(&f.ty)).to_string();
            if !f.nullable && !ty_str.contains("Option") {
                attrs.push(quote! { col = col.not_null(); });
            }
            if let Some(ref default) = f.default {
                attrs.push(quote! { col = col.default(#default); });
            }
            
            quote! { #(#attrs)* }
        })
        .collect();
    
    // Generate active model field setters for INSERT (NotSet for auto-increment PK)
    let insert_active_model_setters: Vec<_> = fields
        .iter()
        .filter(|f| !f.skip)
        .map(|f| {
            let ident = f.ident.as_ref().unwrap();
            let field_name = ident.to_string();
            if f.primary_key && f.auto_increment {
                // For auto-increment PKs, use NotSet to let the database generate the value
                quote! {
                    #ident: ActiveValue::NotSet
                }
            } else if field_name == "created_at" || field_name == "updated_at" {
                // Auto-set timestamps on insert
                quote! {
                    #ident: ActiveValue::Set(::tideorm::chrono::Utc::now())
                }
            } else {
                quote! {
                    #ident: ActiveValue::Set(self.#ident)
                }
            }
        })
        .collect();
    
    // Generate active model field setters for UPDATE (Unchanged for PK, Set for others)
    let update_active_model_setters: Vec<_> = fields
        .iter()
        .filter(|f| !f.skip)
        .map(|f| {
            let ident = f.ident.as_ref().unwrap();
            let field_name = ident.to_string();
            if f.primary_key {
                // For PKs on update, use Unchanged
                quote! {
                    #ident: ActiveValue::Unchanged(self.#ident)
                }
            } else if field_name == "updated_at" {
                // Auto-update updated_at on update
                quote! {
                    #ident: ActiveValue::Set(::tideorm::chrono::Utc::now())
                }
            } else {
                quote! {
                    #ident: ActiveValue::Set(self.#ident)
                }
            }
        })
        .collect();
    
    // Generate internal SeaORM entity module name
    let internal_entity_mod = format_ident!("__tideorm_internal_{}", struct_name.to_string().to_lowercase());
    
    // Generate SeaORM field definitions with attributes for DeriveEntityModel
    let sea_orm_field_defs: Vec<_> = fields
        .iter()
        .filter(|f| !f.skip)
        .map(|f| {
            let ident = f.ident.as_ref().unwrap();
            let ty = &f.ty;
            let column_name = f.column.clone().unwrap_or_else(|| {
                ident.to_string().to_case(Case::Snake)
            });
            
            let mut attrs = vec![];
            
            if f.primary_key {
                attrs.push(quote!(primary_key));
            }
            if f.auto_increment {
                attrs.push(quote!(auto_increment));
            }
            // Always set column_name to be explicit
            attrs.push(quote!(column_name = #column_name));
            
            let sea_orm_attr = quote!(#[sea_orm(#(#attrs),*)]);
            
            quote! {
                #sea_orm_attr
                pub #ident: #ty
            }
        })
        .collect();
    
    let base_impl = quote! {
        // Internal SeaORM entity - NEVER exposed to users
        // Uses SeaORM's own derive macros for correctness
        #[doc(hidden)]
        #[allow(non_snake_case, dead_code, unused_imports, clippy::all)]
        mod #internal_entity_mod {
            use ::tideorm::sea_orm::entity::prelude::*;
            use ::tideorm::sea_orm::{ActiveValue, DeriveEntity, DeriveModel, DeriveActiveModel};
            
            // Entity struct
            #[derive(Copy, Clone, Default, Debug, DeriveEntity)]
            pub struct Entity;
            
            impl EntityName for Entity {
                fn table_name(&self) -> &str {
                    #table_name
                }
            }
            
            // Model struct
            #[derive(Clone, Debug, PartialEq, DeriveModel, DeriveActiveModel)]
            pub struct Model {
                #(#sea_orm_field_defs),*
            }
            
            // Column enum
            #[derive(Copy, Clone, Debug, EnumIter, DeriveColumn)]
            pub enum Column {
                #(#column_variants),*
            }
            
            // Primary key enum
            #[derive(Copy, Clone, Debug, EnumIter, DerivePrimaryKey)]
            pub enum PrimaryKey {
                #pk_column_variant
            }
            
            impl PrimaryKeyTrait for PrimaryKey {
                type ValueType = #pk_type;
                
                fn auto_increment() -> bool {
                    #pk_auto_increment
                }
            }
            
            // Column trait impl
            impl ColumnTrait for Column {
                type EntityName = Entity;
                
                fn def(&self) -> ColumnDef {
                    match self {
                        #(Self::#column_variants => ColumnType::String(StringLen::None).def()),*
                    }
                }
            }
            
            // Relation enum (empty for now)
            #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
            pub enum Relation {}
            
            impl ActiveModelBehavior for ActiveModel {}
        }
        
        impl ::tideorm::model::ModelMeta for #struct_name {
            type PrimaryKey = #pk_type;
            
            fn table_name() -> &'static str {
                #table_name
            }
            
            fn primary_key_name() -> &'static str {
                stringify!(#pk_ident)
            }
            
            fn column_names() -> &'static [&'static str] {
                &[#(#column_names),*]
            }
            
            fn field_names() -> &'static [&'static str] {
                &[#(stringify!(#field_names)),*]
            }
            
            fn hidden_attributes() -> Vec<&'static str> {
                vec![#(#hidden_attrs),*]
            }
            
            fn searchable_fields() -> Vec<&'static str> {
                vec![#(#searchable_fields),*]
            }
            
            fn translatable_fields() -> Vec<&'static str> {
                vec![#(#translatable_fields),*]
            }
            
            fn has_one_attached_file() -> Vec<&'static str> {
                vec![#(#has_one_files),*]
            }
            
            fn has_many_attached_files() -> Vec<&'static str> {
                vec![#(#has_many_files),*]
            }
            
            fn soft_delete_enabled() -> bool {
                #soft_delete_enabled
            }
            
            fn has_timestamps() -> bool {
                #timestamps_enabled
            }
            
            fn indexes() -> Vec<::tideorm::model::IndexDefinition> {
                vec![#(#index_impls),*]
            }
            
            fn unique_indexes() -> Vec<::tideorm::model::IndexDefinition> {
                vec![#(#unique_index_impls),*]
            }
        }
    };
    
    // Generate optional language overrides
    let language_override = if has_custom_languages {
        quote! {
            impl #struct_name {
                /// Model-specific allowed languages (overrides global config)
                pub fn model_allowed_languages() -> Vec<String> {
                    vec![#(#allowed_languages.to_string()),*]
                }
            }
            
            // Override in ModelMeta (we can't extend the trait, so we add a method)
        }
    } else {
        quote! {}
    };
    
    let fallback_override = if has_custom_fallback {
        quote! {
            impl #struct_name {
                /// Model-specific fallback language (overrides global config)
                pub fn model_fallback_language() -> String {
                    #fallback_language.to_string()
                }
            }
        }
    } else {
        quote! {}
    };
    
    // Generate validation implementation
    let validation_impl = if !validation_rules.is_empty() {
        let validation_checks: Vec<_> = validation_rules.iter().map(|(field_name, rules)| {
            let field_ident = format_ident!("{}", field_name);
            quote! {
                {
                    let rules: Vec<::tideorm::validation::ValidationRule> = vec![#(#rules),*];
                    for rule in &rules {
                        if let Some(msg) = ::tideorm::validation::Validator::validate_rule(&self.#field_ident, rule, #field_name) {
                            errors.add(#field_name, msg);
                        }
                    }
                }
            }
        }).collect();
        
        let rules_list: Vec<_> = validation_rules.iter().map(|(field_name, rules)| {
            quote! {
                (#field_name, vec![#(#rules),*])
            }
        }).collect();
        
        quote! {
            impl ::tideorm::validation::Validate for #struct_name {
                fn validation_rules() -> Vec<(&'static str, Vec<::tideorm::validation::ValidationRule>)> {
                    vec![#(#rules_list),*]
                }
                
                fn validate(&self) -> Result<(), ::tideorm::validation::ValidationErrors> {
                    let mut errors = ::tideorm::validation::ValidationErrors::new();
                    
                    #(#validation_checks)*
                    
                    // Run custom validations
                    if let Err(custom_errors) = self.custom_validations() {
                        errors.merge(custom_errors);
                    }
                    
                    errors.to_result()
                }
            }
        }
    } else {
        // Default empty validation implementation
        quote! {
            impl ::tideorm::validation::Validate for #struct_name {
                fn validate(&self) -> Result<(), ::tideorm::validation::ValidationErrors> {
                    self.custom_validations()
                }
            }
        }
    };
    
    quote! {
        #base_impl
        
        #language_override
        #fallback_override
        
        // Internal adapter implementation (hidden from users)
        #[doc(hidden)]
        impl ::tideorm::internal::InternalModel for #struct_name {
            type Entity = #internal_entity_mod::Entity;
            type ActiveModel = #internal_entity_mod::ActiveModel;
            
            fn into_active_model(self) -> Self::ActiveModel {
                use ::tideorm::sea_orm::ActiveValue;
                #internal_entity_mod::ActiveModel {
                    #(#insert_active_model_setters),*
                }
            }
            
            fn from_sea_model(model: #internal_entity_mod::Model) -> Self {
                Self {
                    #(#field_names: model.#field_names),*
                }
            }
        }
        
        impl #struct_name {
            /// Convert to active model for UPDATE operations
            #[doc(hidden)]
            fn __into_update_active_model(self) -> #internal_entity_mod::ActiveModel {
                use ::tideorm::sea_orm::ActiveValue;
                #internal_entity_mod::ActiveModel {
                    #(#update_active_model_setters),*
                }
            }
            
            /// Convert to active model for DELETE operations (only PK set)
            #[doc(hidden)]
            fn __into_delete_active_model(self) -> #internal_entity_mod::ActiveModel {
                use ::tideorm::sea_orm::ActiveValue;
                #internal_entity_mod::ActiveModel {
                    #pk_ident: ActiveValue::Unchanged(self.#pk_ident),
                    ..Default::default()
                }
            }
        }
        
        #[::tideorm::async_trait::async_trait]
        impl ::tideorm::model::Model for #struct_name {
            fn primary_key(&self) -> Self::PrimaryKey {
                self.#pk_ident.clone()
            }
            
            async fn find(id: Self::PrimaryKey) -> ::tideorm::Result<Option<Self>> {
                use ::tideorm::sea_orm::EntityTrait;
                use ::tideorm::internal::InternalModel;
                let result = #internal_entity_mod::Entity::find_by_id(id)
                    .one(::tideorm::db().__internal_connection())
                    .await
                    .map_err(|e| ::tideorm::Error::query(e.to_string()))?;
                Ok(result.map(|m| <Self as InternalModel>::from_sea_model(m)))
            }
            
            async fn destroy(id: Self::PrimaryKey) -> ::tideorm::Result<u64> {
                use ::tideorm::sea_orm::EntityTrait;
                let result = #internal_entity_mod::Entity::delete_by_id(id)
                    .exec(::tideorm::db().__internal_connection())
                    .await
                    .map_err(|e| ::tideorm::Error::query(e.to_string()))?;
                Ok(result.rows_affected)
            }
            
            async fn create(model: Self) -> ::tideorm::Result<Self> {
                model.save().await
            }
            
            async fn delete(self) -> ::tideorm::Result<u64> {
                use ::tideorm::sea_orm::ActiveModelTrait;
                let active = self.__into_delete_active_model();
                let result = active.delete(::tideorm::db().__internal_connection())
                    .await
                    .map_err(|e| ::tideorm::Error::query(e.to_string()))?;
                Ok(result.rows_affected)
            }
            
            async fn save(self) -> ::tideorm::Result<Self> {
                use ::tideorm::sea_orm::ActiveModelTrait;
                use ::tideorm::internal::InternalModel;
                let active = <Self as InternalModel>::into_active_model(self);
                let result = active.insert(::tideorm::db().__internal_connection())
                    .await
                    .map_err(|e| ::tideorm::Error::query(e.to_string()))?;
                Ok(<Self as InternalModel>::from_sea_model(result))
            }
            
            async fn update(self) -> ::tideorm::Result<Self> {
                use ::tideorm::sea_orm::ActiveModelTrait;
                use ::tideorm::internal::InternalModel;
                let active = self.__into_update_active_model();
                let result = active.update(::tideorm::db().__internal_connection())
                    .await
                    .map_err(|e| ::tideorm::Error::query(e.to_string()))?;
                Ok(<Self as InternalModel>::from_sea_model(result))
            }
            
            async fn insert_or_update(
                model: Self,
                conflict_columns: Vec<&str>,
            ) -> ::tideorm::Result<Self> {
                let cols: Vec<String> = conflict_columns.into_iter().map(|s| s.to_string()).collect();
                let builder = ::tideorm::model::OnConflictBuilder::new(cols);
                Self::__insert_with_conflict(model, builder).await
            }
            
            async fn __insert_with_conflict(
                model: Self,
                builder: ::tideorm::model::OnConflictBuilder<Self>,
            ) -> ::tideorm::Result<Self> {
                use ::tideorm::Database;
                use ::tideorm::internal::InternalModel;
                use serde_json::json;
                
                // Build the INSERT part (include ALL columns for upsert, including PK)
                let table = #table_name;
                let columns: Vec<&str> = vec![#(#column_names),*];
                
                // Get values from the model (ALL fields including PK)
                let model_clone = model.clone();
                let values: Vec<serde_json::Value> = vec![
                    #(json!(model_clone.#field_names)),*
                ];
                
                let column_list = columns.iter()
                    .map(|c| format!("\"{}\"", c))
                    .collect::<Vec<_>>()
                    .join(", ");
                
                let value_list = values.iter()
                    .map(|v| match v {
                        serde_json::Value::Null => "NULL".to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::String(s) => format!("'{}'", s.replace('\'', "''")),
                        _ => format!("'{}'", serde_json::to_string(v).unwrap_or_default().replace('\'', "''")),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                
                // Build the ON CONFLICT part
                let conflict_cols = builder.conflict_columns;
                let conflict_list = conflict_cols.iter()
                    .map(|c| format!("\"{}\"", c))
                    .collect::<Vec<_>>()
                    .join(", ");
                
                // Determine which columns to update
                let update_cols: Vec<String> = if let Some(cols) = builder.update_columns {
                    cols
                } else if let Some(exclude) = builder.exclude_columns {
                    columns.iter()
                        .filter(|c| !exclude.contains(&c.to_string()))
                        .map(|c| c.to_string())
                        .collect()
                } else {
                    // Update all columns except conflict columns and primary key
                    let pk_col_name = #pk_column_name;
                    columns.iter()
                        .filter(|c| {
                            let c_str = c.to_string();
                            !conflict_cols.contains(&c_str) && c_str != pk_col_name
                        })
                        .map(|c| c.to_string())
                        .collect()
                };
                
                let update_list = update_cols.iter()
                    .map(|c| format!("\"{}\" = EXCLUDED.\"{}\"", c, c))
                    .collect::<Vec<_>>()
                    .join(", ");
                
                let sql = format!(
                    "INSERT INTO \"{}\" ({}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET {} RETURNING *",
                    table, column_list, value_list, conflict_list, update_list
                );
                
                // Execute the raw SQL and fetch the result
                let results: Vec<Self> = ::tideorm::Database::raw(&sql).await?;
                
                // Return the first (and should be only) result
                results.into_iter().next().ok_or_else(|| {
                    ::tideorm::Error::query("INSERT ... ON CONFLICT returned no rows".to_string())
                })
            }
        }
        
        // Register model for schema synchronization
        impl #struct_name {
            /// Get the model schema for database synchronization
            #[doc(hidden)]
            pub fn __get_sync_schema() -> ::tideorm::sync::ModelSchema {
                use ::tideorm::sync::{ModelSchema, ColumnDef, normalize_rust_type};
                
                let mut schema = ModelSchema::new(#table_name).schema(#schema_name);
                
                // Add column definitions based on field types with proper attributes
                // Store normalized Rust type - conversion to SQL type happens at sync time
                #(
                    {
                        let rust_type = normalize_rust_type(stringify!(#field_types));
                        let mut col = ColumnDef::new(#column_names, rust_type);
                        #sync_column_attrs
                        schema = schema.column(col);
                    }
                )*
                
                schema
            }
            
            /// Register this model for schema synchronization
            #[doc(hidden)]
            #[inline]
            pub fn __register_for_sync() {
                ::tideorm::sync::SyncRegistry::register(Self::__get_sync_schema());
            }
        }
        
        // Implement SyncModel trait for use with TideConfig::models()
        impl ::tideorm::sync::SyncModel for #struct_name {
            fn sync_schema() -> ::tideorm::sync::ModelSchema {
                Self::__get_sync_schema()
            }
        }
        
        #validation_impl
    }
}

/// Simple pluralization (can be enhanced later)
fn pluralize(word: &str) -> String {
    if word.ends_with('s') || word.ends_with('x') || word.ends_with("ch") || word.ends_with("sh") {
        format!("{}es", word)
    } else if word.ends_with('y') && !word.ends_with("ay") && !word.ends_with("ey") && !word.ends_with("oy") && !word.ends_with("uy") {
        format!("{}ies", &word[..word.len()-1])
    } else {
        format!("{}s", word)
    }
}

// =============================================================================
// RELATION MACROS
// =============================================================================

/// Derive BelongsTo relation for a model
///
/// Generates an implementation of the `BelongsTo<Related>` trait.
///
/// # Usage
///
/// ```ignore
/// use tideorm::prelude::*;
///
/// #[derive(Model, Clone, Debug, Serialize, Deserialize)]
/// #[tide(table = "posts")]
/// #[belongs_to(User, foreign_key = "user_id")]
/// pub struct Post {
///     #[tide(primary_key, auto_increment)]
///     pub id: i64,
///     pub user_id: i64,
///     pub title: String,
/// }
///
/// // Now you can use:
/// let post = Post::find(1).await?;
/// let author = post.load_belongs_to::<User>().await?;
/// ```
///
/// # Attributes
///
/// - First argument: The related model type (e.g., `User`)
/// - `foreign_key = "column"`: The foreign key column on this model (required)
/// - `owner_key = "column"`: The primary key on the related model (optional, defaults to related model's PK)
#[proc_macro_attribute]
pub fn belongs_to(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_clone = item.clone();
    let input = parse_macro_input!(item_clone as DeriveInput);
    let struct_name = &input.ident;
    
    // Parse the attribute: belongs_to(User, foreign_key = "user_id", owner_key = "id")
    let attr_str = attr.to_string();
    let (related_type, foreign_key, owner_key) = parse_relation_attr(&attr_str);
    
    let related_ident: proc_macro2::TokenStream = related_type.parse().unwrap();
    
    let owner_key_impl = if let Some(ok) = owner_key {
        quote! {
            fn owner_key() -> &'static str { #ok }
        }
    } else {
        quote! {}
    };
    
    let impl_block = quote! {
        impl ::tideorm::relations::BelongsTo<#related_ident> for #struct_name {
            fn foreign_key() -> &'static str { #foreign_key }
            #owner_key_impl
        }
    };
    
    let original: proc_macro2::TokenStream = item.into();
    let expanded = quote! {
        #original
        #impl_block
    };
    
    TokenStream::from(expanded)
}

/// Derive HasOne relation for a model
///
/// Generates an implementation of the `HasOne<Related>` trait.
///
/// # Usage
///
/// ```ignore
/// use tideorm::prelude::*;
///
/// #[derive(Model, Clone, Debug, Serialize, Deserialize)]
/// #[tide(table = "users")]
/// #[has_one(Profile, foreign_key = "user_id")]
/// pub struct User {
///     #[tide(primary_key, auto_increment)]
///     pub id: i64,
///     pub name: String,
/// }
///
/// // Now you can use:
/// let user = User::find(1).await?;
/// let profile = user.load_has_one::<Profile>().await?;
/// ```
///
/// # Attributes
///
/// - First argument: The related model type (e.g., `Profile`)
/// - `foreign_key = "column"`: The foreign key column on the related model (required)
/// - `local_key = "column"`: The local key on this model (optional, defaults to this model's PK)
#[proc_macro_attribute]
pub fn has_one(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_clone = item.clone();
    let input = parse_macro_input!(item_clone as DeriveInput);
    let struct_name = &input.ident;
    
    let attr_str = attr.to_string();
    let (related_type, foreign_key, local_key) = parse_relation_attr(&attr_str);
    
    let related_ident: proc_macro2::TokenStream = related_type.parse().unwrap();
    
    let local_key_impl = if let Some(lk) = local_key {
        quote! {
            fn local_key() -> &'static str { #lk }
        }
    } else {
        quote! {}
    };
    
    let impl_block = quote! {
        impl ::tideorm::relations::HasOne<#related_ident> for #struct_name {
            fn foreign_key() -> &'static str { #foreign_key }
            #local_key_impl
        }
    };
    
    let original: proc_macro2::TokenStream = item.into();
    let expanded = quote! {
        #original
        #impl_block
    };
    
    TokenStream::from(expanded)
}

/// Derive HasMany relation for a model
///
/// Generates an implementation of the `HasMany<Related>` trait.
///
/// # Usage
///
/// ```ignore
/// use tideorm::prelude::*;
///
/// #[derive(Model, Clone, Debug, Serialize, Deserialize)]
/// #[tide(table = "users")]
/// #[has_many(Post, foreign_key = "user_id")]
/// pub struct User {
///     #[tide(primary_key, auto_increment)]
///     pub id: i64,
///     pub name: String,
/// }
///
/// // Now you can use:
/// let user = User::find(1).await?;
/// let posts = user.load_has_many::<Post>().await?;
/// ```
///
/// # Attributes
///
/// - First argument: The related model type (e.g., `Post`)
/// - `foreign_key = "column"`: The foreign key column on the related model (required)
/// - `local_key = "column"`: The local key on this model (optional, defaults to this model's PK)
#[proc_macro_attribute]
pub fn has_many(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_clone = item.clone();
    let input = parse_macro_input!(item_clone as DeriveInput);
    let struct_name = &input.ident;
    
    let attr_str = attr.to_string();
    let (related_type, foreign_key, local_key) = parse_relation_attr(&attr_str);
    
    let related_ident: proc_macro2::TokenStream = related_type.parse().unwrap();
    
    let local_key_impl = if let Some(lk) = local_key {
        quote! {
            fn local_key() -> &'static str { #lk }
        }
    } else {
        quote! {}
    };
    
    let impl_block = quote! {
        impl ::tideorm::relations::HasMany<#related_ident> for #struct_name {
            fn foreign_key() -> &'static str { #foreign_key }
            #local_key_impl
        }
    };
    
    let original: proc_macro2::TokenStream = item.into();
    let expanded = quote! {
        #original
        #impl_block
    };
    
    TokenStream::from(expanded)
}

/// Parse relation attribute string into (related_type, foreign_key, optional_key)
fn parse_relation_attr(attr: &str) -> (String, String, Option<String>) {
    let attr = attr.trim();
    
    // Parse: RelatedType, foreign_key = "column", local_key = "column"
    let mut parts = attr.splitn(2, ',');
    
    let related_type = parts.next()
        .unwrap_or("")
        .trim()
        .to_string();
    
    let rest = parts.next().unwrap_or("");
    
    let mut foreign_key = String::new();
    let mut optional_key: Option<String> = None;
    
    // Parse key = "value" pairs
    for part in rest.split(',') {
        let part = part.trim();
        if part.starts_with("foreign_key") {
            foreign_key = extract_string_value(part);
        } else if part.starts_with("owner_key") || part.starts_with("local_key") {
            optional_key = Some(extract_string_value(part));
        }
    }
    
    (related_type, foreign_key, optional_key)
}

/// Extract string value from `key = "value"` format
fn extract_string_value(s: &str) -> String {
    s.split('=')
        .nth(1)
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .to_string()
}
