use super::*;

#[allow(missing_docs)]
impl<M: Model> QueryBuilder<M> {
    pub(crate) fn build_sea_condition(&self) -> Condition {
        self.build_sea_condition_for_db(self.db_type_for_sql())
    }

    pub(crate) fn build_sea_condition_for_db(&self, db_type: DatabaseType) -> Condition {
        let mut condition = Condition::all();

        for filter in &self.conditions {
            if let Some(expression) = self.build_condition_expression(filter, db_type) {
                condition = condition.add(expression);
            }
        }

        for group in &self.or_groups {
            if !group.is_empty() {
                condition = condition.add(self.build_or_group_condition(group, db_type));
            }
        }

        if let Some(soft_delete_expression) = self.build_soft_delete_expression(db_type) {
            condition = condition.add(soft_delete_expression);
        }

        condition
    }

    pub(crate) fn db_type_for_sql(&self) -> DatabaseType {
        self.database
            .as_ref()
            .map(|db| db.backend())
            .or_else(|| crate::database::try_db().map(|db| db.backend()))
            .unwrap_or(DatabaseType::Postgres)
    }

    pub(crate) fn current_timestamp_sql() -> &'static str {
        "CURRENT_TIMESTAMP"
    }

    pub(crate) fn sea_value_list(values: &[serde_json::Value]) -> Vec<Value> {
        values
            .iter()
            .map(crate::internal::json_to_db_value)
            .collect()
    }

    pub(crate) fn json_text_value(text: String) -> Value {
        Value::String(Some(text))
    }

    pub(crate) fn json_array_parameter(values: &[serde_json::Value]) -> Value {
        Self::json_text_value(
            serde_json::to_string(values)
                .expect("serializing array predicate values should not fail"),
        )
    }

    pub(crate) fn json_scalar_parameter(value: &serde_json::Value) -> Value {
        Self::json_text_value(
            serde_json::to_string(value)
                .expect("serializing scalar predicate value should not fail"),
        )
    }

    pub(crate) fn placeholder_list(count: usize) -> String {
        std::iter::repeat_n("?", count)
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub(crate) fn sea_column_expr(&self, db_type: DatabaseType, column: &str) -> SimpleExpr {
        if column.contains('(')
            || column.contains('*')
            || column.contains(' ')
            || column.contains('"')
            || column.contains('`')
        {
            return Expr::cust(self.format_column_for_db(db_type, column));
        }

        if let Some((table, field)) = column.split_once('.') {
            if db_sql::validate_identifier("table", table).is_ok()
                && db_sql::validate_identifier("column", field).is_ok()
            {
                return Expr::col((Alias::new(table), Alias::new(field)));
            }
        } else if db_sql::validate_identifier("column", column).is_ok() {
            return Expr::col(Alias::new(column));
        }

        Expr::cust(self.format_column_for_db(db_type, column))
    }

    pub(crate) fn build_custom_expression(&self, sql: String, values: Vec<Value>) -> SimpleExpr {
        if values.is_empty() {
            Expr::cust(sql)
        } else {
            Expr::cust_with_values(sql, values)
        }
    }

    pub(in crate::query::sql) fn condition_spec<'a>(
        condition: &'a WhereCondition,
    ) -> Option<ConditionSpec<'a>> {
        match (&condition.operator, &condition.value) {
            (Operator::Raw, ConditionValue::RawExpr(raw_sql)) => Some(ConditionSpec::Raw {
                column: &condition.column,
                raw_sql,
            }),
            (Operator::Eq, ConditionValue::Single(value)) => Some(ConditionSpec::Compare {
                operator: ComparisonOperator::Eq,
                value,
            }),
            (Operator::NotEq, ConditionValue::Single(value)) => Some(ConditionSpec::Compare {
                operator: ComparisonOperator::NotEq,
                value,
            }),
            (Operator::Gt, ConditionValue::Single(value)) => Some(ConditionSpec::Compare {
                operator: ComparisonOperator::Gt,
                value,
            }),
            (Operator::Gte, ConditionValue::Single(value)) => Some(ConditionSpec::Compare {
                operator: ComparisonOperator::Gte,
                value,
            }),
            (Operator::Lt, ConditionValue::Single(value)) => Some(ConditionSpec::Compare {
                operator: ComparisonOperator::Lt,
                value,
            }),
            (Operator::Lte, ConditionValue::Single(value)) => Some(ConditionSpec::Compare {
                operator: ComparisonOperator::Lte,
                value,
            }),
            (Operator::Like, ConditionValue::Single(value)) => Some(ConditionSpec::Pattern {
                negated: false,
                escaped: false,
                value,
            }),
            (Operator::LikeEscaped, ConditionValue::Single(value)) => {
                Some(ConditionSpec::Pattern {
                    negated: false,
                    escaped: true,
                    value,
                })
            }
            (Operator::NotLike, ConditionValue::Single(value)) => Some(ConditionSpec::Pattern {
                negated: true,
                escaped: false,
                value,
            }),
            (Operator::In, ConditionValue::List(values)) => Some(ConditionSpec::List {
                operator: ListOperator::In,
                values,
            }),
            (Operator::NotIn, ConditionValue::List(values)) => Some(ConditionSpec::List {
                operator: ListOperator::NotIn,
                values,
            }),
            (Operator::EqAny, ConditionValue::List(values)) => Some(ConditionSpec::List {
                operator: ListOperator::EqAny,
                values,
            }),
            (Operator::NeAll, ConditionValue::List(values)) => Some(ConditionSpec::List {
                operator: ListOperator::NeAll,
                values,
            }),
            (Operator::IsNull, ConditionValue::None) => {
                Some(ConditionSpec::NullCheck { negated: false })
            }
            (Operator::IsNotNull, ConditionValue::None) => {
                Some(ConditionSpec::NullCheck { negated: true })
            }
            (Operator::Between, ConditionValue::Range(low, high)) => {
                Some(ConditionSpec::Between { low, high })
            }
            (Operator::JsonContains, ConditionValue::Single(value)) => {
                Some(ConditionSpec::JsonValue {
                    operator: JsonValueOperator::Contains,
                    value,
                })
            }
            (Operator::JsonContainedBy, ConditionValue::Single(value)) => {
                Some(ConditionSpec::JsonValue {
                    operator: JsonValueOperator::ContainedBy,
                    value,
                })
            }
            (Operator::JsonKeyExists, ConditionValue::Single(serde_json::Value::String(value))) => {
                Some(ConditionSpec::JsonString {
                    operator: JsonStringOperator::KeyPresent,
                    value,
                })
            }
            (
                Operator::JsonKeyNotExists,
                ConditionValue::Single(serde_json::Value::String(value)),
            ) => Some(ConditionSpec::JsonString {
                operator: JsonStringOperator::KeyAbsent,
                value,
            }),
            (
                Operator::JsonPathExists,
                ConditionValue::Single(serde_json::Value::String(value)),
            ) => Some(ConditionSpec::JsonString {
                operator: JsonStringOperator::PathPresent,
                value,
            }),
            (
                Operator::JsonPathNotExists,
                ConditionValue::Single(serde_json::Value::String(value)),
            ) => Some(ConditionSpec::JsonString {
                operator: JsonStringOperator::PathAbsent,
                value,
            }),
            (Operator::ArrayContains, ConditionValue::List(values))
            | (Operator::ArrayContainsAll, ConditionValue::List(values)) => {
                Some(ConditionSpec::Array {
                    operator: ArrayOperator::Contains,
                    values,
                })
            }
            (Operator::ArrayContainedBy, ConditionValue::List(values)) => {
                Some(ConditionSpec::Array {
                    operator: ArrayOperator::ContainedBy,
                    values,
                })
            }
            (Operator::ArrayOverlaps, ConditionValue::List(values))
            | (Operator::ArrayContainsAny, ConditionValue::List(values)) => {
                Some(ConditionSpec::Array {
                    operator: ArrayOperator::Overlaps,
                    values,
                })
            }
            (Operator::SubqueryIn, ConditionValue::Subquery(query_sql)) => {
                Some(ConditionSpec::Subquery {
                    negated: false,
                    query_sql,
                })
            }
            (Operator::SubqueryNotIn, ConditionValue::Subquery(query_sql)) => {
                Some(ConditionSpec::Subquery {
                    negated: true,
                    query_sql,
                })
            }
            _ => None,
        }
    }
}
