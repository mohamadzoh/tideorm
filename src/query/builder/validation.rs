//! The checks a query's clauses pass before it runs.

use super::*;

impl<M: Model> QueryBuilder<M> {
    /// Normalize a split column reference to the `table.column` form the
    /// distinct-projection comparison uses.
    ///
    /// An unqualified reference belongs to the model's own table, and a
    /// reference into that table is resolved from its Rust field name to its
    /// database column name so the two spellings compare equal.
    pub(super) fn normalized_column_reference(qualifier: &str, column: &str) -> String {
        let table = M::table_name();
        let qualifier = if qualifier.is_empty() {
            table
        } else {
            qualifier
        };
        let column = if qualifier == table {
            M::column_named(column)
        } else {
            column
        };

        format!("{}.{}", qualifier, column)
    }

    /// The column references a `SELECT DISTINCT` projection exposes, or `None`
    /// when the projection carries an expression this cannot analyse.
    ///
    /// Each SELECT alias is recorded under its bare name as well, because
    /// `ORDER BY <alias>` resolves against the projection rather than the table.
    pub(super) fn distinct_projection_references(&self) -> Option<BTreeSet<String>> {
        // A raw expression or a scalar subquery can project anything at all,
        // including the very column an ORDER BY term names, so once one is
        // present nothing can be proven missing.
        let has_opaque_projection = self.raw_projection().next().is_some()
            || !self.clauses.subquery_select_expressions.is_empty();

        if has_opaque_projection {
            return None;
        }

        let table = M::table_name();
        let mut references = BTreeSet::new();

        // An absent or empty column list renders as the `table.*` fallback,
        // which exposes every column of the model.
        let Some(columns) = self
            .clauses
            .select_columns
            .as_ref()
            .filter(|columns| !columns.is_empty())
        else {
            for column in M::column_names() {
                references.insert(format!("{}.{}", table, column));
            }
            return Some(references);
        };

        for column in columns {
            let (expression, alias) = Self::split_alias(column);

            if let Some(alias) = alias {
                references.insert(alias.to_string());
            }

            let (qualifier, name) = Self::simple_column_reference(expression)?;
            references.insert(Self::normalized_column_reference(qualifier, name));
        }

        Some(references)
    }

    /// Reject an ORDER BY term a `SELECT DISTINCT` cannot sort by.
    ///
    /// PostgreSQL requires every ORDER BY expression of a `SELECT DISTINCT` to
    /// appear in the select list and otherwise fails the statement with
    /// `for SELECT DISTINCT, ORDER BY expressions must appear in select list`.
    /// Checking it here turns that into a builder error that names the offending
    /// column on every backend, instead of an opaque server error on one of
    /// them. Terms added through `order_by_raw()` are trusted opaque SQL and
    /// stay the caller's responsibility, as does an ORDER BY on a query whose
    /// projection contains a raw expression.
    pub(super) fn validate_distinct_order_by(&self) -> std::result::Result<(), String> {
        let Some(projection) = self.distinct_projection_references() else {
            return Ok(());
        };

        for (column, _) in &self.clauses.order_by {
            if raw_order_by_expression(column).is_some() {
                continue;
            }

            let Some((qualifier, name)) = Self::simple_column_reference(order_reference(column))
            else {
                continue;
            };

            if projection.contains(&Self::normalized_column_reference(qualifier, name))
                || projection.contains(name)
            {
                continue;
            }

            return Err(format!(
                "ORDER BY '{}' is not part of the distinct() projection of model '{}'; a SELECT DISTINCT can only be ordered by expressions it selects, so add the column to select() or drop distinct()",
                column.trim(),
                M::table_name()
            ));
        }

        Ok(())
    }

