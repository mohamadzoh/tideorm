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

/// Strips hidden attributes from one eager-loaded relation payload, in place.
///
/// The second argument is the globally hidden attribute list from
/// [`crate::config`], applied on top of the payload model's own.
/// See [`ModelMeta::relation_payload_filters`].
pub type RelationPayloadFilter = fn(&mut serde_json::Value, &[String]);

/// Metadata trait for model information.
pub trait ModelMeta: Sized + Send + Sync + Clone + 'static {
    type PrimaryKey: Send + Sync + Clone + std::fmt::Debug + serde::Serialize + 'static;

    fn table_name() -> &'static str;

    fn primary_key_names() -> &'static [&'static str];

    fn primary_key_name() -> &'static str {
        Self::primary_key_names().first().copied().unwrap_or("id")
    }

    fn primary_key_auto_increment() -> bool {
        false
    }

    fn primary_key_display(primary_key: &Self::PrimaryKey) -> String;

    fn primary_key_is_new(_primary_key: &Self::PrimaryKey) -> bool {
        false
    }

    fn column_names() -> &'static [&'static str];

    fn field_names() -> &'static [&'static str];

    fn canonical_column_name(name: &str) -> Option<&'static str> {
        if let Some(column_name) = Self::column_names()
            .iter()
            .copied()
            .find(|column| *column == name)
        {
            return Some(column_name);
        }

        Self::field_names()
            .iter()
            .copied()
            .zip(Self::column_names().iter().copied())
            .find_map(|(field_name, column_name)| (field_name == name).then_some(column_name))
    }

    /// Split an optionally table-qualified column reference into its qualifier
    /// and this model's canonical database column name.
    ///
    /// `"user_name"`, `"display_name"`, `"users.user_name"` and
    /// `"users.display_name"` all resolve to the column `user_name` on a model
    /// whose table is `users`, so a Rust field name addresses the same column
    /// whether or not it is written qualified. A qualifier naming some other
    /// table is left untouched — only a join can resolve that — and a name this
    /// model does not know is returned as written.
    ///
    /// Query validation canonicalizes a self-qualified reference this way
    /// already, so every SQL renderer has to use this and not
    /// [`canonical_column_name`](ModelMeta::canonical_column_name) alone, or a
    /// reference that validates would be emitted as a column that does not
    /// exist.
    fn canonical_column_parts(name: &str) -> (Option<&str>, &str) {
        match name.split_once('.') {
            Some((table, column)) if table == Self::table_name() => (
                Some(table),
                Self::canonical_column_name(column).unwrap_or(column),
            ),
            Some((table, column)) => (Some(table), column),
            None => (None, Self::canonical_column_name(name).unwrap_or(name)),
        }
    }

    fn hidden_attributes() -> Vec<&'static str> {
        vec!["deleted_at"]
    }

    /// Strip this model's hidden attributes out of one already-serialized
    /// payload of it, in place.
    ///
    /// Handed out as a function pointer by `relation_payload_filters`. A
    /// nested relation payload carries no type tag of its own, so this is the
    /// only way the filter can reach the metadata of the model that actually
    /// produced the payload.
    #[doc(hidden)]
    fn __strip_hidden_payload(value: &mut serde_json::Value, global_hidden: &[String]) {
        crate::model::serialization::strip_model_payload::<Self>(value, global_hidden);
    }

    /// Hidden-attribute filters for this model's eager-loadable relation
    /// payloads, keyed by the serde key each payload is serialized under.
    ///
    /// The derive emits one entry per typed relation field, pointing at the
    /// **target** model's `__strip_hidden_payload`. Without
    /// it `to_json` cannot tell which model a nested payload came from and
    /// filters it with the parent's hidden list — which is how
    /// `post.to_json(None)` after `.with("author")` used to ship the columns
    /// `User` declares hidden.
    ///
    /// Defaults to empty so hand-written `ModelMeta` impls keep compiling; they
    /// simply fall back to the parent's list. `MorphTo` fields stay empty for
    /// the same reason even on generated impls: their target type is only known
    /// at runtime.
    fn relation_payload_filters() -> Vec<(&'static str, RelationPayloadFilter)> {
        vec![]
    }

    fn default_presenter() -> &'static str {
        "default"
    }

    fn searchable_fields() -> Vec<&'static str> {
        vec![]
    }

    fn translatable_fields() -> Vec<&'static str> {
        vec![]
    }

    fn encrypted_fields() -> Vec<&'static str> {
        vec![]
    }

    fn encrypted_column_names() -> Vec<&'static str> {
        vec![]
    }

    fn has_encrypted_fields() -> bool {
        !Self::encrypted_fields().is_empty()
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
}
