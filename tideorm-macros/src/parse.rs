use darling::{FromDeriveInput, FromField, ast::Data};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{GenericArgument, Ident, PathArguments, PathSegment, Type};

use crate::case::{to_pascal_case, to_snake_case};

mod indexes;
mod validation;

pub(crate) use indexes::{IndexDef, parse_field_index_attributes, parse_index_attributes};
pub(crate) use validation::parse_validation_attributes;

/// Returns the text of `ident` with any `r#` raw-identifier prefix removed.
///
/// Raw identifiers such as `r#type` stringify as `"r#type"`, which is neither a
/// legal column name nor a legal input to case conversion or `Ident::new`, so
/// every derived name must be built from the unraw form.
pub(crate) fn unraw_ident(ident: &Ident) -> String {
    ident.unraw().to_string()
}

/// Builds the PascalCase enum variant identifier (`Column`, `PrimaryKey`,
/// `Relation`) for a model field.
///
/// Handles raw identifiers, so a `r#type` field yields the `Type` variant
/// instead of panicking on the invalid identifier `R#type`.
pub(crate) fn variant_ident(ident: &Ident) -> Ident {
    match to_pascal_case(&unraw_ident(ident)).as_str() {
        // The one keyword PascalCase can produce, from a field named `self_`.
        "Self" => format_ident!("Self_"),
        name => format_ident!("{}", name),
    }
}

/// The relation wrapper types a model field can be declared with.
///
/// The wrapper type is what decides the relation kind; the `has_one = ".."`
/// style attributes only have to agree with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelationKind {
    HasOne,
    HasMany,
    BelongsTo,
    HasManyThrough,
    MorphOne,
    MorphMany,
    MorphTo,
    SelfRef,
    SelfRefMany,
}

impl RelationKind {
    const ALL: [Self; 9] = [
        Self::HasOne,
        Self::HasMany,
        Self::BelongsTo,
        Self::HasManyThrough,
        Self::MorphOne,
        Self::MorphMany,
        Self::MorphTo,
        Self::SelfRef,
        Self::SelfRefMany,
    ];

    /// The wrapper type's name.
    pub(crate) fn wrapper(self) -> &'static str {
        match self {
            Self::HasOne => "HasOne",
            Self::HasMany => "HasMany",
            Self::BelongsTo => "BelongsTo",
            Self::HasManyThrough => "HasManyThrough",
            Self::MorphOne => "MorphOne",
            Self::MorphMany => "MorphMany",
            Self::MorphTo => "MorphTo",
            Self::SelfRef => "SelfRef",
            Self::SelfRefMany => "SelfRefMany",
        }
    }

    /// The `#[tideorm(..)]` key that may name this kind explicitly.
    ///
    /// Only the kinds SeaORM models as an entity relation have one; the others
    /// are declared by their wrapper type alone.
    pub(crate) fn attribute(self) -> Option<&'static str> {
        match self {
            Self::HasOne => Some("has_one"),
            Self::HasMany => Some("has_many"),
            Self::BelongsTo => Some("belongs_to"),
            Self::HasManyThrough => Some("has_many_through"),
            _ => None,
        }
    }

    /// Whether the relation maps onto a SeaORM `Relation` variant and `Related`
    /// impl. The polymorphic and self-referencing kinds cannot: their joins need
    /// a type discriminator or point back at the same entity.
    pub(crate) fn is_entity_relation(self) -> bool {
        self.attribute().is_some()
    }

    fn from_wrapper(ident: &Ident) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| ident == kind.wrapper())
    }
}

