//! Project rules: the limits a team writes in `tideorm.toml`'s `[rules]` table
//! and every model and query is held to. Nothing is enforced without that
//! table; `[rules.allow]` exempts named models from some rules.
//!
//! The derive reads the file as a model compiles and writes the rules the
//! model is held to into it (`ModelMeta::__rules`), so a binary carries its
//! rules with no file at run time. The model rules (its columns, keys,
//! indexes, relations) are checked once, when the first database is
//! connected, every broken rule reported at once; the query rules as each
//! query runs.

mod graph;
mod models;

use std::sync::OnceLock;

use crate::error::{Error, Result};
use crate::model::{IndexDefinition, ModelMeta};
use crate::sync::{ColumnDef, CompiledModelRegistration, SyncModel};

/// The rules one model is held to: the project's, less the model's
/// exceptions. Every rule is off in [`Rules::NONE`].
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rules {
    /// Most columns a model may have.
    pub max_columns: Option<u32>,
    /// Most relations a model may declare.
    pub max_relations: Option<u32>,
    /// Most `JSON`/`JSONB` columns a model may have.
    pub max_json_columns: Option<u32>,
    /// Most nullable columns a model may have.
    pub max_nullable_columns: Option<u32>,
    /// Most indexes, unique ones included, a model may declare.
    pub max_indexes: Option<u32>,
    /// Longest table, column or index name, in bytes.
    pub max_identifier_length: Option<u32>,
    /// Table and column names in lowercase `snake_case`.
    pub snake_case_names: bool,
    /// No table or column named by an SQL reserved word.
    pub avoid_reserved_words: bool,
    /// The types a primary key column may have; empty allows any.
    pub primary_key_types: &'static [&'static str],
    /// A primary key of one column.
    pub forbid_composite_primary_keys: bool,
    /// `created_at`/`updated_at` timestamps on every model.
    pub require_timestamps: bool,
    /// Soft delete on every model.
    pub require_soft_delete: bool,
    /// An index leading with every column a relation's foreign key is in.
    pub require_foreign_key_index: bool,
    /// Words that make a column sensitive, which must be hidden or encrypted.
    pub sensitive_fields: &'static [&'static str],
    /// No `REAL`/`DOUBLE` columns, which round money and counts.
    pub forbid_float_columns: bool,
    /// Longest chain of owned relations (`has_one`, `has_many`, `morph_one`,
    /// `morph_many`) from a model.
    pub max_relation_depth: Option<u32>,
    /// No model reachable from itself through owned relations.
    pub forbid_relation_cycles: bool,
    /// Deepest eager load, `with("posts.comments")` being 2.
    pub max_eager_depth: Option<u32>,
    /// Most relations one eager load resolves.
    pub max_eager_relations: Option<u32>,
    /// Most rows one query may read; a read needs a `limit()` within it.
    pub max_rows: Option<u64>,
    /// Most joins one query may have.
    pub max_joins: Option<u32>,
    /// Most values one `where_in`/`where_not_in` may list.
    pub max_in_values: Option<u32>,
    /// No raw SQL: raw query fragments and the raw statements of `Database`.
    pub forbid_raw_sql: bool,
}

impl Rules {
    /// No rule at all: what a model is held to without `[rules]`.
    pub const NONE: Rules = Rules {
        max_columns: None,
        max_relations: None,
        max_json_columns: None,
        max_nullable_columns: None,
        max_indexes: None,
        max_identifier_length: None,
        snake_case_names: false,
        avoid_reserved_words: false,
        primary_key_types: &[],
        forbid_composite_primary_keys: false,
        require_timestamps: false,
        require_soft_delete: false,
        require_foreign_key_index: false,
        sensitive_fields: &[],
        forbid_float_columns: false,
        max_relation_depth: None,
        forbid_relation_cycles: false,
        max_eager_depth: None,
        max_eager_relations: None,
        max_rows: None,
        max_joins: None,
        max_in_values: None,
        forbid_raw_sql: false,
    };
}

/// Which query rules a query is held to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum RuleScope {
    /// A query the application wrote: every rule.
    #[default]
    Developer,
    /// A query whose terminal checked its row count itself, such as the
    /// batches of `chunks()`, which may read one row past it.
    RowsChecked,
    /// A relation's own query: only raw SQL, which a `load_with` closure may add.
    Relation,
    /// A lookup TideORM writes itself: no rule.
    Internal,
}

