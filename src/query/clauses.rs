//! The clauses a query holds, which a [`QueryFragment`](super::QueryFragment)
//! snapshots and [`QueryBuilder::apply`](super::QueryBuilder::apply) merges
//! back.

use super::builder::DISTINCT_SELECT_MARKER;
use super::structure::SubquerySelect;
use super::{CTE, JoinClause, OrGroup, Order, UnionClause, WhereCondition, WindowFunction};

/// Every clause of a query: what a [`QueryBuilder`](super::QueryBuilder)
/// holds besides its database, and all a
/// [`QueryFragment`](super::QueryFragment) carries.
///
/// Each field's doc names the rule [`merge`](Self::merge) follows for it:
///
/// - **appended** — every list slot. Merging is additive, so a merged ORDER
///   BY term becomes a *further* sort key rather than replacing the query's.
/// - **last-wins** — every single-value setter slot. Clauses that left the
///   slot unset leave the query's value alone.
/// - **first-wins** — the deferred failures, so the earliest recorded one is
///   the one reported.
#[derive(Debug, Clone, Default)]
pub(crate) struct Clauses {
    /// WHERE conditions combined with AND. **Appended.**
    pub(crate) conditions: Vec<WhereCondition>,
    /// Parenthesized OR groups, AND-ed with `conditions`. **Appended.**
    pub(crate) or_groups: Vec<OrGroup>,
    /// Index into `or_groups` of the group the `or_where_*` calls share; on
    /// merge it joins the query's own such group instead of being appended.
    pub(crate) simple_or_group: Option<usize>,
    /// ORDER BY terms. **Appended**, so merged terms become the least
    /// significant sort keys. An entry `order_by_raw()` added is a trusted
    /// expression behind a private marker prefix, not a column name.
    pub(crate) order_by: Vec<(String, Order)>,
    /// LIMIT. **Last-wins**, mirroring `.limit(5).limit(10)`.
    pub(crate) limit_value: Option<u64>,
    /// OFFSET. **Last-wins.**
    pub(crate) offset_value: Option<u64>,
    /// The typed half of the projection, from `select()`. **Last-wins**, and
    /// `None` means "not chosen", not "select nothing".
    pub(crate) select_columns: Option<Vec<String>>,
    /// Raw SELECT expressions, plus the sentinel `distinct()` records here, so
    /// that `DISTINCT` travels with the projection it modifies. **Appended**,
    /// the sentinel at most once.
    pub(crate) raw_select_expressions: Vec<String>,
    /// Scalar subquery projections, with their bound values. **Appended.**
    pub(crate) subquery_select_expressions: Vec<SubquerySelect>,
    /// GROUP BY columns. **Appended.**
    pub(crate) group_by: Vec<String>,
    /// HAVING clause templates, AND-ed together, each keeping its `?`
    /// placeholders. **Appended** in lockstep with `having_bindings`.
    pub(crate) having_conditions: Vec<String>,
    /// The values bound to each HAVING clause's placeholders, slot `i` for
    /// clause `i`; empty for a raw clause.
    pub(crate) having_bindings: Vec<Vec<crate::internal::Value>>,
    /// JOIN clauses, in the order they render. **Appended.**
    pub(crate) joins: Vec<JoinClause>,
    /// Compound-select operands. **Appended.**
    pub(crate) unions: Vec<UnionClause>,
    /// Window functions added to the projection. **Appended.**
    pub(crate) window_functions: Vec<WindowFunction>,
    /// `WITH` clause bodies, in declaration order. **Appended.**
    pub(crate) ctes: Vec<CTE>,
    /// Result-cache settings. **Last-wins.**
    pub(crate) cache_options: Option<crate::cache::CacheOptions>,
    /// A caller-supplied cache key, which replaces the structural one.
    /// **Last-wins.**
    pub(crate) cache_key: Option<String>,
    /// The first builder call that failed, reported when the query runs.
    /// **First-wins.**
    pub(crate) invalid_query_reason: Option<String>,
    /// A page number or size `page()` refused, as the field it names and why:
    /// reported as the validation error `Model::paginate` gives the same
    /// input. **First-wins.**
    pub(crate) invalid_page: Option<(&'static str, String)>,
    /// `with_trashed()`. **Last-wins** with `only_trashed`: clauses that set
    /// neither leave the scope alone.
    pub(crate) include_trashed: bool,
    /// `only_trashed()`, which takes precedence over `include_trashed`.
    pub(crate) only_trashed: bool,
    /// `lock_for_update()`. **Sticky**: merged clauses that lock make the
    /// query lock.
    pub(crate) lock_for_update: bool,
}

impl Clauses {
    /// Whether merging these clauses would change nothing. A soft-delete scope
    /// counts: `with_trashed()` alone still changes a query.
    pub(crate) fn is_empty(&self) -> bool {
        self.conditions.is_empty()
            && self.or_groups.is_empty()
            && self.order_by.is_empty()
            && self.limit_value.is_none()
            && self.offset_value.is_none()
            && self.select_columns.is_none()
            && self.raw_select_expressions.is_empty()
            && self.subquery_select_expressions.is_empty()
            && self.group_by.is_empty()
            && self.having_conditions.is_empty()
            && self.joins.is_empty()
            && self.unions.is_empty()
            && self.window_functions.is_empty()
            && self.ctes.is_empty()
            && self.cache_options.is_none()
            && self.cache_key.is_none()
            && self.invalid_query_reason.is_none()
            && self.invalid_page.is_none()
            && !self.include_trashed
            && !self.only_trashed
            && !self.lock_for_update
    }

