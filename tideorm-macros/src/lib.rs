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
#[allow(dead_code)]
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
    
    // Relation attributes (SeaORM-style, defined inside struct)
    /// HasOne relation: #[tide(has_one = "RelatedModel", foreign_key = "fk_column")]
    #[darling(default)]
    has_one: Option<String>,
    
    /// HasMany relation: #[tide(has_many = "RelatedModel", foreign_key = "fk_column")]
    #[darling(default)]
    has_many: Option<String>,
    
    /// BelongsTo relation: #[tide(belongs_to = "RelatedModel", foreign_key = "fk_column")]
    #[darling(default)]
    belongs_to: Option<String>,
    
    /// HasManyThrough relation
    #[darling(default)]
    has_many_through: Option<String>,
    
    /// Foreign key for relations
    #[darling(default)]
    foreign_key: Option<String>,
    
    /// Owner/local key for relations
    #[darling(default)]
    owner_key: Option<String>,
    
    /// Local key for relations
    #[darling(default)]
    local_key: Option<String>,
    
    /// Pivot table for has_many_through
    #[darling(default)]
    pivot: Option<String>,
    
    /// Related key for has_many_through
    #[darling(default)]
    related_key: Option<String>,
    
    /// Morph name for polymorphic relations
    #[darling(default)]
    morph_name: Option<String>,
}

impl ModelField {
    /// Check if this field is a relation field
    fn is_relation(&self) -> bool {
        self.has_one.is_some() || 
        self.has_many.is_some() || 
        self.belongs_to.is_some() ||
        self.has_many_through.is_some()
    }
    
    /// Check if field type looks like a relation type
    fn is_relation_type(&self) -> bool {
        let ty_str = quote!(#(&self.ty)).to_string();
        ty_str.contains("HasOne") || 
        ty_str.contains("HasMany") || 
        ty_str.contains("BelongsTo") ||
        ty_str.contains("MorphOne") ||
        ty_str.contains("MorphMany") ||
        ty_str.contains("MorphTo")
    }
}

/// Index definition parsed from #[index(...)] attribute
#[derive(Debug, Clone)]
struct IndexDef {
    name: Option<String>,
    columns: Vec<String>,
    unique: bool,
}

impl IndexDef {
    fn from_columns(columns: &str, unique: bool) -> Self {
        Self {
            name: None,
            columns: columns.split(',').map(|s| s.trim().to_string()).collect(),
            unique,
        }
    }
    
    fn from_named(name: String, columns: &str, unique: bool) -> Self {
        Self {
            name: Some(name),
            columns: columns.split(',').map(|s| s.trim().to_string()).collect(),
            unique,
        }
    }
    
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
        
