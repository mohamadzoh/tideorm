use darling::{FromDeriveInput, FromField, ast::Data};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Attribute, GenericArgument, Ident, Meta, PathArguments, Type};

#[derive(Debug, Clone, FromField)]
#[darling(attributes(tideorm), forward_attrs(validate))]
pub(crate) struct ModelField {
    pub(crate) ident: Option<Ident>,
    pub(crate) ty: Type,
    pub(crate) attrs: Vec<syn::Attribute>,
    #[darling(default)]
    pub(crate) primary_key: bool,
    #[darling(default)]
    pub(crate) auto_increment: bool,
    #[darling(default)]
    pub(crate) column: Option<String>,
    #[darling(default)]
    pub(crate) nullable: bool,
    #[darling(default)]
    pub(crate) default: Option<String>,
    #[darling(default)]
    pub(crate) skip: bool,
    #[darling(default)]
    pub(crate) has_one: Option<String>,
    #[darling(default)]
    pub(crate) has_many: Option<String>,
    #[darling(default)]
    pub(crate) belongs_to: Option<String>,
    #[darling(default)]
    pub(crate) has_many_through: Option<String>,
    #[darling(default)]
    pub(crate) foreign_key: Option<String>,
    #[darling(default)]
    pub(crate) owner_key: Option<String>,
    #[darling(default)]
    pub(crate) local_key: Option<String>,
    #[darling(default)]
    pub(crate) pivot: Option<String>,
    #[darling(default)]
    pub(crate) related_key: Option<String>,
    #[darling(default)]
    pub(crate) morph_name: Option<String>,
}

impl ModelField {
    fn validation_base_ty(&self) -> &Type {
        validation_base_type(&self.ty)
    }

