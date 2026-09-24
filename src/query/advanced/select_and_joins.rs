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
    /// the model's own columns.
    ///
    /// Read a subset of the columns with [`get_json()`](Self::get_json):
    /// [`get()`](Self::get) refuses a `select()` that leaves model columns out,
    /// because the models it built would carry defaults in their place.
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

    /// Lock the rows this query reads until the transaction ends
    /// (`SELECT ... FOR UPDATE`).
    ///
    /// Use it for a read-check-write inside
    /// [`Database::transaction`](crate::database::Database::transaction): a
    /// second transaction that locks the same rows waits until the first one
    /// commits, then reads what it wrote. Without the lock both read the same
    /// value and the last `update()` wins, silently undoing the other:
    ///
    /// ```rust,ignore
    /// let shipped = Item::transaction(|_tx| Box::pin(async move {
    ///     let mut item = Item::query()
    ///         .where_eq("id", id)
    ///         .lock_for_update()
    ///         .first_or_fail()
    ///         .await?;
    ///     if item.stock < quantity {
    ///         return Ok(false);
    ///     }
    ///     item.stock -= quantity;
    ///     item.update().await?;
    ///     Ok(true)
    /// }))
    /// .await?;
    /// ```
    ///
    /// Outside a transaction the lock ends with the statement. `count()`,
    /// `exists()` and the aggregates lock the rows they read too, and a locked
    /// query never reads from the query cache.
    ///
    /// PostgreSQL rejects the lock on a query with `DISTINCT`, `GROUP BY` or a
    /// `UNION`. SQLite has no row locks and renders nothing: the first write of
    /// a transaction locks the whole database, so the second of two competing
    /// read-check-write transactions fails with a retryable
    /// [`LockNotAvailable`](crate::error::DbFailureKind::LockNotAvailable) error
    /// instead of racing.
    #[must_use]
    pub fn lock_for_update(mut self) -> Self {
        self.lock_for_update = true;
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
        self.select_with_linked(
            M::column_names().to_vec(),
            linked_table,
            local_pk,
            remote_fk,
            linked_columns,
        )
    }

    /// Add an INNER JOIN clause
    ///
    /// Returns only rows with matches in both tables. Name both columns as
    /// `table.column` (or `alias.column`), such as
    /// `inner_join("posts", "users.id", "posts.user_id")`: a bare column name
    /// invalidates the query, and the error surfaces when it runs.
    #[must_use]
    pub fn inner_join(self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Inner, table, None, left_column, right_column)
    }

    /// Add an INNER JOIN clause with an alias
    ///
    /// The columns are `alias.column` or `table.column`, as for
    /// [`inner_join`](Self::inner_join).
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
    /// The columns are `table.column`, as for [`inner_join`](Self::inner_join).
    #[must_use]
    pub fn left_join(self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Left, table, None, left_column, right_column)
    }

    /// Register a join, invalidating the query instead when any part of it is
    /// not a plain identifier.
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
#[path = "../../../tests/unit/query_select_and_joins_tests.rs"]
mod tests;