#[derive(Debug, Clone, FromField)]
#[darling(attributes(tideorm), forward_attrs(validate, serde))]
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
    /// The field's identifier. Models are structs with named fields
    /// (`supports(struct_named)`), so every field has one.
    pub(crate) fn ident(&self) -> &Ident {
        self.ident
            .as_ref()
            .expect("model fields are named struct fields")
    }

    /// The field name as serde, `ModelMeta::field_names` and every name lookup
    /// spell it: without a raw identifier's `r#`.
    pub(crate) fn name(&self) -> String {
        unraw_ident(self.ident())
    }

    /// The database column: `#[tideorm(column = "..")]`, else the snake-cased
    /// field name.
    pub(crate) fn column_name(&self) -> String {
        self.column
            .clone()
            .unwrap_or_else(|| to_snake_case(&self.name()))
    }

    /// Whether `name` is this field's name or its column name.
    pub(crate) fn is_named(&self, name: &str) -> bool {
        self.name() == name || self.column_name() == name
    }

    /// The relation this field declares, taken from its wrapper type.
    pub(crate) fn relation_kind(&self) -> Option<RelationKind> {
        relation_wrapper(&self.ty).map(|(kind, _)| kind)
    }

    /// The relation kinds the field's `has_one = ".."` style attributes name.
    pub(crate) fn declared_relation_kinds(&self) -> impl Iterator<Item = RelationKind> + '_ {
        [
            (RelationKind::HasOne, &self.has_one),
            (RelationKind::HasMany, &self.has_many),
            (RelationKind::BelongsTo, &self.belongs_to),
            (RelationKind::HasManyThrough, &self.has_many_through),
        ]
        .into_iter()
        .filter(|(_, declared)| declared.is_some())
        .map(|(kind, _)| kind)
    }

    /// Whether the field holds its relation wrapper inside an `Option`, `Box`,
    /// `Rc` or `Arc`, which the generated code cannot assign or call through.
    pub(crate) fn wraps_relation(&self) -> bool {
        path_segment(&self.ty).is_some_and(|segment| {
            matches!(
                segment.ident.to_string().as_str(),
                "Option" | "Box" | "Rc" | "Arc"
            )
        }) && relation_wrapper(&self.ty).is_some()
    }

    /// The relation wrapper's type arguments: the related model, then the
    /// pivot model for `HasManyThrough`.
    pub(crate) fn related_types(&self) -> Vec<Type> {
        relation_wrapper(&self.ty)
            .map(|(_, segment)| type_arguments(segment).cloned().collect())
            .unwrap_or_default()
    }

    /// Whether the field is a `Uuid` (not an `Option` of one), in any qualified
    /// spelling.
    pub(crate) fn is_uuid(&self) -> bool {
        option_inner_type(&self.ty).is_none()
            && canonical_schema_type(&type_string(&self.ty)) == "Uuid"
    }

    /// Whether the field is a persisted column: neither skipped nor a relation.
    pub(crate) fn is_column(&self) -> bool {
        !self.skip && self.relation_kind().is_none()
    }

    /// The field's type with any `Option` removed: what validation rules and
    /// encryption apply to.
    pub(crate) fn validation_base_type(&self) -> &Type {
        option_inner_type(&self.ty).unwrap_or(&self.ty)
    }

    /// The field's integer type when a backend's driver cannot store it and read
    /// it back: `i8`, `u8` and `u16` on PostgreSQL, `u64` there and on SQLite.
    pub(crate) fn driver_limited_integer(&self) -> Option<&'static str> {
        match terminal_ident(self.validation_base_type()).as_deref() {
            Some("i8") => Some("i8"),
            Some("u8") => Some("u8"),
            Some("u16") => Some("u16"),
            Some("u64") => Some("u64"),
            _ => None,
        }
    }

    pub(crate) fn supports_string_validations(&self) -> bool {
        matches!(
            terminal_ident(self.validation_base_type()).as_deref(),
            Some("String" | "str" | "Text")
        )
    }

    /// Whether an encrypted column can hold the field: `String` or `Text`,
    /// optionally inside an `Option`.
    pub(crate) fn supports_encryption(&self) -> bool {
        matches!(
            terminal_ident(self.validation_base_type()).as_deref(),
            Some("String" | "Text")
        )
    }

    /// Whether the field is a Rust integer, the only kind of key a database
    /// counter can fill.
    pub(crate) fn is_integer(&self) -> bool {
        matches!(
            terminal_ident(self.validation_base_type()).as_deref(),
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
            )
        )
    }

    pub(crate) fn supports_numeric_validations(&self) -> bool {
        matches!(
            terminal_ident(self.validation_base_type()).as_deref(),
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
                    | "Decimal"
                    | "String"
                    | "str"
                    | "Text"
            )
        )
    }

    /// The field's column definition, or an error on its type when TideORM
    /// cannot store that type.
    pub(crate) fn column_type_expr(&self) -> syn::Result<TokenStream2> {
        let inner_ty = option_inner_type(&self.ty);
        let base_ty = inner_ty.unwrap_or(&self.ty);

        let column_type = if is_utc_datetime_type(base_ty) {
            quote!(::tideorm::orm::ColumnType::TimestampWithTimeZone)
        } else {
            let base_type = type_string(base_ty);
            match canonical_schema_type(&base_type) {
                "i8" | "i16" | "u8" | "u16" => quote!(::tideorm::orm::ColumnType::SmallInteger),
                "i32" | "u32" => quote!(::tideorm::orm::ColumnType::Integer),
                "i64" | "u64" => quote!(::tideorm::orm::ColumnType::BigInteger),
                "f32" => quote!(::tideorm::orm::ColumnType::Float),
                "f64" => quote!(::tideorm::orm::ColumnType::Double),
                "bool" => quote!(::tideorm::orm::ColumnType::Boolean),
                "String" | "&str" | "str" | "Text" => quote!(::tideorm::orm::ColumnType::Text),
                "Uuid" => quote!(::tideorm::orm::ColumnType::Uuid),
                "DateTime" | "NaiveDateTime" => quote!(::tideorm::orm::ColumnType::DateTime),
                "NaiveDate" => quote!(::tideorm::orm::ColumnType::Date),
                "NaiveTime" => quote!(::tideorm::orm::ColumnType::Time),
                "Decimal" => quote!(::tideorm::orm::ColumnType::Decimal(None)),
                "Json" | "JsonValue" | "Value" | "serde_json::Value" | "Jsonb" => {
                    quote!(::tideorm::orm::ColumnType::Json)
                }
                "Vec<u8>" => quote!(::tideorm::orm::ColumnType::Blob),
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
                "Vec<serde_json::Value>" | "Vec<Json>" | "Vec<JsonValue>" | "JsonArray" => {
                    quote!(::tideorm::orm::ColumnType::Array(
                        ::tideorm::orm::sea_query::RcOrArc::new(::tideorm::orm::ColumnType::Json)
                    ))
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        &self.ty,
                        format!(
                            "unsupported TideORM column type '{base_type}': a model field can be \
                             bool, i8-i64, u8-u64, f32, f64, String, Text, Uuid, Decimal, \
                             DateTime<Utc>, NaiveDateTime, NaiveDate, NaiveTime, \
                             Json (serde_json::Value), Vec<u8>, a PostgreSQL array \
                             (Vec<i32>, Vec<i64>, Vec<f64>, Vec<bool>, Vec<String>, Vec<Json>), \
                             or an Option of one. Store an enum as a String, and mark a field \
                             that is not a column #[tideorm(skip)]"
                        ),
                    ));
                }
            }
        };

        Ok(if inner_ty.is_some() || self.nullable {
            quote!(#column_type.def().nullable())
        } else {
            quote!(#column_type.def())
        })
    }
}

