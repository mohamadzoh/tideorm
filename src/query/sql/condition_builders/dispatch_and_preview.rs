use super::*;

/// Whether sea-query can re-bind values into `fragment` without losing a
/// placeholder.
///
/// `Expr::cust_with_values` re-tokenizes the fragment to locate its placeholder
/// markers, and that tokenizer treats `'`, `"`, `` ` `` and `[` as string
/// delimiters with `\` as an escape inside them. A marker hidden inside such a
/// region is skipped by the substitution pass, which would leave a stale marker
/// in the outer statement and silently drop its bound value.
///
/// Both arms are now defence in depth: nothing the builder emits contains either
/// character. The array operators render bracket-free `= ANY(col)` / `unnest(..)`
/// forms, `eq_any`/`ne_all` go through sea-query's own `is_in`, and generated SQL
/// is backslash-free because the LIKE escape character is
/// [`crate::columns::LIKE_ESCAPE_CLAUSE`] (`ESCAPE '!'`) rather than `\`.
///
/// Both of those were live bugs first. `ARRAY[$1, $2]` silently dropped its values
/// and left the markers pointing at whatever else the statement had bound, and
/// `ESCAPE ''` swallowed every later marker in the fragment. Keeping the check
/// means a future construct that reintroduces either character degrades to
/// correct-but-inline SQL instead of binding the wrong values.
///
/// The backslash arm is defence in depth. It used to fire on every LIKE, because
/// the escape clause was `ESCAPE '\'` and the trailing quote read as escaped;
/// generated SQL now uses [`crate::columns::LIKE_ESCAPE_CLAUSE`] (`ESCAPE '!'`)
/// and is backslash-free, so `where_contains` and friends stay parameterized
/// inside a subquery. Keeping the check means any future construct that
/// reintroduces a backslash degrades to correct-but-inline SQL instead of
/// producing a statement whose placeholders no longer line up.
fn fragment_placeholders_survive_rebinding(fragment: &str) -> bool {
    !fragment.contains('\\') && !fragment.contains('[')
}

#[allow(missing_docs)]
impl<M: Model> QueryBuilder<M> {
    pub(crate) fn build_subquery_expression(
        &self,
        column_sql: &str,
        negated: bool,
        query_sql: &str,
    ) -> SimpleExpr {
        Expr::cust(format!(
            "{} {}IN ({})",
            column_sql,
            if negated { "NOT " } else { "" },
            query_sql
        ))
    }

    pub(crate) fn build_subquery_sql(
        &self,
        column: &str,
        negated: bool,
        query_sql: &str,
    ) -> String {
        format!(
            "{} {}IN ({})",
            column,
            if negated { "NOT " } else { "" },
            query_sql
        )
    }

    /// Build a raw condition that carries its own bound parameters.
    ///
    /// `raw_sql` already uses the backend's placeholder marker, so sea-query
    /// renumbers those tokens into the surrounding statement and binds `values`
    /// against them. This is the whole point of the variant: subquery, EXISTS
    /// and `has_related` operands stay parameterized instead of being rendered
    /// as hand-escaped inline literals.
    pub(crate) fn build_raw_condition_expression_with_values(
        &self,
        db_type: DatabaseType,
        column: &str,
        raw_sql: &str,
        values: Vec<Value>,
    ) -> SimpleExpr {
        let sql = if column.is_empty() {
            raw_sql.to_string()
        } else {
            format!("{} {}", self.format_column_for_db(db_type, column), raw_sql)
        };

        self.build_custom_expression(sql, values)
    }

