//! Window functions and the `OVER (..)` clause they are evaluated in.

use super::*;

/// One end of a window frame.
///
/// How the offsets are counted depends on the accompanying [`FrameType`]: under
/// [`Rows`](FrameType::Rows) they are physical row counts, under
/// [`Range`](FrameType::Range) they are offsets from the current row's ORDER BY
/// value, and under [`Groups`](FrameType::Groups) they count peer groups.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// Count peer groups rather than rows. Requires an ORDER BY. PostgreSQL
    /// 11+ and SQLite 3.28+ accept it; MySQL and MariaDB do not, so a query
    /// framed by it fails there before it runs.
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
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
    /// A default without an offset is rendered with offset 1, SQL's own, since
    /// the third argument needs the second. The default is written into the
    /// SQL as given, a literal or a column: **trusted SQL only**. MariaDB takes
    /// no third argument at all, so on MySQL and MariaDB the window renders the
    /// default itself; see [`WindowFunction::to_sql_for_db`].
    Lag(String, Option<u32>, Option<String>),
    /// `LEAD(column, offset, default)` — a value from a later row, with the same
    /// argument rule as [`Lag`](Self::Lag).
    Lead(String, Option<u32>, Option<String>),
    /// `FIRST_VALUE(column)` — the column's value in the frame's first row.
    FirstValue(String),
    /// `LAST_VALUE(column)` — the column's value in the frame's *last* row.
    ///
    /// The default frame ends at the current row, so without a frame that
    /// extends to [`UnboundedFollowing`](FrameBound::UnboundedFollowing) this
    /// returns the current row's own value.
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

