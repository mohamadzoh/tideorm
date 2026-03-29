use super::*;

#[allow(missing_docs)]
impl<M: Model> QueryBuilder<M> {
    pub(super) fn format_column_for_db(&self, db_type: DatabaseType, column: &str) -> String {
        let trimmed = column.trim();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();

        match parts.as_slice() {
            [identifier] => db_sql::format_column(db_type, identifier),
            [identifier, direction]
                if direction.eq_ignore_ascii_case("asc")
                    || direction.eq_ignore_ascii_case("desc") =>
            {
                db_sql::format_identifier_reference(db_type, identifier)
                    .map(|identifier| format!("{} {}", identifier, direction.to_ascii_uppercase()))
                    .unwrap_or_else(|| trimmed.to_string())
            }
            [identifier, as_keyword, alias] if as_keyword.eq_ignore_ascii_case("as") => {
                match (
                    db_sql::format_identifier_reference(db_type, identifier),
                    db_sql::format_identifier_reference(db_type, alias),
                ) {
                    (Some(identifier), Some(alias)) => format!("{} AS {}", identifier, alias),
                    _ => trimmed.to_string(),
                }
            }
            _ => trimmed.to_string(),
        }
    }

    fn format_select_column_for_db(
        &self,
        db_type: DatabaseType,
        table: &str,
        column: &str,
    ) -> String {
        let trimmed = column.trim();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();

        match parts.as_slice() {
            [identifier]
                if !identifier.contains('(')
                    && !identifier.contains('*')
                    && db_sql::format_identifier_reference(db_type, identifier).is_some() =>
            {
                if identifier.contains('.') {
                    self.format_column_for_db(db_type, identifier)
                } else {
                    format!(
                        "{}.{}",
                        db_sql::quote_ident(db_type, table),
                        db_sql::quote_ident(db_type, identifier)
                    )
                }
            }
            [identifier, as_keyword, alias]
                if as_keyword.eq_ignore_ascii_case("as")
                    && !identifier.contains('(')
                    && !identifier.contains('*')
                    && db_sql::format_identifier_reference(db_type, identifier).is_some()
                    && db_sql::format_identifier_reference(db_type, alias).is_some() =>
            {
                let identifier = if identifier.contains('.') {
                    self.format_column_for_db(db_type, identifier)
                } else {
                    format!(
                        "{}.{}",
                        db_sql::quote_ident(db_type, table),
                        db_sql::quote_ident(db_type, identifier)
                    )
                };

                format!("{} AS {}", identifier, db_sql::quote_ident(db_type, alias))
            }
            _ => trimmed.to_string(),
        }
    }

    fn build_select_clause_sql(&self, db_type: DatabaseType) -> String {
        let table = M::table_name();

        if !self.raw_select_expressions.is_empty() || !self.subquery_select_expressions.is_empty() {
            let mut expressions = self.raw_select_expressions.clone();
            expressions.extend(self.subquery_select_expressions.iter().map(
                |(query_sql, alias)| {
                    format!("({}) AS {}", query_sql, db_sql::quote_ident(db_type, alias))
                },
            ));
            for window_function in &self.window_functions {
                expressions.push(window_function.to_sql_for_db(db_type));
            }
            return format!("SELECT {} ", expressions.join(", "));
        }

        if let Some(columns) = &self.select_columns {
            let mut rendered_columns: Vec<String> = columns
                .iter()
                .map(|column| self.format_select_column_for_db(db_type, table, column))
                .collect();

            for window_function in &self.window_functions {
                rendered_columns.push(window_function.to_sql_for_db(db_type));
            }

            return format!("SELECT {} ", rendered_columns.join(", "));
        }

        let mut select_parts = vec![format!("{}.*", db_sql::quote_ident(db_type, table))];
        for window_function in &self.window_functions {
            select_parts.push(window_function.to_sql_for_db(db_type));
        }
        format!("SELECT {} ", select_parts.join(", "))
    }

    fn append_from_and_join_sql(&self, sql: &mut String, db_type: DatabaseType) {
        sql.push_str(&format!(
            "FROM {} ",
            db_sql::quote_ident(db_type, M::table_name())
        ));

        for join in &self.joins {
            let join_table = if let Some(alias) = &join.alias {
                format!(
                    "{} AS {}",
                    db_sql::quote_ident(db_type, &join.table),
                    db_sql::quote_ident(db_type, alias)
                )
            } else {
                db_sql::quote_ident(db_type, &join.table)
            };

            sql.push_str(&format!(
                "{} {} ON {} = {} ",
                join.join_type.as_sql(),
                join_table,
                self.format_column_for_db(db_type, &join.left_column),
                self.format_column_for_db(db_type, &join.right_column)
            ));
        }
    }

    fn append_group_by_and_having_sql(&self, sql: &mut String, db_type: DatabaseType) {
        if !self.group_by.is_empty() {
            let columns: Vec<String> = self
                .group_by
                .iter()
                .map(|column| self.format_column_for_db(db_type, column))
                .collect();
            sql.push_str(&format!("GROUP BY {} ", columns.join(", ")));
        }

        if !self.having_conditions.is_empty() {
            sql.push_str(&format!("HAVING {} ", self.having_conditions.join(" AND ")));
        }
    }

