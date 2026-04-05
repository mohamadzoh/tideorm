use syn::{Attribute, Meta};

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
