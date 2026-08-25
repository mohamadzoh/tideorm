use convert_case::{Case, Casing};
use darling::{FromDeriveInput, FromField, ast::Data};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{GenericArgument, Ident, PathArguments, Type};

mod indexes;
mod validation;

pub(crate) use indexes::{IndexDef, parse_index_attributes};
pub(crate) use validation::parse_validation_attributes;

/// Returns the text of `ident` with any `r#` raw-identifier prefix removed.
///
/// Raw identifiers such as `r#type` stringify as `"r#type"`, which is neither a
/// legal column name nor a legal input to case conversion or `Ident::new`, so
/// every derived name must be built from the unraw form.
pub(crate) fn unraw_ident(ident: &Ident) -> String {
    ident.unraw().to_string()
}

/// Builds the PascalCase `Column` enum variant identifier for a model field.
///
/// Handles raw identifiers, so a `r#type` field yields the `Type` variant
/// instead of panicking on the invalid identifier `R#type`.
pub(crate) fn column_variant_ident(ident: &Ident) -> Ident {
    format_ident!("{}", unraw_ident(ident).to_case(Case::Pascal))
}

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
        let inner_ty = option_inner_type(&self.ty);
        let is_nullable = inner_ty.is_some();
        let base_ty = inner_ty.unwrap_or(&self.ty);
        let base_type: String = quote!(#base_ty)
            .to_string()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let base_type = canonical_schema_type(&base_type);

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

/// Returns the `T` of an `Option<T>` type, honouring parenthesised, grouped and
/// fully qualified spellings such as `std::option::Option<T>`.
///
/// This is the only supported nullability test: a textual `contains("Option")`
/// check misfires on names such as `OptionalMode` or `Vec<Option<String>>`.
pub(crate) fn option_inner_type(ty: &Type) -> Option<&Type> {
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
            if matches!(ident.as_str(), "Option" | "Box" | "Rc" | "Arc")
                && let PathArguments::AngleBracketed(arguments) = &segment.arguments
            {
                for argument in &arguments.args {
                    if let GenericArgument::Type(inner_ty) = argument
                        && let Some(name) = relation_wrapper_name(inner_ty)
                    {
                        return Some(name);
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
    pub(crate) encrypted: Option<String>,
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
    pub(crate) tokenize: bool,
}
