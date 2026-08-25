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

use super::db_sql;
use super::{Order, WhereCondition};
use crate::config::DatabaseType;
use crate::internal::Value;
use crate::model::Model;
use std::marker::PhantomData;

pub use consolidation::JoinResultConsolidator;

/// Type of JOIN operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone)]
pub struct JoinClause {
    /// Which JOIN keyword to emit.
    pub join_type: JoinType,
    /// Name of the joined table.
    pub table: String,
    /// Optional `AS` alias. When set, this — not [`table`](Self::table) — is the
    /// qualifier other clauses must use to reference the joined columns.
    pub alias: Option<String>,
    /// Left-hand side of the `ON` equality, normally `this_table.column`.
    pub left_column: String,
    /// Right-hand side of the `ON` equality, normally `joined_table.column`.
    pub right_column: String,
}

/// A named aggregate over a column.
///
/// This is a descriptive value type, not a builder input: the aggregate
/// terminals ([`sum()`](super::QueryBuilder::sum),
/// [`avg()`](super::QueryBuilder::avg),
/// [`count_distinct()`](super::QueryBuilder::count_distinct), ...) render their
/// own SQL and never construct one of these. Reach for it when your own code
/// needs to carry a user-chosen aggregate around — match on it and call the
/// matching terminal.
#[derive(Debug, Clone)]
pub enum AggregateFunction {
    /// `COUNT(*)` — counts rows, including rows that are NULL in every column.
    Count,
    /// `COUNT(DISTINCT column)` — counts distinct non-NULL values of the column.
    CountDistinct(String),
    /// `SUM(column)`.
    Sum(String),
    /// `AVG(column)`, which ignores NULL rows rather than treating them as zero.
    Avg(String),
    /// `MIN(column)`.
    Min(String),
    /// `MAX(column)`.
    Max(String),
}

/// Which compound-select operator joins two result sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnionType {
    /// `UNION` — combines both sides and removes duplicate rows, which forces
    /// the backend to sort or hash the whole result.
    Union,
    /// `UNION ALL` — concatenates both sides verbatim. Cheaper than
    /// [`Union`](Self::Union), and the right choice whenever the operands are
    /// already known to be disjoint.
    UnionAll,
}

impl UnionType {
    /// The keyword sequence emitted between two compound-select operands.
    pub fn as_sql(&self) -> &'static str {
        match self {
            UnionType::Union => "UNION",
            UnionType::UnionAll => "UNION ALL",
        }
    }
}

/// One operand of a compound select (`UNION`, `UNION ALL`, ...).
///
/// `query_sql` is executable SQL that is concatenated into the outer statement,
/// so it never carries inline literals for builder-supplied values: operands
/// rendered from a `QueryBuilder` are parameterized and their bound values
/// travel next to the SQL in `params`. Raw operands are trusted SQL supplied by
/// the caller and bind nothing.
#[derive(Debug, Clone)]
pub struct UnionClause {
    /// Which compound operator precedes this operand.
    pub union_type: UnionType,
    /// The operand's own select statement.
    ///
    /// It is spliced into the outer statement rather than re-parsed, with two
    /// backend adjustments made at assembly time: PostgreSQL placeholders are
    /// renumbered for the operand's position among the already-bound values, and
    /// the operand is wrapped in parentheses on PostgreSQL and MySQL but not on
    /// SQLite, whose compound-select grammar rejects a parenthesized operand.
    pub query_sql: String,
    pub(crate) params: Vec<Value>,
}

impl UnionClause {
    /// Build an operand from trusted SQL that carries no bound values.
    ///
    /// This is the constructor behind [`union_raw()`](super::QueryBuilder::union_raw)
    /// and [`union_all_raw()`](super::QueryBuilder::union_all_raw). Prefer
    /// [`union()`](super::QueryBuilder::union), which renders the operand from a
    /// second builder and keeps its values as bound parameters.
    pub fn new(union_type: UnionType, query_sql: String) -> Self {
        Self {
            union_type,
            query_sql,
            params: Vec::new(),
        }
    }

    /// Build an operand from parameterized SQL and the values bound to it.
    ///
    /// `params` must be ordered exactly as the placeholders appear in
    /// `query_sql`; the assembler splices them into the outer statement in that
    /// order.
    pub(crate) fn with_params(
        union_type: UnionType,
        query_sql: String,
        params: Vec<Value>,
    ) -> Self {
        Self {
            union_type,
            query_sql,
            params,
        }
    }
}

