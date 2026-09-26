use super::*;

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
    ///
    /// On a query with joins, an unqualified model column is qualified with the
    /// model's table: a joined table can have a column of the same name, which
    /// the database would otherwise reject as ambiguous.
    pub(in crate::query) fn canonical_model_identifier<'a>(
        &self,
        identifier: &'a str,
    ) -> std::borrow::Cow<'a, str> {
        match M::canonical_column_parts(identifier) {
            (Some(table), column) => std::borrow::Cow::Owned(format!("{}.{}", table, column)),
            (None, column) if self.qualifies_model_column(column) => {
                std::borrow::Cow::Owned(format!("{}.{}", M::table_name(), column))
            }
            (None, column) => std::borrow::Cow::Borrowed(column),
        }
    }

    /// Whether an unqualified reference to `column`, a database column name,
    /// is written with the model's table: when it is one of the model's
    /// columns and the query joins another table.
    pub(in crate::query) fn qualifies_model_column(&self, column: &str) -> bool {
        !self.joins.is_empty() && M::column_names().contains(&column)
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

    /// Render a projected column reference; an unqualified one is qualified
    /// with the model's own table.
    fn format_projection_column(
        &self,
        db_type: DatabaseType,
        table: &str,
        identifier: &str,
    ) -> String {
        if identifier.contains('.') {
            return self.format_column_for_db(db_type, identifier);
        }

        format!(
            "{}.{}",
            db_sql::quote_ident(db_type, table),
            db_sql::quote_ident(
                db_type,
                M::canonical_column_name(identifier).unwrap_or(identifier)
            )
        )
    }

    fn format_select_column_for_db(
        &self,
        db_type: DatabaseType,
        table: &str,
        column: &str,
    ) -> String {
        let trimmed = column.trim();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let is_reference =
            |value: &str| db_sql::format_identifier_reference(db_type, value).is_some();

        match parts.as_slice() {
            [identifier] if is_reference(identifier) => {
                self.format_projection_column(db_type, table, identifier)
            }
            [identifier, as_keyword, alias]
                if as_keyword.eq_ignore_ascii_case("as")
                    && is_reference(identifier)
                    && is_reference(alias) =>
            {
                format!(
                    "{} AS {}",
                    self.format_projection_column(db_type, table, identifier),
                    db_sql::quote_ident(db_type, alias)
                )
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
    ///
    /// The projection precedes every other clause, so the subqueries' values
    /// are the first ones bound.
    fn build_select_clause_sql(&self, db_type: DatabaseType, params: &mut Vec<Value>) -> String {
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

        for subquery in &self.subquery_select_expressions {
            let query_sql =
                Self::rebase_operand_placeholders(db_type, &subquery.query_sql, params.len());
            params.extend(subquery.params.iter().cloned());
            expressions.push(format!(
                "({}) AS {}",
                query_sql,
                db_sql::quote_ident(db_type, &subquery.alias)
            ));
        }

        // The default projection names the model's columns, so a column a
        // migration adds cannot change the shape of an already-prepared
        // statement. A raw UNION arm's shape is unknown, typically `SELECT *`,
        // so a query with one keeps `table.*` to stay aligned with it.
        if expressions.is_empty() {
            if self.unions.iter().any(|union| union.raw) {
                expressions.push(format!("{}.*", db_sql::quote_ident(db_type, table)));
            } else {
                expressions.push(db_sql::model_columns_sql::<M>(db_type, Some(table)));
            }
        }

        // Window columns are Rust field names or database columns, as in every
        // other slot, so they are rendered the way the query renders those.
        let canonical = |column: &str| self.canonical_model_identifier(column).into_owned();
        for window_function in &self.window_functions {
            expressions.push(
                window_function
                    .map_columns(&canonical)
                    .to_sql_for_db(db_type),
            );
        }

        let keyword = if self.is_distinct() {
            "SELECT DISTINCT"
        } else {
            "SELECT"
        };

        format!("{} {} ", keyword, expressions.join(", "))
    }

    pub(crate) fn append_from_and_join_sql(&self, sql: &mut String, db_type: DatabaseType) {
        sql.push_str("FROM ");
        db_sql::push_quoted_table::<M>(sql, db_type);
        sql.push(' ');

        for join in &self.joins {
            // Validated as `table` or `schema.table`, so each part is quoted.
            let table = db_sql::format_identifier_reference(db_type, &join.table)
                .unwrap_or_else(|| db_sql::quote_ident(db_type, &join.table));
            let join_table = if let Some(alias) = &join.alias {
                format!("{} AS {}", table, db_sql::quote_ident(db_type, alias))
            } else {
                table
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

    /// Replace each `?` of a HAVING template with the backend's own marker,
    /// numbering from `next_index`.
    ///
    /// Validation guarantees the template carries one `?` per bound value, so a
    /// template without values is emitted as written.
    fn render_having_placeholders(
        template: &str,
        bound: usize,
        db_type: DatabaseType,
        next_index: &mut usize,
    ) -> String {
        if bound == 0 {
            return template.to_string();
        }

        db_sql::map_template_placeholders(template, || {
            let placeholder = db_sql::placeholder(db_type, *next_index);
            *next_index += 1;
            placeholder
        })
    }

    fn append_group_by_and_having_sql(
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
            let mut next_index = params.len() + 1;
            let clauses: Vec<String> = self
                .having_clauses()
                .map(|(template, bindings)| {
                    let clause = Self::render_having_placeholders(
                        template,
                        bindings.len(),
                        db_type,
                        &mut next_index,
                    );
                    params.extend(bindings.iter().cloned());
                    clause
                })
                .collect();

            sql.push_str(&format!("HAVING {} ", clauses.join(" AND ")));
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
    /// Statements are assembled by concatenating SQL strings rather than through
    /// sea-query, so nothing renumbers placeholders here. Each operand was
    /// rendered on its own starting at `$1`; on Postgres it has to be shifted
    /// past every value already bound ahead of it. MySQL and SQLite use bare `?`
    /// markers that only depend on the order values are pushed, so their SQL is
    /// spliced in unchanged.
    fn rebase_operand_placeholders(db_type: DatabaseType, sql: &str, offset: usize) -> String {
        match db_type {
            DatabaseType::Postgres => db_sql::offset_postgres_placeholders(sql, offset),
            DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => sql.to_string(),
        }
    }

    fn append_cte_sql(&self, sql: &mut String, db_type: DatabaseType, params: &mut Vec<Value>) {
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

    fn append_union_sql(&self, sql: &mut String, db_type: DatabaseType, params: &mut Vec<Value>) {
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

    /// `ORDER BY`, then the limit written into the SQL and the offset bound.
    ///
    /// A literal limit keeps one statement per page size, and a bound offset
    /// lets every page reuse it instead of preparing a statement per page; a
    /// bound limit would make SQLite 3.50+ recompile it on every run. An
    /// offset past `i64::MAX` stays literal for the database to reject.
    fn append_order_limit_offset_sql(
        &self,
        sql: &mut String,
        db_type: DatabaseType,
        params: &mut Vec<Value>,
    ) {
        if !self.order_by.is_empty() {
            let order_parts: Vec<String> = self
                .order_by
                .iter()
                .map(|(column, direction)| self.format_order_by_for_db(db_type, column, *direction))
                .collect();
            sql.push_str(&format!(" ORDER BY {}", order_parts.join(", ")));
        }

        match (self.limit_value, self.offset_value) {
            (Some(limit), _) => sql.push_str(&format!(" LIMIT {}", limit)),
            // MySQL, MariaDB, and SQLite have no bare-OFFSET syntax: `OFFSET n`
            // without a preceding LIMIT is a parse error. Supply the dialect's
            // open-ended limit so a standalone `offset()` stays portable.
            (None, Some(_)) => match db_type {
                DatabaseType::Postgres => {}
                DatabaseType::SQLite => sql.push_str(" LIMIT -1"),
                DatabaseType::MySQL | DatabaseType::MariaDB => {
                    sql.push_str(&format!(" LIMIT {}", u64::MAX))
                }
            },
            (None, None) => {}
        }
        if let Some(offset) = self.offset_value {
            let offset = match i64::try_from(offset) {
                Ok(offset) => {
                    crate::internal::push_param(db_type, params, Value::BigInt(Some(offset)))
                }
                Err(_) => offset.to_string(),
            };
            sql.push_str(&format!(" OFFSET {}", offset));
        }
    }

    /// The select core: projection, FROM/JOIN, WHERE, GROUP BY and HAVING, with
    /// the values bound to each in placeholder order.
    pub(crate) fn build_base_select_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        let mut params = Vec::new();
        let mut sql = self.build_select_clause_sql(db_type, &mut params);
        self.append_from_and_join_sql(&mut sql, db_type);

        let (where_sql, where_params) = self.build_where_clause_with_condition_for_db(db_type);
        if !where_sql.is_empty() {
            sql.push_str(&format!(
                "WHERE {} ",
                Self::rebase_operand_placeholders(db_type, &where_sql, params.len())
            ));
        }
        params.extend(where_params);

        self.append_group_by_and_having_sql(&mut sql, db_type, &mut params);
        (sql.trim().to_string(), params)
    }

    /// This query as an operand of another's `UNION`, with its own ordering,
    /// limit and offset. PostgreSQL and MySQL parenthesize an operand, which may
    /// then carry them; SQLite takes a bare select there, so an ordered or
    /// limited operand is read through a derived table. An operand with unions
    /// or CTEs of its own is read whole through a derived table on every
    /// backend, so none of its branches and none of its CTEs are lost.
    pub(crate) fn build_compound_operand_sql_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        if !self.unions.is_empty() || !self.ctes.is_empty() {
            let (sql, params) = self.build_select_sql_with_params_for_db(db_type);
            return (
                format!(
                    "SELECT * FROM ({}) AS {}",
                    sql,
                    db_sql::quote_ident(db_type, "tideorm_union_operand")
                ),
                params,
            );
        }

        let (mut sql, mut params) = self.build_base_select_sql_with_params_for_db(db_type);
        if self.order_by.is_empty() && self.limit_value.is_none() && self.offset_value.is_none() {
            return (sql, params);
        }
        self.append_order_limit_offset_sql(&mut sql, db_type, &mut params);
        if !Self::wraps_compound_operand(db_type) {
            sql = format!(
                "SELECT * FROM ({}) AS {}",
                sql,
                db_sql::quote_ident(db_type, "tideorm_union_operand")
            );
        }
        (sql, params)
    }

    /// Splice the CTE prefix and compound-select operands around the base select,
    /// keeping bound values in placeholder order.
    ///
    /// Placeholders appear left to right in exactly three groups: the `WITH`
    /// prefix precedes the base select, and every union operand follows it. The
    /// values are pushed in that same order — CTE bodies in declaration order,
    /// then the base select's own values, then union operands in order — because
    /// backends bind purely by position.
    pub(crate) fn build_select_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        let (base_sql, base_params) = self.build_base_select_sql_with_params_for_db(db_type);

        let mut sql = String::new();
        let mut params: Vec<Value> = Vec::new();

        self.append_cte_sql(&mut sql, db_type, &mut params);
        sql.push_str(&Self::rebase_operand_placeholders(
            db_type,
            &base_sql,
            params.len(),
        ));
        params.extend(base_params);
        self.append_union_sql(&mut sql, db_type, &mut params);
        self.append_order_limit_offset_sql(&mut sql, db_type, &mut params);
        // SQLite has no row locks: a transaction's first write locks the whole
        // database, so a competing read-check-write fails instead of racing.
        if self.lock_for_update && db_type != DatabaseType::SQLite {
            sql.push_str(" FOR UPDATE");
        }

        (sql.trim().to_string(), params)
    }

    pub(crate) fn build_select_sql_with_params(&self) -> (String, Vec<Value>) {
        self.build_select_sql_with_params_for_db(self.db_type_for_sql())
    }

    /// The statement [`get()`](Self::get) runs, with its bound values inlined.
    ///
    /// For display only: it backs [`build_sql_preview()`](Self::build_sql_preview).
    pub(crate) fn build_select_sql_for_db(&self, db_type: DatabaseType) -> String {
        let (sql, params) = self.build_select_sql_with_params_for_db(db_type);
        db_sql::inline_parameters(db_type, &sql, &params)
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/query_select_rendering_tests.rs"]
mod tests;
