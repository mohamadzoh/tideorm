use syn::{Attribute, Meta};

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ExistingDerives {
    pub(crate) has_debug: bool,
    pub(crate) has_clone: bool,
    pub(crate) has_default: bool,
    pub(crate) has_serialize: bool,
    pub(crate) has_deserialize: bool,
}

pub(crate) fn detect_existing_derives(attrs: &[Attribute]) -> ExistingDerives {
    let mut existing = ExistingDerives::default();
    for attr in attrs {
        if !attr.path().is_ident("derive") {
            continue;
        }
        if let Meta::List(list) = &attr.meta {
            let tokens = list.tokens.to_string();
            existing.has_debug |= tokens.contains("Debug");
            existing.has_clone |= tokens.contains("Clone");
            existing.has_default |= tokens.contains("Default");
            if tokens.contains("Deserialize") {
                existing.has_deserialize = true;
            }
            if tokens.contains("Serialize") && !tokens.contains("Deserialize") {
                existing.has_serialize = true;
            }
            for part in tokens.split(',').map(str::trim) {
                if part == "Serialize"
                    || part.ends_with("::Serialize")
                    || part.starts_with("serde::Serialize")
                {
                    existing.has_serialize = true;
                }
            }
        }
    }
    existing
}

pub(crate) fn pluralize(word: &str) -> String {
    if word.ends_with('s') || word.ends_with('x') || word.ends_with("ch") || word.ends_with("sh") {
        format!("{}es", word)
    } else if word.ends_with('y')
        && !word.ends_with("ay")
        && !word.ends_with("ey")
        && !word.ends_with("oy")
        && !word.ends_with("uy")
    {
        format!("{}ies", &word[..word.len() - 1])
    } else {
        format!("{}s", word)
    }
}
