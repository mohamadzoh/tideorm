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

    /// An index the model declares without a name, named as a migration's
    /// `index`/`unique_index` names it.
    #[doc(hidden)]
    pub fn __generated(table: &str, columns: Vec<String>, unique: bool) -> Self {
        Self::new(
            crate::migration::ddl::index_name(table, &columns, unique),
            columns,
            unique,
        )
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

    /// The schema declared with `#[tideorm(schema = "..")]`, when there is one.
    ///
    /// Every statement then names the table as `schema.table`; without one the
    /// table is found through the connection's search path.
    fn schema_name() -> Option<&'static str> {
        None
    }

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

    /// The Rust field's integer range intersected with SQLite's signed storage range.
    /// Generated models retain signedness here even when their SQL types coincide.
    #[doc(hidden)]
    fn __sqlite_integer_bounds(_column: &str) -> Option<(i64, i64)> {
        None
    }

    fn field_names() -> &'static [&'static str];

    /// Fields a user-supplied serde implementation flattens into its parent.
    /// Their contents have no field boundary after serialization, so a hidden
    /// flattened field cannot safely be removed from that payload.
    #[doc(hidden)]
    fn __flattened_fields() -> &'static [&'static str] {
        &[]
    }

    /// Fields a user-supplied serde implementation never writes (`skip` or
    /// `skip_serializing`). Rendering must not write them back in, the way a
    /// translation of the field would.
    #[doc(hidden)]
    fn __skipped_fields() -> &'static [&'static str] {
        &[]
    }

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

    /// Resolve a Rust field name or a database column name to the field name,
    /// which is the key the model serializes under. A field name wins over a
    /// column name that happens to be spelled the same.
    fn canonical_field_name(name: &str) -> Option<&'static str> {
        if let Some(field_name) = Self::field_names()
            .iter()
            .copied()
            .find(|field| *field == name)
        {
            return Some(field_name);
        }

        Self::field_names()
            .iter()
            .copied()
            .zip(Self::column_names().iter().copied())
            .find_map(|(field_name, column_name)| (column_name == name).then_some(field_name))
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
        match name.rsplit_once('.') {
            Some((table, column)) if Self::owns_table_reference(table) => {
                (Some(Self::table_name()), Self::column_named(column))
            }
            Some((table, column)) => (Some(table), column),
            None => (None, Self::column_named(name)),
        }
    }

    /// The column `name`, a field or column name, stands for; `name` itself
    /// when the model has no such column.
    #[doc(hidden)]
    fn column_named(name: &str) -> &str {
        Self::canonical_column_name(name).unwrap_or(name)
    }

    /// The column `name` names when it is one of this model's own: a field or
    /// column name, bare or qualified with the model's table. `None` for
    /// another table's column or a name the model does not know.
    #[doc(hidden)]
    fn own_column_name(name: &str) -> Option<&'static str> {
        let name = match name.trim().rsplit_once('.') {
            Some((table, column)) if Self::owns_table_reference(table) => column,
            Some(_) => return None,
            None => name.trim(),
        };
        Self::canonical_column_name(name)
    }

    /// Whether a bare or schema-qualified table reference identifies this model.
    #[doc(hidden)]
    fn owns_table_reference(reference: &str) -> bool {
        match reference.split_once('.') {
            Some((schema, table)) => {
                Some(schema) == Self::schema_name() && table == Self::table_name()
            }
            None => reference == Self::table_name(),
        }
    }

    fn hidden_attributes() -> Vec<&'static str> {
        vec!["deleted_at"]
    }

    /// The key `field` is serialized under: its name, unless the model derives
    /// `Serialize` itself and a serde attribute renames it.
    #[doc(hidden)]
    fn serialized_name(field: &str) -> &str {
        field
    }

    /// Whether serializing this model and reading the JSON back reproduces
    /// every stored field. A model whose own serde derive skips or converts a
    /// stored field reports `false`, and its query results are not cached,
    /// since a cached copy would come back without those values.
    #[doc(hidden)]
    fn __serde_round_trips() -> bool {
        true
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
    /// **target** model's `__strip_hidden_payload`. Without it `to_json` cannot
    /// tell which model a nested payload came from and would filter it with
    /// the parent's hidden list, shipping the columns the target model hides.
    ///
    /// Defaults to empty so hand-written `ModelMeta` impls keep compiling; they
    /// simply fall back to the parent's list. `MorphTo` fields stay empty for
    /// the same reason even on generated impls: their target type is only known
    /// at runtime.
    fn relation_payload_filters() -> Vec<(&'static str, RelationPayloadFilter)> {
        vec![]
    }

    fn searchable_fields() -> Vec<&'static str> {
        vec![]
    }

    /// The column this model's `MorphOne`/`MorphMany` keys its children by,
    /// for the morph whose child column holding that key is `id_column`
    /// (`commentable_id`) on the child model stored in `child_table`; `None`
    /// when no relation of this model declares it. An unknown child (`""`)
    /// takes the first relation with that morph name.
    #[doc(hidden)]
    fn __morph_owner_key(_id_column: &str, _child_table: &str) -> Option<&'static str> {
        None
    }

    fn translatable_fields() -> Vec<&'static str> {
        vec![]
    }

    /// The `#[tideorm(encrypted = ..)]` fields, each as `(field, column)`.
    fn encrypted_fields() -> &'static [(&'static str, &'static str)] {
        &[]
    }

    fn has_encrypted_fields() -> bool {
        !Self::encrypted_fields().is_empty()
    }

    /// Whether the encrypted field `field` reads `document` back: a batch
    /// `set()` seals any JSON, which every later load would then fail to
    /// decrypt into a field of another type.
    #[doc(hidden)]
    fn __encrypted_field_holds(_field: &str, _document: &serde_json::Value) -> bool {
        false
    }

    fn allowed_languages() -> Vec<String> {
        crate::config::Config::get_languages()
    }

    fn fallback_language() -> String {
        crate::config::Config::get_fallback_language()
    }

    /// Read the backing translation payload independently of serde output policies.
    #[doc(hidden)]
    fn __translation_payload(&self) -> crate::Result<Option<serde_json::Value>> {
        Ok(None)
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

    /// Tables a row of this model points at with a foreign key, so they have
    /// to hold a row before this one is inserted: the tables of its declared
    /// `belongs_to` relations. An entity manager's flush inserts them first
    /// and deletes them last. A bare foreign-key column with no relation
    /// attribute adds nothing, and a `MorphTo` target has no known table.
    #[cfg(feature = "entity-manager")]
    #[doc(hidden)]
    fn __parent_tables() -> Vec<&'static str> {
        Vec::new()
    }

    /// Tables whose rows point back at this model with a foreign key: the
    /// tables of its declared `has_one` and `has_many` relations. A
    /// `has_many_through` adds none, since relation sync writes the pivot row
    /// once both sides are saved.
    #[cfg(feature = "entity-manager")]
    #[doc(hidden)]
    fn __child_tables() -> Vec<&'static str> {
        Vec::new()
    }

    /// Each `(related table, foreign key, referenced column)` a declared
    /// `belongs_to`, `has_one`, `has_many` or self-referencing relation joins
    /// rows with: a row's foreign key holds the referenced column of a row of
    /// the related table. An entity manager's flush orders the rows of one
    /// table by the ones whose related table is that table.
    #[cfg(feature = "entity-manager")]
    #[doc(hidden)]
    fn __row_references() -> Vec<(&'static str, &'static str, &'static str)> {
        Vec::new()
    }

    /// Make every relation query `database`: the pool the model was loaded
    /// from, or the one an entity manager that holds it reads through.
    #[doc(hidden)]
    fn __attach_query_database(&mut self, _database: &crate::database::Database) {}

    /// Drop every relation's cached rows and load, keeping the relations
    /// wired to this model's keys: an entity manager's identity map holds a
    /// row, not the relations one copy of it loaded.
    #[cfg(feature = "entity-manager")]
    #[doc(hidden)]
    fn __unload_relations(&mut self) {}
}