/// One end of a window frame.
///
/// How the offsets are counted depends on the accompanying [`FrameType`]: under
/// [`Rows`](FrameType::Rows) they are physical row counts, under
/// [`Range`](FrameType::Range) they are offsets from the current row's ORDER BY
/// value, and under [`Groups`](FrameType::Groups) they count peer groups.
#[derive(Debug, Clone)]
pub enum FrameBound {
    /// Start the frame at the first row of the partition.
    UnboundedPreceding,
    /// End the frame at the last row of the partition.
    UnboundedFollowing,
    /// The current row (or, under `RANGE`/`GROUPS`, the current row's whole peer
    /// group).
    CurrentRow,
    /// `n` units before the current row.
    Preceding(u64),
    /// `n` units after the current row.
    Following(u64),
}

impl FrameBound {
    /// Render this bound. The value of a `PRECEDING`/`FOLLOWING` offset is a
    /// `u64` and is therefore safe to interpolate.
    pub fn as_sql(&self) -> String {
        match self {
            FrameBound::UnboundedPreceding => "UNBOUNDED PRECEDING".to_string(),
            FrameBound::UnboundedFollowing => "UNBOUNDED FOLLOWING".to_string(),
            FrameBound::CurrentRow => "CURRENT ROW".to_string(),
            FrameBound::Preceding(n) => format!("{} PRECEDING", n),
            FrameBound::Following(n) => format!("{} FOLLOWING", n),
        }
    }
}

/// The unit a window frame's bounds are measured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    /// Count physical rows. Ties in the ORDER BY are split, so two peer rows can
    /// see different frames — which is what makes this the right choice for a
    /// running total.
    Rows,
    /// Count by ORDER BY *value*, so peers (rows that compare equal) always
    /// enter and leave the frame together. Requires an ORDER BY, and an offset
    /// bound requires exactly one ordering column of a type the offset can be
    /// added to.
    Range,
    /// Count peer groups rather than rows. Requires an ORDER BY, and is
    /// unsupported by MySQL and MariaDB — PostgreSQL 11+ and SQLite 3.28+ accept
    /// it.
    Groups,
}

impl FrameType {
    /// The frame keyword emitted in front of the bounds.
    pub fn as_sql(&self) -> &'static str {
        match self {
            FrameType::Rows => "ROWS",
            FrameType::Range => "RANGE",
            FrameType::Groups => "GROUPS",
        }
    }
}

/// Which window function a [`WindowFunction`] computes.
///
/// Column arguments are quoted as identifiers when rendered, so they are safe to
/// take from a caller; [`Custom`](Self::Custom) and the `default` argument of
/// [`Lag`](Self::Lag)/[`Lead`](Self::Lead) are the two exceptions and are
/// emitted verbatim as trusted SQL.
#[derive(Debug, Clone)]
pub enum WindowFunctionType {
    /// `ROW_NUMBER()` — 1-based position within the partition, never tied.
    RowNumber,
    /// `RANK()` — tied rows share a rank and the next rank skips the gap.
    Rank,
    /// `DENSE_RANK()` — tied rows share a rank with no gap after them.
    DenseRank,
    /// `NTILE(n)` — distribute the partition over `n` buckets as evenly as it
    /// divides.
    Ntile(u32),
    /// `LAG(column, offset, default)` — a value from an earlier row of the
    /// partition.
    ///
    /// The offset and default are only rendered when the offset is `Some`: a
    /// default supplied alongside `None` is silently dropped, because SQL has no
    /// way to pass the third argument without the second.
    Lag(String, Option<i32>, Option<String>),
    /// `LEAD(column, offset, default)` — a value from a later row, with the same
    /// argument rule as [`Lag`](Self::Lag).
    Lead(String, Option<i32>, Option<String>),
    /// `FIRST_VALUE(column)` — the column's value in the frame's first row.
    FirstValue(String),
    /// `LAST_VALUE(column)` — the column's value in the frame's *last* row.
    ///
    /// The default frame ends at the current row, so without a frame that
    /// extends to [`UnboundedFollowing`](FrameBound::UnboundedFollowing) this
    /// returns the current row's own value.
    /// [`last_value()`](super::QueryBuilder::last_value) sets that frame for you.
    LastValue(String),
    /// `NTH_VALUE(column, n)` — the column's value in the frame's `n`-th row,
    /// 1-based.
    NthValue(String, u32),
    /// `SUM(column)` evaluated over the frame instead of collapsing the rows.
    Sum(String),
    /// `AVG(column)` evaluated over the frame.
    Avg(String),
    /// `COUNT(column)`, or `COUNT(*)` when the column is `None`.
    Count(Option<String>),
    /// `MIN(column)` evaluated over the frame.
    Min(String),
    /// `MAX(column)` evaluated over the frame.
    Max(String),
    /// A caller-supplied function expression, emitted verbatim.
    ///
    /// **Trusted SQL only.** It is checked by the shared raw-fragment validator
    /// (statement separators, comments and NUL bytes are rejected), but that is
    /// a backstop and not a sanitizer — never build one out of request input.
    Custom(String),
}

