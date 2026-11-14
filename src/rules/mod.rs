//! Project rules: the limits a team writes in `tideorm.toml`'s `[rules]` table
//! and every model and query is held to. Nothing is enforced without that
//! table; `[rules.allow]` exempts named models from some rules.
//!
//! The derive reads the file as a model compiles. The model rules (its
//! columns, keys, indexes, relations) are checked right there: the derive
//! reports what it can see as compile errors, and writes `const` items that
//! call `columns` and `graph` for what needs a field's type or another
//! model, so `cargo check` fails on every broken rule. The query rules depend
//! on values a query only has as it runs, so the derive writes the ones the
//! model is held to into it (`ModelMeta::__rules`) and each query checks them.

mod columns;
mod graph;
mod message;

use std::sync::OnceLock;

use crate::error::{Error, Result};
use crate::model::ModelMeta;
use crate::sync::CompiledModelRegistration;

pub use columns::{__check_float_column, __check_json_columns, __check_key_type};
pub use graph::{
    __check_foreign_key, __check_relation_cycles, __check_relation_depth, __model_id, ModelNode,
};

/// The query rules one model is held to: the project's, less the model's
/// exceptions. Every rule is off in [`Rules::NONE`].
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rules {
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
