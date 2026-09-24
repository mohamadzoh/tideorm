use super::*;

impl<M: Model> QueryBuilder<M> {
    pub(crate) fn build_condition_expression(
        &self,
        condition: &WhereCondition,
        db_type: DatabaseType,
    ) -> Option<SimpleExpr> {
        let column = condition.column.as_str();
        let column_expr = || self.sea_column_expr(db_type, column);
        let column_sql = || self.format_column_for_db(db_type, column);

        Some(match Self::condition_spec(condition)? {
            ConditionSpec::Raw { raw_sql, values } => {
                self.build_raw_condition_expression(db_type, column, raw_sql, values.to_vec())
            }
            ConditionSpec::Compare { operator, value } => {
                self.build_compare_expression(column, column_expr(), operator, value)
            }
            ConditionSpec::Pattern {
                negated,
                escaped,
                value,
            } => self.build_pattern_expression(db_type, column, negated, escaped, value),
            ConditionSpec::List { operator, values } => {
                self.build_list_expression(db_type, column, column_expr(), operator, values)
            }
            ConditionSpec::NullCheck { negated } => {
                self.build_null_check_expression(column_expr(), negated)
            }
            ConditionSpec::Between { low, high } => {
                self.build_between_expression(column, column_expr(), low, high)
            }
            ConditionSpec::JsonValue { operator, value } => {
                self.build_json_value_expression(db_type, &column_sql(), operator, value)
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
                self.build_array_expression(db_type, &column_sql(), operator, values)
            }
        })
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
}
