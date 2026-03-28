use super::*;
use crate::error::Result;
use crate::internal::{
    EntityTrait, Expr, FromQueryResult, QueryFilter, QuerySelect, translate_error,
};
use crate::model::Model;

impl<M: Model> QueryBuilder<M> {
    /// Add an ORDER BY clause.
    pub fn order_by(
        mut self,
        column: impl crate::columns::IntoColumnName,
        direction: Order,
    ) -> Self {
        self.order_by
            .push((column.column_name().to_string(), direction));
        self
    }

    /// Order by ascending
    pub fn order_asc(self, column: impl crate::columns::IntoColumnName) -> Self {
        self.order_by(column, Order::Asc)
    }

    /// Order by descending
    pub fn order_desc(self, column: impl crate::columns::IntoColumnName) -> Self {
        self.order_by(column, Order::Desc)
    }

    /// Order by latest (created_at DESC)
    pub fn latest(self) -> Self {
        self.order_desc("created_at")
    }

    /// Order by oldest (created_at ASC)
    pub fn oldest(self) -> Self {
        self.order_asc("created_at")
    }

    // =========================================================================
    // PAGINATION
    // =========================================================================

    /// Limit the number of results
    pub fn limit(mut self, n: u64) -> Self {
        self.limit_value = Some(n);
        self
    }

    /// Skip a number of results
    pub fn offset(mut self, n: u64) -> Self {
        self.offset_value = Some(n);
        self
    }

    /// Paginate results using 1-based page numbers.
    pub fn page(self, page: u64, per_page: u64) -> Self {
        let mut query = self;

        if page == 0 {
            query.invalidate_query("invalid pagination: page must be at least 1".to_string());
            return query;
        }

        if per_page == 0 {
            query.invalidate_query(
                "invalid pagination: per_page must be greater than 0".to_string(),
            );
            return query;
        }

        let offset = (page - 1) * per_page;
        query.limit(per_page).offset(offset)
    }

    /// Take only the first N records
    pub fn take(self, n: u64) -> Self {
        self.limit(n)
    }

    /// Skip the first N records
    pub fn skip(self, n: u64) -> Self {
        self.offset(n)
    }

    // =========================================================================
    // SELECT
    // =========================================================================

    /// Select specific columns
    pub fn select(mut self, columns: Vec<&str>) -> Self {
        self.select_columns = Some(columns.into_iter().map(|s| s.to_string()).collect());
        self
    }

    /// Select columns from this table and also from a linked/joined table
    ///
    /// Use this for partial model queries that need columns from a related
    /// table without loading the full related model.
    pub fn select_with_linked(
        mut self,
        local_columns: Vec<&str>,
        linked_table: &str,
        local_fk: &str,
        remote_pk: &str,
        linked_columns: Vec<&str>,
    ) -> Self {
        // Set local columns with table prefix
        let table_name = M::table_name();
        let mut all_columns: Vec<String> = local_columns
            .iter()
            .map(|c| format!("{}.{}", table_name, c))
            .collect();

        // Add linked columns with table prefix
        for col in linked_columns {
            all_columns.push(format!("{}.{}", linked_table, col));
        }

        self.select_columns = Some(all_columns);

        // Add the join
        self.joins.push(JoinClause {
            join_type: JoinType::Left,
            table: linked_table.to_string(),
            alias: None,
            left_column: format!("{}.{}", table_name, local_fk),
            right_column: format!("{}.{}", linked_table, remote_pk),
        });

        self
    }

    /// Select all columns from this table plus specific columns from a linked table
    pub fn select_also_linked(
        mut self,
        linked_table: &str,
        local_pk: &str,
        remote_fk: &str,
        linked_columns: Vec<&str>,
    ) -> Self {
        let table_name = M::table_name();

        // Start with all local columns
        let local_cols: Vec<String> = M::column_names()
            .iter()
            .map(|c| format!("{}.{}", table_name, c))
            .collect();

        // Add linked columns
        let mut all_columns = local_cols;
        for col in linked_columns {
            all_columns.push(format!("{}.{}", linked_table, col));
        }

        self.select_columns = Some(all_columns);

        // Add the join
        self.joins.push(JoinClause {
            join_type: JoinType::Left,
            table: linked_table.to_string(),
            alias: None,
            left_column: format!("{}.{}", table_name, local_pk),
            right_column: format!("{}.{}", linked_table, remote_fk),
        });

        self
    }

    // =========================================================================
    // JOIN OPERATIONS
    // =========================================================================

