use super::*;
use crate::orm::sql_builder::{
    CommonTableExpression, LockType, OrderedStatement, SelectExpr, SelectExprTrait, WithClause,
};

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
        self.model_identifier(identifier, !self.clauses.joins.is_empty())
    }

    /// [`canonical_model_identifier`](Self::canonical_model_identifier), with
    /// an unqualified model column qualified whenever `qualify_own` is set.
    pub(in crate::query) fn model_identifier<'a>(
        &self,
        identifier: &'a str,
        qualify_own: bool,
    ) -> std::borrow::Cow<'a, str> {
        match M::canonical_column_parts(identifier) {
            (Some(table), column) if table == M::table_name() => {
                std::borrow::Cow::Owned(format!("{}.{}", self.own_table_ref(), column))
            }
            (Some(table), column) => std::borrow::Cow::Owned(format!("{}.{}", table, column)),
            (None, column) if qualify_own && M::column_names().contains(&column) => {
                std::borrow::Cow::Owned(format!("{}.{}", self.own_table_ref(), column))
            }
            (None, column) => std::borrow::Cow::Borrowed(column),
        }
    }

    /// The name the model's table goes by in this query: its own alias inside
    /// a `where_has` over the same table, else the table's name.
    pub(in crate::query) fn own_table_ref(&self) -> std::borrow::Cow<'static, str> {
        match self.self_join_depth {
            0 => std::borrow::Cow::Borrowed(M::table_name()),
            depth => std::borrow::Cow::Owned(crate::query::predicates::related_alias(depth)),
        }
    }

    /// Whether an unqualified reference to `column`, a database column name,
    /// is written with the model's table: when it is one of the model's
    /// columns and the query joins another table.
    pub(in crate::query) fn qualifies_model_column(&self, column: &str) -> bool {
        !self.clauses.joins.is_empty() && M::column_names().contains(&column)
    }

    /// Render a column reference a condition, a join or an ordering names:
    /// canonicalized and quoted part by part. Validation leaves nothing else
    /// in these slots, so anything else is written as it is.
    pub(crate) fn format_column_for_db(&self, db_type: DatabaseType, column: &str) -> String {
        let trimmed = column.trim();
        if trimmed.contains(char::is_whitespace) {
            return trimmed.to_string();
        }
        db_sql::format_column_or_trusted_expression(
            db_type,
            self.canonical_model_identifier(trimmed).as_ref(),
        )
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
            db_sql::quote_ident(db_type, M::column_named(identifier))
        )
    }

    fn format_select_column_for_db(
        &self,
        db_type: DatabaseType,
        table: &str,
        column: &str,
    ) -> String {
        let is_reference =
            |value: &str| db_sql::format_identifier_reference(db_type, value).is_some();

        match Self::split_alias(column) {
            (identifier, None) if is_reference(identifier) => {
                self.format_projection_column(db_type, table, identifier)
            }
            (identifier, Some(alias)) if is_reference(identifier) && is_reference(alias) => {
                format!(
                    "{} AS {}",
                    self.format_projection_column(db_type, table, identifier),
                    db_sql::quote_ident(db_type, alias)
                )
            }
            _ => column.trim().to_string(),
        }
    }

    /// Add the projection from every source that contributed one.
    ///
    /// The four sources compose in a fixed order — the typed columns of
    /// `select()`, the raw expressions of `select_raw()`, the scalar subqueries
    /// of `select_subquery()`, then the window functions of `window()` — so a
    /// query that mixes them keeps every requested output column instead of
    /// silently dropping the typed half. The default projection (the model's
    /// columns, or `table.*` beside a raw union arm) is only supplied when
    /// nothing but window functions was selected, because it is a default
    /// rather than a contribution.
    ///
    /// `distinct()` records itself as a sentinel among the raw expressions
    /// because it modifies the projection as a whole. The sentinel is dropped
    /// here and becomes the statement's `DISTINCT`, so it never reaches SQL
    /// and never counts as a contribution.
    fn project(&self, select: &mut SelectStatement, db_type: DatabaseType) -> Result<()> {
        let table = M::table_name();
        let mut outputs: Vec<SelectExpr> = self
            .clauses
            .select_columns
            .iter()
            .flatten()
            .map(|column| self.format_select_column_for_db(db_type, table, column))
            .chain(self.raw_projection().cloned())
            .map(|expression| Expr::cust(expression).into())
            .collect();

        for subquery in &self.clauses.subquery_select_expressions {
            let query = subquery.query.select(db_type)?;
            outputs
                .push(Expr::SubQuery(None, Box::new(query.into())).alias(subquery.alias.clone()));
        }

        // The default projection names the model's columns, so a column a
        // migration adds cannot change the shape of an already-prepared
        // statement. A raw UNION arm's shape is unknown, typically `SELECT *`,
        // so a query with one keeps `table.*` to stay aligned with it.
        if outputs.is_empty() {
            outputs.push(
                Expr::cust(if self.reads_table_wildcard() {
                    format!("{}.*", db_sql::quote_ident(db_type, table))
                } else {
                    db_sql::model_columns_sql::<M>(db_type, Some(table))
                })
                .into(),
            );
        }

        // Window columns are Rust field names or database columns, as in every
        // other slot, so they are rendered the way the query renders those.
        let canonical = |column: &str| self.canonical_model_identifier(column).into_owned();
        for window_function in &self.clauses.window_functions {
            if window_function.frame_type == Some(crate::query::FrameType::Groups)
                && window_function.frame_start.is_some()
                && !db_type.dialect().groups_frames()
            {
                return Err(Error::query(format!(
                    "window '{}' is framed by GROUPS, which {:?} does not support; use ROWS or RANGE",
                    window_function.alias, db_type
                )));
            }
            outputs.push(
                Expr::cust(
                    window_function
                        .map_columns(&canonical)
                        .to_sql_for_db(db_type),
                )
                .into(),
            );
        }

        if self.is_distinct() {
            select.distinct();
        }
        select.exprs(outputs);
        Ok(())
    }

    /// Add the query's table, each join's `<JOIN> <table> ON <a> = <b>`, and
    /// the `WHERE` filter.
    pub(super) fn add_tables_and_filter(
        &self,
        select: &mut SelectStatement,
        db_type: DatabaseType,
    ) -> Result<()> {
        select.from(db_sql::table_name(M::schema_name(), M::table_name()));

        for join in &self.clauses.joins {
            // Validated as `table` or `schema.table`.
            let (schema, table) = crate::query::builder::schema_and_table(&join.table);
            let table = db_sql::table_name(schema, table);
            let on = Expr::cust(format!(
                "{} = {}",
                self.format_column_for_db(db_type, &join.left_column),
                self.format_column_for_db(db_type, &join.right_column)
            ));
            match &join.alias {
                Some(alias) => {
                    select.join_as(join.join_type.to_builder(), table, alias.clone(), on)
                }
                None => select.join(join.join_type.to_builder(), table, on),
            };
        }

        let filter = self.where_bound(db_type)?;
        if !filter.is_empty() {
            select.and_where(filter.into());
        }
        Ok(())
    }

    fn group_by_and_having(&self, select: &mut SelectStatement, db_type: DatabaseType) {
        select.add_group_by(
            self.clauses
                .group_by
                .iter()
                .map(|column| Expr::cust(self.format_column_for_db(db_type, column))),
        );

        if !self.clauses.having_conditions.is_empty() {
            let clauses = self
                .clauses
                .having_conditions
                .iter()
                .map(|clause| self.having_bound(db_type, clause));
            select.and_having(BoundSql::join(clauses, " AND ").into());
        }
    }

    /// The query's CTEs, `RECURSIVE` when one refers to itself, or `None`
    /// without one.
    fn with_clause(&self, db_type: DatabaseType) -> Result<Option<WithClause>> {
        if self.clauses.ctes.is_empty() {
            return Ok(None);
        }

        let mut with = WithClause::new();
        with.recursive(self.clauses.ctes.iter().any(|cte| cte.recursive));
        for cte in &self.clauses.ctes {
            let body = match &cte.query {
                Some(query) => query.select(db_type)?,
                None => SelectStatement::custom(BoundSql::sql(cte.query_sql.as_str())),
            };
            let mut table = CommonTableExpression::new();
            table.table_name(cte.name.clone()).query(body);
            if let Some(columns) = &cte.columns {
                table.columns(columns.iter().cloned());
            }
            with.cte(table);
        }
        Ok(Some(with))
    }

    /// Order `statement` as the query is ordered.
    pub(crate) fn add_ordering(
        &self,
        statement: &mut impl OrderedStatement,
        db_type: DatabaseType,
    ) {
        for (column, direction) in &self.clauses.order_by {
            let (expression, direction) = self.order_term(db_type, column, *direction);
            statement.order_by_expr(Expr::cust(expression), direction.to_builder());
        }
    }

    /// A single ORDER BY term: its expression and direction.
    ///
    /// Entries created by `order_by_raw()` carry the raw-expression marker and
    /// are emitted verbatim; everything else has already been restricted to a
    /// resolvable column reference by validation. A column that carries its own
    /// `ASC`/`DESC` suffix keeps it, because appending the tuple direction on top
    /// would render `"name" DESC ASC`.
    pub(in crate::query) fn order_term(
        &self,
        db_type: DatabaseType,
        column: &str,
        direction: Order,
    ) -> (String, Order) {
        if let Some(expression) = crate::query::builder::raw_order_by_expression(column) {
            return (expression.to_string(), direction);
        }

        let trimmed = column.trim();
        let (reference, direction) =
            crate::query::builder::split_direction(trimmed).unwrap_or((trimmed, direction));
        // A union's ORDER BY sorts its result rows, which name a column by its
        // output name alone: `ORDER BY "users"."name"` is refused there.
        if !self.clauses.unions.is_empty()
            && reference
                .split('.')
                .all(crate::internal::sql_safety::is_safe_identifier_segment)
        {
            let output = self
                .derived_output_name(reference)
                .unwrap_or_else(|| M::canonical_column_parts(reference).1.to_string());
            return (db_sql::quote_ident(db_type, &output), direction);
        }

        (self.format_column_for_db(db_type, reference), direction)
    }

    /// Everything but the row lock: the CTEs, the select core (projection,
    /// FROM and joins, WHERE, GROUP BY and HAVING), every compound operand,
    /// then the ordering, the limit and the offset.
    ///
    /// The builder writes the limit into the SQL and binds the offset: a
    /// literal limit keeps one statement per page size, and a bound offset lets
    /// every page reuse it instead of preparing a statement per page; a bound
    /// limit would make SQLite 3.50+ recompile it on every run.
    fn unlocked_statement(&self, db_type: DatabaseType) -> Result<SelectStatement> {
        let with = self.with_clause(db_type)?;
        let mut select = SelectStatement::new();
        self.project(&mut select, db_type)?;
        self.add_tables_and_filter(&mut select, db_type)?;
        self.group_by_and_having(&mut select, db_type);
        if let Some(with) = with {
            select.with_cte(with);
        }

        for union in &self.clauses.unions {
            let operand = match &union.operand {
                Operand::Query(query) => query.compound_operand(db_type)?,
                Operand::Sql { sql, values } => SelectStatement::custom(db_sql::template_bound(
                    sql,
                    values.clone().unwrap_or_default(),
                )),
            };
            select.union(union.union_type.to_builder(), operand);
        }

        self.add_ordering(&mut select, db_type);
        if let Some(limit) = self.clauses.limit_value {
            select.limit(limit);
        }
        if let Some(offset) = self.clauses.offset_value {
            select.offset(offset);
        }
        Ok(select)
    }

    /// This query as an operand of another's `UNION`, with its own ordering,
    /// limit and offset. An operand with unions or CTEs of its own is read
    /// whole through a derived table, so none of its branches and none of its
    /// CTEs are lost.
    pub(in crate::query) fn compound_operand_statement(
        &self,
        db_type: DatabaseType,
    ) -> Result<SelectStatement> {
        if self.clauses.unions.is_empty() && self.clauses.ctes.is_empty() {
            return self.unlocked_statement(db_type);
        }
        Ok(db_sql::select_from_derived(
            "*",
            self.select_statement(db_type)?,
            "tideorm_union_operand",
        ))
    }

    /// The whole statement [`get()`](Self::get) runs.
    pub(in crate::query) fn select_statement(
        &self,
        db_type: DatabaseType,
    ) -> Result<SelectStatement> {
        let mut select = self.unlocked_statement(db_type)?;
        if self.clauses.lock_for_update {
            select.lock(LockType::Update);
        }
        Ok(select)
    }

    /// [`select_statement`](Self::select_statement), rendered.
    pub(crate) fn build_select_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
    ) -> Result<(String, Vec<Value>)> {
        db_sql::built(db_type, &self.select_statement(db_type)?)
    }

    pub(crate) fn build_select_sql_with_params(&self) -> Result<(String, Vec<Value>)> {
        self.build_select_sql_with_params_for_db(self.db_type_for_sql()?)
    }

    /// The statement [`get()`](Self::get) runs, with its bound values inlined.
    ///
    /// For display only: it backs [`build_sql_preview()`](Self::build_sql_preview).
    /// A query that cannot be rendered shows why, as an SQL comment.
    pub(crate) fn build_select_sql_for_db(&self, db_type: DatabaseType) -> String {
        match self.build_select_sql_with_params_for_db(db_type) {
            Ok((sql, params)) => db_sql::inline_parameters(db_type, &sql, &params),
            Err(error) => format!("-- {error}"),
        }
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/query_select_rendering_tests.rs"]
mod tests;
