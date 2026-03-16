#![allow(missing_docs)]

use super::{Order, WhereCondition};
use crate::model::Model;
use std::marker::PhantomData;

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
    pub fn as_sql(&self) -> String {
        match self {
            WindowFunctionType::RowNumber => "ROW_NUMBER()".to_string(),
            WindowFunctionType::Rank => "RANK()".to_string(),
            WindowFunctionType::DenseRank => "DENSE_RANK()".to_string(),
            WindowFunctionType::Ntile(n) => format!("NTILE({})", n),
            WindowFunctionType::Lag(col, offset, default) => {
                let mut s = format!("LAG(\"{}\"", col);
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
                let mut s = format!("LEAD(\"{}\"", col);
                if let Some(o) = offset {
                    s.push_str(&format!(", {}", o));
                    if let Some(d) = default {
                        s.push_str(&format!(", {}", d));
                    }
                }
                s.push(')');
                s
            }
            WindowFunctionType::FirstValue(col) => format!("FIRST_VALUE(\"{}\")", col),
            WindowFunctionType::LastValue(col) => format!("LAST_VALUE(\"{}\")", col),
            WindowFunctionType::NthValue(col, n) => format!("NTH_VALUE(\"{}\", {})", col, n),
            WindowFunctionType::Sum(col) => format!("SUM(\"{}\")", col),
            WindowFunctionType::Avg(col) => format!("AVG(\"{}\")", col),
            WindowFunctionType::Count(col) => match col {
                Some(c) => format!("COUNT(\"{}\")", c),
                None => "COUNT(*)".to_string(),
            },
            WindowFunctionType::Min(col) => format!("MIN(\"{}\")", col),
            WindowFunctionType::Max(col) => format!("MAX(\"{}\")", col),
            WindowFunctionType::Custom(expr) => expr.clone(),
        }
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

    pub fn order_by(mut self, column: impl crate::columns::IntoColumnName, direction: Order) -> Self {
        self.order_by.push((column.column_name().to_string(), direction));
        self
    }

    pub fn frame(mut self, frame_type: FrameType, start: FrameBound, end: FrameBound) -> Self {
        self.frame_type = Some(frame_type);
        self.frame_start = Some(start);
        self.frame_end = Some(end);
        self
    }

    pub fn to_sql(&self) -> String {
        let mut sql = self.function.as_sql();
        sql.push_str(" OVER (");

        let mut clauses = Vec::new();

        if !self.partition_by.is_empty() {
            let cols: Vec<String> = self.partition_by.iter().map(|c| format!("\"{}\"", c)).collect();
            clauses.push(format!("PARTITION BY {}", cols.join(", ")));
        }

        if !self.order_by.is_empty() {
            let orders: Vec<String> = self.order_by.iter().map(|(col, dir)| format!("\"{}\" {}", col, dir.as_str())).collect();
            clauses.push(format!("ORDER BY {}", orders.join(", ")));
        }

        if let (Some(frame_type), Some(start)) = (&self.frame_type, &self.frame_start) {
            let frame_sql = if let Some(end) = &self.frame_end {
                format!("{} BETWEEN {} AND {}", frame_type.as_sql(), start.as_sql(), end.as_sql())
            } else {
                format!("{} {}", frame_type.as_sql(), start.as_sql())
            };
            clauses.push(frame_sql);
        }

        sql.push_str(&clauses.join(" "));
        sql.push_str(&format!(") AS \"{}\"", self.alias));
        sql
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
    pub order_by: Vec<(String, Order)>,
    pub group_by: Vec<String>,
    pub having_conditions: Vec<String>,
    pub joins: Vec<JoinClause>,
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
            order_by: Vec::new(),
            group_by: Vec::new(),
            having_conditions: Vec::new(),
            joins: Vec::new(),
            invalid_query_reason: None,
            include_trashed: false,
            only_trashed: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty()
            && self.order_by.is_empty()
            && self.group_by.is_empty()
            && self.having_conditions.is_empty()
            && self.joins.is_empty()
            && self.invalid_query_reason.is_none()
    }

    pub fn condition_count(&self) -> usize {
        self.conditions.len()
    }
}

pub struct JoinResultConsolidator;

impl JoinResultConsolidator {
    pub fn consolidate_two<A, B, K, F>(items: Vec<(A, B)>, key_fn: F) -> Vec<(A, Vec<B>)>
    where
        A: Clone,
        K: Eq + std::hash::Hash,
        F: Fn(&A) -> K,
    {
        use std::collections::HashMap;

        let mut groups: HashMap<K, (A, Vec<B>)> = HashMap::new();
        let mut order: Vec<K> = Vec::new();

        for (a, b) in items {
            let key = key_fn(&a);
            if let Some((_, bs)) = groups.get_mut(&key) {
                bs.push(b);
            } else {
                order.push(key_fn(&a));
                groups.insert(key, (a, vec![b]));
            }
        }

        order.into_iter().filter_map(|k| groups.remove(&k)).collect()
    }

