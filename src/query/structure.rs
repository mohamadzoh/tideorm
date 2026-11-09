//! Clause structures shared by [`QueryBuilder`](super::QueryBuilder) and the SQL
//! renderers behind it.
//!
//! Everything here describes a clause rather than storing rendered SQL. That is
//! what lets a query be snapshotted into a [`QueryFragment`], replayed onto a
//! different builder, and only then rendered for whichever backend the
//! connection turns out to be — and it is why the types that do carry SQL
//! ([`UnionClause`], [`CTE`]) keep their bound values beside it instead of
//! inlining them as literals.

mod consolidation;
mod window;

use super::Order;
use super::db_sql;
use crate::config::DatabaseType;
use crate::model::Model;
use std::fmt::Write as _;
use std::marker::PhantomData;

pub use consolidation::JoinResultConsolidator;
pub use window::{FrameBound, FrameType, WindowFunction, WindowFunctionType};

/// Type of JOIN operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum JoinType {
    /// `INNER JOIN` — keeps only rows that matched on both sides.
    Inner,
    /// `LEFT JOIN` — keeps every row of the model's own table, filling the
    /// joined table's columns with NULL where nothing matched.
    Left,
    /// `RIGHT JOIN` — the mirror of [`Left`](Self::Left).
    ///
    /// SQLite only learned this in 3.39 (2022); older builds fail to parse the
    /// statement, so a `LEFT JOIN` with the operands swapped is the portable
    /// spelling.
    Right,
}

impl JoinType {
    /// The keyword sequence emitted in front of the joined table.
    pub fn as_sql(&self) -> &'static str {
        match self {
            JoinType::Inner => "INNER JOIN",
            JoinType::Left => "LEFT JOIN",
            JoinType::Right => "RIGHT JOIN",
        }
    }

    /// The join as the statement builder takes it.
    pub(crate) fn to_builder(self) -> crate::orm::sql_builder::JoinType {
        match self {
            JoinType::Inner => crate::orm::sql_builder::JoinType::InnerJoin,
            JoinType::Left => crate::orm::sql_builder::JoinType::LeftJoin,
            JoinType::Right => crate::orm::sql_builder::JoinType::RightJoin,
        }
    }
}

/// One JOIN registered on a query by
/// [`inner_join()`](super::QueryBuilder::inner_join) and its siblings.
///
/// Every field is validated as an identifier (or `table.column` reference) when
/// the join is added: an unsafe value invalidates the builder and the clause is
/// never pushed, which is precisely why rendering can quote all four parts
/// without further escaping. It is also why a rejected join must not land here —
/// the alias, or the table name when there is no alias, is added to the set of
/// qualifiers that `table.column` references elsewhere in the query are allowed
/// to use.
///
/// The `ON` condition is always a single equality between two columns; there is
/// no representation for a composite or non-equi join.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct JoinClause {
    /// Which JOIN keyword to emit.
    pub(crate) join_type: JoinType,
    /// Name of the joined table.
    pub(crate) table: String,
    /// Optional `AS` alias. When set, this — not [`table`](Self::table) — is the
    /// qualifier other clauses must use to reference the joined columns.
    pub(crate) alias: Option<String>,
    /// Left-hand side of the `ON` equality, normally `this_table.column`.
    pub(crate) left_column: String,
    /// Right-hand side of the `ON` equality, normally `joined_table.column`.
    pub(crate) right_column: String,
}

impl JoinClause {
    /// The joined table's name without its schema: `events` for `audit.events`.
    pub(crate) fn bare_table(&self) -> &str {
        self.table.rsplit('.').next().unwrap_or(&self.table)
    }

    /// The name other clauses qualify the joined columns with: the alias,
    /// else the bare table name.
    pub(crate) fn qualifier(&self) -> &str {
        self.alias.as_deref().unwrap_or_else(|| self.bare_table())
    }
}