/// Constructors for the functions that read a column, which take a column
/// name or a typed column (`Sale::AMOUNT`), written with its table.
macro_rules! column_functions {
    ($($(#[$doc:meta])* $name:ident => $variant:ident;)*) => {$(
        $(#[$doc])*
        pub fn $name(column: impl crate::columns::IntoColumnName) -> Self {
            Self::$variant(crate::columns::column_reference(&column, None))
        }
    )*};
}

impl WindowFunctionType {
    column_functions! {
        /// `FIRST_VALUE(column)`.
        first_value => FirstValue;
        /// `LAST_VALUE(column)`.
        last_value => LastValue;
        /// `SUM(column)` over the frame.
        sum => Sum;
        /// `AVG(column)` over the frame.
        avg => Avg;
        /// `MIN(column)` over the frame.
        min => Min;
        /// `MAX(column)` over the frame.
        max => Max;
    }

    /// `COUNT(column)` over the frame; [`Count(None)`](Self::Count) is
    /// `COUNT(*)`.
    pub fn count(column: impl crate::columns::IntoColumnName) -> Self {
        Self::Count(Some(crate::columns::column_reference(&column, None)))
    }

    /// `NTH_VALUE(column, n)`, `n` 1-based.
    pub fn nth_value(column: impl crate::columns::IntoColumnName, n: u32) -> Self {
        Self::NthValue(crate::columns::column_reference(&column, None), n)
    }

    /// `LAG(column, offset)`: the column's value `offset` rows earlier in
    /// the partition, NULL where there is no such row.
    pub fn lag(column: impl crate::columns::IntoColumnName, offset: u32) -> Self {
        Self::Lag(
            crate::columns::column_reference(&column, None),
            Some(offset),
            None,
        )
    }

    /// `LAG(column, offset, default)`: as [`lag`](Self::lag), with `default`
    /// where there is no earlier row. `default` is written into the SQL as
    /// given, a literal (`0`, `'none'`) or a column: **trusted SQL only**; it
    /// is checked for statement separators and comments, never escaped.
    pub fn lag_with_raw_default(
        column: impl crate::columns::IntoColumnName,
        offset: u32,
        default: impl Into<String>,
    ) -> Self {
        Self::Lag(
            crate::columns::column_reference(&column, None),
            Some(offset),
            Some(default.into()),
        )
    }

    /// `LEAD(column, offset)`: the column's value `offset` rows later in the
    /// partition, NULL where there is no such row.
    pub fn lead(column: impl crate::columns::IntoColumnName, offset: u32) -> Self {
        Self::Lead(
            crate::columns::column_reference(&column, None),
            Some(offset),
            None,
        )
    }

    /// `LEAD(column, offset, default)`: as [`lead`](Self::lead), with
    /// `default`, trusted SQL as for
    /// [`lag_with_raw_default`](Self::lag_with_raw_default), where there is no
    /// later row.
    pub fn lead_with_raw_default(
        column: impl crate::columns::IntoColumnName,
        offset: u32,
        default: impl Into<String>,
    ) -> Self {
        Self::Lead(
            crate::columns::column_reference(&column, None),
            Some(offset),
            Some(default.into()),
        )
    }

    /// The column the function reads, if it takes one.
    pub(crate) fn column(&self) -> Option<&str> {
        match self {
            Self::Lag(column, ..)
            | Self::Lead(column, ..)
            | Self::FirstValue(column)
            | Self::LastValue(column)
            | Self::NthValue(column, _)
            | Self::Sum(column)
            | Self::Avg(column)
            | Self::Count(Some(column))
            | Self::Min(column)
            | Self::Max(column) => Some(column),
            Self::RowNumber
            | Self::Rank
            | Self::DenseRank
            | Self::Ntile(_)
            | Self::Count(None)
            | Self::Custom(_) => None,
        }
    }

    /// The column whose own values this function returns (`LAG`, `LEAD`, the
    /// `*_VALUE` functions, `MIN`, `MAX`), so its output decodes and decrypts
    /// as that column. A sum, an average or a count is a new value.
    pub(crate) fn value_column(&self) -> Option<&str> {
        match self {
            Self::Lag(column, ..)
            | Self::Lead(column, ..)
            | Self::FirstValue(column)
            | Self::LastValue(column)
            | Self::NthValue(column, _)
            | Self::Min(column)
            | Self::Max(column) => Some(column),
            _ => None,
        }
    }

    /// This function with its column argument, if any, passed through `column`.
    fn map_column(&self, column: &dyn Fn(&str) -> String) -> Self {
        match self {
            Self::Lag(col, offset, default) => Self::Lag(column(col), *offset, default.clone()),
            Self::Lead(col, offset, default) => Self::Lead(column(col), *offset, default.clone()),
            Self::FirstValue(col) => Self::FirstValue(column(col)),
            Self::LastValue(col) => Self::LastValue(column(col)),
            Self::NthValue(col, n) => Self::NthValue(column(col), *n),
            Self::Sum(col) => Self::Sum(column(col)),
            Self::Avg(col) => Self::Avg(column(col)),
            Self::Count(Some(col)) => Self::Count(Some(column(col))),
            Self::Min(col) => Self::Min(column(col)),
            Self::Max(col) => Self::Max(column(col)),
            Self::RowNumber
            | Self::Rank
            | Self::DenseRank
            | Self::Ntile(_)
            | Self::Count(None)
            | Self::Custom(_) => self.clone(),
        }
    }

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
            WindowFunctionType::Lag(col, offset, default)
            | WindowFunctionType::Lead(col, offset, default) => {
                let name = if matches!(self, WindowFunctionType::Lag(..)) {
                    "LAG"
                } else {
                    "LEAD"
                };
                let mut s = format!("{}({}", name, quote_column(col));
                match (offset, default) {
                    (offset, Some(default)) => {
                        let _ = write!(s, ", {}, {}", offset.unwrap_or(1), default);
                    }
                    (Some(offset), None) => {
                        let _ = write!(s, ", {}", offset);
                    }
                    (None, None) => {}
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
}

/// A window function and the `OVER (..)` clause it is evaluated in.
///
/// Registered on a query with [`window()`](crate::query::QueryBuilder::window); the
/// named helpers ([`row_number()`](crate::query::QueryBuilder::row_number),
/// [`running_sum()`](crate::query::QueryBuilder::running_sum), ...) assemble the common
/// shapes. Window functions are appended to the projection *after* every other
/// select source and never replace the default projection, so a query that
/// selects nothing else still returns the model's own columns alongside them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[must_use = "a window function selects nothing until a query takes it"]
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
    /// handing it to [`window()`](crate::query::QueryBuilder::window).
    pub fn new(function: WindowFunctionType, alias: impl Into<String>) -> Self {
        Self {
            function,
            partition_by: Vec::new(),
            order_by: Vec::new(),
            frame_type: None,
            frame_start: None,
            frame_end: None,
            alias: alias.into(),
        }
    }

    /// This window with every column it names passed through `column`: its
    /// function's argument and its `PARTITION BY` and `ORDER BY` columns.
    pub(crate) fn map_columns(&self, column: &dyn Fn(&str) -> String) -> Self {
        Self {
            function: self.function.map_column(column),
            partition_by: self.partition_by.iter().map(|c| column(c)).collect(),
            order_by: self
                .order_by
                .iter()
                .map(|(c, direction)| (column(c), *direction))
                .collect(),
            frame_type: self.frame_type,
            frame_start: self.frame_start.clone(),
            frame_end: self.frame_end.clone(),
            alias: self.alias.clone(),
        }
    }

    /// Append `PARTITION BY` columns: one column, or several as a tuple or an
    /// array. Repeated calls accumulate left to right.
    pub fn partition_by(mut self, columns: impl crate::columns::IntoColumnNames) -> Self {
        self.partition_by
            .extend(crate::columns::map_columns(&columns, |column| {
                crate::columns::column_reference(&column, None)
            }));
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
            .push((crate::columns::column_reference(&column, None), direction));
        self
    }

    /// Set the whole frame at once, as `BETWEEN start AND end`.
    ///
    /// The classic running-total frame is
    /// `frame(FrameType::Rows, FrameBound::UnboundedPreceding, FrameBound::CurrentRow)`,
    /// which is what [`running_sum()`](crate::query::QueryBuilder::running_sum)
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
    /// unordered, unframed window renders as a bare `OVER ()`. On MySQL and
    /// MariaDB a `LAG`/`LEAD` default is chosen by a `CASE` around the call.
    pub fn to_sql_for_db(&self, db_type: DatabaseType) -> String {
        let over = self.over_clause_for_db(db_type);
        let with_default = match &self.function {
            WindowFunctionType::Lag(column, offset, Some(default)) => {
                Some(("LAG", column, offset.unwrap_or(1), default))
            }
            WindowFunctionType::Lead(column, offset, Some(default)) => {
                Some(("LEAD", column, offset.unwrap_or(1), default))
            }
            _ => None,
        };
        let call = match with_default {
            Some((name, column, offset, default)) => db_type.dialect().window_default(
                name,
                &db_sql::format_column(db_type, column),
                &offset.to_string(),
                default,
                &over,
            ),
            None => format!("{} OVER ({over})", self.function.as_sql_for_db(db_type)),
        };
        format!("{call} AS {}", db_sql::quote_ident(db_type, &self.alias))
    }

    /// What goes inside `OVER (..)`: the partition, the order and the frame.
    fn over_clause_for_db(&self, db_type: DatabaseType) -> String {
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

        clauses.join(" ")
    }
}