impl WindowFunctionType {
    /// Render the function call itself — no `OVER (..)` clause, which
    /// [`WindowFunction::to_sql_for_db`] adds around it.
    ///
    /// Column arguments go through the strict column formatter, so anything that
    /// is not a plain `column`/`table.column` reference is quoted as one
    /// identifier rather than passed through as SQL. Numeric arguments are
    /// integers and safe to interpolate.
    pub fn as_sql_for_db(&self, db_type: DatabaseType) -> String {
        let quote_column = |column: &str| db_sql::format_column(db_type, column);

        match self {
            WindowFunctionType::RowNumber => "ROW_NUMBER()".to_string(),
            WindowFunctionType::Rank => "RANK()".to_string(),
            WindowFunctionType::DenseRank => "DENSE_RANK()".to_string(),
            WindowFunctionType::Ntile(n) => format!("NTILE({})", n),
            WindowFunctionType::Lag(col, offset, default) => {
                let mut s = format!("LAG({}", quote_column(col));
                if let Some(o) = offset {
                    s.push_str(&format!(", {}", o));
                    if let Some(d) = default {
                        s.push_str(&format!(", {}", d));
                    }
                }
                s.push(')');
                s
            }
            WindowFunctionType::Lead(col, offset, default) => {
                let mut s = format!("LEAD({}", quote_column(col));
                if let Some(o) = offset {
                    s.push_str(&format!(", {}", o));
                    if let Some(d) = default {
                        s.push_str(&format!(", {}", d));
                    }
                }
                s.push(')');
                s
            }
            WindowFunctionType::FirstValue(col) => {
                format!("FIRST_VALUE({})", quote_column(col))
            }
            WindowFunctionType::LastValue(col) => {
                format!("LAST_VALUE({})", quote_column(col))
            }
            WindowFunctionType::NthValue(col, n) => {
                format!("NTH_VALUE({}, {})", quote_column(col), n)
            }
            WindowFunctionType::Sum(col) => format!("SUM({})", quote_column(col)),
            WindowFunctionType::Avg(col) => format!("AVG({})", quote_column(col)),
            WindowFunctionType::Count(col) => match col {
                Some(c) => format!("COUNT({})", quote_column(c)),
                None => "COUNT(*)".to_string(),
            },
            WindowFunctionType::Min(col) => format!("MIN({})", quote_column(col)),
            WindowFunctionType::Max(col) => format!("MAX({})", quote_column(col)),
            WindowFunctionType::Custom(expr) => expr.clone(),
        }
    }

    /// Render the call with PostgreSQL identifier quoting.
    ///
    /// A convenience for previews and tests only. Executed queries go through
    /// [`as_sql_for_db`](Self::as_sql_for_db) with the connection's real backend,
    /// because MySQL and MariaDB reject `"column"` under their default
    /// `sql_mode`.
    pub fn as_sql(&self) -> String {
        self.as_sql_for_db(DatabaseType::Postgres)
    }
}

