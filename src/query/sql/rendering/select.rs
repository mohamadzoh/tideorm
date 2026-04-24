use super::*;

#[allow(missing_docs)]
impl<M: Model> QueryBuilder<M> {
    fn canonical_model_identifier<'a>(&self, identifier: &'a str) -> std::borrow::Cow<'a, str> {
        match M::canonical_column_name(identifier) {
            Some(column_name) => std::borrow::Cow::Borrowed(column_name),
            None => std::borrow::Cow::Borrowed(identifier),
        }
    }

    pub(crate) fn format_column_for_db(&self, db_type: DatabaseType, column: &str) -> String {
        let trimmed = column.trim();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();

        match parts.as_slice() {
            [identifier] => db_sql::format_column_or_trusted_expression(
                db_type,
                self.canonical_model_identifier(identifier).as_ref(),
            ),
            [identifier, direction]
                if direction.eq_ignore_ascii_case("asc")
                    || direction.eq_ignore_ascii_case("desc") =>
            {
                db_sql::format_identifier_reference(
                    db_type,
                    self.canonical_model_identifier(identifier).as_ref(),
                )
                .map(|identifier| format!("{} {}", identifier, direction.to_ascii_uppercase()))
                .unwrap_or_else(|| trimmed.to_string())
            }
            [identifier, as_keyword, alias] if as_keyword.eq_ignore_ascii_case("as") => {
                let identifier = self.canonical_model_identifier(identifier);
                match (
                    db_sql::format_identifier_reference(db_type, identifier.as_ref()),
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
                    let identifier = self.canonical_model_identifier(identifier);
                    format!(
                        "{}.{}",
                        db_sql::quote_ident(db_type, table),
                        db_sql::quote_ident(db_type, identifier.as_ref())
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
                    let identifier = self.canonical_model_identifier(identifier);
                    format!(
                        "{}.{}",
                        db_sql::quote_ident(db_type, table),
                        db_sql::quote_ident(db_type, identifier.as_ref())
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

    pub(crate) fn append_from_and_join_sql(&self, sql: &mut String, db_type: DatabaseType) {
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

    fn render_having_preview_sql(
        &self,
        db_type: DatabaseType,
        sql_template: &str,
        params: &[serde_json::Value],
    ) -> String {
        if params.is_empty() {
            return sql_template.to_string();
        }

        let mut rendered = String::new();
        let mut params_iter = params.iter();

        for ch in sql_template.chars() {
            if ch == '?' {
                if let Some(value) = params_iter.next() {
                    rendered.push_str(&self.format_preview_value(db_type, value));
                } else {
                    rendered.push('?');
                }
            } else {
                rendered.push(ch);
            }
        }

        rendered
    }

    fn render_having_parameterized_sql(
        &self,
        sql_template: &str,
        params: &[serde_json::Value],
        db_type: DatabaseType,
        next_param_index: &mut usize,
    ) -> String {
        if params.is_empty() {
            return sql_template.to_string();
        }

        if !matches!(db_type, DatabaseType::Postgres) {
            return sql_template.to_string();
        }

        let mut rendered = String::new();
        let mut replaced = 0usize;

        for ch in sql_template.chars() {
            if ch == '?' {
                rendered.push_str(&format!("${}", *next_param_index));
                *next_param_index += 1;
                replaced += 1;
            } else {
                rendered.push(ch);
            }
        }

        debug_assert_eq!(replaced, params.len());
        rendered
    }

    pub(crate) fn materialized_having_conditions(&self, db_type: DatabaseType) -> Vec<String> {
        self.having_conditions
            .iter()
            .enumerate()
            .map(|(index, sql_template)| {
                let params = self
                    .having_bindings
                    .get(index)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                self.render_having_preview_sql(db_type, sql_template, params)
            })
            .collect()
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
            sql.push_str(&format!(
                "HAVING {} ",
                self.materialized_having_conditions(db_type).join(" AND ")
            ));
        }
    }

    fn append_group_by_and_having_sql_with_params(
        &self,
        sql: &mut String,
        db_type: DatabaseType,
        params: &mut Vec<Value>,
    ) {
        if !self.group_by.is_empty() {
            let columns: Vec<String> = self
                .group_by
                .iter()
                .map(|column| self.format_column_for_db(db_type, column))
                .collect();
            sql.push_str(&format!("GROUP BY {} ", columns.join(", ")));
        }

        if !self.having_conditions.is_empty() {
            let mut next_param_index = params.len() + 1;
            let rendered_having: Vec<String> = self
                .having_conditions
                .iter()
                .enumerate()
                .map(|(index, sql_template)| {
                    let bindings = self
                        .having_bindings
                        .get(index)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    let clause_sql = self.render_having_parameterized_sql(
                        sql_template,
                        bindings,
                        db_type,
                        &mut next_param_index,
                    );
                    params.extend(bindings.iter().map(crate::internal::json_to_db_value));
                    clause_sql
                })
                .collect();

            sql.push_str(&format!("HAVING {} ", rendered_having.join(" AND ")));
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

    pub(crate) fn build_base_select_sql(&self) -> String {
        self.build_base_select_sql_for_db(self.db_type_for_sql())
    }

    pub(crate) fn build_base_select_sql_for_db(&self, db_type: DatabaseType) -> String {
        let where_sql = self.build_where_sql_for_db(db_type);
        self.assemble_base_select_sql(db_type, &where_sql)
    }

    pub(crate) fn build_base_select_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        let (where_sql, mut params) = self.build_where_clause_with_condition_for_db(db_type);
        let mut sql = String::new();

        sql.push_str(&self.build_select_clause_sql(db_type));
        self.append_from_and_join_sql(&mut sql, db_type);

        if !where_sql.is_empty() {
            sql.push_str(&format!("WHERE {} ", where_sql));
        }

        self.append_group_by_and_having_sql_with_params(&mut sql, db_type, &mut params);
        (sql.trim().to_string(), params)
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

    pub(crate) fn build_select_sql_with_params(&self) -> (String, Vec<Value>) {
        self.build_select_sql_with_params_for_db(self.db_type_for_sql())
    }
}
