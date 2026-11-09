use crate::case::{capitalize, convert, to_pascal_case, to_snake_case};
use std::str::FromStr;
use syn::{LitStr, meta::ParseNestedMeta};

#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaseStyle {
    CamelCase,
    KebabCase,
    MixedCase,
    ShoutySnakeCase,
    SnakeCase,
    TitleCase,
    UpperCase,
    LowerCase,
    ScreamingKebabCase,
    PascalCase,
}

const VALID_CASE_STYLES: &[&str] = &[
    "camelCase",
    "PascalCase",
    "kebab-case",
    "snake_case",
    "SCREAMING_SNAKE_CASE",
    "SCREAMING-KEBAB-CASE",
    "lowercase",
    "UPPERCASE",
    "title_case",
    "mixed_case",
];

impl FromStr for CaseStyle {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, ()> {
        Ok(match text {
            "PascalCase" => CaseStyle::PascalCase,
            "camelCase" => CaseStyle::CamelCase,
            "snake_case" => CaseStyle::SnakeCase,
            "kebab-case" => CaseStyle::KebabCase,
            "SCREAMING-KEBAB-CASE" => CaseStyle::ScreamingKebabCase,
            "SCREAMING_SNAKE_CASE" => CaseStyle::ShoutySnakeCase,
            "title_case" => CaseStyle::TitleCase,
            "mixed_case" => CaseStyle::MixedCase,
            "lowercase" => CaseStyle::LowerCase,
            "UPPERCASE" => CaseStyle::UpperCase,
            _ => return Err(()),
        })
    }
}

impl CaseStyle {
    /// `text` in this style, split into words by the rule the model derive
    /// names tables and columns by.
    pub fn apply(self, text: &str) -> String {
        match self {
            CaseStyle::PascalCase => to_pascal_case(text),
            CaseStyle::KebabCase => convert(text, "-", str::to_lowercase),
            CaseStyle::ShoutySnakeCase => convert(text, "_", str::to_uppercase),
            CaseStyle::SnakeCase => to_snake_case(text),
            CaseStyle::TitleCase => convert(text, " ", capitalize),
            CaseStyle::UpperCase => text.to_uppercase(),
            CaseStyle::LowerCase => text.to_lowercase(),
            CaseStyle::ScreamingKebabCase => convert(text, "-", str::to_uppercase),
            CaseStyle::CamelCase | CaseStyle::MixedCase => {
                let pascal = to_pascal_case(text);
                let mut chars = pascal.chars();
                chars
                    .next()
                    .map(|first| first.to_lowercase().chain(chars).collect())
                    .unwrap_or_default()
            }
        }
    }
}

impl TryFrom<&ParseNestedMeta<'_>> for CaseStyle {
    type Error = syn::Error;

    fn try_from(value: &ParseNestedMeta) -> Result<Self, Self::Error> {
        let meta_string_literal: LitStr = value.value()?.parse()?;
        let value_string = meta_string_literal.value();
        match CaseStyle::from_str(value_string.as_str()) {
            Ok(rule) => Ok(rule),
            Err(()) => Err(value.error(format!(
                "Unknown value for attribute parameter: `{value_string}`. Valid values are: `{VALID_CASE_STYLES:?}`"
            ))),
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/orm/case_style_tests.rs"]
mod tests;