/// A scalar subquery projected by
/// [`select_subquery()`](super::QueryBuilder::select_subquery), rendered with
/// the statement.
#[derive(Debug, Clone)]
pub(crate) struct SubquerySelect {
    pub(crate) query: std::sync::Arc<dyn super::Embedded>,
    pub(crate) alias: String,
    pub(crate) source: Option<(String, String)>,
    /// Captured before the subquery loses its model and join-alias scope.
    pub(crate) encrypted_column: Option<crate::model::EncryptedColumn>,
    pub(crate) column_type: Option<crate::orm::ColumnType>,
}

/// Which compound-select operator joins two result sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum UnionType {
    /// `UNION` — combines both sides and removes duplicate rows, which forces
    /// the backend to sort or hash the whole result.
    Union,
    /// `UNION ALL` — concatenates both sides verbatim. Cheaper than
    /// [`Union`](Self::Union), and the right choice whenever the operands are
    /// already known to be disjoint.
    UnionAll,
}

impl UnionType {
    /// The operator as the statement builder takes it.
    pub(crate) fn to_builder(self) -> crate::orm::sql_builder::UnionType {
        match self {
            UnionType::Union => crate::orm::sql_builder::UnionType::Distinct,
            UnionType::UnionAll => crate::orm::sql_builder::UnionType::All,
        }
    }
}

/// One operand of a compound select (`UNION`, `UNION ALL`, ...).
#[derive(Debug, Clone)]
pub(crate) struct UnionClause {
    /// Which compound operator precedes this operand.
    pub(crate) union_type: UnionType,
    /// The operand: a query, rendered with the statement, or a caller's
    /// trusted SQL. It is parenthesized on PostgreSQL and MySQL but not on
    /// SQLite, whose compound-select grammar rejects a parenthesized operand.
    pub(crate) operand: super::Operand,
    /// Whether the operand is caller-written SQL of unknown shape, or a query
    /// reading `table.*` because it holds one.
    pub(crate) raw: bool,
    /// Captured while a typed operand still has its model and projection scope.
    pub(crate) projection: Option<UnionProjection>,
}

/// Types and encryption scopes in SQL projection order. Unknown shapes cannot safely
/// share a decoder with a known encrypted projection.
#[derive(Debug, Clone)]
pub(crate) struct UnionProjection {
    pub(crate) columns: Vec<Option<crate::model::EncryptedColumn>>,
    pub(crate) types: Vec<Option<crate::orm::ColumnType>>,
    pub(crate) complete: bool,
    pub(crate) has_encrypted: bool,
}

/// Types that use the same model JSON decoder. Keep integer widths and float
/// precision distinct: decoding another arm through a narrower type can lose data.
pub(crate) fn union_column_types_compatible(
    a: &crate::orm::ColumnType,
    b: &crate::orm::ColumnType,
) -> bool {
    use crate::orm::ColumnType::*;
    a == b
        || matches!(
            (a, b),
            (Char(_) | String(_) | Text, Char(_) | String(_) | Text)
                | (
                    Binary(_) | VarBinary(_) | Blob,
                    Binary(_) | VarBinary(_) | Blob
                )
                | (Decimal(_) | Money(_), Decimal(_) | Money(_))
                | (Json | JsonBinary, Json | JsonBinary)
                | (DateTime | Timestamp, DateTime | Timestamp)
        )
}

impl UnionClause {
    pub(crate) fn new(union_type: UnionType, operand: super::Operand) -> Self {
        Self {
            union_type,
            operand,
            raw: false,
            projection: None,
        }
    }
}

/// A named `WITH` clause body spliced in front of the outer select.
///
/// A body [`with_query()`](super::QueryBuilder::with_query) takes is kept as
/// its query and rendered with the statement, its values bound.
#[derive(Debug, Clone)]
#[must_use = "a CTE does nothing until a query takes it"]
pub struct CTE {
    /// The name the outer query references the CTE by. Quoted when rendered and
    /// validated as an identifier when the query is checked.
    pub name: String,
    /// Optional explicit output column list, rendered as `name (a, b) AS (..)`.
    /// `None` keeps whatever names the body itself projects.
    pub columns: Option<Vec<String>>,
    /// The CTE body: a complete select statement in trusted SQL. Empty for a
    /// body taken from a query.
    pub query_sql: String,
    /// Whether the body refers to the CTE's own name.
    ///
    /// The keyword is per-statement, not per-CTE: one recursive CTE promotes the
    /// entire `WITH` prefix to `WITH RECURSIVE`, which is exactly what the SQL
    /// standard requires.
    pub recursive: bool,
    pub(crate) query: Option<std::sync::Arc<dyn super::Embedded>>,
}