    pub fn consolidate_two_optional<A, B, K, F>(items: Vec<(A, Option<B>)>, key_fn: F) -> Vec<(A, Vec<B>)>
    where
        A: Clone,
        K: Eq + std::hash::Hash,
        F: Fn(&A) -> K,
    {
        use std::collections::HashMap;

        let mut groups: HashMap<K, (A, Vec<B>)> = HashMap::new();
        let mut order: Vec<K> = Vec::new();

        for (a, maybe_b) in items {
            let key = key_fn(&a);
            if let Some((_, bs)) = groups.get_mut(&key) {
                if let Some(b) = maybe_b {
                    bs.push(b);
                }
            } else {
                order.push(key_fn(&a));
                let bs = maybe_b.into_iter().collect();
                groups.insert(key, (a, bs));
            }
        }

        order.into_iter().filter_map(|k| groups.remove(&k)).collect()
    }

    #[allow(clippy::type_complexity)]
    pub fn consolidate_three<A, B, C, KA, KB, FA, FB>(
        items: Vec<(A, B, C)>,
        key_a: FA,
        key_b: FB,
    ) -> Vec<(A, Vec<(B, Vec<C>)>)>
    where
        A: Clone,
        B: Clone,
        KA: Eq + std::hash::Hash + Clone,
        KB: Eq + std::hash::Hash + Clone,
        FA: Fn(&A) -> KA,
        FB: Fn(&B) -> KB,
    {
        use std::collections::HashMap;

        let mut a_groups: HashMap<KA, (A, HashMap<KB, (B, Vec<C>)>, Vec<KB>)> = HashMap::new();
        let mut a_order: Vec<KA> = Vec::new();

        for (a, b, c) in items {
            let ka = key_a(&a);
            let kb = key_b(&b);

            if let Some((_, b_groups, b_order)) = a_groups.get_mut(&ka) {
                if let Some((_, cs)) = b_groups.get_mut(&kb) {
                    cs.push(c);
                } else {
                    b_order.push(kb.clone());
                    b_groups.insert(kb, (b, vec![c]));
                }
            } else {
                a_order.push(ka.clone());
                let mut b_groups = HashMap::new();
                let b_order = vec![kb.clone()];
                b_groups.insert(kb, (b, vec![c]));
                a_groups.insert(ka, (a, b_groups, b_order));
            }
        }

        a_order
            .into_iter()
            .filter_map(|ka| {
                a_groups.remove(&ka).map(|(a, mut b_groups, b_order)| {
                    let bs: Vec<(B, Vec<C>)> = b_order.into_iter().filter_map(|kb| b_groups.remove(&kb)).collect();
                    (a, bs)
                })
            })
            .collect()
    }

    #[allow(clippy::type_complexity)]
    pub fn consolidate_three_optional<A, B, C, KA, KB, FA, FB>(
        items: Vec<(A, B, Option<C>)>,
        key_a: FA,
        key_b: FB,
    ) -> Vec<(A, Vec<(B, Vec<C>)>)>
    where
        A: Clone,
        B: Clone,
        KA: Eq + std::hash::Hash + Clone,
        KB: Eq + std::hash::Hash + Clone,
        FA: Fn(&A) -> KA,
        FB: Fn(&B) -> KB,
    {
        use std::collections::HashMap;

        let mut a_groups: HashMap<KA, (A, HashMap<KB, (B, Vec<C>)>, Vec<KB>)> = HashMap::new();
        let mut a_order: Vec<KA> = Vec::new();

        for (a, b, maybe_c) in items {
            let ka = key_a(&a);
            let kb = key_b(&b);

            if let Some((_, b_groups, b_order)) = a_groups.get_mut(&ka) {
                if let Some((_, cs)) = b_groups.get_mut(&kb) {
                    if let Some(c) = maybe_c {
                        cs.push(c);
                    }
                } else {
                    b_order.push(kb.clone());
                    let cs = maybe_c.into_iter().collect();
                    b_groups.insert(kb, (b, cs));
                }
            } else {
                a_order.push(ka.clone());
                let mut b_groups = HashMap::new();
                let b_order = vec![kb.clone()];
                let cs = maybe_c.into_iter().collect();
                b_groups.insert(kb, (b, cs));
                a_groups.insert(ka, (a, b_groups, b_order));
            }
        }

        a_order
            .into_iter()
            .filter_map(|ka| {
                a_groups.remove(&ka).map(|(a, mut b_groups, b_order)| {
                    let bs: Vec<(B, Vec<C>)> = b_order.into_iter().filter_map(|kb| b_groups.remove(&kb)).collect();
                    (a, bs)
                })
            })
            .collect()
    }
}