/// A window function and the `OVER (..)` clause it is evaluated in.
///
/// Registered on a query with [`window()`](super::QueryBuilder::window); the
/// named helpers ([`row_number()`](super::QueryBuilder::row_number),
/// [`running_sum()`](super::QueryBuilder::running_sum), ...) assemble the common
/// shapes. Window functions are appended to the projection *after* every other
/// select source and never suppress the `table.*` fallback, so a query that
/// selects nothing else still returns the model's own columns alongside them.
#[derive(Debug, Clone)]
pub struct WindowFunction {
    /// The function being computed.
    pub function: WindowFunctionType,
    /// `PARTITION BY` columns. Empty means the whole result set is one
    /// partition.
    pub partition_by: Vec<String>,
    /// The window's own `ORDER BY`, independent of the query's. Ranking
    /// functions are meaningless without it, and a `RANGE`/`GROUPS` frame
    /// requires it.
    pub order_by: Vec<(String, Order)>,
    /// Frame unit. Rendered only together with
    /// [`frame_start`](Self::frame_start).
    pub frame_type: Option<FrameType>,
    /// Frame start bound. Nothing is rendered unless both this and
    /// [`frame_type`](Self::frame_type) are set.
    pub frame_start: Option<FrameBound>,
    /// Frame end bound. With it the frame renders as `BETWEEN start AND end`;
    /// without it, as the shorthand `<unit> <start>`. Setting only the end bound
    /// renders no frame at all.
    pub frame_end: Option<FrameBound>,
    /// Output column name. Quoted when rendered, and validated as an identifier
    /// when the query is checked.
    pub alias: String,
}

impl WindowFunction {
    /// Start an unpartitioned, unordered, unframed window under `alias`.
    ///
    /// Refine it with [`partition_by()`](Self::partition_by),
    /// [`order_by()`](Self::order_by) and [`frame()`](Self::frame) before
    /// handing it to [`window()`](super::QueryBuilder::window).
    pub fn new(function: WindowFunctionType, alias: &str) -> Self {
        Self {
            function,
            partition_by: Vec::new(),
            order_by: Vec::new(),
            frame_type: None,
            frame_start: None,
            frame_end: None,
            alias: alias.to_string(),
        }
    }

    /// Append a `PARTITION BY` column. Repeated calls accumulate left to right.
    pub fn partition_by(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.partition_by.push(column.column_name().to_string());
        self
    }

    /// Append an `ORDER BY` term to the window. Repeated calls accumulate into a
    /// compound sort key; this is the window's own ordering and does not affect
    /// the row order the query returns.
    pub fn order_by(
        mut self,
        column: impl crate::columns::IntoColumnName,
        direction: Order,
    ) -> Self {
        self.order_by
            .push((column.column_name().to_string(), direction));
        self
    }

    /// Set the whole frame at once, as `BETWEEN start AND end`.
    ///
    /// The classic running-total frame is
    /// `frame(FrameType::Rows, FrameBound::UnboundedPreceding, FrameBound::CurrentRow)`,
    /// which is what [`running_sum()`](super::QueryBuilder::running_sum)
    /// installs.
    pub fn frame(mut self, frame_type: FrameType, start: FrameBound, end: FrameBound) -> Self {
        self.frame_type = Some(frame_type);
        self.frame_start = Some(start);
        self.frame_end = Some(end);
        self
    }

    /// Render `<function> OVER (PARTITION BY .. ORDER BY .. <frame>) AS <alias>`
    /// for `db_type`. This is the rendering the executable path uses.
    ///
    /// Empty clauses are omitted rather than emitted empty, so an unpartitioned,
    /// unordered, unframed window renders as a bare `OVER ()`.
    pub fn to_sql_for_db(&self, db_type: DatabaseType) -> String {
        let mut sql = self.function.as_sql_for_db(db_type);
        sql.push_str(" OVER (");

        let mut clauses = Vec::new();

        if !self.partition_by.is_empty() {
            let cols: Vec<String> = self
                .partition_by
                .iter()
                .map(|c| db_sql::format_column(db_type, c))
                .collect();
            clauses.push(format!("PARTITION BY {}", cols.join(", ")));
        }

        if !self.order_by.is_empty() {
            let orders: Vec<String> = self
                .order_by
                .iter()
                .map(|(col, dir)| {
                    format!("{} {}", db_sql::format_column(db_type, col), dir.as_str())
                })
                .collect();
            clauses.push(format!("ORDER BY {}", orders.join(", ")));
        }

        if let (Some(frame_type), Some(start)) = (&self.frame_type, &self.frame_start) {
            let frame_sql = if let Some(end) = &self.frame_end {
                format!(
                    "{} BETWEEN {} AND {}",
                    frame_type.as_sql(),
                    start.as_sql(),
                    end.as_sql()
                )
            } else {
                format!("{} {}", frame_type.as_sql(), start.as_sql())
            };
            clauses.push(frame_sql);
        }

        sql.push_str(&clauses.join(" "));
        sql.push_str(&format!(
            ") AS {}",
            db_sql::quote_ident(db_type, &self.alias)
        ));
        sql
    }

