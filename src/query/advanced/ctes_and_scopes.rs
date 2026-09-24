use super::*;

impl<M: Model> QueryBuilder<M> {
    /// Add a CTE (WITH clause) to the query
    ///
    /// CTEs allow you to define temporary named result sets that can be
    /// referenced within the main query.
    #[must_use]
    pub fn with_cte(self, cte: CTE) -> Self {
        self.push_cte("with_cte", cte)
    }

    fn push_cte(mut self, method: &str, cte: CTE) -> Self {
        if let Err(reason) = Self::validate_cte_clause(&cte) {
            self.invalidate_query(format!("invalid CTE for {}(): {}", method, reason));
        }

        self.ctes.push(cte);
        self
    }

    /// Add a CTE from another query builder
    #[must_use]
    pub fn with_query<N: Model>(mut self, name: &str, query: QueryBuilder<N>) -> Self {
        if let Err(reason) = crate::query::db_sql::validate_identifier("CTE name", name) {
            self.invalidate_query(reason);
        }

        if let Err(err) = query.ensure_query_is_valid() {
            self.invalidate_query(format!("invalid subquery for with_query(): {}", err));
        }

        // The body is spliced into the outer statement and executed, so it goes
        // through the parameterized renderer instead of the debug preview
        // renderer: its values stay bound parameters rather than inline literals.
        // It is the whole statement, ordering and limit included, which a CTE
        // body may carry on every backend.
        let db_type = self.db_type_for_sql();
        let (query_sql, params) = query.build_select_sql_with_params_for_db(db_type);
        self.ctes.push(CTE::with_params(name, query_sql, params));
        self
    }

    /// Add a CTE with column aliases
    ///
    /// Trusted SQL only. Do not pass user-controlled input; prefer `with_query()` when the
    /// subquery can be expressed with `QueryBuilder`.
    #[must_use]
    pub fn with_cte_columns(self, name: &str, columns: Vec<&str>, sql: &str) -> Self {
        self.push_cte(
            "with_cte_columns",
            CTE::with_columns(name, columns, sql.to_string()),
        )
    }

    /// Add a recursive CTE
    ///
    /// Use recursive CTEs for hierarchical or tree-structured data.
    #[must_use]
    pub fn with_recursive_cte(
        mut self,
        name: &str,
        columns: Vec<&str>,
        base_case: &str,
        recursive_case: &str,
    ) -> Self {
        if let Err(reason) = crate::query::db_sql::validate_identifier("CTE name", name) {
            self.invalidate_query(reason);
        }

        for column in &columns {
            if let Err(reason) = crate::query::db_sql::validate_identifier("CTE column", column) {
                self.invalidate_query(reason);
                break;
            }
        }

        if let Err(reason) = crate::query::db_sql::validate_subquery_sql(base_case) {
            self.invalidate_query(format!(
                "invalid subquery for with_recursive_cte() base query: {}",
                reason
            ));
        }

        if let Err(reason) = crate::query::db_sql::validate_subquery_sql(recursive_case) {
            self.invalidate_query(format!(
                "invalid subquery for with_recursive_cte() recursive query: {}",
                reason
            ));
        }

        let full_sql = format!("{} UNION ALL {}", base_case, recursive_case);
        let cte = CTE::with_columns(name, columns, full_sql).recursive();
        self.ctes.push(cte);
        self
    }

    /// Include soft-deleted records in the query results
    ///
    /// By default, soft-deleted records (where `deleted_at` is not NULL) are excluded.
    /// Use this method to include them.
    #[must_use]
    pub fn with_trashed(mut self) -> Self {
        self.include_trashed = true;
        self.only_trashed = false;
        self
    }

    /// Only return soft-deleted records
    ///
    /// Returns only records where `deleted_at` is not NULL.
    #[must_use]
    pub fn only_trashed(mut self) -> Self {
        self.only_trashed = true;
        self.include_trashed = false;
        self
    }

    /// Apply a scope function to modify the query
    ///
    /// Scopes are reusable query fragments that can be applied to any query.
    /// Use scopes to define common query patterns once and reuse them.
    #[must_use]
    pub fn scope<F>(self, f: F) -> Self
    where
        F: FnOnce(Self) -> Self,
    {
        f(self)
    }

    /// Apply a conditional scope
    ///
    /// Only applies the scope function if the condition is true.
    #[must_use]
    pub fn when<F>(self, condition: bool, f: F) -> Self
    where
        F: FnOnce(Self) -> Self,
    {
        if condition { f(self) } else { self }
    }

    /// Apply a scope based on an Option value
    ///
    /// If the option is Some, applies the scope function with the value.
    /// If None, returns the query unchanged.
    #[must_use]
    pub fn when_some<T, F>(self, option: Option<T>, f: F) -> Self
    where
        F: FnOnce(Self, T) -> Self,
    {
        match option {
            Some(value) => f(self, value),
            None => self,
        }
    }
}