    /// Whether `distinct()` was called.
    pub(crate) fn is_distinct(&self) -> bool {
        self.raw_select_expressions
            .iter()
            .any(|expression| expression == DISTINCT_SELECT_MARKER)
    }

    /// Merge `other` in, as replaying the builder calls that made it would,
    /// each slot by the rule its doc names.
    pub(crate) fn merge(&mut self, other: &Clauses) {
        self.conditions.extend_from_slice(&other.conditions);
        self.merge_or_groups(other);
        self.order_by.extend_from_slice(&other.order_by);

        if other.limit_value.is_some() {
            self.limit_value = other.limit_value;
        }
        if other.offset_value.is_some() {
            self.offset_value = other.offset_value;
        }
        if other.select_columns.is_some() {
            self.select_columns = other.select_columns.clone();
        }

        // The DISTINCT sentinel is a flag rather than an expression, so a
        // distinct query does not collect a second copy of it.
        let already_distinct = self.is_distinct();
        self.raw_select_expressions.extend(
            other
                .raw_select_expressions
                .iter()
                .filter(|expression| {
                    expression.as_str() != DISTINCT_SELECT_MARKER || !already_distinct
                })
                .cloned(),
        );
        self.subquery_select_expressions
            .extend_from_slice(&other.subquery_select_expressions);

        self.group_by.extend_from_slice(&other.group_by);
        self.merge_having(other);
        self.joins.extend_from_slice(&other.joins);
        self.unions.extend_from_slice(&other.unions);
        self.window_functions
            .extend_from_slice(&other.window_functions);
        self.ctes.extend_from_slice(&other.ctes);

        if other.cache_options.is_some() {
            self.cache_options = other.cache_options.clone();
        }
        if other.cache_key.is_some() {
            self.cache_key = other.cache_key.clone();
        }

        if self.invalid_query_reason.is_none() {
            self.invalid_query_reason = other.invalid_query_reason.clone();
        }
        if self.invalid_page.is_none() {
            self.invalid_page = other.invalid_page.clone();
        }

        if other.only_trashed {
            self.only_trashed = true;
            self.include_trashed = false;
        } else if other.include_trashed {
            self.include_trashed = true;
            self.only_trashed = false;
        }

        self.lock_for_update |= other.lock_for_update;
    }

    /// Append `other`'s HAVING clauses, each with the values bound to it.
    ///
    /// `having_bindings` is topped up to the clause count first: clauses whose
    /// two vectors had drifted apart would otherwise shift the pairing for
    /// every clause that follows.
    fn merge_having(&mut self, other: &Clauses) {
        self.having_bindings
            .resize(self.having_conditions.len(), Vec::new());

        for (index, condition) in other.having_conditions.iter().enumerate() {
            self.having_conditions.push(condition.clone());
            self.having_bindings.push(
                other
                    .having_bindings
                    .get(index)
                    .cloned()
                    .unwrap_or_default(),
            );
        }
    }

    /// Append `other`'s OR groups, merging the group its `or_where_*` calls
    /// built into this query's own.
    fn merge_or_groups(&mut self, other: &Clauses) {
        for (index, group) in other.or_groups.iter().enumerate() {
            if other.simple_or_group != Some(index) {
                self.or_groups.push(group.clone());
                continue;
            }

            match self
                .simple_or_group
                .and_then(|own| self.or_groups.get_mut(own))
            {
                Some(own) => own.conditions.extend_from_slice(&group.conditions),
                None => {
                    self.simple_or_group = Some(self.or_groups.len());
                    self.or_groups.push(group.clone());
                }
            }
        }
    }
}