    /// Render with PostgreSQL identifier quoting, for previews and tests.
    ///
    /// Prefer [`to_sql_for_db`](Self::to_sql_for_db), which is what the
    /// executable path calls: the double-quoted identifiers this emits are
    /// rejected by MySQL and MariaDB under their default `sql_mode`.
    pub fn to_sql(&self) -> String {
        self.to_sql_for_db(DatabaseType::Postgres)
    }
}

/// A named `WITH` clause body spliced in front of the outer select.
///
/// Like [`UnionClause`], a body rendered from a `QueryBuilder` is parameterized
/// and keeps its bound values in `params` instead of inlining them as literals.
#[derive(Debug, Clone)]
pub struct CTE {
    /// The name the outer query references the CTE by. Quoted when rendered and
    /// validated as an identifier when the query is checked.
    pub name: String,
    /// Optional explicit output column list, rendered as `name (a, b) AS (..)`.
    /// `None` keeps whatever names the body itself projects.
    pub columns: Option<Vec<String>>,
    /// The CTE body: a complete select statement, spliced into the outer
    /// statement with its PostgreSQL placeholders renumbered for its position.
    pub query_sql: String,
    /// Whether the body refers to the CTE's own name.
    ///
    /// The keyword is per-statement, not per-CTE: one recursive CTE promotes the
    /// entire `WITH` prefix to `WITH RECURSIVE`, which is exactly what the SQL
    /// standard requires. It also switches validation of
    /// [`query_sql`](Self::query_sql) to the compound-subquery validator, since
    /// a recursive body is a `UNION ALL` of two selects.
    pub recursive: bool,
    pub(crate) params: Vec<Value>,
}

impl CTE {
    /// Build a non-recursive CTE from trusted SQL with no bound values.
    ///
    /// **Trusted SQL only** — the body is validated as a subquery but not
    /// sanitized. Prefer [`with_query()`](super::QueryBuilder::with_query),
    /// which renders the body from a second builder and keeps its values as
    /// bound parameters.
    pub fn new(name: &str, query_sql: String) -> Self {
        Self {
            name: name.to_string(),
            columns: None,
            query_sql,
            recursive: false,
            params: Vec::new(),
        }
    }

    /// Build a non-recursive CTE that renames its output columns.
    ///
    /// Carries the same trusted-SQL caveat as [`new()`](Self::new). The column
    /// list must have exactly as many entries as the body projects.
    pub fn with_columns(name: &str, columns: Vec<&str>, query_sql: String) -> Self {
        Self {
            name: name.to_string(),
            columns: Some(columns.into_iter().map(|s| s.to_string()).collect()),
            query_sql,
            recursive: false,
            params: Vec::new(),
        }
    }