    /// Add an INNER JOIN clause
    ///
    /// Returns only rows with matches in both tables.
    pub fn inner_join(self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Inner, table, None, left_column, right_column)
    }

    /// Add an INNER JOIN clause with an alias
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
    pub fn left_join(self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Left, table, None, left_column, right_column)
    }

    /// Add a LEFT JOIN clause with an alias
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
    pub fn right_join(self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.join(JoinType::Right, table, None, left_column, right_column)
    }

    /// Add a RIGHT JOIN clause with an alias
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

    // =========================================================================
    // AGGREGATIONS
    // =========================================================================

    /// Add a GROUP BY clause
    pub fn group_by(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.group_by.push(column.column_name().to_string());
        self
    }

    /// Add multiple GROUP BY columns
    pub fn group_by_columns(mut self, columns: Vec<&str>) -> Self {
        for col in columns {
            self.group_by.push(col.to_string());
        }
        self
    }

    /// Add a HAVING clause (raw SQL condition)
    pub fn having(mut self, condition: &str) -> Self {
        if let Err(reason) =
            crate::query::db_sql::validate_raw_sql_fragment("HAVING raw SQL", condition)
        {
            self.invalidate_query(reason);
        }

        self.having_conditions.push(condition.to_string());
        self
    }

    /// Add HAVING with COUNT condition
    pub fn having_count_gt(self, value: i64) -> Self {
        self.having(&format!("COUNT(*) > {}", value))
    }

    /// Add HAVING with COUNT >= condition
    pub fn having_count_gte(self, value: i64) -> Self {
        self.having(&format!("COUNT(*) >= {}", value))
    }

    /// Add HAVING with COUNT < condition
    pub fn having_count_lt(self, value: i64) -> Self {
        self.having(&format!("COUNT(*) < {}", value))
    }

    /// Add HAVING with COUNT <= condition
    pub fn having_count_lte(self, value: i64) -> Self {
        self.having(&format!("COUNT(*) <= {}", value))
    }

    /// Add HAVING with SUM condition
    pub fn having_sum_gt(self, column: impl crate::columns::IntoColumnName, value: f64) -> Self {
        let db_type = self.db_type_for_sql();
        let col = db_sql::quote_ident(db_type, column.column_name());
        self.having(&format!("SUM({}) > {}", col, value))
    }

    /// Add HAVING with AVG condition
    pub fn having_avg_gt(self, column: impl crate::columns::IntoColumnName, value: f64) -> Self {
        let db_type = self.db_type_for_sql();
        let col = db_sql::quote_ident(db_type, column.column_name());
        self.having(&format!("AVG({}) > {}", col, value))
    }

    /// Calculate SUM of a column
    pub async fn sum(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        let db_type = self.db_type_for_sql();
        let col = db_sql::quote_ident(db_type, column.column_name());
        let expr = db_sql::cast_to_float(db_type, &format!("SUM({})", col));
        self.aggregate_f64(&expr, "sum_result").await
    }

    /// Calculate AVG of a column
    pub async fn avg(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        let db_type = self.db_type_for_sql();
        let col = db_sql::quote_ident(db_type, column.column_name());
        let expr = db_sql::cast_to_float(db_type, &format!("AVG({})", col));
        self.aggregate_f64(&expr, "avg_result").await
    }

    /// Find MIN value of a column
    pub async fn min(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        let db_type = self.db_type_for_sql();
        let col = db_sql::quote_ident(db_type, column.column_name());
        let expr = db_sql::cast_to_float(db_type, &format!("MIN({})", col));
        self.aggregate_f64(&expr, "min_result").await
    }

    /// Find MAX value of a column
    pub async fn max(self, column: impl crate::columns::IntoColumnName) -> Result<f64> {
        let db_type = self.db_type_for_sql();
        let col = db_sql::quote_ident(db_type, column.column_name());
        let expr = db_sql::cast_to_float(db_type, &format!("MAX({})", col));
        self.aggregate_f64(&expr, "max_result").await
    }

    /// Count distinct values of a column
    pub async fn count_distinct(self, column: impl crate::columns::IntoColumnName) -> Result<u64> {
        use crate::database::Connection;

        #[derive(Debug, FromQueryResult)]
        struct CountResult {
            count_result: i64,
        }

        self.ensure_query_is_valid()?;

        let db_type = self.db_type_for_sql();
        let col = db_sql::quote_ident(db_type, column.column_name());

        let db = self.current_db()?;
        let error_context = self.build_query_error_context(Some(self.build_sql_preview()));

        let mut select = M::Entity::find();

        // Apply WHERE conditions
        if !self.conditions.is_empty() || !self.or_groups.is_empty() || M::soft_delete_enabled() {
            let condition = self.build_sea_condition();
            select = select.filter(condition);
        }

        // Build COUNT(DISTINCT column) expression
        let count_expr = Expr::cust(format!("COUNT(DISTINCT {})", col));

        let result: Option<CountResult> = match db.__get_connection()? {
            crate::database::ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(
                    select
                        .select_only()
                        .column_as(count_expr, "count_result")
                        .into_model::<CountResult>()
                        .one(conn.connection()),
                )
                .await
            }
            crate::database::ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(
                    select
                        .select_only()
                        .column_as(count_expr, "count_result")
                        .into_model::<CountResult>()
                        .one(tx.as_ref()),
                )
                .await
            }
        }
        .map_err(translate_error)
        .map_err(|err| err.with_context(error_context))?;

        result
            .map(|r| crate::internal::count_to_u64(r.count_result, "COUNT(DISTINCT ...)"))
            .transpose()
            .map(|count| count.unwrap_or(0))
    }

    /// Internal helper for f64 aggregations
    async fn aggregate_f64(self, expr_sql: &str, _alias: &str) -> Result<f64> {
        use crate::database::Connection;

        #[derive(Debug, FromQueryResult)]
        struct AggResult {
            agg_result: Option<f64>,
        }

        self.ensure_query_is_valid()?;

        let db = self.current_db()?;
        let error_context = self.build_query_error_context(Some(self.build_sql_preview()));

        let mut select = M::Entity::find();

        // Apply WHERE conditions
        if !self.conditions.is_empty() || !self.or_groups.is_empty() || M::soft_delete_enabled() {
            let condition = self.build_sea_condition();
            select = select.filter(condition);
        }

        // Build aggregate expression
        let agg_expr = Expr::cust(expr_sql.to_string());

        let result: Option<AggResult> = match db.__get_connection()? {
            crate::database::ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(
                    select
                        .select_only()
                        .column_as(agg_expr, "agg_result")
                        .into_model::<AggResult>()
                        .one(conn.connection()),
                )
                .await
            }
            crate::database::ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(
                    select
                        .select_only()
                        .column_as(agg_expr, "agg_result")
                        .into_model::<AggResult>()
                        .one(tx.as_ref()),
                )
                .await
            }
        }
        .map_err(translate_error)
        .map_err(|err| err.with_context(error_context))?;

        Ok(result.and_then(|r| r.agg_result).unwrap_or(0.0))
    }

    // =========================================================================
    // UNION OPERATIONS
    // =========================================================================

    /// Add a UNION with another query
    ///
    /// UNION combines the results of two queries and removes duplicates.
    pub fn union<N: Model>(mut self, other: QueryBuilder<N>) -> Self {
        self.unions.push(UnionClause {
            union_type: UnionType::Union,
            query_sql: other.build_base_select_sql(),
        });
        self
    }

    /// Add a UNION ALL with another query
    ///
    /// UNION ALL combines all results including duplicates (faster than UNION).
    pub fn union_all<N: Model>(mut self, other: QueryBuilder<N>) -> Self {
        self.unions.push(UnionClause {
            union_type: UnionType::UnionAll,
            query_sql: other.build_base_select_sql(),
        });
        self
    }

    /// Add a raw UNION query
    ///
    /// Use when you need to union with a complex SQL query.
    pub fn union_raw(mut self, sql: &str) -> Self {
        if let Err(reason) = crate::query::db_sql::validate_subquery_sql(sql) {
            self.invalidate_query(format!("invalid subquery for union_raw(): {}", reason));
        }

        self.unions.push(UnionClause {
            union_type: UnionType::Union,
            query_sql: sql.to_string(),
        });
        self
    }

    /// Add a raw UNION ALL query
    pub fn union_all_raw(mut self, sql: &str) -> Self {
        if let Err(reason) = crate::query::db_sql::validate_subquery_sql(sql) {
            self.invalidate_query(format!("invalid subquery for union_all_raw(): {}", reason));
        }

        self.unions.push(UnionClause {
            union_type: UnionType::UnionAll,
            query_sql: sql.to_string(),
        });
        self
    }

    // =========================================================================
    // WINDOW FUNCTIONS
    // =========================================================================

    /// Add a window function to the SELECT clause
    pub fn window(mut self, window_fn: WindowFunction) -> Self {
        if let Err(reason) = Self::validate_window_function(&window_fn) {
            self.invalidate_query(reason);
        }

        self.window_functions.push(window_fn);
        self
    }

    /// Add ROW_NUMBER() window function
    pub fn row_number(
        mut self,
        alias: &str,
        partition_by: Option<&str>,
        order_by: &str,
        order: Order,
    ) -> Self {
        let mut wf =
            WindowFunction::new(WindowFunctionType::RowNumber, alias).order_by(order_by, order);
        if let Some(partition) = partition_by {
            wf = wf.partition_by(partition);
        }
        self.window_functions.push(wf);
        self
    }

    /// Add RANK() window function
    pub fn rank(
        mut self,
        alias: &str,
        partition_by: Option<&str>,
        order_by: &str,
        order: Order,
    ) -> Self {
        let mut wf = WindowFunction::new(WindowFunctionType::Rank, alias).order_by(order_by, order);
        if let Some(partition) = partition_by {
            wf = wf.partition_by(partition);
        }
        self.window_functions.push(wf);
        self
    }

    /// Add DENSE_RANK() window function
    ///
    /// Similar to RANK() but without gaps in ranking values.
    pub fn dense_rank(
        mut self,
        alias: &str,
        partition_by: Option<&str>,
        order_by: &str,
        order: Order,
    ) -> Self {
        let mut wf =
            WindowFunction::new(WindowFunctionType::DenseRank, alias).order_by(order_by, order);
        if let Some(partition) = partition_by {
            wf = wf.partition_by(partition);
        }
        self.window_functions.push(wf);
        self
    }

    /// Add LAG() window function
    ///
    /// Access data from a previous row.
    #[allow(clippy::too_many_arguments)]
    pub fn lag(
        mut self,
        alias: &str,
        column: &str,
        offset: i32,
        default: Option<&str>,
        partition_by: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        let wf = WindowFunction::new(
            WindowFunctionType::Lag(
                column.to_string(),
                Some(offset),
                default.map(|s| s.to_string()),
            ),
            alias,
        )
        .partition_by(partition_by)
        .order_by(order_by, order);

        if let Err(reason) = Self::validate_window_function(&wf) {
            self.invalidate_query(reason);
        }

        self.window_functions.push(wf);
        self
    }

    /// Add LEAD() window function
    ///
    /// Access data from a following row.
    #[allow(clippy::too_many_arguments)]
    pub fn lead(
        mut self,
        alias: &str,
        column: &str,
        offset: i32,
        default: Option<&str>,
        partition_by: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        let wf = WindowFunction::new(
            WindowFunctionType::Lead(
                column.to_string(),
                Some(offset),
                default.map(|s| s.to_string()),
            ),
            alias,
        )
        .partition_by(partition_by)
        .order_by(order_by, order);

        if let Err(reason) = Self::validate_window_function(&wf) {
            self.invalidate_query(reason);
        }

        self.window_functions.push(wf);
        self
    }

    /// Add running SUM() window function
    pub fn running_sum(mut self, alias: &str, column: &str, order_by: &str, order: Order) -> Self {
        let wf = WindowFunction::new(WindowFunctionType::Sum(column.to_string()), alias)
            .order_by(order_by, order)
            .frame(
                FrameType::Rows,
                FrameBound::UnboundedPreceding,
                FrameBound::CurrentRow,
            );
        self.window_functions.push(wf);
        self
    }

    /// Add running AVG() window function
    pub fn running_avg(mut self, alias: &str, column: &str, order_by: &str, order: Order) -> Self {
        let wf = WindowFunction::new(WindowFunctionType::Avg(column.to_string()), alias)
            .order_by(order_by, order)
            .frame(
                FrameType::Rows,
                FrameBound::UnboundedPreceding,
                FrameBound::CurrentRow,
            );
        self.window_functions.push(wf);
        self
    }

    /// Add NTILE() window function
    ///
    /// Distribute rows into specified number of groups.
    pub fn ntile(mut self, alias: &str, buckets: u32, order_by: &str, order: Order) -> Self {
        let wf = WindowFunction::new(WindowFunctionType::Ntile(buckets), alias)
            .order_by(order_by, order);
        self.window_functions.push(wf);
        self
    }

    /// Add FIRST_VALUE() window function
    pub fn first_value(
        mut self,
        alias: &str,
        column: &str,
        partition_by: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        let wf = WindowFunction::new(WindowFunctionType::FirstValue(column.to_string()), alias)
            .partition_by(partition_by)
            .order_by(order_by, order);
        self.window_functions.push(wf);
        self
    }

    /// Add LAST_VALUE() window function
    /// Add LAST_VALUE() window function
    pub fn last_value(
        mut self,
        alias: &str,
        column: &str,
        partition_by: &str,
        order_by: &str,
        order: Order,
    ) -> Self {
        let wf = WindowFunction::new(WindowFunctionType::LastValue(column.to_string()), alias)
            .partition_by(partition_by)
            .order_by(order_by, order)
            // Need to extend frame to see last value
            .frame(
                FrameType::Rows,
                FrameBound::UnboundedPreceding,
                FrameBound::UnboundedFollowing,
            );
        self.window_functions.push(wf);
        self
    }
    // COMMON TABLE EXPRESSIONS (CTEs)
    // =========================================================================

    /// Add a CTE (WITH clause) to the query
    ///
    /// CTEs allow you to define temporary named result sets that can be
    /// referenced within the main query.
    pub fn with_cte(mut self, cte: CTE) -> Self {
        if let Err(reason) = Self::validate_cte_clause(&cte) {
            self.invalidate_query(format!("invalid CTE for with_cte(): {}", reason));
        }

        self.ctes.push(cte);
        self
    }

    /// Add a CTE from another query builder
    pub fn with_query<N: Model>(mut self, name: &str, query: QueryBuilder<N>) -> Self {
        if let Err(reason) = crate::query::db_sql::validate_identifier("CTE name", name) {
            self.invalidate_query(reason);
        }

        if let Err(err) = query.ensure_query_is_valid() {
            self.invalidate_query(format!("invalid subquery for with_query(): {}", err));
        }

        self.ctes
            .push(CTE::new(name, query.build_base_select_sql()));
        self
    }

    /// Add a CTE with column aliases
    pub fn with_cte_columns(mut self, name: &str, columns: Vec<&str>, sql: &str) -> Self {
        if let Err(reason) = crate::query::db_sql::validate_identifier("CTE name", name) {
            self.invalidate_query(reason);
        }

        for column in &columns {
            if let Err(reason) = crate::query::db_sql::validate_identifier("CTE column", column) {
                self.invalidate_query(reason);
                break;
            }
        }

        if let Err(reason) = crate::query::db_sql::validate_subquery_sql(sql) {
            self.invalidate_query(format!(
                "invalid subquery for with_cte_columns(): {}",
                reason
            ));
        }

        self.ctes
            .push(CTE::with_columns(name, columns, sql.to_string()));
        self
    }

    /// Add a recursive CTE
    ///
    /// Use recursive CTEs for hierarchical or tree-structured data.
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

    // =========================================================================
    // SOFT DELETE QUERIES
    // =========================================================================

    /// Include soft-deleted records in the query results
    ///
    /// By default, soft-deleted records (where `deleted_at` is not NULL) are excluded.
    /// Use this method to include them.
    pub fn with_trashed(mut self) -> Self {
        self.include_trashed = true;
        self.only_trashed = false;
        self
    }

    /// Only return soft-deleted records
    ///
    /// Returns only records where `deleted_at` is not NULL.
    pub fn only_trashed(mut self) -> Self {
        self.only_trashed = true;
        self.include_trashed = false;
        self
    }

    /// Exclude soft-deleted records (default behavior)
    ///
    /// This is the default, but can be used to explicitly exclude soft-deleted
    /// records after calling `with_trashed()`.
    pub fn without_trashed(mut self) -> Self {
        self.include_trashed = false;
        self.only_trashed = false;
        self
    }

    // =========================================================================
    // SCOPES (Reusable query fragments)
    // =========================================================================

    /// Apply a scope function to modify the query
    ///
    /// Scopes are reusable query fragments that can be applied to any query.
    /// Use scopes to define common query patterns once and reuse them.
    pub fn scope<F>(self, f: F) -> Self
    where
        F: FnOnce(Self) -> Self,
    {
        f(self)
    }

    /// Apply a conditional scope
    ///
    /// Only applies the scope function if the condition is true.
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
    pub fn when_some<T, F>(self, option: Option<T>, f: F) -> Self
    where
        F: FnOnce(Self, T) -> Self,
    {
        match option {
            Some(value) => f(self, value),
            None => self,
        }
    }

    // =========================================================================
}
