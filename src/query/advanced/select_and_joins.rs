use super::*;

impl<M: Model> QueryBuilder<M> {
    /// Select specific columns.
    ///
    /// This is the *typed* half of the projection and it replaces whatever a
    /// previous `select()` chose — the last call wins. The raw halves,
    /// [`select_raw()`](Self::select_raw) and
    /// [`select_subquery()`](Self::select_subquery), accumulate instead, and all
    /// three compose: a query that calls `select()` and `select_raw()` renders
    /// the typed columns first, then the raw expressions, then the subqueries.
    /// Only when nothing at all was selected does the projection fall back to
    /// the model's own `table.*`.
    #[must_use]
    pub fn select(mut self, columns: Vec<&str>) -> Self {
        self.select_columns = Some(columns.into_iter().map(|s| s.to_string()).collect());
        self
    }

    /// Deduplicate the result rows with `SELECT DISTINCT`.
    ///
    /// The usual reason to reach for this is a join that fans rows out: a
    /// many-to-many pivot holding more than one row for the same pair repeats
    /// the related model once per pivot row. Collapsing the duplicates in the
    /// database keeps them off the wire, instead of transferring them and
    /// discarding them afterwards.
    ///
    /// It composes with every projection source — the typed columns of
    /// [`select()`](Self::select), the raw expressions of `select_raw()`, the
    /// scalar subqueries of `select_subquery()` and the `table.*` fallback all
    /// render behind the one `DISTINCT` keyword — and with the terminals:
    /// `count()` counts the deduplicated rows, and `exists()` is unaffected
    /// because deduplication cannot change whether a row exists. Calling it more
    /// than once is idempotent.
    ///
    /// PostgreSQL requires every `ORDER BY` expression of a `SELECT DISTINCT` to
    /// appear in the select list. A query that breaks that rule is rejected by
    /// validation with an `invalid_query` error naming the column, rather than
    /// being sent to the server; ordering by an expression supplied through
    /// `order_by_raw()` cannot be checked and stays the caller's responsibility.
    #[must_use]
    pub fn distinct(mut self) -> Self {
        if !self.is_distinct() {
            self.raw_select_expressions
                .push(crate::query::builder::DISTINCT_SELECT_MARKER.to_string());
        }
        self
    }

    /// Select columns from this table and also from a linked/joined table
    ///
    /// Use this for partial model queries that need columns from a related
    /// table without loading the full related model.
    ///
    /// Like [`select()`](Self::select) this replaces the typed projection; the
    /// LEFT JOIN it registers is validated exactly as
    /// [`left_join()`](Self::left_join) validates its own.
    #[must_use]
    pub fn select_with_linked(
        self,
        local_columns: Vec<&str>,
        linked_table: &str,
        local_fk: &str,
        remote_pk: &str,
        linked_columns: Vec<&str>,
    ) -> Self {
        let table_name = M::table_name();
        let mut all_columns: Vec<String> = local_columns
            .iter()
            .map(|c| format!("{}.{}", table_name, c))
            .collect();

        for col in linked_columns {
            all_columns.push(format!("{}.{}", linked_table, col));
        }

        let mut query = self.join(
            JoinType::Left,
            linked_table,
            None,
            &format!("{}.{}", table_name, local_fk),
            &format!("{}.{}", linked_table, remote_pk),
        );
        query.select_columns = Some(all_columns);
        query
    }

    /// Select all columns from this table plus specific columns from a linked table
    ///
    /// Carries the same projection and JOIN-validation rules as
    /// [`select_with_linked()`](Self::select_with_linked).
    #[must_use]
    pub fn select_also_linked(
        self,
        linked_table: &str,
        local_pk: &str,
        remote_fk: &str,
        linked_columns: Vec<&str>,
    ) -> Self {
        let table_name = M::table_name();

        let mut all_columns: Vec<String> = M::column_names()
            .iter()
            .map(|c| format!("{}.{}", table_name, c))
            .collect();

        for col in linked_columns {
            all_columns.push(format!("{}.{}", linked_table, col));
        }

        let mut query = self.join(
            JoinType::Left,
            linked_table,
            None,
            &format!("{}.{}", table_name, local_pk),
            &format!("{}.{}", linked_table, remote_fk),
        );
        query.select_columns = Some(all_columns);
        query
    }

    // =========================================================================
    // JOIN OPERATIONS
    // =========================================================================