    /// Build a CTE from parameterized SQL and the values bound to it.
    ///
    /// `params` must be ordered exactly as the placeholders appear in
    /// `query_sql`.
    pub(crate) fn with_params(name: &str, query_sql: String, params: Vec<Value>) -> Self {
        Self {
            name: name.to_string(),
            columns: None,
            query_sql,
            recursive: false,
            params,
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

    /// Render the CTE using PostgreSQL identifier quoting.
    ///
    /// Prefer [`to_sql_for_db`](Self::to_sql_for_db): MySQL and MariaDB reject
    /// `"name"` under their default `sql_mode`, so this rendering is only
    /// correct on PostgreSQL and SQLite. The Postgres default is kept for
    /// backwards compatibility, matching [`WindowFunction::to_sql`].
    pub fn to_sql(&self) -> String {
        self.to_sql_for_db(DatabaseType::Postgres)
    }

    /// Render the CTE for `db_type`, quoting the name and the optional column
    /// list with that backend's identifier quote character.
    ///
    /// This is the rendering the executable path uses. It emits only the
    /// `name (columns) AS (body)` entry — the `WITH` / `WITH RECURSIVE` keyword
    /// and the commas between entries belong to the statement, not to a single
    /// CTE.
    pub fn to_sql_for_db(&self, db_type: DatabaseType) -> String {
        self.to_sql_with_body_for_db(db_type, &self.query_sql)
    }

    /// Render the `name (columns) AS (..)` shell for `db_type` around an
    /// already-rendered body.
    pub(crate) fn to_sql_with_body_for_db(&self, db_type: DatabaseType, body_sql: &str) -> String {
        let mut sql = db_sql::quote_ident(db_type, &self.name);

        if let Some(ref cols) = self.columns {
            let col_list: Vec<String> = cols
                .iter()
                .map(|c| db_sql::quote_ident(db_type, c))
                .collect();
            sql.push_str(&format!(" ({})", col_list.join(", ")));
        }

        sql.push_str(&format!(" AS ({})", body_sql));
        sql
    }
}

/// A reusable snapshot of a [`QueryBuilder`](super::QueryBuilder)'s clauses.
///
/// A fragment stores clauses verbatim rather than rendered SQL, so it carries no
/// inline literals and stays backend-agnostic: HAVING clauses keep their `?`
/// placeholders and travel next to their bound values in `having_bindings`,
/// exactly as [`UnionClause`] and [`CTE`] operands do.
///
/// Produced by [`QueryBuilder::consolidate()`](super::QueryBuilder::consolidate)
/// and merged back in by [`QueryBuilder::apply()`](super::QueryBuilder::apply),
/// whose documentation defines the merge semantics. The fields mirror the
/// builder's own one for one, and each one's doc names which of the three merge
/// rules it follows:
///
/// - **appended** — every list slot. Merging is additive, so a fragment's
///   ORDER BY term becomes a *further* sort key rather than replacing the
///   builder's.
/// - **last-wins** — every single-value setter slot. A fragment that left the
///   slot unset leaves the builder's value alone.
/// - **first-wins** — [`invalid_query_reason`](Self::invalid_query_reason)
///   alone, so the earliest recorded failure is the one reported.
///
/// The type cannot be constructed with a struct literal from outside the crate
/// because of a private `PhantomData` field. That is deliberate: it keeps the
/// [`order_by`](Self::order_by) raw-expression marker unforgeable, since a
/// fragment can only ever be obtained from a builder that already validated the
/// clauses it holds.
#[derive(Debug, Clone)]
pub struct QueryFragment<M: Model> {
    pub(crate) _marker: PhantomData<M>,
    /// WHERE conditions combined with AND. **Appended.**
    pub conditions: Vec<WhereCondition>,
    /// Parenthesized OR groups, AND-ed with [`conditions`](Self::conditions).
    /// **Appended.**
    pub or_groups: Vec<super::OrGroup>,
    /// ORDER BY terms as `(column, direction)` pairs. **Appended**, so the
    /// fragment's terms become the least significant sort keys.
    ///
    /// The first element is *not* necessarily a column name: an entry added by
    /// [`order_by_raw()`](super::QueryBuilder::order_by_raw) is a trusted SQL
    /// expression carrying a private marker prefix that both validation and
    /// rendering use to tell it apart from a validated column reference. Treat
    /// the string as opaque; matching it against your model's columns will not
    /// work.
    pub order_by: Vec<(String, Order)>,
    /// LIMIT. **Last-wins**, mirroring `.limit(5).limit(10)`.
    pub limit_value: Option<u64>,
    /// OFFSET. **Last-wins.** A standalone offset is portable — rendering
    /// supplies the open-ended LIMIT that MySQL, MariaDB and SQLite require.
    pub offset_value: Option<u64>,
    /// The typed half of the projection, from
    /// [`select()`](super::QueryBuilder::select). **Last-wins**, and `None`
    /// means "not chosen", not "select nothing".
    pub select_columns: Option<Vec<String>>,
    /// Raw SELECT expressions, plus the sentinel `QueryBuilder::distinct()`
    /// records here. The sentinel travels with the projection it modifies, so a
    /// fragment round trip preserves `DISTINCT` without a slot of its own; the
    /// renderer strips it and emits the keyword instead.
    ///
    /// **Appended**, except that the sentinel is deduplicated on merge so an
    /// already-distinct builder does not collect a second copy.
    pub raw_select_expressions: Vec<String>,
    /// Scalar subquery projections as `(subquery_sql, alias)` pairs, from
    /// [`select_subquery()`](super::QueryBuilder::select_subquery).
    /// **Appended.**
    pub subquery_select_expressions: Vec<(String, String)>,
    /// GROUP BY columns. **Appended.** These are validated as column references
    /// only — unlike ORDER BY there is no raw escape hatch, because GROUP BY is
    /// rendered outside any quoted literal.
    pub group_by: Vec<String>,
    /// HAVING clause templates, AND-ed together. **Appended** in lockstep with
    /// `having_bindings`.
    ///
    /// Each entry keeps its `?` placeholders unsubstituted; a clause's
    /// placeholder count must equal the length of its binding slot, and
    /// validation rejects the query outright when it does not, rather than
    /// leaving PostgreSQL's `$n` numbering to drift for every later parameter.
    pub having_conditions: Vec<String>,
    /// Values bound to each HAVING clause, indexed in lockstep with
    /// `having_conditions`: slot `i` holds the values for the `?` placeholders
    /// in clause `i`, and is empty for a clause written as raw SQL.
    pub(crate) having_bindings: Vec<Vec<serde_json::Value>>,
    /// JOIN clauses, in the order they will be rendered. **Appended.**
    pub joins: Vec<JoinClause>,
    /// Compound-select operands. **Appended.**
    pub unions: Vec<UnionClause>,
    /// Window functions added to the projection. **Appended.**
    pub window_functions: Vec<WindowFunction>,
    /// `WITH` clause bodies, in declaration order. **Appended.**
    pub ctes: Vec<CTE>,
    /// Result-cache settings. **Last-wins**; `None` means the query is not
    /// cached rather than "use the default TTL".
    pub cache_options: Option<crate::cache::CacheOptions>,
    /// Caller-supplied cache key from
    /// [`cache_with_key()`](super::QueryBuilder::cache_with_key). **Last-wins.**
    /// When set it replaces the structural key entirely, so two genuinely
    /// different queries sharing a key share a cache entry — it is still
    /// namespaced per model and per connection.
    pub cache_key: Option<String>,
    /// The first builder call that failed, deferred to execution time.
    /// **First-wins**, so the earliest failure is the one reported and later
    /// ones do not mask it.
    pub invalid_query_reason: Option<String>,
    /// `with_trashed()`: include soft-deleted rows. **Last-wins**, and mutually
    /// exclusive with [`only_trashed`](Self::only_trashed) — a fragment that set
    /// neither leaves the builder's scope untouched.
    pub include_trashed: bool,
    /// `only_trashed()`: return *only* soft-deleted rows. Takes precedence over
    /// [`include_trashed`](Self::include_trashed) when merged.
    pub only_trashed: bool,
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
            conditions: Vec::new(),
            or_groups: Vec::new(),
            order_by: Vec::new(),
            limit_value: None,
            offset_value: None,
            select_columns: None,
            raw_select_expressions: Vec::new(),
            subquery_select_expressions: Vec::new(),
            group_by: Vec::new(),
            having_conditions: Vec::new(),
            having_bindings: Vec::new(),
            joins: Vec::new(),
            unions: Vec::new(),
            window_functions: Vec::new(),
            ctes: Vec::new(),
            cache_options: None,
            cache_key: None,
            invalid_query_reason: None,
            include_trashed: false,
            only_trashed: false,
        }
    }

