mod select;

use super::*;

#[allow(missing_docs)]
impl<M: Model> QueryBuilder<M> {
    fn build_condition_sql_for_db(
        &self,
        condition: &WhereCondition,
        db_type: DatabaseType,
    ) -> Option<String> {
        let spec = Self::condition_spec(condition)?;

        if let ConditionSpec::Raw { column, raw_sql } = spec {
            return Some(self.build_raw_condition_sql(db_type, column, raw_sql));
        }

        let column = self.format_column_for_db(db_type, &condition.column);

        match spec {
            ConditionSpec::Raw { .. } => None,
            ConditionSpec::Compare { operator, value } => {
                Some(self.build_compare_sql(db_type, &column, operator, value))
            }
            ConditionSpec::Pattern {
                negated,
                escaped,
                value,
            } => Some(self.build_pattern_sql(db_type, &column, negated, escaped, value)),
            ConditionSpec::List { operator, values } => {
                Some(self.build_list_sql(db_type, &column, operator, values))
            }
            ConditionSpec::NullCheck { negated } => {
                Some(self.build_null_check_sql(&column, negated))
            }
            ConditionSpec::Between { low, high } => {
                Some(self.build_between_sql(db_type, &column, low, high))
            }
            ConditionSpec::JsonValue { operator, value } => {
                Some(self.build_json_value_sql(db_type, &condition.column, operator, value))
            }
            ConditionSpec::JsonString { operator, value } => {
                Some(self.build_json_string_sql(db_type, &condition.column, operator, value))
            }
            ConditionSpec::Array { operator, values } => {
                Some(self.build_array_sql(db_type, &condition.column, operator, values))
            }
            ConditionSpec::Subquery { negated, query_sql } => {
                Some(self.build_subquery_sql(&column, negated, query_sql))
            }
        }
    }

    /// Isolate a rendered condition from the surrounding `AND`/`OR` join.
    ///
    /// A rendered condition can carry top-level logical operators of its own —
    /// user supplied raw fragments as well as some backend specific composite
    /// predicates — so joining them bare lets SQL precedence silently swallow
    /// sibling filters (`tenant = 5 AND a = 1 OR b = 2`). The sea-query
    /// execution path parenthesizes custom expressions for exactly this reason;
    /// wrapping unconditionally here keeps both execution paths in agreement.
    fn parenthesize_condition_sql(expression: &str) -> String {
        format!("({})", expression)
    }

    fn build_or_group_sql_for_db(&self, group: &OrGroup, db_type: DatabaseType) -> String {
        let mut parts = Vec::new();

        for condition in &group.conditions {
            if let Some(expression) = self.build_condition_sql_for_db(condition, db_type) {
                parts.push(Self::parenthesize_condition_sql(&expression));
            }
        }

        for nested_group in &group.nested_groups {
            let nested_sql = self.build_or_group_sql_for_db(nested_group, db_type);
            if !nested_sql.is_empty() {
                parts.push(format!("({})", nested_sql));
            }
        }

        parts.join(&format!(" {} ", group.combine_with.as_sql()))
    }

    pub(crate) fn build_where_sql_for_db(&self, db_type: DatabaseType) -> String {
        let mut clauses = Vec::new();

        for condition in &self.conditions {
            if let Some(expression) = self.build_condition_sql_for_db(condition, db_type) {
                clauses.push(Self::parenthesize_condition_sql(&expression));
            }
        }

        for group in &self.or_groups {
            let group_sql = self.build_or_group_sql_for_db(group, db_type);
            if !group_sql.is_empty() {
                clauses.push(format!("({})", group_sql));
            }
        }

        if M::soft_delete_enabled() {
            let deleted_at = db_sql::quote_ident(db_type, M::deleted_at_column());
            if self.only_trashed {
                clauses.push(format!("{} IS NOT NULL", deleted_at));
            } else if !self.include_trashed {
                clauses.push(format!("{} IS NULL", deleted_at));
            }
        }

        clauses.join(" AND ")
    }

    pub(crate) fn build_count_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        let mut count_query = self.clone();
        count_query.order_by.clear();
        count_query.limit_value = None;
        count_query.offset_value = None;

        if count_query.unions.is_empty()
            && count_query.ctes.is_empty()
            && count_query.group_by.is_empty()
            && count_query.having_conditions.is_empty()
            && count_query.raw_select_expressions.is_empty()
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
            exists_query.select_columns = None;
            exists_query.raw_select_expressions = vec!["1".to_string()];
            exists_query.subquery_select_expressions.clear();
            exists_query.window_functions.clear();

            if exists_query.ctes.is_empty() {
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
