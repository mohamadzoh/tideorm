use super::*;

impl<M: Model> QueryBuilder<M> {
    pub(crate) fn build_condition_expression(
        &self,
        condition: &WhereCondition,
        db_type: DatabaseType,
    ) -> Option<SimpleExpr> {
        let column = condition.column.as_str();
        let column_expr = || self.sea_column_expr(db_type, column);
        let column_sql = || self.condition_column_sql(db_type, column);

        Some(match Self::condition_spec(condition)? {
            ConditionSpec::Raw {
                raw_sql,
                values,
                template,
            } => {
                // A template whose placeholders do not match its values has
                // already failed validation; it is rendered without them, as
                // the engine would index past the values otherwise, on paths
                // such as `debug()` that render before they validate.
                if template && db_sql::count_template_placeholders(raw_sql) != values.len() {
                    return Some(self.build_raw_condition_expression(
                        db_type,
                        column,
                        raw_sql,
                        Vec::new(),
                    ));
                }
                let sql = if template {
                    db_sql::render_template(db_type, raw_sql, 1)
                } else {
                    raw_sql.to_string()
                };
                self.build_raw_condition_expression(db_type, column, &sql, values.to_vec())
            }
            ConditionSpec::Compare { operator, value } => {
                let column_type = self.column_type(column);
                let negated = matches!(operator, ComparisonOperator::NotEq);
                match operator {
                    // A JSON column compares documents, not their text.
                    ComparisonOperator::Eq | ComparisonOperator::NotEq
                        if is_json_type(column_type.as_ref()) =>
                    {
                        self.build_json_equals_expression(db_type, &column_sql(), value, negated)
                    }
                    // Any other column holds no JSON document: a null is SQL NULL.
                    ComparisonOperator::Eq | ComparisonOperator::NotEq if value.is_null() => {
                        self.build_null_check_expression(column_expr(), negated)
                    }
                    _ => operator.apply(
                        column_expr(),
                        crate::internal::json_to_column_value(value, column_type.as_ref()),
                    ),
                }
            }
            ConditionSpec::CompareColumns { operator, other } => {
                operator.apply(column_expr(), self.sea_column_expr(db_type, other))
            }
            ConditionSpec::Pattern {
                negated,
                escaped,
                value,
            } => self.build_pattern_expression(db_type, column, negated, escaped, value),
            ConditionSpec::List {
                operator,
                values,
                documents,
            } => self.build_list_expression(
                db_type,
                column,
                column_expr(),
                operator,
                values,
                documents,
            ),
            ConditionSpec::NullCheck { negated } => {
                self.build_null_check_expression(column_expr(), negated)
            }
            ConditionSpec::Between { low, high, negated } => {
                self.build_between_expression(column, column_expr(), low, high, negated)
            }
            ConditionSpec::JsonValue { containment, value } => {
                self.build_json_value_expression(db_type, &column_sql(), containment, value)
            }
            ConditionSpec::JsonExists {
                existence,
                negated,
                target,
            } => self.build_json_exists_expression(
                db_type,
                &column_sql(),
                existence,
                negated,
                target,
            ),
            ConditionSpec::Array { operator, values } => {
                self.build_array_expression(db_type, &column_sql(), operator, column, values)
            }
        })
    }

    pub(crate) fn build_or_group_condition(
        &self,
        group: &OrGroup,
        db_type: DatabaseType,
    ) -> Condition {
        let condition = match group.combine_with {
            LogicalOp::And => Condition::all(),
            LogicalOp::Or => Condition::any(),
        };
        self.add_filters(condition, &group.conditions, &group.nested_groups, db_type)
    }

    /// `condition` with each of `conditions` and each non-empty group added.
    pub(in crate::query::sql) fn add_filters(
        &self,
        mut condition: Condition,
        conditions: &[WhereCondition],
        groups: &[OrGroup],
        db_type: DatabaseType,
    ) -> Condition {
        for filter in conditions {
            if let Some(expression) = self.build_condition_expression(filter, db_type) {
                condition = condition.add(expression);
            }
        }
        for group in groups.iter().filter(|group| !group.is_empty()) {
            condition = condition.add(self.build_or_group_condition(group, db_type));
        }
        condition
    }

    pub(crate) fn build_soft_delete_expression(&self, db_type: DatabaseType) -> Option<SimpleExpr> {
        match query_scope_for::<M>(self.clauses.include_trashed, self.clauses.only_trashed) {
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
        let condition = self.build_sea_condition_for_db(db_type);
        if condition.is_empty() {
            return (String::new(), Vec::new());
        }

        let mut query = Query::select();
        query.expr(Expr::cust("1"));
        query.cond_where(condition);

        let (mut sql, values) = match db_type {
            DatabaseType::Postgres => query.build(PostgresQueryBuilder),
            DatabaseType::MySQL | DatabaseType::MariaDB => query.build(MysqlQueryBuilder),
            DatabaseType::SQLite => query.build(SqliteQueryBuilder),
        };

        const WHERE: &str = " WHERE ";
        match sql.find(WHERE) {
            Some(start) => {
                sql.drain(..start + WHERE.len());
                (sql, values.into_iter().collect())
            }
            None => (String::new(), Vec::new()),
        }
    }
}
