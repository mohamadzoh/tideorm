use super::*;

impl<M: Model> QueryBuilder<M> {
    /// Add a CTE (WITH clause) to the query
    ///
    /// CTEs allow you to define temporary named result sets that can be
    /// referenced within the main query.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().with_cte(CTE::new("selected", "SELECT id FROM users".to_owned())).validate().unwrap();
    /// ```
    #[must_use]
    pub fn with_cte(self, cte: CTE) -> Self {
        self.push_cte("with_cte", cte)
    }

    fn push_cte(mut self, method: &str, cte: CTE) -> Self {
        if let Err(reason) = Self::validate_cte_clause(&cte) {
            self.invalidate_query(format!("invalid CTE for {}(): {}", method, reason));
        }

        self.clauses.ctes.push(cte);
        self
    }

    /// Add a CTE from another query builder
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().with_query("selected", User::query().select(vec!["id"])).validate().unwrap();
    /// ```
    #[must_use]
    pub fn with_query<N: Model>(mut self, name: &str, query: QueryBuilder<N>) -> Self {
        self.absorb_operand_error("subquery", "with_query", &query);
        self.clauses.dependencies.extend(query.cache_tables());

        // The body is spliced into the outer statement and executed, so it goes
        // through the parameterized renderer instead of the debug preview
        // renderer: its values stay bound parameters rather than inline literals.
        // It is the whole statement, ordering and limit included, which a CTE
        // body may carry on every backend.
        let db_type = self.db_type_for_sql();
        let (query_sql, params) = query.build_select_sql_with_params_for_db(db_type);
        self.push_cte("with_query", CTE::with_params(name, query_sql, params))
    }

    /// Add a recursive CTE
    ///
    /// Use recursive CTEs for hierarchical or tree-structured data.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().with_recursive_cte("numbers", vec!["n"], "SELECT 1", "SELECT n + 1 FROM numbers WHERE n < 3").validate().unwrap();
    /// ```
    #[must_use]
    pub fn with_recursive_cte(
        mut self,
        name: &str,
        columns: Vec<&str>,
        base_case: &str,
        recursive_case: &str,
    ) -> Self {
        // Each half is checked as a plain query; the CTE as a whole then as
        // every other CTE is.
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
        self.push_cte(
            "with_recursive_cte",
            CTE::with_columns(name, columns, full_sql).recursive(),
        )
    }

    /// Include soft-deleted records in the query results
    ///
    /// By default, soft-deleted records (where `deleted_at` is not NULL) are excluded.
    /// Use this method to include them.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().with_trashed().validate().unwrap();
    /// ```
    #[must_use]
    pub fn with_trashed(mut self) -> Self {
        self.clauses.include_trashed = true;
        self.clauses.only_trashed = false;
        self
    }

    /// Only return soft-deleted records
    ///
    /// Returns only records where `deleted_at` is not NULL.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().only_trashed().validate().unwrap();
    /// ```
    #[must_use]
    pub fn only_trashed(mut self) -> Self {
        self.clauses.only_trashed = true;
        self.clauses.include_trashed = false;
        self
    }
}