impl CTE {
    /// Build a non-recursive CTE from trusted SQL with no bound values.
    ///
    /// **Trusted SQL only** — the body is validated as a subquery but not
    /// sanitized. Prefer [`with_query()`](super::QueryBuilder::with_query),
    /// which renders the body from a second builder and keeps its values as
    /// bound parameters.
    pub fn new(name: impl Into<String>, query_sql: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            columns: None,
            query_sql: query_sql.into(),
            recursive: false,
            query: None,
        }
    }

    /// Build a non-recursive CTE that renames its output columns.
    ///
    /// Carries the same trusted-SQL caveat as [`new()`](Self::new). The column
    /// list must have exactly as many entries as the body projects.
    pub fn with_columns(
        name: impl Into<String>,
        columns: impl crate::columns::IntoColumnNames,
        query_sql: impl Into<String>,
    ) -> Self {
        Self {
            columns: Some(crate::columns::column_names(&columns)),
            ..Self::new(name, query_sql)
        }
    }

    /// A CTE whose body is `query`.
    pub(crate) fn of_query(name: &str, query: impl super::Embedded + 'static) -> Self {
        Self {
            query: Some(std::sync::Arc::new(query)),
            ..Self::new(name, String::new())
        }
    }

    /// Mark the CTE recursive.
    ///
    /// The body must then be `<base case> UNION ALL <recursive case>` with the
    /// recursive half selecting from the CTE's own name — nothing here checks
    /// that shape.
    /// [`with_recursive_cte()`](super::QueryBuilder::with_recursive_cte)
    /// assembles it for you.
    pub fn recursive(mut self) -> Self {
        self.recursive = true;
        self
    }
}

/// A reusable snapshot of a [`QueryBuilder`](super::QueryBuilder)'s clauses.
///
/// A fragment stores clauses verbatim rather than rendered SQL, so it carries no
/// inline literals and stays backend-agnostic: HAVING clauses keep their `?`
/// placeholders next to their bound values, exactly as `UnionClause` and
/// [`CTE`] operands do.
///
/// Produced by [`QueryBuilder::consolidate()`](super::QueryBuilder::consolidate)
/// and merged back in by [`QueryBuilder::apply()`](super::QueryBuilder::apply),
/// whose documentation defines the merge semantics. Its clauses are private,
/// so a fragment can only come from a builder that already validated them.
#[derive(Debug, Clone)]
#[must_use = "a fragment does nothing until `apply()` takes it"]
pub struct QueryFragment<M: Model> {
    pub(crate) _marker: PhantomData<M>,
    pub(crate) clauses: super::clauses::Clauses,
}

impl<M: Model> Default for QueryFragment<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: Model> QueryFragment<M> {
    /// An empty fragment: applying it to a builder changes nothing.
    ///
    /// Useful as the identity element when folding conditional fragments
    /// together; a fragment is normally obtained from
    /// [`consolidate()`](super::QueryBuilder::consolidate) instead.
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
            clauses: super::clauses::Clauses::default(),
        }
    }

    /// Whether applying this fragment would be a no-op.
    ///
    /// Soft-delete scope counts as state even though it adds no clause of its
    /// own: a fragment whose only content is `with_trashed()` still changes the
    /// query it is applied to, so reporting it as empty would be wrong.
    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }

    /// How many WHERE predicates the fragment carries, counting the contents of
    /// every OR group and of the groups nested inside them.
    ///
    /// This is the "does this actually filter anything" measure, so it counts
    /// only the WHERE clause: HAVING conditions, joins and CTE bodies are not
    /// included no matter how selective they are.
    pub fn condition_count(&self) -> usize {
        self.clauses.conditions.len()
            + self
                .clauses
                .or_groups
                .iter()
                .map(super::OrGroup::condition_count)
                .sum::<usize>()
    }
}
