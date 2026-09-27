mod select;

use super::*;

impl<M: Model> QueryBuilder<M> {
    pub(crate) fn build_count_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        let mut count_query = self.clone();
        count_query.order_by.clear();
        count_query.limit_value = None;
        count_query.offset_value = None;
        // A grouped query counts its groups, one row each, so the inner query
        // selects a constant unless the caller chose a projection: selecting
        // the grouping columns would name two of one name (`orders.status`,
        // `customers.status`) alike in the derived table, which MySQL refuses.
        if !count_query.group_by.is_empty() && !count_query.has_explicit_projection() {
            count_query.select_columns = None;
            count_query.raw_select_expressions = vec!["1".to_string()];
        }

        if count_query.unions.is_empty()
            && count_query.ctes.is_empty()
            && count_query.group_by.is_empty()
            && count_query.having_conditions.is_empty()
            && count_query.raw_select_expressions.is_empty()
            && !count_query.lock_for_update
        {
            let (where_sql, params) = count_query.build_where_clause_with_condition_for_db(db_type);
            let mut sql = String::from("SELECT COUNT(*) AS count ");
            count_query.append_from_and_join_sql(&mut sql, db_type);
            if !where_sql.is_empty() {
                sql.push_str(&format!("WHERE {}", where_sql));
            } else {
                sql.truncate(sql.trim_end().len());
            }
            return (sql, params);
        }

        let (inner_sql, params) = count_query.build_select_sql_with_params_for_db(db_type);
        (
            format!(
                "SELECT COUNT(*) AS count FROM ({}) AS {}",
                inner_sql,
                db_sql::quote_ident(db_type, "tideorm_count_subquery")
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
        exists_query.order_by.clear();
        exists_query.limit_value = None;
        exists_query.offset_value = None;

        if exists_query.unions.is_empty() {
            // HAVING can name one of the caller's select aliases, so a query
            // with both keeps its projection.
            let having_needs_projection = !exists_query.having_conditions.is_empty()
                && exists_query.has_explicit_projection();
            if !having_needs_projection {
                exists_query.select_columns = None;
                exists_query.raw_select_expressions = vec!["1".to_string()];
                exists_query.subquery_select_expressions.clear();
                exists_query.window_functions.clear();
            }

            if exists_query.ctes.is_empty() && !exists_query.lock_for_update {
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
                "SELECT 1 FROM ({}) AS {} LIMIT 1",
                inner_sql,
                db_sql::quote_ident(db_type, "tideorm_exists_subquery")
            ),
            params,
        )
    }

    pub(super) fn build_exists_sql_with_params(&self) -> (String, Vec<Value>) {
        self.build_exists_sql_with_params_for_db(self.db_type_for_sql())
    }
}
