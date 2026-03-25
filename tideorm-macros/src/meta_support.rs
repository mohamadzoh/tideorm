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
    match word {
        "person" => return "people".to_string(),
        "man" => return "men".to_string(),
        "woman" => return "women".to_string(),
        "child" => return "children".to_string(),
        "tooth" => return "teeth".to_string(),
        "foot" => return "feet".to_string(),
        "mouse" => return "mice".to_string(),
        "goose" => return "geese".to_string(),
        "leaf" => return "leaves".to_string(),
        "knife" => return "knives".to_string(),
        "life" => return "lives".to_string(),
        "wife" => return "wives".to_string(),
        "wolf" => return "wolves".to_string(),
        "calf" => return "calves".to_string(),
        "half" => return "halves".to_string(),
        "loaf" => return "loaves".to_string(),
        "self" => return "selves".to_string(),
        "shelf" => return "shelves".to_string(),
        "thief" => return "thieves".to_string(),
        "quiz" => return "quizzes".to_string(),
        "fez" => return "fezzes".to_string(),
        _ => {}
    }

    if word.ends_with('s')
        || word.ends_with('x')
        || word.ends_with('z')
        || word.ends_with("ch")
        || word.ends_with("sh")
    {
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
    use super::{detect_existing_derives, pluralize};

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

    #[test]
    fn pluralize_handles_common_irregular_nouns() {
        assert_eq!(pluralize("person"), "people");
        assert_eq!(pluralize("child"), "children");
        assert_eq!(pluralize("mouse"), "mice");
    }

    #[test]
    fn pluralize_handles_f_and_fe_endings() {
        assert_eq!(pluralize("leaf"), "leaves");
        assert_eq!(pluralize("knife"), "knives");
        assert_eq!(pluralize("profile"), "profiles");
        assert_eq!(pluralize("roof"), "roofs");
    }

    #[test]
    fn pluralize_handles_z_suffixes() {
        assert_eq!(pluralize("quiz"), "quizzes");
        assert_eq!(pluralize("fez"), "fezzes");
        assert_eq!(pluralize("bus"), "buses");
        assert_eq!(pluralize("status"), "statuses");
        assert_eq!(pluralize("topaz"), "topazes");
    }
}