impl RuleScope {
    pub(crate) fn checks_rows(self) -> bool {
        self == Self::Developer
    }

    pub(crate) fn checks_shape(self) -> bool {
        matches!(self, Self::Developer | Self::RowsChecked)
    }

    pub(crate) fn checks_raw_sql(self) -> bool {
        self != Self::Internal
    }
}

/// One relation of a model, as the model rules read it.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationOutline {
    /// The relation's field.
    pub field: &'static str,
    /// The related model's table, `schema.table` when it names a schema;
    /// `None` for a `MorphTo`, whose related table varies by row.
    pub related: Option<String>,
    /// Whether the model owns the related rows (`has_one`, `has_many`,
    /// `morph_one`, `morph_many`): the edges relation depth follows.
    pub owns: bool,
    /// Each table and column holding one of the relation's foreign keys.
    pub foreign_keys: Vec<(String, &'static str)>,
}

impl RelationOutline {
    /// A relation whose related rows `field`'s model owns.
    pub fn owning<R: ModelMeta>(field: &'static str) -> Self {
        Self {
            field,
            related: Some(qualified_table::<R>()),
            owns: true,
            foreign_keys: Vec::new(),
        }
    }

    /// A relation to rows the model does not own: `belongs_to`,
    /// `has_many_through` and the self-references.
    pub fn referencing<R: ModelMeta>(field: &'static str) -> Self {
        Self {
            owns: false,
            ..Self::owning::<R>(field)
        }
    }

    /// A `MorphTo`, whose related table varies by row.
    pub fn morph_to(field: &'static str) -> Self {
        Self {
            field,
            related: None,
            owns: false,
            foreign_keys: Vec::new(),
        }
    }

    /// Add the foreign key `column` of `T`'s table.
    #[must_use]
    pub fn keyed<T: ModelMeta>(mut self, column: &'static str) -> Self {
        // `foreign_key` may name the field rather than its column.
        let column = T::canonical_column_name(column).unwrap_or(column);
        self.foreign_keys.push((qualified_table::<T>(), column));
        self
    }
}

/// A model as the model rules check it.
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct ModelOutline {
    /// The model's type.
    pub name: &'static str,
    /// Its table, `schema.table` when it names a schema.
    pub table: String,
    /// The rules it is held to.
    pub rules: Rules,
    /// Its columns, as sync declares them.
    pub columns: Vec<ColumnDef>,
    /// Its primary key columns.
    pub primary_keys: Vec<String>,
    /// Its declared indexes, unique ones included.
    pub indexes: Vec<IndexDefinition>,
    /// Its relations.
    pub relations: Vec<RelationOutline>,
    /// Columns kept out of the model's JSON or encrypted.
    pub protected_columns: Vec<String>,
    /// Whether it soft deletes.
    pub soft_delete: bool,
    /// Whether it keeps `created_at`/`updated_at`.
    pub timestamps: bool,
}

/// `M` as the model rules check it: what its registration points at.
#[doc(hidden)]
pub fn __outline<M: ModelMeta + SyncModel>() -> ModelOutline {
    let schema = M::sync_schema();
    let protected_columns = M::hidden_attributes()
        .into_iter()
        .map(|field| M::canonical_column_name(field).unwrap_or(field))
        .chain(M::encrypted_fields().iter().map(|(_, column)| *column))
        .map(str::to_string)
        .collect();
    ModelOutline {
        name: std::any::type_name::<M>(),
        table: qualified_table::<M>(),
        rules: *M::__rules(),
        columns: schema.columns,
        primary_keys: schema.primary_keys,
        indexes: schema.indexes,
        relations: M::__relations(),
        protected_columns,
        soft_delete: M::soft_delete_enabled(),
        timestamps: M::has_timestamps(),
    }
}

fn qualified_table<M: ModelMeta>() -> String {
    match M::schema_name() {
        Some(schema) => format!("{schema}.{}", M::table_name()),
        None => M::table_name().to_string(),
    }
}