    pub(crate) fn build_condition_expression(
        &self,
        condition: &WhereCondition,
        db_type: DatabaseType,
    ) -> Option<SimpleExpr> {
        // Intercepted ahead of `condition_spec` because a `ConditionSpec` can
        // only borrow SQL, never carry the bound values along with it.
        if let (
            Operator::Raw,
            ConditionValue::RawExprWithValues {
                sql,
                values,
                preview_sql,
            },
        ) = (&condition.operator, &condition.value)
        {
            let (raw_sql, values) =
                if values.is_empty() || fragment_placeholders_survive_rebinding(sql) {
                    (sql.as_str(), values.clone())
                } else {
                    (preview_sql.as_str(), Vec::new())
                };

            return Some(self.build_raw_condition_expression_with_values(
                db_type,
                &condition.column,
                raw_sql,
                values,
            ));
        }

        let spec = Self::condition_spec(condition)?;

        if let ConditionSpec::Raw { column, raw_sql } = spec {
            return Some(self.build_raw_condition_expression(db_type, column, raw_sql));
        }

        let column_expr = self.sea_column_expr(db_type, &condition.column);
        let column_sql = self.format_column_for_db(db_type, &condition.column);

        match spec {
            ConditionSpec::Raw { .. } => None,
            ConditionSpec::Compare { operator, value } => {
                Some(self.build_compare_expression(column_expr, operator, value))
            }
            ConditionSpec::Pattern {
                negated,
                escaped,
                value,
            } => Some(self.build_pattern_expression(
                db_type,
                column_expr,
                &column_sql,
                negated,
                escaped,
                value,
            )),
            ConditionSpec::List { operator, values } => {
                Some(self.build_list_expression(column_expr, operator, values))
            }
            ConditionSpec::NullCheck { negated } => {
                Some(self.build_null_check_expression(column_expr, negated))
            }
            ConditionSpec::Between { low, high } => {
                Some(self.build_between_expression(column_expr, low, high))
            }
            ConditionSpec::JsonValue { operator, value } => {
                Some(self.build_json_value_expression(db_type, &column_sql, operator, value))
            }
            ConditionSpec::JsonString { operator, value } => {
                Some(self.build_json_string_expression(db_type, &column_sql, operator, value))
            }
            ConditionSpec::Array { operator, values } => Some(self.build_array_expression(
                db_type,
                column_expr,
                &column_sql,
                operator,
                values,
            )),
            ConditionSpec::Subquery { negated, query_sql } => {
                Some(self.build_subquery_expression(&column_sql, negated, query_sql))
            }
        }
    }

    pub(crate) fn build_or_group_condition(
        &self,
        group: &OrGroup,
        db_type: DatabaseType,
    ) -> Condition {
        let mut condition = match group.combine_with {
            LogicalOp::And => Condition::all(),
            LogicalOp::Or => Condition::any(),
        };

        for filter in &group.conditions {
            if let Some(expression) = self.build_condition_expression(filter, db_type) {
                condition = condition.add(expression);
            }
        }

        for nested_group in &group.nested_groups {
            if !nested_group.is_empty() {
                condition = condition.add(self.build_or_group_condition(nested_group, db_type));
            }
        }

        condition
    }

    pub(crate) fn build_soft_delete_expression(&self, db_type: DatabaseType) -> Option<SimpleExpr> {
        match query_scope_for::<M>(self.include_trashed, self.only_trashed) {
            SoftDeleteScope::Disabled | SoftDeleteScope::WithTrashed => None,
            SoftDeleteScope::ActiveOnly => Some(
                self.sea_column_expr(db_type, M::deleted_at_column())
                    .is_null(),
            ),
            SoftDeleteScope::OnlyTrashed => Some(
                self.sea_column_expr(db_type, M::deleted_at_column())
                    .is_not_null(),
            ),
        }
    }

    pub(crate) fn build_where_clause_with_condition_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        let has_filters = !self.conditions.is_empty()
            || !self.or_groups.is_empty()
            || self.build_soft_delete_expression(db_type).is_some();
        if !has_filters {
            return (String::new(), Vec::new());
        }

        let mut query = Query::select();
        query.expr(Expr::cust("1"));
        query.cond_where(self.build_sea_condition_for_db(db_type));

        let (sql, values) = match db_type {
            DatabaseType::Postgres => query.build(PostgresQueryBuilder),
            DatabaseType::MySQL | DatabaseType::MariaDB => query.build(MysqlQueryBuilder),
            DatabaseType::SQLite => query.build(SqliteQueryBuilder),
        };

        match sql.split_once(" WHERE ") {
            Some((_, where_sql)) => (where_sql.to_string(), values.into_iter().collect()),
            None => (String::new(), Vec::new()),
        }
    }

    pub(crate) fn format_preview_value(
        &self,
        db_type: DatabaseType,
        value: &serde_json::Value,
    ) -> String {
        match value {
            serde_json::Value::Null => "NULL".to_string(),
            serde_json::Value::Bool(boolean) => boolean.to_string(),
            serde_json::Value::Number(number) => number.to_string(),
            // This is only for the non-executable debug preview path. It renders
            // approximate inline literals for humans and must never be reused for
            // executable SQL; real query paths stay parameterized instead.
            serde_json::Value::String(text) => format!(
                "'{}'",
                crate::internal::sql_safety::escape_sql_literal_for_db(db_type, text)
            ),
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                format!(
                    "'{}'",
                    crate::internal::sql_safety::escape_sql_literal_for_db(
                        db_type,
                        &value.to_string(),
                    )
                )
            }
        }
    }
}
