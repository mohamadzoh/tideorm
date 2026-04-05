#![allow(missing_docs)]

mod consolidation;

use super::db_sql;
use super::{Order, WhereCondition};
use crate::config::DatabaseType;
use crate::model::Model;
use std::marker::PhantomData;

pub use consolidation::JoinResultConsolidator;

/// Type of JOIN operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinType {
    Inner,
    Left,
    Right,
}

impl JoinType {
    pub fn as_sql(&self) -> &'static str {
        match self {
            JoinType::Inner => "INNER JOIN",
            JoinType::Left => "LEFT JOIN",
            JoinType::Right => "RIGHT JOIN",
        }
    }
}

#[derive(Debug, Clone)]
pub struct JoinClause {
    pub join_type: JoinType,
    pub table: String,
    pub alias: Option<String>,
    pub left_column: String,
    pub right_column: String,
}

#[derive(Debug, Clone)]
pub enum AggregateFunction {
    Count,
    CountDistinct(String),
    Sum(String),
    Avg(String),
    Min(String),
    Max(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnionType {
    Union,
    UnionAll,
}

impl UnionType {
    pub fn as_sql(&self) -> &'static str {
        match self {
            UnionType::Union => "UNION",
            UnionType::UnionAll => "UNION ALL",
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnionClause {
    pub union_type: UnionType,
    pub query_sql: String,
}

#[derive(Debug, Clone)]
pub enum FrameBound {
    UnboundedPreceding,
    UnboundedFollowing,
    CurrentRow,
    Preceding(u64),
    Following(u64),
}

impl FrameBound {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    Rows,
    Range,
    Groups,
}

impl FrameType {
    pub fn as_sql(&self) -> &'static str {
        match self {
            FrameType::Rows => "ROWS",
            FrameType::Range => "RANGE",
            FrameType::Groups => "GROUPS",
        }
    }
}

#[derive(Debug, Clone)]
pub enum WindowFunctionType {
    RowNumber,
    Rank,
    DenseRank,
    Ntile(u32),
    Lag(String, Option<i32>, Option<String>),
    Lead(String, Option<i32>, Option<String>),
    FirstValue(String),
    LastValue(String),
    NthValue(String, u32),
    Sum(String),
    Avg(String),
    Count(Option<String>),
    Min(String),
    Max(String),
    Custom(String),
}

impl WindowFunctionType {
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

    pub fn as_sql(&self) -> String {
        self.as_sql_for_db(DatabaseType::Postgres)
    }
}

#[derive(Debug, Clone)]
pub struct WindowFunction {
    pub function: WindowFunctionType,
    pub partition_by: Vec<String>,
    pub order_by: Vec<(String, Order)>,
    pub frame_type: Option<FrameType>,
    pub frame_start: Option<FrameBound>,
    pub frame_end: Option<FrameBound>,
    pub alias: String,
}

impl WindowFunction {
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

    pub fn partition_by(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.partition_by.push(column.column_name().to_string());
        self
    }

    pub fn order_by(
        mut self,
        column: impl crate::columns::IntoColumnName,
        direction: Order,
    ) -> Self {
        self.order_by
            .push((column.column_name().to_string(), direction));
        self
    }

    pub fn frame(mut self, frame_type: FrameType, start: FrameBound, end: FrameBound) -> Self {
        self.frame_type = Some(frame_type);
        self.frame_start = Some(start);
        self.frame_end = Some(end);
        self
    }

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

    pub fn to_sql(&self) -> String {
        self.to_sql_for_db(DatabaseType::Postgres)
    }
}

#[derive(Debug, Clone)]
pub struct CTE {
    pub name: String,
    pub columns: Option<Vec<String>>,
    pub query_sql: String,
    pub recursive: bool,
}

impl CTE {
    pub fn new(name: &str, query_sql: String) -> Self {
        Self {
            name: name.to_string(),
            columns: None,
            query_sql,
            recursive: false,
        }
    }

    pub fn with_columns(name: &str, columns: Vec<&str>, query_sql: String) -> Self {
        Self {
            name: name.to_string(),
            columns: Some(columns.into_iter().map(|s| s.to_string()).collect()),
            query_sql,
            recursive: false,
        }
    }

    pub fn recursive(mut self) -> Self {
        self.recursive = true;
        self
    }

    pub fn to_sql(&self) -> String {
        let mut sql = format!("\"{}\"", self.name);

        if let Some(ref cols) = self.columns {
            let col_list: Vec<String> = cols.iter().map(|c| format!("\"{}\"", c)).collect();
            sql.push_str(&format!(" ({})", col_list.join(", ")));
        }

        sql.push_str(&format!(" AS ({})", self.query_sql));
        sql
    }
}

#[derive(Debug, Clone)]
pub struct QueryFragment<M: Model> {
    pub(crate) _marker: PhantomData<M>,
    pub conditions: Vec<WhereCondition>,
    pub or_groups: Vec<super::OrGroup>,
    pub order_by: Vec<(String, Order)>,
    pub limit_value: Option<u64>,
    pub offset_value: Option<u64>,
    pub select_columns: Option<Vec<String>>,
    pub raw_select_expressions: Vec<String>,
    pub subquery_select_expressions: Vec<(String, String)>,
    pub group_by: Vec<String>,
    pub having_conditions: Vec<String>,
    pub joins: Vec<JoinClause>,
    pub unions: Vec<UnionClause>,
    pub window_functions: Vec<WindowFunction>,
    pub ctes: Vec<CTE>,
    pub cache_options: Option<crate::cache::CacheOptions>,
    pub cache_key: Option<String>,
    pub invalid_query_reason: Option<String>,
    pub include_trashed: bool,
    pub only_trashed: bool,
}

impl<M: Model> Default for QueryFragment<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: Model> QueryFragment<M> {
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

    pub fn condition_count(&self) -> usize {
        self.conditions.len()
            + self
                .or_groups
                .iter()
                .map(super::OrGroup::condition_count)
                .sum::<usize>()
    }
}