/// The field `name` refers to, by field name or column name.
pub(crate) fn find_db_field<'a>(fields: &'a [ModelField], name: &str) -> Option<&'a ModelField> {
    fields.iter().find(|field| field.is_named(name))
}

/// Maps a path to one of the crate's exported column-type aliases, or to
/// `String`, onto the bare name, so `tideorm::types::Json` and `Json` pick the
/// same arm, as do `std::string::String` and `String`.
fn canonical_schema_type(ty: &str) -> &str {
    const ALIASES: [&str; 16] = [
        "String",
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
    ];

    ALIASES
        .into_iter()
        .find(|alias| {
            ty == *alias
                || ty
                    .strip_suffix(alias)
                    .is_some_and(|prefix| prefix.ends_with("::"))
        })
        .unwrap_or(ty)
}

/// `ty` as compact source text, without the spaces `quote!` puts between
/// tokens: `Option<chrono::DateTime<chrono::Utc>>`.
pub(crate) fn type_string(ty: &Type) -> String {
    quote!(#ty)
        .to_string()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

/// The last path segment `ty` names, seen through references, parentheses and
/// the invisible groups macro substitution wraps types in.
fn path_segment(ty: &Type) -> Option<&PathSegment> {
    match ty {
        Type::Group(group) => path_segment(&group.elem),
        Type::Paren(paren) => path_segment(&paren.elem),
        Type::Reference(reference) => path_segment(&reference.elem),
        Type::Path(type_path) => type_path.path.segments.last(),
        _ => None,
    }
}

/// The type arguments of one path segment: `T` in `Option<T>`.
fn type_arguments(segment: &PathSegment) -> impl Iterator<Item = &Type> {
    let arguments = match &segment.arguments {
        PathArguments::AngleBracketed(arguments) => Some(&arguments.args),
        _ => None,
    };

    arguments
        .into_iter()
        .flatten()
        .filter_map(|argument| match argument {
            GenericArgument::Type(ty) => Some(ty),
            _ => None,
        })
}

/// The name of the last path segment of `ty`.
fn terminal_ident(ty: &Type) -> Option<String> {
    path_segment(ty).map(|segment| segment.ident.to_string())
}

/// Returns the `T` of an `Option<T>` type, for any spelling of `Option`
/// (`std::option::Option<T>`, a macro-substituted type, ...).
///
/// This is the only supported nullability test: a textual `contains("Option")`
/// check misfires on names such as `OptionalMode` or `Vec<Option<String>>`.
pub(crate) fn option_inner_type(ty: &Type) -> Option<&Type> {
    let segment = path_segment(ty)?;
    if segment.ident != "Option" {
        return None;
    }

    type_arguments(segment).next()
}

/// Whether `ty` is an `Option<..>` and therefore maps to a nullable column.
pub(crate) fn is_optional_type(ty: &Type) -> bool {
    option_inner_type(ty).is_some()
}

/// Whether `ty` spells `chrono::NaiveDateTime` in any qualified form.
pub(crate) fn is_naive_datetime_type(ty: &Type) -> bool {
    path_segment(ty).is_some_and(|segment| segment.ident == "NaiveDateTime")
}

/// Whether `ty` spells `chrono::DateTime<chrono::Utc>` in any qualified form.
pub(crate) fn is_utc_datetime_type(ty: &Type) -> bool {
    path_segment(ty).is_some_and(|segment| {
        segment.ident == "DateTime"
            && type_arguments(segment)
                .any(|argument| terminal_ident(argument).as_deref() == Some("Utc"))
    })
}

/// The relation wrapper `ty` names, seen through `Option`, `Box`, `Rc` and
/// `Arc`, together with the wrapper's own path segment.
fn relation_wrapper(ty: &Type) -> Option<(RelationKind, &PathSegment)> {
    let segment = path_segment(ty)?;
    if matches!(
        segment.ident.to_string().as_str(),
        "Option" | "Box" | "Rc" | "Arc"
    ) {
        return type_arguments(segment).find_map(relation_wrapper);
    }

    RelationKind::from_wrapper(&segment.ident).map(|kind| (kind, segment))
}

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(tideorm), supports(struct_named), forward_attrs(serde))]
pub(crate) struct ModelInput {
    pub(crate) ident: Ident,
    /// The struct's `#[serde(..)]` attributes.
    pub(crate) attrs: Vec<syn::Attribute>,
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
