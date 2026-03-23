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
            existing.has_serialize |= tokens.contains("Serialize");
            if tokens.contains("Deserialize") {
                existing.has_deserialize = true;
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

#[cfg(test)]
mod tests {
    use super::detect_existing_derives;

    #[test]
    fn detect_existing_derives_finds_serialize_and_deserialize_together() {
        let item: syn::DeriveInput = syn::parse_quote! {
            #[derive(Serialize, Deserialize)]
            struct Example;
        };

        let existing = detect_existing_derives(&item.attrs);

        assert!(existing.has_serialize);
        assert!(existing.has_deserialize);
    }

    #[test]
    fn detect_existing_derives_finds_path_qualified_serialize() {
        let item: syn::DeriveInput = syn::parse_quote! {
            #[derive(serde::Serialize)]
            struct Example;
        };

        let existing = detect_existing_derives(&item.attrs);

        assert!(existing.has_serialize);
        assert!(!existing.has_deserialize);
    }
}
