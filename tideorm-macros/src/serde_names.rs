//! The keys a model's own `Serialize` derive writes its fields under.
//!
//! TideORM's serializer writes each field under its name, but a model that
//! derives `Serialize` itself can rename fields with serde attributes, and
//! what TideORM does to the serialized JSON — removing hidden attributes,
//! filling in translations — has to address the keys serde wrote. These are
//! serde's own rules for struct fields.

use syn::meta::ParseNestedMeta;
use syn::{Attribute, LitStr, Token};

/// A `#[serde(rename_all = "..")]` rule.
#[derive(Clone, Copy)]
pub(crate) enum RenameRule {
    Lower,
    Upper,
    Pascal,
    Camel,
    Snake,
    ScreamingSnake,
    Kebab,
    ScreamingKebab,
}

impl RenameRule {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "lowercase" => Self::Lower,
            "UPPERCASE" => Self::Upper,
            "PascalCase" => Self::Pascal,
            "camelCase" => Self::Camel,
            "snake_case" => Self::Snake,
            "SCREAMING_SNAKE_CASE" => Self::ScreamingSnake,
            "kebab-case" => Self::Kebab,
            "SCREAMING-KEBAB-CASE" => Self::ScreamingKebab,
            _ => return None,
        })
    }

    /// serde's `RenameRule::apply_to_field`.
    pub(crate) fn apply(self, field: &str) -> String {
        match self {
            Self::Lower | Self::Snake => field.to_owned(),
            Self::Upper | Self::ScreamingSnake => field.to_ascii_uppercase(),
            Self::Pascal => {
                let mut pascal = String::new();
                let mut capitalize = true;
                for ch in field.chars() {
                    if ch == '_' {
                        capitalize = true;
                    } else if capitalize {
                        pascal.push(ch.to_ascii_uppercase());
                        capitalize = false;
                    } else {
                        pascal.push(ch);
                    }
                }
                pascal
            }
            Self::Camel => {
                let pascal = Self::Pascal.apply(field);
                let mut chars = pascal.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
                    None => pascal,
                }
            }
            Self::Kebab => field.replace('_', "-"),
            Self::ScreamingKebab => Self::ScreamingSnake.apply(field).replace('_', "-"),
        }
    }
}

/// The struct's serialize-side `rename_all` rule.
pub(crate) fn rename_all(attrs: &[Attribute]) -> Option<RenameRule> {
    let mut rule = None;
    for_each_serde_item(attrs, |meta| {
        if meta.path.is_ident("rename_all") {
            if let Some(name) = serialize_side(meta)? {
                rule = RenameRule::parse(&name);
            }
        } else {
            skip_item(meta)?;
        }
        Ok(())
    });
    rule
}

/// The key serde serializes a field named `name` under, or `None` when it
/// skips the field.
pub(crate) fn field_key(
    attrs: &[Attribute],
    name: &str,
    rule: Option<RenameRule>,
) -> Option<String> {
    let mut rename = None;
    let mut skipped = false;
    for_each_serde_item(attrs, |meta| {
        if meta.path.is_ident("rename") {
            rename = serialize_side(meta)?;
        } else if meta.path.is_ident("skip") || meta.path.is_ident("skip_serializing") {
            skipped = true;
        } else {
            skip_item(meta)?;
        }
        Ok(())
    });
    if skipped {
        return None;
    }
    Some(rename.unwrap_or_else(|| rule.map_or_else(|| name.to_owned(), |rule| rule.apply(name))))
}

/// Visit every item of every `#[serde(..)]` attribute. Malformed input is
/// serde's to report, so a parse failure just ends the visit.
fn for_each_serde_item(
    attrs: &[Attribute],
    mut visit: impl FnMut(&ParseNestedMeta) -> syn::Result<()>,
) {
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        let _ = attr.parse_nested_meta(|meta| visit(&meta));
    }
}

/// The name in `key = ".."` or in `key(serialize = "..", deserialize = "..")`.
fn serialize_side(meta: &ParseNestedMeta) -> syn::Result<Option<String>> {
    if meta.input.peek(Token![=]) {
        return Ok(Some(meta.value()?.parse::<LitStr>()?.value()));
    }
    let mut name = None;
    meta.parse_nested_meta(|inner| {
        let value = inner.value()?.parse::<LitStr>()?.value();
        if inner.path.is_ident("serialize") {
            name = Some(value);
        }
        Ok(())
    })?;
    Ok(name)
}

/// Consume an item this module does not read: a flag, `key = value`, or a
/// parenthesized list.
fn skip_item(meta: &ParseNestedMeta) -> syn::Result<()> {
    if meta.input.peek(Token![=]) {
        meta.value()?.parse::<syn::Expr>()?;
    } else if meta.input.peek(syn::token::Paren) {
        meta.parse_nested_meta(|inner| skip_item(&inner))?;
    }
    Ok(())
}