    pub(super) fn validate_condition(
        condition: &WhereCondition,
        known_qualifiers: Option<&BTreeSet<String>>,
    ) -> std::result::Result<(), String> {
        match (&condition.operator, &condition.value) {
            (Operator::Raw, ConditionValue::RawExpr(raw_sql)) => {
                let kind = if condition.column.is_empty() {
                    "WHERE raw SQL"
                } else {
                    "WHERE raw column expression"
                };
                db_sql::validate_raw_sql_fragment(kind, raw_sql)
            }
            (Operator::Raw, ConditionValue::RawTemplate { sql, values }) => {
                db_sql::validate_raw_sql_fragment("WHERE raw SQL", sql)?;
                Self::validate_template("WHERE raw SQL", sql, values)
            }
            _ => Ok(()),
        }?;

        // The column slot is rendered as SQL, so it is an allowlist like ORDER BY:
        // anything but a plain or table-qualified column would be spliced in
        // verbatim, and `1=1 OR name` turns `where_eq(.., x)` into a filter that
        // matches every row.
        if !condition.column.is_empty() {
            if db_sql::validate_identifier_reference("WHERE column", &condition.column).is_err() {
                return Err(format!(
                    "unsafe WHERE column '{}': expected a column or table.column reference; use where_raw() for trusted SQL expressions and never pass user input to it",
                    condition.column
                ));
            }
            Self::validate_model_column_reference(
                "WHERE column",
                &condition.column,
                known_qualifiers,
            )?;
        }

        // The other column of a column comparison is rendered the same way.
        if let ConditionValue::Column(other) = &condition.value {
            if db_sql::validate_identifier_reference("WHERE column", other).is_err() {
                return Err(format!(
                    "unsafe WHERE comparison column '{}': expected a column or table.column reference",
                    other
                ));
            }
            Self::validate_model_column_reference("WHERE column", other, known_qualifiers)?;
        }

        Ok(())
    }

    /// Refuse a raw template whose `?` placeholders and values do not pair up,
    /// which would shift PostgreSQL's `$n` numbering or leave a marker unbound.
    fn validate_template(
        kind: &str,
        sql: &str,
        values: &[crate::internal::Value],
    ) -> std::result::Result<(), String> {
        let placeholders = db_sql::count_template_placeholders(sql);
        if placeholders == values.len() {
            return Ok(());
        }
        Err(format!(
            "{kind} '{sql}' has {placeholders} placeholder(s) but {} bound value(s)",
            values.len()
        ))
    }

    pub(in crate::query) fn validate_window_function(
        window_function: &WindowFunction,
        known_qualifiers: Option<&BTreeSet<String>>,
    ) -> std::result::Result<(), String> {
        db_sql::validate_identifier("window alias", &window_function.alias)?;

        for column in &window_function.partition_by {
            Self::validate_model_column_reference(
                "window PARTITION BY column",
                column,
                known_qualifiers,
            )?;
        }

        for (column, _) in &window_function.order_by {
            Self::validate_model_column_reference(
                "window ORDER BY column",
                column,
                known_qualifiers,
            )?;
        }

        let function = &window_function.function;
        if let Some(column) = function.column() {
            Self::validate_model_column_reference(
                "window function column",
                column,
                known_qualifiers,
            )?;
        }
        match function {
            WindowFunctionType::Lag(_, _, Some(default))
            | WindowFunctionType::Lead(_, _, Some(default)) => {
                db_sql::validate_raw_sql_fragment("LAG/LEAD default expression", default)?;
            }
            WindowFunctionType::Custom(expression) => {
                db_sql::validate_raw_sql_fragment("window function expression", expression)?;
            }
            _ => {}
        }

        Ok(())
    }

    /// Fail this query when `operand`, a query it embeds through `method`,
    /// could not run on its own; `what` names the operand in the message. The
    /// tables `operand` reads become this query's cache dependencies too.
    pub(in crate::query) fn absorb_operand_error<N: Model>(
        &mut self,
        what: &str,
        method: &str,
        operand: &QueryBuilder<N>,
    ) {
        self.clauses.dependencies.extend(operand.cache_tables());
        self.clauses
            .operand_databases
            .extend(operand.database.iter().cloned());
        self.clauses
            .operand_databases
            .extend(operand.clauses.operand_databases.iter().cloned());
        if let Err(err) = operand.ensure_query_is_executable() {
            self.invalidate_query(format!("invalid {what} for {method}(): {err}"));
        }
    }

