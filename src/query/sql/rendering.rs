mod select;

use super::*;

impl<M: Model> QueryBuilder<M> {
    /// `SELECT <projection>` over the query's table, joins and filters alone,
    /// for a count or an aggregate that no other clause shapes.
    pub(in crate::query) fn build_plain_select_sql(
        &self,
        db_type: DatabaseType,
        projection: &str,
    ) -> (String, Vec<Value>) {
        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        let mut sql = format!("SELECT {projection} ");
        self.append_from_and_join_sql(&mut sql, db_type);
        if !where_sql.is_empty() {
            sql.push_str(&format!("WHERE {}", where_sql));
        }
        (sql.trim_end().to_string(), params)
    }

    pub(crate) fn build_count_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        let mut count_query = self.clone();
        count_query.clauses.order_by.clear();
        count_query.clauses.limit_value = None;
        count_query.clauses.offset_value = None;
        // A grouped query counts its groups, one row each, so the inner query
        // selects a constant unless the caller chose a projection: selecting
        // the grouping columns would name two of one name (`orders.status`,
        // `customers.status`) alike in the derived table, which MySQL refuses.
        if !count_query.clauses.group_by.is_empty() && !count_query.has_explicit_projection() {
            count_query.clauses.select_columns = None;
            count_query.clauses.raw_select_expressions = vec!["1".to_string()];
        }

        if count_query.clauses.unions.is_empty()
            && count_query.clauses.ctes.is_empty()
            && count_query.clauses.group_by.is_empty()
            && count_query.clauses.having_conditions.is_empty()
            && count_query.clauses.raw_select_expressions.is_empty()
            && !count_query.clauses.lock_for_update
        {
            return count_query.build_plain_select_sql(db_type, "COUNT(*) AS count");
        }

        let (inner_sql, params) = count_query.build_select_sql_with_params_for_db(db_type);
        (
            db_sql::select_from_derived(
                db_type,
                "COUNT(*) AS count",
                &inner_sql,
                "tideorm_count_subquery",
            ),
            params,
        )
    }

    pub(super) fn build_count_sql_with_params(&self) -> (String, Vec<Value>) {
        self.build_count_sql_with_params_for_db(self.db_type_for_sql())
    }

    pub(crate) fn build_exists_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        let mut exists_query = self.clone();
        exists_query.clauses.order_by.clear();
        exists_query.clauses.limit_value = None;
        exists_query.clauses.offset_value = None;

        if exists_query.clauses.unions.is_empty() {
            // HAVING can name one of the caller's select aliases, so a query
            // with both keeps its projection.
            let having_needs_projection = !exists_query.clauses.having_conditions.is_empty()
                && exists_query.has_explicit_projection();
            if !having_needs_projection {
                exists_query.clauses.select_columns = None;
                exists_query.clauses.raw_select_expressions = vec!["1".to_string()];
                exists_query.clauses.subquery_select_expressions.clear();
                exists_query.clauses.window_functions.clear();
            }

            if exists_query.clauses.ctes.is_empty() && !exists_query.clauses.lock_for_update {
                let (inner_sql, params) =
                    exists_query.build_base_select_sql_with_params_for_db(db_type);
                return (
                    format!(
                        "SELECT EXISTS({} LIMIT 1) AS {}",
                        inner_sql,
                        db_sql::quote_ident(db_type, "exists_result")
                    ),
                    params,
                );
            }
        }

        let (inner_sql, params) = exists_query.build_select_sql_with_params_for_db(db_type);
        (
            format!(
                "{} LIMIT 1",
                db_sql::select_from_derived(db_type, "1", &inner_sql, "tideorm_exists_subquery")
            ),
            params,
        )
    }

    pub(super) fn build_exists_sql_with_params(&self) -> (String, Vec<Value>) {
        self.build_exists_sql_with_params_for_db(self.db_type_for_sql())
    }
}