    /// Whether applying this fragment would be a no-op.
    ///
    /// Soft-delete scope counts as state even though it adds no clause of its
    /// own: a fragment whose only content is `with_trashed()` still changes the
    /// query it is applied to, so reporting it as empty would be wrong.
    pub fn is_empty(&self) -> bool {
        let has_query_state = !self.conditions.is_empty()
            || !self.or_groups.is_empty()
            || !self.order_by.is_empty()
            || self.limit_value.is_some()
            || self.offset_value.is_some()
            || self.select_columns.is_some()
            || !self.raw_select_expressions.is_empty()
            || !self.subquery_select_expressions.is_empty()
            || !self.group_by.is_empty()
            || !self.having_conditions.is_empty()
            || !self.joins.is_empty()
            || !self.unions.is_empty()
            || !self.window_functions.is_empty()
            || !self.ctes.is_empty()
            || self.cache_options.is_some()
            || self.cache_key.is_some()
            || self.invalid_query_reason.is_some();

        let has_soft_delete_scope = self.include_trashed || self.only_trashed;

        !has_query_state && !has_soft_delete_scope
    }

    /// How many WHERE predicates the fragment carries, counting the contents of
    /// every OR group and of the groups nested inside them.
    ///
    /// This is the "does this actually filter anything" measure, so it counts
    /// only the WHERE clause: HAVING conditions, joins and CTE bodies are not
    /// included no matter how selective they are.
    pub fn condition_count(&self) -> usize {
        self.conditions.len()
            + self
                .or_groups
                .iter()
                .map(super::OrGroup::condition_count)
                .sum::<usize>()
    }
}