    pub(crate) fn validation_base_type(&self) -> String {
        let ty = self.validation_base_ty();
        quote!(#ty)
            .to_string()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect()
    }

    pub(crate) fn supports_string_validations(&self) -> bool {
        matches!(
            terminal_type_ident(self.validation_base_ty()).as_deref(),
            Some("String" | "str")
        )
    }

    pub(crate) fn supports_numeric_validations(&self) -> bool {
        matches!(
            terminal_type_ident(self.validation_base_ty()).as_deref(),
            Some(
                "i8" | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "isize"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "usize"
                    | "f32"
                    | "f64"
                    | "String"
                    | "str"
            )
        )
    }

    pub(crate) fn is_relation(&self) -> bool {
        self.has_one.is_some()
            || self.has_many.is_some()
            || self.belongs_to.is_some()
            || self.has_many_through.is_some()
    }

    pub(crate) fn is_relation_type(&self) -> bool {
        relation_wrapper_name(&self.ty)
            .map(|name| {
                matches!(
                    name,
                    "HasOne"
                        | "HasMany"
                        | "BelongsTo"
                        | "HasManyThrough"
                        | "MorphOne"
                        | "MorphMany"
                        | "MorphTo"
                        | "SelfRef"
                        | "SelfRefMany"
                )
            })
            .unwrap_or(false)
    }
    pub(crate) fn column_type_expr(&self) -> TokenStream2 {
        let ty = &self.ty;
        let ty_str = quote!(#ty).to_string();
        let ty_str: String = ty_str.chars().filter(|c| !c.is_whitespace()).collect();
        let is_nullable = ty_str.starts_with("Option<");
        let base_type = if is_nullable {
            ty_str
                .strip_prefix("Option<")
                .and_then(|s| s.strip_suffix('>'))
                .unwrap_or(&ty_str)
        } else {
            &ty_str
        };
        let base_type = canonical_schema_type(base_type);

        let column_type = match base_type.as_str() {
            "i8" | "i16" | "u8" | "u16" => quote!(::tideorm::orm::ColumnType::SmallInteger),
            "i32" | "u32" => quote!(::tideorm::orm::ColumnType::Integer),
            "i64" | "u64" => quote!(::tideorm::orm::ColumnType::BigInteger),
            "f32" => quote!(::tideorm::orm::ColumnType::Float),
            "f64" => quote!(::tideorm::orm::ColumnType::Double),
            "bool" => quote!(::tideorm::orm::ColumnType::Boolean),
            "String" | "&str" | "str" => quote!(::tideorm::orm::ColumnType::Text),
            "Uuid" | "uuid::Uuid" => quote!(::tideorm::orm::ColumnType::Uuid),
            s if s.contains("DateTime<Utc>")
                || s.contains("DateTime<chrono::Utc>")
                || s.contains("chrono::DateTime<Utc>")
                || s.contains("chrono::DateTime<chrono::Utc>") =>
            {
                quote!(::tideorm::orm::ColumnType::TimestampWithTimeZone)
            }
            "DateTime" | "NaiveDateTime" | "chrono::NaiveDateTime" => {
                quote!(::tideorm::orm::ColumnType::DateTime)
            }
            "NaiveDate" | "chrono::NaiveDate" => quote!(::tideorm::orm::ColumnType::Date),
            "NaiveTime" | "chrono::NaiveTime" => quote!(::tideorm::orm::ColumnType::Time),
            "Decimal" | "rust_decimal::Decimal" => {
                quote!(::tideorm::orm::ColumnType::Decimal(None))
            }
            "Json" | "JsonValue" | "Value" | "serde_json::Value" | "Jsonb" => {
                quote!(::tideorm::orm::ColumnType::Json)
            }
            "Vec<u8>" => quote!(::tideorm::orm::ColumnType::Binary(
                ::tideorm::orm::sea_query::BlobSize::Blob(None)
            )),
            "Vec<i32>" | "IntArray" => quote!(::tideorm::orm::ColumnType::Array(
                ::tideorm::orm::sea_query::RcOrArc::new(::tideorm::orm::ColumnType::Integer)
            )),
            "Vec<i64>" | "BigIntArray" => quote!(::tideorm::orm::ColumnType::Array(
                ::tideorm::orm::sea_query::RcOrArc::new(::tideorm::orm::ColumnType::BigInteger)
            )),
            "Vec<String>" | "TextArray" => quote!(::tideorm::orm::ColumnType::Array(
                ::tideorm::orm::sea_query::RcOrArc::new(::tideorm::orm::ColumnType::Text)
            )),
            "Vec<bool>" | "BoolArray" => quote!(::tideorm::orm::ColumnType::Array(
                ::tideorm::orm::sea_query::RcOrArc::new(::tideorm::orm::ColumnType::Boolean)
            )),
            "Vec<f64>" | "FloatArray" => quote!(::tideorm::orm::ColumnType::Array(
                ::tideorm::orm::sea_query::RcOrArc::new(::tideorm::orm::ColumnType::Double)
            )),
            _ => {
                let message = format!(
                    "unsupported TideORM column type '{}' in schema generation; set an explicit column type or use a supported Rust type",
                    base_type
                );
                quote!({
                    ::core::compile_error!(#message);
                    ::tideorm::orm::ColumnType::Text
                })
            }
        };

        if is_nullable || self.nullable {
            quote!(#column_type.def().nullable())
        } else {
            quote!(#column_type.def())
        }
    }
}

fn canonical_schema_type(ty: &str) -> String {
    let normalized = ty.trim();

    for alias in [
        "Json",
        "JsonValue",
        "JsonArray",
        "Jsonb",
        "IntArray",
        "BigIntArray",
        "TextArray",
        "BoolArray",
        "FloatArray",
        "Decimal",
        "Uuid",
        "NaiveDate",
        "NaiveTime",
        "NaiveDateTime",
        "Text",
    ] {
        if normalized == alias || normalized.ends_with(&format!("::{}", alias)) {
            return alias.to_string();
        }
    }

    normalized.to_string()
}

fn validation_base_type(ty: &Type) -> &Type {
    if let Some(inner) = option_inner_type(ty) {
        inner
    } else {
        ty
    }
}

fn option_inner_type(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Group(group) => option_inner_type(&group.elem),
        Type::Paren(paren) => option_inner_type(&paren.elem),
        Type::Path(type_path) => {
            let segment = type_path.path.segments.last()?;
            if segment.ident != "Option" {
                return None;
            }

            match &segment.arguments {
                PathArguments::AngleBracketed(args) => args.args.iter().find_map(|arg| match arg {
                    GenericArgument::Type(inner) => Some(inner),
                    _ => None,
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

fn terminal_type_ident(ty: &Type) -> Option<String> {
    match ty {
        Type::Group(group) => terminal_type_ident(&group.elem),
        Type::Paren(paren) => terminal_type_ident(&paren.elem),
        Type::Reference(reference) => terminal_type_ident(&reference.elem),
        Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

pub(crate) fn relation_wrapper_name(ty: &Type) -> Option<&str> {
    match ty {
        Type::Group(group) => relation_wrapper_name(&group.elem),
        Type::Paren(paren) => relation_wrapper_name(&paren.elem),
        Type::Reference(reference) => relation_wrapper_name(&reference.elem),
        Type::Path(type_path) => {
            let segment = type_path.path.segments.last()?;
            let ident = segment.ident.to_string();
            if matches!(ident.as_str(), "Option" | "Box" | "Rc" | "Arc") {
                if let PathArguments::AngleBracketed(arguments) = &segment.arguments {
                    for argument in &arguments.args {
                        if let GenericArgument::Type(inner_ty) = argument {
                            if let Some(name) = relation_wrapper_name(inner_ty) {
                                return Some(name);
                            }
                        }
                    }
                }
            }
            Some(match ident.as_str() {
                "HasOne" => "HasOne",
                "HasMany" => "HasMany",
                "BelongsTo" => "BelongsTo",
                "HasManyThrough" => "HasManyThrough",
                "MorphOne" => "MorphOne",
                "MorphMany" => "MorphMany",
                "MorphTo" => "MorphTo",
                "SelfRef" => "SelfRef",
                "SelfRefMany" => "SelfRefMany",
                _ => return None,
            })
        }
        _ => None,
    }
}

pub(crate) fn relation_generic_types(ty: &Type) -> Vec<Type> {
    match ty {
        Type::Group(group) => relation_generic_types(&group.elem),
        Type::Paren(paren) => relation_generic_types(&paren.elem),
        Type::Reference(reference) => relation_generic_types(&reference.elem),
        Type::Path(type_path) => {
            let Some(segment) = type_path.path.segments.last() else {
                return Vec::new();
            };

            let ident = segment.ident.to_string();
            if matches!(ident.as_str(), "Option" | "Box" | "Rc" | "Arc") {
                if let PathArguments::AngleBracketed(arguments) = &segment.arguments {
                    for argument in &arguments.args {
                        if let GenericArgument::Type(inner_ty) = argument {
                            return relation_generic_types(inner_ty);
                        }
                    }
                }
                return Vec::new();
            }

            if !matches!(
                ident.as_str(),
                "HasOne"
                    | "HasMany"
                    | "BelongsTo"
                    | "HasManyThrough"
                    | "MorphOne"
                    | "MorphMany"
                    | "MorphTo"
                    | "SelfRef"
                    | "SelfRefMany"
            ) {
                return Vec::new();
            }

            match &segment.arguments {
                PathArguments::AngleBracketed(arguments) => arguments
                    .args
                    .iter()
                    .filter_map(|argument| match argument {
                        GenericArgument::Type(inner_ty) => Some(inner_ty.clone()),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct IndexDef {
    pub(crate) name: Option<String>,
    pub(crate) columns: Vec<String>,
    pub(crate) unique: bool,
}

impl IndexDef {
    pub(crate) fn from_columns(columns: &str, unique: bool) -> Self {
        Self {
            name: None,
            columns: columns.split(',').map(|s| s.trim().to_string()).collect(),
            unique,
        }
    }

    pub(crate) fn from_named(name: String, columns: &str, unique: bool) -> Self {
        Self {
            name: Some(name),
            columns: columns.split(',').map(|s| s.trim().to_string()).collect(),
            unique,
        }
    }

    pub(crate) fn get_name(&self, table_name: &str) -> String {
        self.name.clone().unwrap_or_else(|| {
            let prefix = if self.unique { "uidx" } else { "idx" };
            format!("{}_{}_{}", prefix, table_name, self.columns.join("_"))
        })
    }
}

pub(crate) fn parse_index_attributes(attrs: &[Attribute]) -> (Vec<IndexDef>, Vec<IndexDef>) {
    let mut indexes = Vec::new();
    let mut unique_indexes = Vec::new();
    for attr in attrs {
        let is_index = attr.path().is_ident("index");
        let is_unique_index = attr.path().is_ident("unique_index");
        if !is_index && !is_unique_index {
            continue;
        }
        let unique = is_unique_index;
        if let Meta::List(list) = &attr.meta {
            let tokens = list.tokens.to_string();
            if tokens.contains("name") && tokens.contains("columns") {
                let mut name = None;
                let mut columns = None;
                let _ = attr.parse_nested_meta(|nested| {
                    if nested.path.is_ident("name") {
                        name = Some(nested.value()?.parse::<syn::LitStr>()?.value());
                    } else if nested.path.is_ident("columns") {
                        columns = Some(nested.value()?.parse::<syn::LitStr>()?.value());
                    }
                    Ok(())
                });
                if let Some(cols) = columns {
                    let idx = if let Some(name) = name {
                        IndexDef::from_named(name, &cols, unique)
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
    }
    (indexes, unique_indexes)
}

pub(crate) fn parse_validation_attributes(
    field_name: &str,
    field: &ModelField,
) -> syn::Result<Vec<TokenStream2>> {
    let mut rules = Vec::new();
    for attr in &field.attrs {
        if !attr.path().is_ident("validate") {
            continue;
        }
        if let Meta::List(list) = &attr.meta {
            for part in list.tokens.to_string().split(',').map(str::trim) {
                match part {
                    "required" => {
                        rules.push(quote!(::tideorm::validation::ValidationRule::Required))
                    }
                    "email" => {
                        ensure_validation_compatibility(field_name, field, attr, "email")?;
                        rules.push(quote!(::tideorm::validation::ValidationRule::Email));
                    }
                    "url" => {
                        ensure_validation_compatibility(field_name, field, attr, "url")?;
                        rules.push(quote!(::tideorm::validation::ValidationRule::Url));
                    }
                    "alpha" => {
                        ensure_validation_compatibility(field_name, field, attr, "alpha")?;
                        rules.push(quote!(::tideorm::validation::ValidationRule::Alpha));
                    }
                    "alphanumeric" => {
                        ensure_validation_compatibility(field_name, field, attr, "alphanumeric")?;
                        rules.push(quote!(::tideorm::validation::ValidationRule::Alphanumeric))
                    }
                    "numeric" => {
                        ensure_validation_compatibility(field_name, field, attr, "numeric")?;
                        rules.push(quote!(::tideorm::validation::ValidationRule::Numeric))
                    }
                    "uuid" => {
                        ensure_validation_compatibility(field_name, field, attr, "uuid")?;
                        rules.push(quote!(::tideorm::validation::ValidationRule::Uuid));
                    }
                    _ if part.starts_with("min_length") => {
                        ensure_validation_compatibility(field_name, field, attr, "min_length")?;
                        push_parsed_rule(
                            &mut rules,
                            part,
                            "min_length",
                            |n: usize| quote!(::tideorm::validation::ValidationRule::MinLength(#n)),
                        )
                    }
                    _ if part.starts_with("max_length") => {
                        ensure_validation_compatibility(field_name, field, attr, "max_length")?;
                        push_parsed_rule(
                            &mut rules,
                            part,
                            "max_length",
                            |n: usize| quote!(::tideorm::validation::ValidationRule::MaxLength(#n)),
                        )
                    }
                    _ if part.starts_with("length")
                        && !part.contains("min_")
                        && !part.contains("max_") =>
                    {
                        ensure_validation_compatibility(field_name, field, attr, "length")?;
                        push_parsed_rule(
                            &mut rules,
                            part,
                            "length",
                            |n: usize| quote!(::tideorm::validation::ValidationRule::Length(#n)),
                        )
                    }
                    _ if part.starts_with("min") && !part.contains("length") => {
                        ensure_validation_compatibility(field_name, field, attr, "min")?;
                        push_parsed_rule(
                            &mut rules,
                            part,
                            "min",
                            |n: f64| quote!(::tideorm::validation::ValidationRule::Min(#n)),
                        )
                    }
                    _ if part.starts_with("max") && !part.contains("length") => {
                        ensure_validation_compatibility(field_name, field, attr, "max")?;
                        push_parsed_rule(
                            &mut rules,
                            part,
                            "max",
                            |n: f64| quote!(::tideorm::validation::ValidationRule::Max(#n)),
                        )
                    }
                    _ if part.starts_with("range") => {
                        ensure_validation_compatibility(field_name, field, attr, "range")?;
                        if let Some(value) = extract_value(part, "range") {
                            let parts: Vec<_> = value.trim_matches('"').split("..").collect();
                            if parts.len() == 2 {
                                if let (Ok(min), Ok(max)) =
                                    (parts[0].parse::<f64>(), parts[1].parse::<f64>())
                                {
                                    rules.push(quote!(::tideorm::validation::ValidationRule::Range(#min, #max)));
                                }
                            }
                        }
                    }
                    _ if part.starts_with("regex") => {
                        ensure_validation_compatibility(field_name, field, attr, "regex")?;
                        if let Some(value) = extract_value(part, "regex") {
                            let pattern = value.trim_matches('"');
                            rules.push(quote!(::tideorm::validation::ValidationRule::Regex(#pattern.to_string())));
                        }
                    }
                    _ if part.starts_with("custom") => {
                        if let Some(value) = extract_value(part, "custom") {
                            let msg = value.trim_matches('"');
                            rules.push(quote!(::tideorm::validation::ValidationRule::Custom(#msg.to_string())));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(rules)
}

fn ensure_validation_compatibility(
    field_name: &str,
    field: &ModelField,
    attr: &Attribute,
    rule: &str,
) -> syn::Result<()> {
    let expects_string = matches!(
        rule,
        "email"
            | "url"
            | "alpha"
            | "alphanumeric"
            | "numeric"
            | "uuid"
            | "min_length"
            | "max_length"
            | "length"
            | "regex"
    );
    let expects_numeric = matches!(rule, "min" | "max" | "range");

    let compatible = if expects_string {
        field.supports_string_validations()
    } else if expects_numeric {
        field.supports_numeric_validations()
    } else {
        true
    };

    if compatible {
        return Ok(());
    }

    let expected = if expects_string {
        "a string field"
    } else if expects_numeric {
        "a numeric field or string field"
    } else {
        "a compatible field"
    };

    Err(syn::Error::new_spanned(
        attr,
        format!(
            "validation rule '{}' is incompatible with field '{}' of type '{}'; expected {}",
            rule,
            field_name,
            field.validation_base_type(),
            expected
        ),
    ))
}

fn push_parsed_rule<T, F>(rules: &mut Vec<TokenStream2>, input: &str, key: &str, build: F)
where
    T: std::str::FromStr,
    F: FnOnce(T) -> TokenStream2,
{
    if let Some(value) = extract_value(input, key) {
        if let Ok(parsed) = value.parse::<T>() {
            rules.push(build(parsed));
        }
    }
}

pub(crate) fn extract_value(input: &str, key: &str) -> Option<String> {
    let input = input.trim();
    if let Some(pos) = input.find('=') {
        let current = input[..pos].trim();
        if current == key {
            return Some(input[pos + 1..].trim().to_string());
        }
    }
    if let Some(inner) = input
        .strip_prefix(key)
        .map(str::trim_start)
        .and_then(|value| value.strip_prefix('('))
        .and_then(|value| value.strip_suffix(')'))
    {
        return Some(inner.trim().to_string());
    }
    None
}

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(tideorm), supports(struct_named))]
pub(crate) struct ModelInput {
    pub(crate) ident: Ident,
    pub(crate) data: Data<(), ModelField>,
    #[darling(default)]
    pub(crate) table: Option<String>,
    #[darling(default)]
    pub(crate) schema: Option<String>,
    #[darling(default)]
    pub(crate) soft_delete: bool,
    #[darling(default)]
    pub(crate) deleted_at_column: Option<String>,
    #[darling(default)]
    pub(crate) timestamps: bool,
    #[darling(default)]
    pub(crate) hidden: Option<String>,
    #[darling(default)]
    pub(crate) translatable: Option<String>,
    #[darling(default)]
    pub(crate) languages: Option<String>,
    #[darling(default)]
    pub(crate) fallback_language: Option<String>,
    #[darling(default)]
    pub(crate) has_one_files: Option<String>,
    #[darling(default)]
    pub(crate) has_many_files: Option<String>,
    #[darling(default)]
    pub(crate) searchable: Option<String>,
    #[darling(default)]
    pub(crate) skip_debug: bool,
    #[darling(default)]
    pub(crate) skip_clone: bool,
    #[darling(default)]
    pub(crate) skip_default: bool,
    #[darling(default)]
    pub(crate) skip_serialize: bool,
    #[darling(default)]
    pub(crate) skip_deserialize: bool,
    #[darling(default)]
    pub(crate) skip_derives: bool,
    #[darling(default)]
    #[allow(dead_code)]
    pub(crate) auto_derives: bool,
    #[darling(default)]
    #[allow(dead_code)]
    pub(crate) auto_debug: bool,
    #[darling(default)]
    #[allow(dead_code)]
    pub(crate) auto_clone: bool,
    #[darling(default)]
    #[allow(dead_code)]
    pub(crate) auto_default: bool,
    #[darling(default)]
    #[allow(dead_code)]
    pub(crate) auto_serialize: bool,
    #[darling(default)]
    #[allow(dead_code)]
    pub(crate) auto_deserialize: bool,
    #[darling(default)]
    pub(crate) tokenize: bool,
}
