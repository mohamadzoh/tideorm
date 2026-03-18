#![allow(missing_docs)]

/// Index definition for database schema generation.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexDefinition {
    /// Index name (auto-generated if not specified).
    pub name: String,
    /// Column names in the index.
    pub columns: Vec<String>,
    /// Whether this is a unique index.
    pub unique: bool,
}

impl IndexDefinition {
    /// Create a new index definition.
    pub fn new(name: impl Into<String>, columns: Vec<String>, unique: bool) -> Self {
        Self {
            name: name.into(),
            columns,
            unique,
        }
    }

    /// Parse index definitions from the macro format.
    pub fn parse(_table_name: &str, input: &str, unique: bool) -> Vec<Self> {
        if input.is_empty() {
            return vec![];
        }

        input
            .split(';')
            .filter(|s| !s.trim().is_empty())
            .map(|part| {
                let part = part.trim();
                let (name, columns) = if let Some((n, cols)) = part.split_once(':') {
                    (n.trim().to_string(), cols)
                } else {
                    let cols = part;
                    let prefix = if unique { "uidx" } else { "idx" };
                    let col_part = cols.replace(',', "_").replace(' ', "");
                    (format!("{}_{}", prefix, col_part), cols)
                };

                let columns: Vec<String> = columns
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                IndexDefinition::new(name, columns, unique)
            })
            .collect()
    }
}

/// Metadata trait for model information.
pub trait ModelMeta: Sized + Send + Sync + Clone + 'static {
    type PrimaryKey: Send + Sync + Clone + std::fmt::Debug + std::fmt::Display + 'static;

    fn table_name() -> &'static str;

    fn primary_key_name() -> &'static str;

    fn primary_key_auto_increment() -> bool {
        false
    }

    fn column_names() -> &'static [&'static str];

    fn field_names() -> &'static [&'static str];

    fn hidden_attributes() -> Vec<&'static str> {
        vec!["deleted_at"]
    }

    fn default_presenter() -> &'static str {
        "default"
    }

    fn searchable_fields() -> Vec<&'static str> {
        vec![]
    }

    fn default_order() -> Vec<(&'static str, &'static str)> {
        vec![("id", "DESC")]
    }

    fn option_set_label() -> &'static str {
        "id"
    }

    fn option_set_search_fields() -> Vec<&'static str> {
        vec![]
    }

    fn translatable_fields() -> Vec<&'static str> {
        vec![]
    }

    fn allowed_languages() -> Vec<String> {
        crate::config::Config::get_languages()
    }

    fn fallback_language() -> String {
        crate::config::Config::get_fallback_language()
    }

    #[cfg(feature = "translations")]
    fn has_translations() -> bool {
        !Self::translatable_fields().is_empty()
    }

    #[cfg(not(feature = "translations"))]
    fn has_translations() -> bool {
        false
    }

    fn has_one_attached_file() -> Vec<&'static str> {
        vec![]
    }

    fn has_many_attached_files() -> Vec<&'static str> {
        vec![]
    }

    fn files_relations() -> Vec<&'static str> {
        let mut relations = Self::has_one_attached_file();
        relations.extend(Self::has_many_attached_files());
        relations
    }

    #[cfg(feature = "attachments")]
    fn has_file_attachments() -> bool {
        !Self::files_relations().is_empty()
    }

    #[cfg(not(feature = "attachments"))]
    fn has_file_attachments() -> bool {
        false
    }

    #[cfg(feature = "attachments")]
    fn file_url_generator() -> crate::config::FileUrlGenerator {
        crate::config::Config::get_file_url_generator()
    }

    #[inline]
    #[cfg(feature = "attachments")]
    fn generate_file_url(field_name: &str, file: &crate::attachments::FileAttachment) -> String {
        Self::file_url_generator()(field_name, file)
    }

    fn soft_delete_enabled() -> bool {
        false
    }

    fn deleted_at_column() -> &'static str {
        "deleted_at"
    }

    fn has_timestamps() -> bool {
        false
    }

    fn indexes() -> Vec<IndexDefinition> {
        vec![]
    }

    fn unique_indexes() -> Vec<IndexDefinition> {
        vec![]
    }

    fn all_indexes() -> Vec<IndexDefinition> {
        let mut all = Self::indexes();
        all.extend(Self::unique_indexes());
        all
    }

    fn has_indexes() -> bool {
        !Self::indexes().is_empty() || !Self::unique_indexes().is_empty()
    }

    fn tokenization_enabled() -> bool {
        false
    }

    fn token_encoder() -> Option<crate::tokenization::TokenEncoder> {
        None
    }

    fn token_decoder() -> Option<crate::tokenization::TokenDecoder> {
        None
    }
}