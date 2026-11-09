//! Compound queries: `union()`, `union_all()` and their raw forms.

use super::*;

impl<M: Model> QueryBuilder<M> {
    /// Add `other` as a compound-select operand, rendered with this query for
    /// the backend it runs on, its values bound.
    ///
    /// The operand is validated like any subquery: one this query could not
    /// run on its own (an unknown column, an ordering expression outside
    /// `order_by_raw()`, `page(0, n)`) invalidates this query.
    pub(super) fn push_compound_operand<N: Model>(
        mut self,
        union_type: UnionType,
        method: &str,
        other: QueryBuilder<N>,
    ) -> Self {
        self.absorb_operand_error("operand", method, &other);
        // An operand with a raw arm of its own reads `table.*`, in the table's
        // column order, as a raw arm does; mixing arms is refused inside it too.
        let raw = other.reads_table_wildcard();
        if let Err(error) = other.ensure_union_projections_are_valid() {
            self.invalidate_query(error.to_string());
        }
        // Such an operand's columns are as unknown as a raw arm's.
        let projection = (!raw).then(|| other.compound_projection());
        let mut operand = UnionClause::new(
            union_type,
            crate::query::Operand::Query(std::sync::Arc::new(other)),
        );
        operand.raw = raw;
        operand.projection = projection;
        self.clauses.unions.push(operand);
        self
    }

    /// Add a UNION with another query
    ///
    /// UNION combines the results of two queries and removes duplicates.
    /// Explicitly bound operands must resolve to the same database and
    /// transaction as the outer query, as with subqueries and CTEs.
    /// Encrypted projections are rejected because randomized ciphertext cannot
    /// deduplicate plaintext values; use compatible [`union_all()`](Self::union_all)
    /// projections instead.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().union(User::query().where_eq(User::ACTIVE, false)).validate().unwrap();
    /// ```
    pub fn union<N: Model>(self, other: QueryBuilder<N>) -> Self {
        self.push_compound_operand(UnionType::Union, "union", other)
    }

    /// Add a UNION ALL with another query
    ///
    /// UNION ALL combines all results including duplicates (faster than UNION).
    /// Explicitly bound operands must resolve to the same database and
    /// transaction as the outer query.
    /// If a projection includes encrypted columns, every operand must have the
    /// same encryption table and column at each output position. Use explicit
    /// columns or the default model projection so these scopes can be checked.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().union_all(User::query().where_eq(User::ACTIVE, false)).validate().unwrap();
    /// ```
    pub fn union_all<N: Model>(self, other: QueryBuilder<N>) -> Self {
        self.push_compound_operand(UnionType::UnionAll, "union_all", other)
    }

    /// Add a raw UNION query
    ///
    /// Trusted SQL only. Do not pass user-controlled input; prefer `union()` with a
    /// `QueryBuilder` whenever possible.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().union_raw("SELECT * FROM users").validate().unwrap();
    /// ```
    pub fn union_raw(self, sql: &str) -> Self {
        self.push_raw_union(UnionType::Union, "union_raw", sql)
    }

    /// Add a raw UNION ALL query
    ///
    /// Trusted SQL only. Do not pass user-controlled input; prefer `union_all()` with a
    /// `QueryBuilder` whenever possible.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().union_all_raw("SELECT * FROM users").validate().unwrap();
    /// ```
    pub fn union_all_raw(self, sql: &str) -> Self {
        self.push_raw_union(UnionType::UnionAll, "union_all_raw", sql)
    }

    /// Add a raw UNION query whose `?` placeholders bind `params`, in order
    ///
    /// The SQL is **trusted**, checked as for [`union_raw`](Self::union_raw);
    /// the values are bound, never written into it. Write `?` on every
    /// backend.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().union_raw_with("SELECT * FROM users WHERE id = ?", [1_i64.into()]).validate().unwrap();
    /// ```
    pub fn union_raw_with(
        self,
        sql: &str,
        params: impl IntoIterator<Item = crate::internal::DbValue>,
    ) -> Self {
        self.push_raw_union_with(UnionType::Union, "union_raw_with", sql, params)
    }

    /// Add a raw UNION ALL query whose `?` placeholders bind `params`, in
    /// order
    ///
    /// The `UNION ALL` form of [`union_raw_with`](Self::union_raw_with).
    pub fn union_all_raw_with(
        self,
        sql: &str,
        params: impl IntoIterator<Item = crate::internal::DbValue>,
    ) -> Self {
        self.push_raw_union_with(UnionType::UnionAll, "union_all_raw_with", sql, params)
    }

    pub(super) fn push_raw_union_with(
        mut self,
        union_type: UnionType,
        method: &str,
        sql: &str,
        params: impl IntoIterator<Item = crate::internal::DbValue>,
    ) -> Self {
        if let Err(reason) = crate::query::db_sql::validate_subquery_sql(sql) {
            self.invalidate_query(format!("invalid subquery for {method}(): {reason}"));
        }
        let values = params
            .into_iter()
            .map(crate::internal::bindable_value)
            .collect();
        self.push_raw_operand(union_type, sql, Some(values))
    }

    pub(super) fn push_raw_union(mut self, union_type: UnionType, method: &str, sql: &str) -> Self {
        if let Err(reason) = crate::query::db_sql::validate_subquery_sql(sql) {
            self.invalidate_query(format!("invalid subquery for {method}(): {reason}"));
        }
        self.push_raw_operand(union_type, sql, None)
    }

    pub(super) fn push_raw_operand(
        mut self,
        union_type: UnionType,
        sql: &str,
        values: Option<Vec<Value>>,
    ) -> Self {
        let sql = sql.to_string();
        let mut operand = UnionClause::new(union_type, crate::query::Operand::Sql { sql, values });
        operand.raw = true;
        self.clauses.unions.push(operand);
        self
    }
}