    fn append_cte_sql(&self, sql: &mut String) {
        if self.ctes.is_empty() {
            return;
        }

        let recursive = self.ctes.iter().any(|cte| cte.recursive);
        sql.push_str(if recursive {
            "WITH RECURSIVE "
        } else {
            "WITH "
        });
        let cte_parts: Vec<String> = self.ctes.iter().map(CTE::to_sql).collect();
        sql.push_str(&cte_parts.join(", "));
        sql.push(' ');
    }

    fn append_union_sql(&self, sql: &mut String, wrap_subqueries: bool) {
        for union in &self.unions {
            if wrap_subqueries {
                sql.push_str(&format!(
                    " {} ({})",
                    union.union_type.as_sql(),
                    union.query_sql
                ));
            } else {
                sql.push_str(&format!(
                    " {} {}",
                    union.union_type.as_sql(),
                    union.query_sql
                ));
            }
        }
    }

    fn append_order_limit_offset_sql(&self, sql: &mut String, db_type: DatabaseType) {
        if !self.order_by.is_empty() {
            let order_parts: Vec<String> = self
                .order_by
                .iter()
                .map(|(column, direction)| {
                    format!(
                        "{} {}",
                        self.format_column_for_db(db_type, column),
                        direction.as_str()
                    )
                })
                .collect();
            sql.push_str(&format!(" ORDER BY {}", order_parts.join(", ")));
        }

        if let Some(limit) = self.limit_value {
            sql.push_str(&format!(" LIMIT {}", limit));
        }

        if let Some(offset) = self.offset_value {
            sql.push_str(&format!(" OFFSET {}", offset));
        }
    }

    fn assemble_base_select_sql(&self, db_type: DatabaseType, where_sql: &str) -> String {
        let mut sql = String::new();

        sql.push_str(&self.build_select_clause_sql(db_type));
        self.append_from_and_join_sql(&mut sql, db_type);

        if !where_sql.is_empty() {
            sql.push_str(&format!("WHERE {} ", where_sql));
        }

        self.append_group_by_and_having_sql(&mut sql, db_type);
        sql.trim().to_string()
    }

    fn assemble_select_sql(
        &self,
        db_type: DatabaseType,
        base_sql: &str,
        wrap_union_subqueries: bool,
    ) -> String {
        let mut sql = String::new();

        self.append_cte_sql(&mut sql);
        sql.push_str(base_sql);
        self.append_union_sql(&mut sql, wrap_union_subqueries);
        self.append_order_limit_offset_sql(&mut sql, db_type);

        sql.trim().to_string()
    }

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
                Some(self.build_compare_sql(&column, operator, value))
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
                Some(self.build_between_sql(&column, low, high))
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

    pub(super) fn render_array_values(&self, values: &[serde_json::Value]) -> Vec<String> {
        values
            .iter()
            .map(|value| match value {
                serde_json::Value::String(text) => format!("'{}'", text.replace("'", "''")),
                serde_json::Value::Number(number) => number.to_string(),
                serde_json::Value::Bool(boolean) => boolean.to_string(),
                serde_json::Value::Null => "NULL".to_string(),
                _ => format!("'{}'", value.to_string().replace("'", "''")),
            })
            .collect()
    }

    fn build_or_group_sql_for_db(&self, group: &OrGroup, db_type: DatabaseType) -> String {
        let mut parts = Vec::new();

        for condition in &group.conditions {
            if let Some(expression) = self.build_condition_sql_for_db(condition, db_type) {
                parts.push(expression);
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
                clauses.push(expression);
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

    pub(crate) fn build_base_select_sql(&self) -> String {
        self.build_base_select_sql_for_db(self.db_type_for_sql())
    }

    pub(crate) fn build_base_select_sql_for_db(&self, db_type: DatabaseType) -> String {
        let where_sql = self.build_where_sql_for_db(db_type);
        self.assemble_base_select_sql(db_type, &where_sql)
    }

    fn build_base_select_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        let (where_sql, params) = self.build_where_clause_with_condition_for_db(db_type);
        (self.assemble_base_select_sql(db_type, &where_sql), params)
    }

    pub(crate) fn build_select_sql(&self) -> String {
        self.build_select_sql_for_db(self.db_type_for_sql())
    }

    pub(crate) fn build_select_sql_for_db(&self, db_type: DatabaseType) -> String {
        let base_sql = self.build_base_select_sql_for_db(db_type);
        self.assemble_select_sql(db_type, &base_sql, false)
    }

    pub(crate) fn build_select_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        let (base_sql, params) = self.build_base_select_sql_with_params_for_db(db_type);
        (self.assemble_select_sql(db_type, &base_sql, true), params)
    }

    pub(super) fn build_select_sql_with_params(&self) -> (String, Vec<Value>) {
        self.build_select_sql_with_params_for_db(self.db_type_for_sql())
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
                let (inner_sql, params) = exists_query.build_base_select_sql_with_params_for_db(db_type);
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
