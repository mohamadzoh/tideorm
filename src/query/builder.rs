use super::{
    CTE, ConditionValue, Operator, QueryBuilder, QueryFragment, WhereCondition, WindowFunction,
    WindowFunctionType, db_sql,
};
use crate::error::{Error, Result};
use crate::model::Model;
use std::collections::BTreeSet;
use std::marker::PhantomData;

mod column_refs;
mod projection;
mod validation;

/// Marker stamped onto ORDER BY entries produced by `QueryBuilder::order_by_raw`.
///
/// ORDER BY is rendered outside any quoted literal, so `order_by()` only accepts
/// resolvable column references. `order_by_raw()` is the explicit opt-in for a
/// trusted SQL expression, and it tags the stored entry with this marker so both
/// validation and rendering can tell the two apart without widening the column
/// allowlist. The control characters keep the marker outside anything a column
/// name or a user-supplied sort parameter can legitimately contain, and
/// `order_by()` rejects values carrying it.
pub(in crate::query) const RAW_ORDER_BY_MARKER: &str = "\u{1}tideorm_raw_order_by\u{1}";

/// Tag a caller-trusted ORDER BY expression so it bypasses column validation.
pub(in crate::query) fn raw_order_by_entry(expression: &str) -> String {
    format!("{}{}", RAW_ORDER_BY_MARKER, expression.trim())
}

/// Return the trusted expression behind an `order_by_raw()` entry, if any.
pub(in crate::query) fn raw_order_by_expression(value: &str) -> Option<&str> {
    value.strip_prefix(RAW_ORDER_BY_MARKER)
}

/// Whether a value carries the `order_by_raw()` marker anywhere inside it.
///
/// `order_by()` uses this to refuse forged markers before they are stored.
pub(in crate::query) fn contains_raw_order_by_marker(value: &str) -> bool {
    value.contains(RAW_ORDER_BY_MARKER)
}

/// A column reference and the `ASC`/`DESC` written after it, if one is.
pub(in crate::query) fn split_direction(term: &str) -> Option<(&str, super::Order)> {
    let (column, direction) = term.trim().rsplit_once(char::is_whitespace)?;
    let direction = if direction.eq_ignore_ascii_case("asc") {
        super::Order::Asc
    } else if direction.eq_ignore_ascii_case("desc") {
        super::Order::Desc
    } else {
        return None;
    };
    Some((column.trim_end(), direction))
}

/// The column reference of an ORDER BY term, without its direction.
fn order_reference(term: &str) -> &str {
    let term = term.trim();
    split_direction(term).map_or(term, |(reference, _)| reference)
}

/// A `table` or `schema.table` reference as its schema, if any, and table.
pub(in crate::query) fn schema_and_table(reference: &str) -> (Option<&str>, &str) {
    match reference.split_once('.') {
        Some((schema, table)) => (Some(schema), table),
        None => (None, reference),
    }
}

/// Sentinel entry recorded in `raw_select_expressions` by
/// [`QueryBuilder::distinct`](super::QueryBuilder::distinct).
///
/// `DISTINCT` is a prefix of the projection rather than an expression inside it,
/// but the builder keeps the whole projection in the select accumulators no
/// matter which of `select()`/`select_raw()`/`select_subquery()` contributed to
/// it. Recording the request as a sentinel in that same accumulator is what
/// keeps every existing consumer of the projection correct without a second
/// mechanism: `consolidate()`/`apply()` carry it, `generate_cache_key()` hashes
/// it so a distinct query never collides with its non-distinct twin, and
/// `build_count_sql_with_params_for_db()` leaves its `SELECT COUNT(*)` fast path
/// for the derived-table path — which is the difference between counting the
/// deduplicated rows and counting the duplicates `DISTINCT` exists to remove.
///
/// The renderer strips the sentinel and emits the keyword in its place, so it
/// never reaches SQL. The control characters keep it outside anything a real
/// SELECT expression can contain.
pub(in crate::query) const DISTINCT_SELECT_MARKER: &str = "\u{1}tideorm_select_distinct\u{1}";

impl<M: Model> QueryBuilder<M> {
    /// Collect the table/alias qualifiers that are valid for column references
    /// in the current query: the model's own table plus the alias (or table
    /// name when no alias was provided) for each registered JOIN clause.
    pub(in crate::query) fn known_qualifiers(&self) -> BTreeSet<String> {
        let mut qualifiers = BTreeSet::new();
        qualifiers.insert(M::table_name().to_string());
        if let Some(schema) = M::schema_name() {
            qualifiers.insert(format!("{schema}.{}", M::table_name()));
        }
        // A `schema.table` join is referenced by its table name.
        for join in &self.clauses.joins {
            qualifiers.insert(join.qualifier().to_string());
            if join.alias.is_none() {
                qualifiers.insert(join.table.clone());
            }
        }
        qualifiers
    }