    pub(in crate::query) fn validate_cte_clause(cte: &CTE) -> std::result::Result<(), String> {
        db_sql::validate_identifier("CTE name", &cte.name)?;

        if let Some(columns) = &cte.columns {
            for column in columns {
                db_sql::validate_identifier("CTE column", column)?;
            }
        }

        // A CTE's body is parenthesized, so a UNION inside it stays inside it.
        match cte.query {
            Some(_) => Ok(()),
            None => db_sql::validate_compound_subquery_sql(&cte.query_sql),
        }
    }

    /// Refuse an embedded query (subquery, CTE, union operand) bound to another
    /// database or transaction than this one: it renders into this statement,
    /// so it would silently run here instead.
    pub(super) fn ensure_operands_share_connection(&self) -> Result<()> {
        if self.clauses.operand_databases.is_empty() {
            return Ok(());
        }
        let outer = self.current_db()?.__get_connection()?;
        for database in &self.clauses.operand_databases {
            if !database.__get_connection()?.is_same(&outer) {
                return Err(Error::query(
                    "query operand is bound to a different database or transaction",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn validate_query_fragments(&self) -> Result<()> {
        self.ensure_operands_share_connection()?;
        let qualifiers = self.known_qualifiers();
        let qualifiers = Some(&qualifiers);

        for condition in self.all_conditions() {
            Self::validate_condition(condition, qualifiers).map_err(Error::query)?;
        }

        for (column, _) in &self.clauses.order_by {
            if raw_order_by_expression(column).is_none() {
                self.ensure_encrypted_input_is_comparable(order_reference(column), "ORDER BY")?;
            }
            Self::validate_order_or_group_value("ORDER BY column", column, true, qualifiers)
                .map_err(Error::query)?;
        }

        for column in &self.clauses.group_by {
            self.ensure_encrypted_input_is_comparable(column, "GROUP BY")?;
            Self::validate_order_or_group_value("GROUP BY column", column, false, qualifiers)
                .map_err(Error::query)?;
        }

        for (column, operation) in &self.clauses.having_comparisons {
            self.ensure_encrypted_input_is_comparable(column, operation)?;
        }
        for clause in &self.clauses.having_conditions {
            let crate::query::HavingClause::Raw {
                sql: having,
                values: bindings,
            } = clause
            else {
                continue;
            };
            db_sql::validate_having_sql_fragment("HAVING raw SQL", having).map_err(Error::query)?;
            Self::validate_template("HAVING raw SQL", having, bindings).map_err(Error::query)?;
        }

        if let Some(columns) = &self.clauses.select_columns {
            for column in columns {
                Self::validate_select_value(column, qualifiers).map_err(Error::query)?;
            }
        }
        self.validate_projection_names().map_err(Error::query)?;

        if self.is_distinct() {
            self.ensure_distinct_projection_is_comparable()?;
            self.validate_distinct_order_by().map_err(Error::query)?;
        }

        // A raw arm's SQL was checked when it was added, under its method's name.
        for union in &self.clauses.unions {
            if let crate::query::Operand::Sql {
                sql,
                values: Some(values),
            } = &union.operand
            {
                Self::validate_template("UNION raw SQL", sql, values).map_err(Error::query)?;
            }
        }
        self.ensure_union_projections_are_valid()?;

        for window_function in &self.clauses.window_functions {
            Self::validate_window_function(window_function, qualifiers).map_err(Error::query)?;
            // Window inputs refer to source columns, not SELECT aliases.
            let comparable = |column: &str, operation: &str| match Self::column_source(column) {
                Some((table, column)) => {
                    self.ensure_encrypted_source_is_comparable(&table, &column, operation)
                }
                None => Ok(()),
            };
            match &window_function.function {
                WindowFunctionType::Min(column) | WindowFunctionType::Max(column) => {
                    comparable(column, "window MIN/MAX")?
                }
                WindowFunctionType::Sum(column) | WindowFunctionType::Avg(column) => {
                    comparable(column, "window SUM/AVG")?
                }
                _ => {}
            }
            for column in &window_function.partition_by {
                comparable(column, "window PARTITION BY")?;
            }
            for (column, _) in &window_function.order_by {
                comparable(column, "window ORDER BY")?;
            }
        }

        for cte in &self.clauses.ctes {
            Self::validate_cte_clause(cte).map_err(Error::query)?;
        }

        Ok(())
    }
}
