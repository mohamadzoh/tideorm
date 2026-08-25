use super::*;

#[allow(missing_docs)]
impl<M: Model> QueryBuilder<M> {
    /// Canonicalize a column reference to its database column name, keeping any
    /// table qualifier it already carries.
    ///
    /// Validation resolves a self-qualified Rust field name through the
    /// field-name map, so rendering has to agree: without the qualified half,
    /// `order_desc("users.userName")` validates and then renders
    /// `"users"."userName"` — a column that does not exist. A qualifier naming
    /// some other table, and anything that is not a column reference at all,
    /// round-trips unchanged because the qualifier and the remainder are
    /// rejoined exactly as they were split.
    fn canonical_model_identifier<'a>(&self, identifier: &'a str) -> std::borrow::Cow<'a, str> {
        match M::canonical_column_parts(identifier) {
            (Some(table), column) => std::borrow::Cow::Owned(format!("{}.{}", table, column)),
            (None, column) => std::borrow::Cow::Borrowed(column),
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

    /// Render the projection from every source that contributed one.
    ///
    /// The four sources compose in a fixed order — the typed columns of
    /// `select()`, the raw expressions of `select_raw()`, the scalar subqueries
    /// of `select_subquery()`, then the window functions of `window()` — so a
    /// query that mixes them keeps every requested output column instead of
    /// silently dropping the typed half. `table.*` is only supplied when nothing
    /// but window functions was selected, because it is a default rather than a
    /// contribution.
    ///
    /// `distinct()` records itself as a sentinel among the raw expressions
    /// because it modifies the projection as a whole. The sentinel is dropped
    /// here and re-emitted as the `DISTINCT` keyword in front of the joined
    /// list, so it never reaches SQL and never counts towards the `table.*`
    /// fallback.
    fn build_select_clause_sql(&self, db_type: DatabaseType) -> String {
        let table = M::table_name();
        let mut expressions: Vec<String> = Vec::new();

        if let Some(columns) = &self.select_columns {
            for column in columns {
                expressions.push(self.format_select_column_for_db(db_type, table, column));
            }
        }

        expressions.extend(
            self.raw_select_expressions
                .iter()
                .filter(|expression| {
                    expression.as_str() != crate::query::builder::DISTINCT_SELECT_MARKER
                })
                .cloned(),
        );

        for (query_sql, alias) in &self.subquery_select_expressions {
            let alias = db_sql::quote_ident(db_type, alias);
            expressions.push(format!("({}) AS {}", query_sql, alias));
        }

        if expressions.is_empty() {
            expressions.push(format!("{}.*", db_sql::quote_ident(db_type, table)));
        }

        for window_function in &self.window_functions {
            expressions.push(window_function.to_sql_for_db(db_type));
        }

        let keyword = if self.is_distinct() {
            "SELECT DISTINCT"
        } else {
            "SELECT"
        };

        format!("{} {} ", keyword, expressions.join(", "))
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

    fn append_cte_sql(&self, sql: &mut String, db_type: DatabaseType) {
        if self.ctes.is_empty() {
            return;
        }

        sql.push_str(self.cte_keyword());
        let cte_parts: Vec<String> = self
            .ctes
            .iter()
            .map(|cte| cte.to_sql_for_db(db_type))
            .collect();
        sql.push_str(&cte_parts.join(", "));
        sql.push(' ');
    }

    fn append_union_sql(&self, sql: &mut String) {
        for union in &self.unions {
            sql.push_str(&format!(
                " {} {}",
                union.union_type.as_sql(),
                union.query_sql
            ));
        }
    }

    fn cte_keyword(&self) -> &'static str {
        if self.ctes.iter().any(|cte| cte.recursive) {
            "WITH RECURSIVE "
        } else {
            "WITH "
        }
    }

    /// Whether a compound-select operand is parenthesized for `db_type`.
    ///
    /// SQLite's compound-select grammar only accepts a bare select-core after
    /// `UNION`/`INTERSECT`/`EXCEPT`; a parenthesized operand fails to parse with
    /// `near "(": syntax error`. Postgres and MySQL accept either form, and keep
    /// the parenthesized one.
    fn wraps_compound_operand(db_type: DatabaseType) -> bool {
        !matches!(db_type, DatabaseType::SQLite)
    }

    /// Renumber a separately rendered operand's placeholders for its position in
    /// the assembled statement.
    ///
    /// Compound selects are assembled by concatenating SQL strings rather than
    /// through sea-query, so nothing renumbers placeholders here. Each operand
    /// was rendered on its own starting at `$1`; on Postgres it has to be shifted
    /// past every value already bound ahead of it. MySQL and SQLite use bare `?`
    /// markers that only depend on the order values are pushed, so their SQL is
    /// spliced in unchanged.
    fn rebase_operand_placeholders(db_type: DatabaseType, sql: &str, offset: usize) -> String {
        match db_type {
            DatabaseType::Postgres => {
                crate::model::BatchUpdateBuilder::<M>::offset_postgres_placeholders(sql, offset)
            }
            DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => sql.to_string(),
        }
    }

    fn append_cte_sql_with_params(
        &self,
        sql: &mut String,
        db_type: DatabaseType,
        params: &mut Vec<Value>,
    ) {
        if self.ctes.is_empty() {
            return;
        }

        sql.push_str(self.cte_keyword());

        let mut cte_parts = Vec::with_capacity(self.ctes.len());
        for cte in &self.ctes {
            let body_sql = Self::rebase_operand_placeholders(db_type, &cte.query_sql, params.len());
            cte_parts.push(cte.to_sql_with_body_for_db(db_type, &body_sql));
            params.extend(cte.params.iter().cloned());
        }

        sql.push_str(&cte_parts.join(", "));
        sql.push(' ');
    }

    fn append_union_sql_with_params(
        &self,
        sql: &mut String,
        db_type: DatabaseType,
        params: &mut Vec<Value>,
    ) {
        for union in &self.unions {
            let operand_sql =
                Self::rebase_operand_placeholders(db_type, &union.query_sql, params.len());

            if Self::wraps_compound_operand(db_type) {
                sql.push_str(&format!(" {} ({})", union.union_type.as_sql(), operand_sql));
            } else {
                sql.push_str(&format!(" {} {}", union.union_type.as_sql(), operand_sql));
            }

            params.extend(union.params.iter().cloned());
        }
    }

    /// Render a single ORDER BY term.
    ///
    /// Entries created by `order_by_raw()` carry the raw-expression marker and
    /// are emitted verbatim; everything else has already been restricted to a
    /// resolvable column reference by validation. A column that carries its own
    /// `ASC`/`DESC` suffix keeps it, because appending the tuple direction on top
    /// would render `"name" DESC ASC`.
    fn format_order_by_for_db(
        &self,
        db_type: DatabaseType,
        column: &str,
        direction: Order,
    ) -> String {
        if let Some(expression) = crate::query::builder::raw_order_by_expression(column) {
            return format!("{} {}", expression, direction.as_str());
        }

        let trimmed = column.trim();
        if let Some((reference, suffix)) = trimmed.split_once(char::is_whitespace) {
            let suffix = suffix.trim();
            if suffix.eq_ignore_ascii_case("asc") || suffix.eq_ignore_ascii_case("desc") {
                return format!(
                    "{} {}",
                    db_sql::format_column(
                        db_type,
                        self.canonical_model_identifier(reference).as_ref()
                    ),
                    suffix.to_ascii_uppercase()
                );
            }
        }

        format!(
            "{} {}",
            self.format_column_for_db(db_type, trimmed),
            direction.as_str()
        )
    }

    fn append_order_limit_offset_sql(&self, sql: &mut String, db_type: DatabaseType) {
        if !self.order_by.is_empty() {
            let order_parts: Vec<String> = self
                .order_by
                .iter()
                .map(|(column, direction)| self.format_order_by_for_db(db_type, column, *direction))
                .collect();
            sql.push_str(&format!(" ORDER BY {}", order_parts.join(", ")));
        }

        match (self.limit_value, self.offset_value) {
            (Some(limit), Some(offset)) => {
                sql.push_str(&format!(" LIMIT {} OFFSET {}", limit, offset));
            }
            (Some(limit), None) => sql.push_str(&format!(" LIMIT {}", limit)),
            // MySQL, MariaDB, and SQLite have no bare-OFFSET syntax: `OFFSET n`
            // without a preceding LIMIT is a parse error. Supply the dialect's
            // open-ended limit so a standalone `offset()` stays portable.
            (None, Some(offset)) => match db_type {
                DatabaseType::Postgres => sql.push_str(&format!(" OFFSET {}", offset)),
                DatabaseType::SQLite => sql.push_str(&format!(" LIMIT -1 OFFSET {}", offset)),
                DatabaseType::MySQL | DatabaseType::MariaDB => {
                    sql.push_str(&format!(" LIMIT {} OFFSET {}", u64::MAX, offset))
                }
            },
            (None, None) => {}
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

    fn assemble_select_sql(&self, db_type: DatabaseType, base_sql: &str) -> String {
        let mut sql = String::new();

        self.append_cte_sql(&mut sql, db_type);
        sql.push_str(base_sql);
        self.append_union_sql(&mut sql);
        self.append_order_limit_offset_sql(&mut sql, db_type);

        sql.trim().to_string()
    }

    /// Splice the CTE prefix and compound-select operands around the base select,
    /// keeping bound values in placeholder order.
    ///
    /// Placeholders appear left to right in exactly three groups: the `WITH`
    /// prefix precedes the base select, and every union operand follows it. The
    /// values are pushed in that same order — CTE bodies in declaration order,
    /// then the base select's own values, then union operands in order — because
    /// backends bind purely by position.
    fn assemble_select_sql_with_params(
        &self,
        db_type: DatabaseType,
        base_sql: &str,
        base_params: Vec<Value>,
    ) -> (String, Vec<Value>) {
        let mut sql = String::new();
        let mut params: Vec<Value> = Vec::new();

        self.append_cte_sql_with_params(&mut sql, db_type, &mut params);

        sql.push_str(&Self::rebase_operand_placeholders(
            db_type,
            base_sql,
            params.len(),
        ));
        params.extend(base_params);

        self.append_union_sql_with_params(&mut sql, db_type, &mut params);
        self.append_order_limit_offset_sql(&mut sql, db_type);

        (sql.trim().to_string(), params)
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
        self.assemble_select_sql(db_type, &base_sql)
    }

    pub(crate) fn build_select_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        let (base_sql, base_params) = self.build_base_select_sql_with_params_for_db(db_type);
        self.assemble_select_sql_with_params(db_type, &base_sql, base_params)
    }

    pub(crate) fn build_select_sql_with_params(&self) -> (String, Vec<Value>) {
        self.build_select_sql_with_params_for_db(self.db_type_for_sql())
    }
}

#[cfg(test)]
mod tests {
    use crate::config::DatabaseType;
    use crate::model::Model;
    use crate::query::CTE;

    #[tideorm::model(table = "select_render_users")]
    struct SelectRenderUser {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
    }

    #[tideorm::model(table = "select_render_accounts")]
    struct SelectRenderAccount {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        #[tideorm(column = "user_name")]
        user_name_field: String,
    }

    fn cte() -> CTE {
        CTE::new("recent", "SELECT id FROM select_render_users".to_string())
    }

    #[test]
    fn test_typed_and_raw_selections_compose_in_one_projection() {
        let sql = SelectRenderUser::query()
            .select(vec!["id", "name"])
            .select_raw("COUNT(*) AS total")
            .build_select_sql_for_db(DatabaseType::Postgres);

        assert_eq!(
            sql,
            "SELECT \"select_render_users\".\"id\", \"select_render_users\".\"name\", COUNT(*) AS total FROM \"select_render_users\""
        );
    }

    #[test]
    fn test_distinct_prefixes_the_projection() {
        let sql = SelectRenderUser::query()
            .distinct()
            .build_select_sql_for_db(DatabaseType::Postgres);

        assert_eq!(
            sql,
            "SELECT DISTINCT \"select_render_users\".* FROM \"select_render_users\""
        );
    }

    #[test]
    fn test_distinct_composes_with_every_projection_source() {
        let sql = SelectRenderUser::query()
            .select(vec!["id", "name"])
            .select_raw("COUNT(*) AS total")
            .distinct()
            .build_select_sql_for_db(DatabaseType::Postgres);

        assert_eq!(
            sql,
            "SELECT DISTINCT \"select_render_users\".\"id\", \"select_render_users\".\"name\", COUNT(*) AS total FROM \"select_render_users\""
        );
    }

    #[test]
    fn test_distinct_sentinel_never_reaches_parameterized_sql() {
        let (sql, _) = SelectRenderUser::query()
            .distinct()
            .where_eq("name", "ada")
            .build_select_sql_with_params_for_db(DatabaseType::Postgres);

        assert!(
            sql.starts_with("SELECT DISTINCT \"select_render_users\".*"),
            "{sql}"
        );
        assert!(!sql.contains('\u{1}'), "{sql}");
    }

    #[test]
    fn test_distinct_count_counts_the_deduplicated_rows() {
        let (sql, _) = SelectRenderUser::query()
            .distinct()
            .build_count_sql_with_params_for_db(DatabaseType::Postgres);

        // The `SELECT COUNT(*)` fast path would count the duplicate rows that
        // DISTINCT exists to collapse, so a distinct query has to count the
        // deduplicated result set through the derived table instead.
        assert_eq!(
            sql,
            "SELECT COUNT(*) AS count FROM (SELECT DISTINCT \"select_render_users\".* FROM \"select_render_users\") AS \"tideorm_count_subquery\""
        );
    }

    #[test]
    fn test_distinct_does_not_change_the_exists_projection() {
        let (sql, _) = SelectRenderUser::query()
            .distinct()
            .build_exists_sql_with_params_for_db(DatabaseType::Postgres);

        assert_eq!(
            sql,
            "SELECT EXISTS(SELECT 1 FROM \"select_render_users\" LIMIT 1) AS \"exists_result\""
        );
    }

    #[test]
    fn test_distinct_survives_a_fragment_round_trip_without_duplicating() {
        let fragment = SelectRenderUser::query().distinct().consolidate();

        assert!(!fragment.is_empty());

        let sql = SelectRenderUser::query()
            .distinct()
            .apply(&fragment)
            .build_select_sql_for_db(DatabaseType::Postgres);

        assert_eq!(
            sql,
            "SELECT DISTINCT \"select_render_users\".* FROM \"select_render_users\""
        );
    }

    #[test]
    fn test_empty_selection_falls_back_to_the_model_projection() {
        let sql = SelectRenderUser::query()
            .select(Vec::new())
            .build_select_sql_for_db(DatabaseType::Postgres);

        assert_eq!(
            sql,
            "SELECT \"select_render_users\".* FROM \"select_render_users\""
        );
    }

    #[test]
    fn test_cte_name_uses_backend_identifier_quoting() {
        let query = SelectRenderUser::query().with_cte(cte());

        let mysql_sql = query.build_select_sql_for_db(DatabaseType::MySQL);
        assert!(
            mysql_sql.starts_with("WITH `recent` AS ("),
            "MySQL rejects a double-quoted CTE name: {mysql_sql}"
        );

        let postgres_sql = query.build_select_sql_for_db(DatabaseType::Postgres);
        assert!(
            postgres_sql.starts_with("WITH \"recent\" AS ("),
            "{postgres_sql}"
        );
    }

    #[test]
    fn test_parameterized_cte_name_uses_backend_identifier_quoting() {
        let query = SelectRenderUser::query().with_cte(cte());
        let (sql, _) = query.build_select_sql_with_params_for_db(DatabaseType::MySQL);

        assert!(sql.starts_with("WITH `recent` AS ("), "{sql}");
    }

    #[test]
    fn test_self_qualified_rust_field_name_renders_the_database_column() {
        let sql = SelectRenderAccount::query()
            .select(vec!["select_render_accounts.user_name_field"])
            .group_by("select_render_accounts.user_name_field")
            .order_desc("select_render_accounts.user_name_field")
            .build_select_sql_for_db(DatabaseType::Postgres);

        assert!(
            !sql.contains("user_name_field"),
            "the Rust field name must not reach SQL: {sql}"
        );
        assert!(
            sql.starts_with("SELECT \"select_render_accounts\".\"user_name\" "),
            "{sql}"
        );
        assert!(
            sql.contains("GROUP BY \"select_render_accounts\".\"user_name\""),
            "{sql}"
        );
        assert!(
            sql.contains("ORDER BY \"select_render_accounts\".\"user_name\" DESC"),
            "{sql}"
        );
    }

    #[test]
    fn test_unqualified_rust_field_name_still_renders_the_database_column() {
        let sql = SelectRenderAccount::query()
            .order_asc("user_name_field")
            .build_select_sql_for_db(DatabaseType::Postgres);

        assert!(sql.ends_with("ORDER BY \"user_name\" ASC"), "{sql}");
    }

    #[test]
    fn test_foreign_qualifier_is_left_untouched() {
        let sql = SelectRenderAccount::query()
            .inner_join(
                "select_render_logins",
                "select_render_accounts.id",
                "select_render_logins.account_id",
            )
            .build_select_sql_for_db(DatabaseType::Postgres);

        assert!(
            sql.contains(
                "ON \"select_render_accounts\".\"id\" = \"select_render_logins\".\"account_id\""
            ),
            "{sql}"
        );
    }
}