        match &attr.meta {
            Meta::List(list) => {
                let tokens = list.tokens.to_string();
                
                if tokens.contains("name") && tokens.contains("columns") {
                    let mut name = None;
                    let mut columns = None;
                    
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
    
    if let Some(pos) = input.find('=') {
        let k = input[..pos].trim();
        if k == key {
            return Some(input[pos + 1..].trim().to_string());
        }
    }
    
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
#[allow(dead_code)]
struct ModelInput {
    ident: Ident,
    data: Data<(), ModelField>,
    
    /// Table name override
    #[darling(default)]
    table: Option<String>,
    
    /// Schema name (for postgres)
    #[darling(default)]
    schema: Option<String>,
    
    /// Enable soft deletes
    #[darling(default)]
    soft_delete: bool,
    
    /// Enable timestamps
    #[darling(default)]
    timestamps: bool,
    
    /// Hidden attributes
    #[darling(default)]
    hidden: Option<String>,
    
    /// Translatable fields
    #[darling(default)]
    translatable: Option<String>,
    
    /// Allowed languages
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
    
    // Auto-derive control options
    /// Skip auto-generating Debug impl (use when manually deriving Debug)
    #[darling(default)]
    skip_debug: bool,
    
    /// Skip auto-generating Clone impl (use when manually deriving Clone)
    #[darling(default)]
    skip_clone: bool,
    
    /// Skip auto-generating Default impl (use when manually deriving Default)
    #[darling(default)]
    skip_default: bool,
    
    /// Skip auto-generating Serialize impl (use when manually deriving Serialize)
    #[darling(default)]
    skip_serialize: bool,
    
    /// Skip auto-generating Deserialize impl (use when manually deriving Deserialize)
    #[darling(default)]
    skip_deserialize: bool,
    
    /// Skip all auto-derives (Debug, Clone, Default, Serialize, Deserialize)
    #[darling(default)]
    skip_derives: bool,
    
    /// Enable auto-derives - automatically implement Debug, Clone, Default, Serialize, Deserialize
    /// Use this when you want the Model macro to generate all common traits automatically.
    /// Example: #[tide(table = "users", auto_derives)]
    #[darling(default)]
    auto_derives: bool,
    
    /// Enable auto Debug impl
    #[darling(default)]
    auto_debug: bool,
    
    /// Enable auto Clone impl
    #[darling(default)]
    auto_clone: bool,
    
    /// Enable auto Default impl  
    #[darling(default)]
    auto_default: bool,
    
    /// Enable auto Serialize impl
    #[darling(default)]
    auto_serialize: bool,
    
    /// Enable auto Deserialize impl
    #[darling(default)]
    auto_deserialize: bool,
}

/// Derive macro for TideORM models
///
/// The `Model` derive macro generates all necessary implementations for your struct
/// to work with TideORM's database operations.
///
/// # Auto-Generated Traits
///
/// By default, the Model derive automatically generates:
/// - `Debug` - for printing/logging
/// - `Clone` - for cloning instances
/// - `Default` - required for internal operations
/// - `Serialize` - for JSON serialization (via serde)
/// - `Deserialize` - for JSON deserialization (via serde)
///
/// # Simplest Usage
///
/// ```rust,ignore
/// // Just #[derive(Model)] - everything else is auto-generated!
/// #[derive(Model)]
/// #[tide(table = "users")]
/// pub struct User {
///     #[tide(primary_key, auto_increment)]
///     pub id: i64,
///     pub name: String,
/// }
/// ```
///
/// # Skip Auto-Derives (for custom implementations)
///
/// If you need custom implementations, use skip flags:
///
/// ```rust,ignore
/// #[derive(Model)]
/// #[tide(table = "users", skip_debug)]  // Skip Debug only
/// pub struct User { ... }
///
/// #[derive(Model)]
/// #[tide(table = "users", skip_derives)]  // Skip all auto-derives
/// pub struct User { ... }
/// ```
/// #[tide(table = "users")]
/// pub struct User {
///     #[tide(primary_key, auto_increment)]
///     pub id: i64,
///     pub name: String,
/// }
/// ```
///
/// # Complete Example
///
/// ```rust,ignore
/// use tideorm::prelude::*;
///
/// #[derive(Model)]
/// #[tide(table = "users")]
/// #[index("email")]
/// #[unique_index("email")]
/// pub struct User {
///     #[tide(primary_key, auto_increment)]
///     pub id: i64,
///     
///     #[validate(email)]
///     pub email: String,
///     
///     pub name: String,
///     
///     // Relation defined inside the struct (SeaORM-style)
///     #[tide(has_one = "Profile", foreign_key = "user_id")]
///     pub profile: HasOne<Profile>,
///     
///     #[tide(has_many = "Post", foreign_key = "user_id")]
///     pub posts: HasMany<Post>,
/// }
/// ```
#[proc_macro_derive(Model, attributes(tide, index, unique_index, validate))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    
    // Detect derives from remaining attributes (other #[derive(...)] attributes that haven't been processed yet)
    let existing_derives = detect_existing_derives(&input.attrs);
    
    let (indexes, unique_indexes) = parse_index_attributes(&input.attrs);
    
    let model_input = match ModelInput::from_derive_input(&input) {
        Ok(v) => v,
        Err(e) => return e.write_errors().into(),
    };
    
    let expanded = generate_model_impl(&model_input, indexes, unique_indexes, &existing_derives);
    TokenStream::from(expanded)
}

/// Struct to track which derives are already present
#[derive(Debug, Default)]
struct ExistingDerives {
    has_debug: bool,
    has_clone: bool,
    has_default: bool,
    has_serialize: bool,
    has_deserialize: bool,
}

/// Detect existing derive macros by examining the struct definition for derive attributes.
/// 
/// IMPORTANT: When multiple derives are in the same #[derive(...)] list like:
/// `#[derive(Debug, Clone, Model, Serialize)]`
/// 
/// Rust processes each derive independently and removes them from the attribute list.
/// By the time Model runs, Debug and Clone have already been processed and removed.
/// 
/// However, if derives are on SEPARATE lines like:
/// ```
/// #[derive(Debug, Clone)]
/// #[derive(Model)]
/// ```
/// Then we CAN detect them because the first #[derive(...)] attribute is still present.
/// 
/// This function checks for traits in any remaining #[derive(...)] attributes.
fn detect_existing_derives(attrs: &[Attribute]) -> ExistingDerives {
    let mut existing = ExistingDerives::default();
    
    for attr in attrs {
        if attr.path().is_ident("derive") {
            if let Meta::List(list) = &attr.meta {
                let tokens_str = list.tokens.to_string();
                // Check for each trait in the derive list
                // Use word boundaries to avoid false matches
                if tokens_str.contains("Debug") {
                    existing.has_debug = true;
                }
                if tokens_str.contains("Clone") {
                    existing.has_clone = true;
                }
                if tokens_str.contains("Default") {
                    existing.has_default = true;
                }
                if tokens_str.contains("Serialize") && !tokens_str.contains("Deserialize") {
                    existing.has_serialize = true;
                }
                if tokens_str.contains("Deserialize") {
                    existing.has_deserialize = true;
                    // Deserialize contains "Serialize" so check both
                    if tokens_str.matches("Serialize").count() > tokens_str.matches("Deserialize").count() {
                        existing.has_serialize = true;
                    }
                }
                // More robust check for Serialize
                for part in tokens_str.split(',') {
                    let part = part.trim();
                    if part == "Serialize" || part.ends_with("::Serialize") || part.starts_with("serde::Serialize") {
                        existing.has_serialize = true;
                    }
                }
            }
        }
    }
    
    existing
}

fn generate_model_impl(input: &ModelInput, indexes: Vec<IndexDef>, unique_indexes: Vec<IndexDef>, existing_derives: &ExistingDerives) -> proc_macro2::TokenStream {
    let struct_name = &input.ident;
    let table_name = input.table.clone().unwrap_or_else(|| {
        let name = struct_name.to_string().to_case(Case::Snake);
        pluralize(&name)
    });
    
    let schema_name = input.schema.clone().unwrap_or_else(|| "public".to_string());
    let soft_delete_enabled = input.soft_delete;
    
    // Auto-derive control flags
    // By default, ALL common traits are auto-generated to simplify model definitions.
    // Users only need #[derive(Model)] - no need for Debug, Clone, Serialize, Deserialize.
    //
    // The macro automatically detects if a trait is already derived and skips it to avoid conflicts.
    // Users can explicitly skip generation with skip_* flags if needed.
    //
    // Generated by default (unless already derived or explicitly skipped):
    // - Default (required for internal operations)
    // - Debug
    // - Clone  
    // - Serialize
    // - Deserialize
    
    let should_gen_debug = !input.skip_derives && !input.skip_debug && !existing_derives.has_debug;
    let should_gen_clone = !input.skip_derives && !input.skip_clone && !existing_derives.has_clone;
    let should_gen_default = !existing_derives.has_default;
    let should_gen_serialize = !input.skip_derives && !input.skip_serialize && !existing_derives.has_serialize;
    let should_gen_deserialize = !input.skip_derives && !input.skip_deserialize && !existing_derives.has_deserialize;
    
    let hidden_attrs: Vec<String> = input.hidden.as_ref()
        .map(|s| s.split(',').map(|v| v.trim().to_string()).collect())
        .unwrap_or_else(|| vec!["deleted_at".to_string()]);
    
    let translatable_fields: Vec<String> = input.translatable.as_ref()
        .map(|s| s.split(',').map(|v| v.trim().to_string()).collect())
        .unwrap_or_default();
    
    let has_custom_languages = input.languages.is_some();
    let allowed_languages: Vec<String> = input.languages.as_ref()
        .map(|s| s.split(',').map(|v| v.trim().to_string()).collect())
        .unwrap_or_default();
    
    let has_custom_fallback = input.fallback_language.is_some();
    let fallback_language = input.fallback_language.clone().unwrap_or_default();
    
    let has_one_files: Vec<String> = input.has_one_files.as_ref()
        .map(|s| s.split(',').map(|v| v.trim().to_string()).collect())
        .unwrap_or_default();
    
    let has_many_files: Vec<String> = input.has_many_files.as_ref()
        .map(|s| s.split(',').map(|v| v.trim().to_string()).collect())
        .unwrap_or_default();
    
    let searchable_fields: Vec<String> = input.searchable.as_ref()
        .map(|s| s.split(',').map(|v| v.trim().to_string()).collect())
        .unwrap_or_default();
    
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
    
    // Separate relation fields from database fields
    let db_fields: Vec<_> = fields.iter()
        .filter(|f| !f.skip && !f.is_relation() && !f.is_relation_type())
        .collect();
    
    let relation_fields: Vec<_> = fields.iter()
        .filter(|f| f.is_relation() || f.is_relation_type())
        .collect();
    
    // Parse validation rules
    let validation_rules: Vec<_> = db_fields.iter()
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
    let pk_field = db_fields.iter().find(|f| f.primary_key);
    let pk_ident = pk_field
        .and_then(|f| f.ident.as_ref())
        .cloned()
        .unwrap_or_else(|| format_ident!("id"));
    let pk_type = pk_field
        .map(|f| &f.ty)
        .cloned()
        .unwrap_or_else(|| syn::parse_quote!(i64));
    
    // Generate column names and field mappings (only for DB fields)
    let field_names: Vec<_> = db_fields.iter()
        .filter_map(|f| f.ident.as_ref())
        .collect();
    
    let field_types: Vec<_> = db_fields.iter()
        .map(|f| &f.ty)
        .collect();
    
    let column_names: Vec<_> = db_fields.iter()
        .map(|f| {
            f.column.clone().unwrap_or_else(|| {
                f.ident
                    .as_ref()
                    .map(|i| i.to_string().to_case(Case::Snake))
                    .unwrap_or_default()
            })
        })
        .collect();
    
    // Generate column enum variants
    let column_variants: Vec<_> = db_fields.iter()
        .filter_map(|f| f.ident.as_ref())
        .map(|i| format_ident!("{}", i.to_string().to_case(Case::Pascal)))
        .collect();
    
    // Primary key info
    let pk_column_variant = format_ident!("{}", pk_ident.to_string().to_case(Case::Pascal));
    let pk_column_name = pk_field
        .and_then(|f| f.column.clone())
        .unwrap_or_else(|| pk_ident.to_string().to_case(Case::Snake));
    let pk_auto_increment = pk_field.map(|f| f.auto_increment).unwrap_or(false);
    
    // Detect timestamp fields
    let has_created_at = db_fields.iter().any(|f| {
        f.ident.as_ref().map(|i| i.to_string() == "created_at").unwrap_or(false)
    });
    let has_updated_at = db_fields.iter().any(|f| {
        f.ident.as_ref().map(|i| i.to_string() == "updated_at").unwrap_or(false)
    });
    let timestamps_enabled = input.timestamps || (has_created_at && has_updated_at);
    
    // Generate sync column attributes
    let sync_column_attrs: Vec<_> = db_fields.iter()
        .map(|f| {
            let mut attrs = Vec::new();
            
            if f.primary_key {
                attrs.push(quote! { col = col.primary_key(); });
            }
            if f.auto_increment {
                attrs.push(quote! { col = col.auto_increment(); });
            }
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
    
    // Generate active model field setters for INSERT (only DB fields)
    let insert_active_model_setters: Vec<_> = db_fields.iter()
        .map(|f| {
            let ident = f.ident.as_ref().unwrap();
            let field_name = ident.to_string();
            if f.primary_key && f.auto_increment {
                quote! {
                    #ident: ActiveValue::NotSet
                }
            } else if field_name == "created_at" || field_name == "updated_at" {
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
    
    // Generate active model field setters for UPDATE (only DB fields)
    let update_active_model_setters: Vec<_> = db_fields.iter()
        .map(|f| {
            let ident = f.ident.as_ref().unwrap();
            let field_name = ident.to_string();
            if f.primary_key {
                quote! {
                    #ident: ActiveValue::Unchanged(self.#ident)
                }
            } else if field_name == "updated_at" {
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
    
    // Generate relation initialization code (currently relation fields use Default::default())
    // This is kept for future enhancement to auto-wire relation contexts
    let _relation_inits: Vec<_> = relation_fields.iter()
        .filter_map(|f| {
            let ident = f.ident.as_ref()?;
            let _ty = &f.ty;
            
            // For has_one relations
            if let Some(ref _related) = f.has_one {
                let fk = f.foreign_key.as_deref().unwrap_or("id");
                let lk = f.local_key.as_deref().unwrap_or("id");
                return Some(quote! {
                    #ident: ::tideorm::relations::HasOne::new(#fk, #lk)
                        .with_parent_pk(::serde_json::json!(self.#pk_ident))
                });
            }
            
            // For has_many relations
            if let Some(ref _related) = f.has_many {
                let fk = f.foreign_key.as_deref().unwrap_or("id");
                let lk = f.local_key.as_deref().unwrap_or("id");
                return Some(quote! {
                    #ident: ::tideorm::relations::HasMany::new(#fk, #lk)
                        .with_parent_pk(::serde_json::json!(self.#pk_ident))
                });
            }
            
            // For belongs_to relations
            if let Some(ref _related) = f.belongs_to {
                let fk = f.foreign_key.as_deref().unwrap_or("id");
                let ok = f.owner_key.as_deref().unwrap_or("id");
                // Get the FK field value
                let fk_ident = format_ident!("{}", fk);
                return Some(quote! {
                    #ident: ::tideorm::relations::BelongsTo::new(#fk, #ok)
                        .with_fk_value(::serde_json::json!(self.#fk_ident))
                });
            }
            
            // Default: use Default::default()
            Some(quote! {
                #ident: Default::default()
            })
        })
        .collect();
    
    // Generate internal SeaORM entity module name
    let internal_entity_mod = format_ident!("__tideorm_internal_{}", struct_name.to_string().to_lowercase());
    
    // Generate SeaORM field definitions (only DB fields)
    let sea_orm_field_defs: Vec<_> = db_fields.iter()
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
            attrs.push(quote!(column_name = #column_name));
            
            let sea_orm_attr = quote!(#[sea_orm(#(#attrs),*)]);
            
            quote! {
                #sea_orm_attr
                pub #ident: #ty
            }
        })
        .collect();
    
    // All field names for struct conversion (DB fields only)
    let all_field_names: Vec<_> = db_fields.iter()
        .filter_map(|f| f.ident.as_ref())
        .collect();
    
    // Generate Default impl field initializers for ALL fields (both DB and relation fields)
    let default_field_inits: Vec<_> = fields.iter()
        .filter_map(|f| {
            let ident = f.ident.as_ref()?;
            Some(quote! {
                #ident: Default::default()
            })
        })
        .collect();
    
    // Collect ALL field names and types for auto-derive implementations
    let all_fields_for_derives: Vec<_> = fields.iter()
        .filter_map(|f| {
            let ident = f.ident.as_ref()?;
            let ty = &f.ty;
            Some((ident.clone(), ty.clone()))
        })
        .collect();
    
    let derive_field_names: Vec<_> = all_fields_for_derives.iter().map(|(i, _)| i.clone()).collect();
    let derive_field_names_str: Vec<_> = derive_field_names.iter().map(|i| i.to_string()).collect();
    
    let base_impl = quote! {
        // Internal SeaORM entity
        #[doc(hidden)]
        #[allow(non_snake_case, dead_code, unused_imports, clippy::all)]
        mod #internal_entity_mod {
            use ::tideorm::sea_orm::entity::prelude::*;
            use ::tideorm::sea_orm::{ActiveValue, DeriveEntity, DeriveModel, DeriveActiveModel};
            
            #[derive(Copy, Clone, Default, Debug, DeriveEntity)]
            pub struct Entity;
            
            impl EntityName for Entity {
                fn table_name(&self) -> &'static str {
                    #table_name
                }
            }
            
            #[derive(Clone, Debug, PartialEq, DeriveModel, DeriveActiveModel)]
            pub struct Model {
                #(#sea_orm_field_defs),*
            }
            
            #[derive(Copy, Clone, Debug, EnumIter, DeriveColumn)]
            pub enum Column {
                #(#column_variants),*
            }
            
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
            
            impl ColumnTrait for Column {
                type EntityName = Entity;
                
                fn def(&self) -> ColumnDef {
                    match self {
                        #(Self::#column_variants => ColumnType::String(StringLen::None).def()),*
                    }
                }
            }
            
            // Empty relation enum - relations are handled by TideORM directly
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
                pub fn model_allowed_languages() -> Vec<String> {
                    vec![#(#allowed_languages.to_string()),*]
                }
            }
        }
    } else {
        quote! {}
    };
    
    let fallback_override = if has_custom_fallback {
        quote! {
            impl #struct_name {
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
                    
                    if let Err(custom_errors) = self.custom_validations() {
                        errors.merge(custom_errors);
                    }
                    
                    errors.to_result()
                }
            }
        }
    } else {
        quote! {
            impl ::tideorm::validation::Validate for #struct_name {
                fn validate(&self) -> Result<(), ::tideorm::validation::ValidationErrors> {
                    self.custom_validations()
                }
            }
        }
    };
    
    let base_output = quote! {
        #base_impl
        
        #language_override
        #fallback_override
        
        // Internal adapter implementation
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
                    #(#all_field_names: model.#all_field_names),*,
                    // Initialize relation fields with defaults
                    ..Default::default()
                }
            }
        }
        
        impl #struct_name {
            #[doc(hidden)]
            fn __into_update_active_model(self) -> #internal_entity_mod::ActiveModel {
                use ::tideorm::sea_orm::ActiveValue;
                #internal_entity_mod::ActiveModel {
                    #(#update_active_model_setters),*
                }
            }
            
            #[doc(hidden)]
            fn __into_delete_active_model(self) -> #internal_entity_mod::ActiveModel {
                use ::tideorm::sea_orm::ActiveValue;
                #internal_entity_mod::ActiveModel {
                    #pk_ident: ActiveValue::Unchanged(self.#pk_ident),
                    ..Default::default()
                }
            }
            
            /// Initialize relation fields with parent context
            pub fn with_relations(mut self) -> Self {
                // This would be called after loading from DB to set up relation contexts
                self
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
                
                let table = #table_name;
                let pk_col_name = #pk_column_name;
                let pk_is_auto_increment = #pk_auto_increment;
                
                // All columns and values
                let all_columns: Vec<&str> = vec![#(#column_names),*];
                let model_clone = model.clone();
                let all_values: Vec<serde_json::Value> = vec![
                    #(json!(model_clone.#field_names)),*
                ];
                
                // Determine if we should include the PK column
                // Include PK if: it's in conflict columns, OR it's not auto-increment
                let include_pk = builder.conflict_columns.contains(&pk_col_name.to_string()) || !pk_is_auto_increment;
                
                // Filter columns and values based on whether to include PK
                let (columns, values): (Vec<&str>, Vec<serde_json::Value>) = all_columns.iter()
                    .zip(all_values.into_iter())
                    .filter(|(col, _)| {
                        if *col == &pk_col_name && pk_is_auto_increment && !include_pk {
                            false
                        } else {
                            true
                        }
                    })
                    .map(|(col, val)| (*col, val))
                    .unzip();
                
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
                
                let conflict_cols = builder.conflict_columns;
                let conflict_list = conflict_cols.iter()
                    .map(|c| format!("\"{}\"", c))
                    .collect::<Vec<_>>()
                    .join(", ");
                
                let update_cols: Vec<String> = if let Some(cols) = builder.update_columns {
                    cols
                } else if let Some(exclude) = builder.exclude_columns {
                    columns.iter()
                        .filter(|c| !exclude.contains(&c.to_string()))
                        .map(|c| c.to_string())
                        .collect()
                } else {
                    // Default: update all columns except conflict columns and primary key
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
                
                let results: Vec<Self> = ::tideorm::Database::raw(&sql).await?;
                
                results.into_iter().next().ok_or_else(|| {
                    ::tideorm::Error::query("INSERT ... ON CONFLICT returned no rows".to_string())
                })
            }
        }
        
        // Register model for schema synchronization
        impl #struct_name {
            #[doc(hidden)]
            pub fn __get_sync_schema() -> ::tideorm::sync::ModelSchema {
                use ::tideorm::sync::{ModelSchema, ColumnDef, normalize_rust_type};
                
                let mut schema = ModelSchema::new(#table_name).schema(#schema_name);
                
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
            
            #[doc(hidden)]
            #[inline]
            pub fn __register_for_sync() {
                ::tideorm::sync::SyncRegistry::register(Self::__get_sync_schema());
            }
        }
        
        impl ::tideorm::sync::SyncModel for #struct_name {
            fn sync_schema() -> ::tideorm::sync::ModelSchema {
                Self::__get_sync_schema()
            }
        }
        
        #validation_impl
    };
    
    // Auto-generated Default impl (only if auto_derives or auto_default is set)
    let default_impl = if should_gen_default {
        quote! {
            impl ::std::default::Default for #struct_name {
                fn default() -> Self {
                    Self {
                        #(#default_field_inits),*
                    }
                }
            }
        }
    } else {
        quote! {}
    };
    
    // Auto-generated Debug impl (only if auto_derives or auto_debug is set)
    let debug_impl = if should_gen_debug {
        quote! {
            impl ::std::fmt::Debug for #struct_name {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    f.debug_struct(stringify!(#struct_name))
                        #(.field(#derive_field_names_str, &self.#derive_field_names))*
                        .finish()
                }
            }
        }
    } else {
        quote! {}
    };
    
    // Auto-generated Clone impl (only if auto_derives or auto_clone is set)
    let clone_impl = if should_gen_clone {
        quote! {
            impl ::std::clone::Clone for #struct_name {
                fn clone(&self) -> Self {
                    Self {
                        #(#derive_field_names: self.#derive_field_names.clone()),*
                    }
                }
            }
        }
    } else {
        quote! {}
    };
    
    // Auto-generated Serialize impl (only if auto_derives or auto_serialize is set)
    let serialize_impl = if should_gen_serialize {
        let field_count = derive_field_names.len();
        quote! {
            impl ::serde::Serialize for #struct_name {
                fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
                where
                    S: ::serde::Serializer,
                {
                    use ::serde::ser::SerializeStruct;
                    let mut state = serializer.serialize_struct(stringify!(#struct_name), #field_count)?;
                    #(state.serialize_field(#derive_field_names_str, &self.#derive_field_names)?;)*
                    state.end()
                }
            }
        }
    } else {
        quote! {}
    };
    
    // Auto-generated Deserialize impl (only if auto_derives or auto_deserialize is set)
    let deserialize_impl = if should_gen_deserialize {
        let field_count = derive_field_names.len();
        let field_indices: Vec<_> = (0..field_count).collect();
        let field_names_upper: Vec<_> = derive_field_names.iter()
            .map(|i| format_ident!("__field_{}", i))
            .collect();
        
        quote! {
            impl<'de> ::serde::Deserialize<'de> for #struct_name {
                fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
                where
                    D: ::serde::Deserializer<'de>,
                {
                    #[allow(non_camel_case_types)]
                    enum __Field {
                        #(#field_names_upper,)*
                        __ignore,
                    }
                    
                    struct __FieldVisitor;
                    
                    impl<'de> ::serde::de::Visitor<'de> for __FieldVisitor {
                        type Value = __Field;
                        
                        fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                            formatter.write_str("field identifier")
                        }
                        
                        fn visit_str<E>(self, value: &str) -> ::std::result::Result<__Field, E>
                        where
                            E: ::serde::de::Error,
                        {
                            match value {
                                #(#derive_field_names_str => Ok(__Field::#field_names_upper),)*
                                _ => Ok(__Field::__ignore),
                            }
                        }
                    }
                    
                    impl<'de> ::serde::Deserialize<'de> for __Field {
                        fn deserialize<D>(deserializer: D) -> ::std::result::Result<__Field, D::Error>
                        where
                            D: ::serde::Deserializer<'de>,
                        {
                            deserializer.deserialize_identifier(__FieldVisitor)
                        }
                    }
                    
                    struct __Visitor;
                    
                    impl<'de> ::serde::de::Visitor<'de> for __Visitor {
                        type Value = #struct_name;
                        
                        fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                            formatter.write_str(concat!("struct ", stringify!(#struct_name)))
                        }
                        
                        fn visit_map<A>(self, mut map: A) -> ::std::result::Result<#struct_name, A::Error>
                        where
                            A: ::serde::de::MapAccess<'de>,
                        {
                            #(let mut #field_names_upper: Option<_> = None;)*
                            
                            while let Some(key) = map.next_key()? {
                                match key {
                                    #(__Field::#field_names_upper => {
                                        if #field_names_upper.is_some() {
                                            return Err(::serde::de::Error::duplicate_field(#derive_field_names_str));
                                        }
                                        #field_names_upper = Some(map.next_value()?);
                                    })*
                                    __Field::__ignore => {
                                        let _ = map.next_value::<::serde::de::IgnoredAny>()?;
                                    }
                                }
                            }
                            
                            Ok(#struct_name {
                                #(#derive_field_names: #field_names_upper.unwrap_or_default()),*
                            })
                        }
                        
                        fn visit_seq<A>(self, mut seq: A) -> ::std::result::Result<#struct_name, A::Error>
                        where
                            A: ::serde::de::SeqAccess<'de>,
                        {
                            #(
                                let #field_names_upper = seq.next_element()?
                                    .ok_or_else(|| ::serde::de::Error::invalid_length(#field_indices, &self))?;
                            )*
                            
                            Ok(#struct_name {
                                #(#derive_field_names: #field_names_upper),*
                            })
                        }
                    }
                    
                    const FIELDS: &'static [&'static str] = &[#(#derive_field_names_str),*];
                    deserializer.deserialize_struct(stringify!(#struct_name), FIELDS, __Visitor)
                }
            }
        }
    } else {
        quote! {}
    };
    
    quote! {
        #base_output
        
        #default_impl
        #debug_impl
        #clone_impl
        #serialize_impl
        #deserialize_impl
    }
}

/// Simple pluralization
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
// LEGACY RELATION MACROS (for backward compatibility)
// =============================================================================

/// Derive BelongsTo relation for a model (legacy attribute macro)
///
/// This is kept for backward compatibility. The recommended approach is to
/// define relations inside the model struct using field attributes.
#[proc_macro_attribute]
pub fn belongs_to(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_clone = item.clone();
    let input = parse_macro_input!(item_clone as DeriveInput);
    let _struct_name = &input.ident;
    
    let attr_str = attr.to_string();
    let (related_type, _foreign_key, owner_key) = parse_relation_attr(&attr_str);
    
    let _related_ident: proc_macro2::TokenStream = related_type.parse().unwrap();
    
    let _owner_key_impl = if let Some(ok) = owner_key {
        quote! {
            fn owner_key() -> &'static str { #ok }
        }
    } else {
        quote! {}
    };
    
    let impl_block = quote! {
        // Legacy trait-based relation for backward compatibility
    };
    
    let original: proc_macro2::TokenStream = item.into();
    let expanded = quote! {
        #original
        #impl_block
    };
    
    TokenStream::from(expanded)
}

/// Derive HasOne relation for a model (legacy attribute macro)
#[proc_macro_attribute]
pub fn has_one(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_clone = item.clone();
    let input = parse_macro_input!(item_clone as DeriveInput);
    let _struct_name = &input.ident;
    
    let attr_str = attr.to_string();
    let (related_type, _foreign_key, local_key) = parse_relation_attr(&attr_str);
    
    let _related_ident: proc_macro2::TokenStream = related_type.parse().unwrap();
    
    let _local_key_impl = if let Some(lk) = local_key {
        quote! {
            fn local_key() -> &'static str { #lk }
        }
    } else {
        quote! {}
    };
    
    let impl_block = quote! {
        // Legacy trait-based relation for backward compatibility
    };
    
    let original: proc_macro2::TokenStream = item.into();
    let expanded = quote! {
        #original
        #impl_block
    };
    
    TokenStream::from(expanded)
}

/// Derive HasMany relation for a model (legacy attribute macro)
#[proc_macro_attribute]
pub fn has_many(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_clone = item.clone();
    let input = parse_macro_input!(item_clone as DeriveInput);
    let _struct_name = &input.ident;
    
    let attr_str = attr.to_string();
    let (related_type, _foreign_key, local_key) = parse_relation_attr(&attr_str);
    
    let _related_ident: proc_macro2::TokenStream = related_type.parse().unwrap();
    
    let _local_key_impl = if let Some(lk) = local_key {
        quote! {
            fn local_key() -> &'static str { #lk }
        }
    } else {
        quote! {}
    };
    
    let impl_block = quote! {
        // Legacy trait-based relation for backward compatibility
    };
    
    let original: proc_macro2::TokenStream = item.into();
    let expanded = quote! {
        #original
        #impl_block
    };
    
    TokenStream::from(expanded)
}

/// Parse relation attribute string
fn parse_relation_attr(attr: &str) -> (String, String, Option<String>) {
    let attr = attr.trim();
    
    let mut parts = attr.splitn(2, ',');
    
    let related_type = parts.next()
        .unwrap_or("")
        .trim()
        .to_string();
    
    let rest = parts.next().unwrap_or("");
    
    let mut foreign_key = String::new();
    let mut optional_key: Option<String> = None;
    
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

/// Attribute macro for defining TideORM models (SeaORM 2.0 style).
///
/// This is the recommended way to define models, similar to SeaORM 2.0's `#[sea_orm::model]`.
/// It automatically adds the `#[derive(Model)]` along with other common derives.
///
/// # Example
///
/// ```rust,ignore
/// use tideorm::prelude::*;
///
/// #[tideorm::model]
/// #[tide(table = "users")]
/// pub struct User {
///     #[tide(primary_key, auto_increment)]
///     pub id: i64,
///     pub name: String,
///     pub email: String,
/// }
/// ```
///
/// This is equivalent to:
/// ```rust,ignore
/// #[derive(Model)]
/// #[tide(table = "users")]
/// pub struct User {
///     // ...
/// }
/// ```
///
/// The macro automatically implements:
/// - `Debug` - for printing/logging
/// - `Clone` - for cloning instances
/// - `Default` - for creating default instances
/// - `Serialize` - for JSON serialization
/// - `Deserialize` - for JSON deserialization
#[proc_macro_attribute]
pub fn model(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let name = &input.ident;
    let vis = &input.vis;
    let attrs = &input.attrs;
    let generics = &input.generics;
    
    // Get struct fields
    let fields = match &input.data {
        syn::Data::Struct(data) => &data.fields,
        _ => {
            return syn::Error::new_spanned(
                &input,
                "#[tideorm::model] can only be applied to structs"
            ).to_compile_error().into();
        }
    };
    
    // Preserve all attributes except derive (we'll add our own)
    let other_attrs: Vec<_> = attrs.iter()
        .filter(|a| !a.path().is_ident("derive"))
        .collect();
    
    // Generate the struct with derive(Model)
    quote! {
        #[derive(tideorm::Model)]
        #(#other_attrs)*
        #vis struct #name #generics #fields
    }.into()
}