    /// Create a new query builder
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// let query = QueryBuilder::<User>::new();
    /// query.validate().unwrap();
    /// ```
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
            database: None,
            clauses: super::clauses::Clauses::default(),
            self_join_depth: 0,
        }
    }

    pub(crate) fn with_database(mut self, database: crate::database::Database) -> Self {
        self.database = Some(database);
        self
    }

    /// Start a bulk `UPDATE` of the rows this query matches, keeping its
    /// filters, its soft-delete scope and the database a
    /// [`query_with`](crate::model::Model::query_with) handle names:
    ///
    /// ```ignore
    /// User::query()
    ///     .inactive() // a scope
    ///     .where_lt("last_login_at", cutoff)
    ///     .update_all()
    ///     .set("status", "dormant")
    ///     .execute()
    ///     .await?;
    /// ```
    ///
    /// As with [`Model::update_all`], trashed
    /// rows stay out unless the query includes them. A query that joins,
    /// groups, pages or unions cannot be an `UPDATE`, and running one fails.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// let update = User::query().where_eq(User::ACTIVE, false).update_all().set("active", true);
    /// ```
    #[must_use]
    pub fn update_all(self) -> crate::model::BatchUpdateBuilder<M> {
        crate::model::BatchUpdateBuilder::from_query(self)
    }

    /// Refuse a key lookup on a paged query: the page would decide whether
    /// the row is found, missing one inside it and finding one outside it.
    pub(crate) fn ensure_key_lookup_is_unpaged(&self) -> Result<()> {
        if self.clauses.limit_value.is_some() || self.clauses.offset_value.is_some() {
            return Err(Error::query(format!(
                "find() looks a {} row up by its key, so it takes no limit() or offset(); \
                 filter the query instead",
                M::table_name()
            )));
        }
        Ok(())
    }

    /// The first part of this query an `UPDATE` cannot keep, if any: one
    /// that reshapes the rows rather than filters them.
    pub(crate) fn update_blocker(&self) -> Option<&'static str> {
        [
            (!self.clauses.joins.is_empty(), "a join"),
            (!self.clauses.unions.is_empty(), "a union"),
            (!self.clauses.ctes.is_empty(), "a CTE"),
            (!self.clauses.group_by.is_empty(), "group_by()"),
            (!self.clauses.having_conditions.is_empty(), "having()"),
            (
                !self.clauses.window_functions.is_empty(),
                "a window function",
            ),
            (
                self.clauses.limit_value.is_some() || self.clauses.offset_value.is_some(),
                "limit() or offset()",
            ),
        ]
        .into_iter()
        .find_map(|(present, part)| present.then_some(part))
    }

    /// The first part of this query a `DELETE` or a trash mutation cannot
    /// keep: an [`update_blocker`](Self::update_blocker), a projection or an
    /// ordering.
    pub(crate) fn mutation_blocker(&self) -> Option<&'static str> {
        self.update_blocker().or_else(|| {
            [
                (self.has_explicit_projection(), "a select()"),
                (self.is_distinct(), "distinct()"),
                (!self.clauses.order_by.is_empty(), "an ordering"),
            ]
            .into_iter()
            .find_map(|(present, part)| present.then_some(part))
        })
    }

    /// This query over live rows only, whatever scope it had.
    pub(crate) fn live_rows_only(mut self) -> Self {
        self.clauses.include_trashed = false;
        self.clauses.only_trashed = false;
        self
    }

    /// Promote this query into the eager-loading builder and batch-load a relation.
    ///
    /// This preserves any filters, ordering, pagination, cache settings, and explicit
    /// database handle already attached to the query.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users")]
    /// # struct User { #[tideorm(primary_key)] id: i64, #[tideorm(has_many = "Post", foreign_key = "user_id")] posts: HasMany<Post> }
    /// # #[tideorm::model(table = "posts")]
    /// # struct Post { #[tideorm(primary_key)] id: i64, user_id: i64 }
    /// # fn main() {
    /// let query = User::query().with(User::POSTS);
    /// # }
    /// ```
    pub fn with(self, relation: &str) -> crate::relations::EagerQueryBuilder<M> {
        crate::relations::EagerQueryBuilder::from_query(self).with(relation)
    }

    /// Promote this query into the eager-loading builder and batch-load multiple relations.
    ///
    /// This preserves any filters, ordering, pagination, cache settings, and explicit
    /// database handle already attached to the query.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users")]
    /// # struct User { #[tideorm(primary_key)] id: i64, #[tideorm(has_many = "Post", foreign_key = "user_id")] posts: HasMany<Post> }
    /// # #[tideorm::model(table = "posts")]
    /// # struct Post { #[tideorm(primary_key)] id: i64, user_id: i64 }
    /// # fn main() {
    /// let query = User::query().with_many([User::POSTS]);
    /// # }
    /// ```
    pub fn with_many(
        self,
        relations: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> crate::relations::EagerQueryBuilder<M> {
        crate::relations::EagerQueryBuilder::from_query(self).with_many(relations)
    }

    /// The database a `query_with` handle named, if one did.
    pub(crate) fn named_database(&self) -> Option<crate::database::Database> {
        self.database.clone()
    }

    /// The database the query runs on: the one a `query_with` handle named,
    /// else the scope's.
    pub(crate) fn current_db(&self) -> Result<crate::database::Database> {
        self.named_database()
            .map_or_else(crate::database::__current_db, Ok)
    }

    /// Consolidate the current query clauses into a reusable fragment.
    ///
    /// The fragment is a verbatim snapshot of the builder's clauses, never
    /// rendered SQL. HAVING clauses, subqueries, union operands and CTE bodies
    /// keep their values bound and are rendered when the query runs: nothing
    /// is baked into an inline literal, so the fragment stays backend-agnostic
    /// and survives the round trip through [`apply`](Self::apply) with its
    /// parameters intact.
    ///
    /// See [`apply`](Self::apply) for how a fragment merges back into a builder.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// let fragment = User::query().where_eq(User::ACTIVE, true).consolidate();
    /// assert_eq!(fragment.condition_count(), 1);
    /// ```
    pub fn consolidate(&self) -> QueryFragment<M> {
        QueryFragment {
            _marker: PhantomData,
            clauses: self.clauses.clone(),
        }
    }

    /// Apply a reusable fragment to the current query builder.
    ///
    /// The merge replays the fragment's builder calls on top of this query, so
    /// every slot behaves exactly as the matching setter does:
    ///
    /// - **List slots are appended, never dropped**: WHERE conditions, OR
    ///   groups, ORDER BY terms, raw and subquery SELECT expressions, GROUP BY
    ///   columns, HAVING clauses (with their bound values), JOINs, compound
    ///   selects, window functions and CTEs. A fragment ordering by
    ///   `created_at` therefore adds a sort key to whatever the builder already
    ///   ordered by, exactly as a second `order_desc()` call would. The
    ///   fragment's `or_where_*` conditions join the builder's own shared OR
    ///   group, exactly as those calls would.
    /// - **Single-value slots are last-wins**: `limit`, `offset`, `select`,
    ///   cache options and cache key. A value the fragment carries overrides the
    ///   builder's, and a slot the fragment left unset keeps the builder's —
    ///   mirroring `.limit(5).limit(10)`.
    /// - **`distinct()` is a latch**: a fragment that requested it turns it on,
    ///   and a fragment that did not leaves the builder's own choice alone,
    ///   exactly as calling `distinct()` twice does.
    /// - **Soft-delete scope is last-wins** in the same sense: a fragment that
    ///   set neither `with_trashed()` nor `only_trashed()` leaves the scope
    ///   alone.
    /// - **`invalid_query_reason` is the sole first-wins slot**, matching
    ///   `invalidate_query()`: the earliest recorded failure is the one
    ///   reported.
    ///
    /// Nothing is silently discarded, so applying a fragment produced by
    /// [`consolidate`](Self::consolidate) to an empty builder reproduces the
    /// original query.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// let fragment = User::query().where_eq(User::ACTIVE, true).consolidate();
    /// User::query().apply(&fragment).validate().unwrap();
    /// ```
    pub fn apply(mut self, fragment: &QueryFragment<M>) -> Self {
        self.clauses.merge(&fragment.clauses);
        self
    }

    pub(super) fn invalidate_query(&mut self, reason: String) {
        if self.clauses.invalid_query_reason.is_none() {
            self.clauses.invalid_query_reason = Some(reason);
        }
    }

    pub(super) fn ensure_query_is_valid(&self) -> Result<()> {
        if let Some(reason) = &self.clauses.invalid_query_reason {
            return Err(Error::query(reason));
        }
        if let Some((field, message)) = &self.clauses.invalid_page {
            return Err(Error::validation(*field, message.clone()));
        }

        self.validate_query_fragments()?;

        Ok(())
    }

    pub(super) fn validate_join_clause(
        table: &str,
        alias: Option<&str>,
        left_column: &str,
        right_column: &str,
    ) -> std::result::Result<(), String> {
        if db_sql::validate_identifier_reference("JOIN table", table).is_err() {
            return Err(format!(
                "unsafe JOIN table '{}': expected a table or schema.table using only ASCII letters, numbers, and underscores",
                table
            ));
        }

        if let Some(alias) = alias {
            db_sql::validate_identifier("JOIN alias", alias)?;
        }

        db_sql::validate_join_column(left_column)?;
        db_sql::validate_join_column(right_column)?;
        Ok(())
    }
}