/// Refuse to connect while a compiled model breaks the project rules, naming
/// every broken rule. Checked once per process.
pub(crate) fn ensure_models_follow_rules() -> Result<()> {
    static BROKEN: OnceLock<Vec<String>> = OnceLock::new();
    let broken = BROKEN.get_or_init(|| {
        let models: Vec<ModelOutline> = inventory::iter::<CompiledModelRegistration>
            .into_iter()
            .map(|model| (model.outline)())
            .collect();
        broken_rules(&models)
    });
    if broken.is_empty() {
        return Ok(());
    }
    Err(Error::configuration(format!(
        "the models break the project rules of tideorm.toml ([rules]); fix them, \
         or exempt a model in [rules.allow]:\n  - {}",
        broken.join("\n  - ")
    )))
}

/// Every model rule `models` break, one line each.
pub(crate) fn broken_rules(models: &[ModelOutline]) -> Vec<String> {
    if models.iter().all(|model| model.rules == Rules::NONE) {
        return Vec::new();
    }
    let mut broken: Vec<String> = models.iter().flat_map(models::broken_rules).collect();
    broken.extend(graph::broken_rules(models));
    broken
}

/// Whether any compiled model's project forbids raw SQL.
pub(crate) fn raw_sql_forbidden_anywhere() -> bool {
    static FORBIDDEN: OnceLock<bool> = OnceLock::new();
    *FORBIDDEN.get_or_init(|| {
        inventory::iter::<CompiledModelRegistration>
            .into_iter()
            .any(|model| model.forbids_raw_sql)
    })
}

/// Whether raw SQL may run for `model`'s rules, or, with no model, for the
/// project's: refused with the reason when not.
pub(crate) fn ensure_raw_sql_allowed(what: &str, model: Option<(&str, &Rules)>) -> Result<()> {
    if !raw_sql_forbidden_anywhere() {
        return Ok(());
    }
    match model {
        Some((_, rules)) if !rules.forbid_raw_sql => Ok(()),
        Some((name, _)) => Err(raw_sql_refused(what, Some(name))),
        None => Err(raw_sql_refused(what, None)),
    }
}

pub(crate) fn raw_sql_refused(what: &str, model: Option<&str>) -> Error {
    let held = model.map_or_else(
        || "a statement that names no model".to_string(),
        |name| short_name(name).to_string(),
    );
    Error::query(format!(
        "{what} is raw SQL, which the project rules forbid for {held} \
         (forbid_raw_sql in tideorm.toml)"
    ))
}

/// Refuse a read of every row of `M` when its rules cap the rows of a query.
pub(crate) fn ensure_rows_allowed<M: ModelMeta>(
    what: impl std::fmt::Display,
    rows: Option<u64>,
) -> Result<()> {
    let Some(max) = M::__rules().max_rows else {
        return Ok(());
    };
    let reads = match rows {
        Some(rows) if rows <= max => return Ok(()),
        Some(rows) => format!("{what} reads up to {rows} rows"),
        None => format!("{what} reads every matching row"),
    };
    Err(Error::query(format!(
        "{reads} of {}, and the project rules allow at most {max} per query \
         (max_rows in tideorm.toml): read at most that many with limit(), paginate() or chunk()",
        short_name(std::any::type_name::<M>())
    )))
}

/// Refuse an eager load deeper or wider than `M`'s rules allow.
pub(crate) fn ensure_eager_load_allowed<M: ModelMeta>(
    depth: usize,
    relations: usize,
) -> Result<()> {
    let rules = M::__rules();
    let name = short_name(std::any::type_name::<M>());
    if let Some(max) = rules.max_eager_depth.filter(|max| depth > *max as usize) {
        return Err(Error::query(format!(
            "this eager load of {name} nests {depth} relations deep, and the project rules \
             allow {max} (max_eager_depth in tideorm.toml)"
        )));
    }
    if let Some(max) = rules
        .max_eager_relations
        .filter(|max| relations > *max as usize)
    {
        return Err(Error::query(format!(
            "this eager load of {name} resolves {relations} relations, and the project rules \
             allow {max} (max_eager_relations in tideorm.toml)"
        )));
    }
    Ok(())
}

/// A type's name without its module path.
pub(crate) fn short_name(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

#[cfg(test)]
#[path = "../../tests/unit/rules_tests.rs"]
mod tests;