    /// Add an INNER JOIN clause
    ///
    /// Returns only rows with matches in both tables.
    #[must_use]
    pub fn inner_join(self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Inner, table, None, left_column, right_column)
    }

    /// Add an INNER JOIN clause with an alias
    #[must_use]
    pub fn inner_join_as(
        self,
        table: &str,
        alias: &str,
        left_column: &str,
        right_column: &str,
    ) -> Self {
        self.join(
            JoinType::Inner,
            table,
            Some(alias),
            left_column,
            right_column,
        )
    }

    /// Add a LEFT JOIN clause
    ///
    /// Returns all rows from the left table, and matched rows from the right.
    #[must_use]
    pub fn left_join(self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Left, table, None, left_column, right_column)
    }

    /// Add a LEFT JOIN clause with an alias
    #[must_use]
    pub fn left_join_as(
        self,
        table: &str,
        alias: &str,
        left_column: &str,
        right_column: &str,
    ) -> Self {
        self.join(
            JoinType::Left,
            table,
            Some(alias),
            left_column,
            right_column,
        )
    }

    /// Add a RIGHT JOIN clause
    ///
    /// Returns all rows from the right table, and matched rows from the left.
    #[must_use]
    pub fn right_join(self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Right, table, None, left_column, right_column)
    }

    /// Add a RIGHT JOIN clause with an alias
    #[must_use]
    pub fn right_join_as(
        self,
        table: &str,
        alias: &str,
        left_column: &str,
        right_column: &str,
    ) -> Self {
        self.join(
            JoinType::Right,
            table,
            Some(alias),
            left_column,
            right_column,
        )
    }

    /// Generic join method (internal)
    fn join(
        mut self,
        join_type: JoinType,
        table: &str,
        alias: Option<&str>,
        left_column: &str,
        right_column: &str,
    ) -> Self {
        if let Err(reason) = Self::validate_join_clause(table, alias, left_column, right_column) {
            self.invalidate_query(reason);
            return self;
        }

        self.joins.push(JoinClause {
            join_type,
            table: table.to_string(),
            alias: alias.map(|s| s.to_string()),
            left_column: left_column.to_string(),
            right_column: right_column.to_string(),
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::model::Model;

    #[tideorm::model(table = "linked_select_users")]
    struct LinkedSelectUser {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        profile_id: i64,
    }

    const UNSAFE_TABLE: &str = "profiles\" ON 1 = 1; DROP TABLE profiles; --";

    #[test]
    fn test_select_with_linked_validates_its_join_table() {
        let query = LinkedSelectUser::query().select_with_linked(
            vec!["id"],
            UNSAFE_TABLE,
            "profile_id",
            "id",
            vec!["bio"],
        );

        let err = query
            .ensure_query_is_valid()
            .expect_err("an unsafe linked table must invalidate the query");
        assert!(err.to_string().contains("unsafe JOIN table"), "{err}");
        assert!(
            !query.known_qualifiers().iter().any(|q| q.contains("DROP")),
            "a rejected join must not whitelist its qualifier"
        );
    }

    #[test]
    fn test_select_also_linked_validates_its_join_table() {
        let query = LinkedSelectUser::query().select_also_linked(
            UNSAFE_TABLE,
            "id",
            "user_id",
            vec!["bio"],
        );

        let err = query
            .ensure_query_is_valid()
            .expect_err("an unsafe linked table must invalidate the query");
        assert!(err.to_string().contains("unsafe JOIN table"), "{err}");
        assert!(
            !query.known_qualifiers().iter().any(|q| q.contains("DROP")),
            "a rejected join must not whitelist its qualifier"
        );
    }

    #[test]
    fn test_distinct_is_idempotent() {
        let sql = LinkedSelectUser::query()
            .distinct()
            .distinct()
            .build_select_sql_for_db(crate::config::DatabaseType::Postgres);

        assert_eq!(
            sql,
            "SELECT DISTINCT \"linked_select_users\".* FROM \"linked_select_users\""
        );
    }

    #[test]
    fn test_distinct_rejects_order_by_outside_the_projection() {
        let err = LinkedSelectUser::query()
            .inner_join("profiles", "linked_select_users.profile_id", "profiles.id")
            .distinct()
            .order_desc("profiles.created_at")
            .ensure_query_is_valid()
            .expect_err("a SELECT DISTINCT cannot order by a column it does not select");

        assert!(
            err.to_string().contains("not part of the distinct()"),
            "{err}"
        );
    }

    #[test]
    fn test_distinct_allows_order_by_a_projected_column() {
        let query = LinkedSelectUser::query()
            .inner_join("profiles", "linked_select_users.profile_id", "profiles.id")
            .distinct()
            .order_desc("linked_select_users.id");

        assert!(query.ensure_query_is_valid().is_ok());
    }

    #[test]
    fn test_distinct_narrows_the_allowed_order_by_to_an_explicit_select() {
        let err = LinkedSelectUser::query()
            .select(vec!["id"])
            .distinct()
            .order_desc("profile_id")
            .ensure_query_is_valid()
            .expect_err("an explicit select() narrows what a SELECT DISTINCT can order by");

        assert!(
            err.to_string().contains("not part of the distinct()"),
            "{err}"
        );
    }

    #[test]
    fn test_distinct_leaves_raw_order_by_and_raw_projections_to_the_caller() {
        let raw_order = LinkedSelectUser::query()
            .distinct()
            .order_by_raw("profile_id", crate::query::Order::Desc);
        assert!(raw_order.ensure_query_is_valid().is_ok());

        let raw_projection = LinkedSelectUser::query()
            .inner_join("profiles", "linked_select_users.profile_id", "profiles.id")
            .select_raw("profiles.bio")
            .distinct()
            .order_desc("profiles.bio");
        assert!(raw_projection.ensure_query_is_valid().is_ok());
    }

    #[test]
    fn test_select_with_linked_still_registers_a_valid_join() {
        let query = LinkedSelectUser::query().select_with_linked(
            vec!["id"],
            "profiles",
            "profile_id",
            "id",
            vec!["bio"],
        );

        assert!(query.ensure_query_is_valid().is_ok());
        assert!(query.known_qualifiers().contains("profiles"));
    }
}
