use super::*;

impl<M: Model> QueryBuilder<M> {
    pub(crate) fn build_condition_expression(
        &self,
        condition: &WhereCondition,
        db_type: DatabaseType,
    ) -> Option<SimpleExpr> {
        let column = condition.column.as_str();
        let column_expr = || self.column_expr(db_type, column);
        let column_sql = || self.condition_column_sql(db_type, column);

        Some(match Self::condition_spec(condition)? {
            ConditionSpec::Raw { raw_sql, values } => {
                let sql = self.raw_condition_sql(db_type, column, raw_sql);
                // Without values the fragment is written as given: reading
                // its placeholders would refuse a `?` meant as an operator.
                if values.is_empty() {
                    Expr::cust(sql)
                } else {
                    db_sql::template_bound(&sql, values.to_vec()).into()
                }
            }
            ConditionSpec::Subquery(subquery) => match subquery.render(db_type) {
                Ok(rendered) => {
                    let mut sql = BoundSql::sql(self.raw_condition_sql(db_type, column, ""));
                    sql.push(rendered);
                    sql.into()
                }
                Err(error) => Expr::Invalid(RenderError::Unsupported(error.to_string())),
            },
            ConditionSpec::Compare { operator, value } => {
                let column_type = self.column_type(column);
                let negated = matches!(operator, ComparisonOperator::NotEq);
                let json_column = is_json_type(column_type.as_ref());
                match operator {
                    ComparisonOperator::Eq | ComparisonOperator::NotEq
                        if crate::query::is_sql_null(value)
                            || (!json_column && crate::query::is_json_null(value)) =>
                    {
                        // Any column but a JSON one holds no JSON document: a
                        // JSON null is SQL NULL there.
                        Self::build_null_check_expression(column_expr(), negated)
                    }
                    // A JSON column compares documents, not their text.
                    ComparisonOperator::Eq | ComparisonOperator::NotEq if json_column => {
                        Self::build_json_equals_expression(
                            db_type,
                            &column_sql(),
                            &crate::internal::value_document(value),
                            negated,
                        )
                    }
                    _ => operator.apply(
                        column_expr(),
                        crate::internal::column_value(value, column_type.as_ref()),
                    ),
                }
            }
            ConditionSpec::CompareColumns { operator, other } => {
                operator.apply(column_expr(), self.column_expr(db_type, other))
            }
            ConditionSpec::Pattern { negated, value } => {
                self.build_pattern_expression(db_type, column, negated, value)
            }
            ConditionSpec::Literal {
                leading,
                trailing,
                value,
            } => self.build_literal_expression(db_type, column, leading, trailing, value),
            ConditionSpec::List { operator, values } => {
                self.build_list_expression(db_type, column, column_expr(), operator, values)
            }
            ConditionSpec::NullCheck { negated } => {
                Self::build_null_check_expression(column_expr(), negated)
            }
            ConditionSpec::Between { low, high, negated } => {
                self.build_between_expression(column, column_expr(), low, high, negated)
            }
            ConditionSpec::JsonValue { containment, value } => {
                Self::build_json_value_expression(db_type, &column_sql(), containment, value)
            }
            ConditionSpec::JsonExists {
                existence,
                negated,
                target,
            } => self.build_json_exists_expression(db_type, column, existence, negated, target),
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
            SoftDeleteScope::ActiveOnly => {
                Some(self.column_expr(db_type, M::deleted_at_column()).is_null())
            }
            SoftDeleteScope::OnlyTrashed => Some(
                self.column_expr(db_type, M::deleted_at_column())
                    .is_not_null(),
            ),
        }
    }

    /// The WHERE predicate for `db_type`, empty without one, with its values
    /// bound in place.
    pub(crate) fn where_bound(&self, db_type: DatabaseType) -> Result<BoundSql> {
        let condition = self.build_condition_for_db(db_type);
        if condition.is_empty() {
            return Ok(BoundSql::default());
        }
        Self::render_condition(db_type, &condition)
    }

    pub(crate) fn build_where_clause_with_condition_for_db(
        &self,
        db_type: DatabaseType,
    ) -> Result<(String, Vec<Value>)> {
        Ok(self.where_bound(db_type)?.build(db_type.dialect()))
    }

    /// `condition` rendered for `db_type`, with its values bound in place.
    pub(in crate::query::sql) fn render_condition(
        db_type: DatabaseType,
        condition: &Condition,
    ) -> Result<BoundSql> {
        db_type
            .build_condition(condition)
            .map_err(crate::internal::translate_error)
    }
}
