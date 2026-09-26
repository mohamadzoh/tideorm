use super::*;

/// `check` repeated `count` times, joined by `combine` and parenthesized.
fn repeated_check(check: String, count: usize, combine: &str) -> String {
    format!("({})", vec![check; count].join(combine))
}

impl<M: Model> QueryBuilder<M> {
    pub(crate) fn build_null_check_expression(
        &self,
        column_expr: SimpleExpr,
        negated: bool,
    ) -> SimpleExpr {
        if negated {
            column_expr.is_not_null()
        } else {
            column_expr.is_null()
        }
    }

    pub(crate) fn build_between_expression(
        &self,
        column: &str,
        column_expr: SimpleExpr,
        low: &serde_json::Value,
        high: &serde_json::Value,
        negated: bool,
    ) -> SimpleExpr {
        let (low, high) = (
            self.column_value(column, low),
            self.column_value(column, high),
        );
        if negated {
            column_expr.not_between(low, high)
        } else {
            column_expr.between(low, high)
        }
    }

    pub(in crate::query::sql) fn build_json_value_expression(
        &self,
        db_type: DatabaseType,
        column_sql: &str,
        operator: JsonValueOperator,
        value: &serde_json::Value,
    ) -> SimpleExpr {
        let bound = match operator {
            JsonValueOperator::Contains => db_sql::json_contains_bound(db_type, column_sql, value),
            JsonValueOperator::ContainedBy => {
                db_sql::json_contained_by_bound(db_type, column_sql, value)
            }
        };

        self.build_custom_expression(bound.sql, bound.values)
    }

    pub(in crate::query::sql) fn build_json_exists_expression(
        &self,
        db_type: DatabaseType,
        column_sql: &str,
        existence: JsonExistence,
        negated: bool,
        target: &str,
    ) -> SimpleExpr {
        match db_sql::json_exists_bound(db_type, column_sql, existence, target, negated) {
            Some(bound) => self.build_custom_expression(bound.sql, bound.values),
            None => Expr::cust(db_sql::invalid_json_path_predicate()),
        }
    }

    pub(in crate::query::sql) fn build_array_expression(
        &self,
        db_type: DatabaseType,
        column_sql: &str,
        operator: ArrayOperator,
        values: &[serde_json::Value],
    ) -> SimpleExpr {
        match db_type {
            DatabaseType::Postgres => {
                let (sql, bound) = match operator {
                    ArrayOperator::Contains => (
                        db_sql::postgres_array_contains(
                            column_sql,
                            &db_sql::placeholders(db_type, values.len()),
                        ),
                        values.to_vec(),
                    ),
                    ArrayOperator::ContainedBy => {
                        let (listed, null_allowed) = split_nulls(values);
                        let operands = db_sql::placeholders(db_type, listed.len());
                        (
                            db_sql::postgres_array_contained_by(
                                column_sql,
                                &operands,
                                null_allowed,
                            ),
                            listed,
                        )
                    }
                    ArrayOperator::Overlaps => (
                        db_sql::postgres_array_overlaps(
                            column_sql,
                            &db_sql::placeholders(db_type, values.len()),
                        ),
                        values.to_vec(),
                    ),
                };
                self.build_custom_expression(sql, Self::sea_value_list(&bound))
            }
            // The JSON text is bound as is: `JSON_CONTAINS` parses it on both
            // servers, and MariaDB has no `CAST(.. AS JSON)`.
            DatabaseType::MySQL | DatabaseType::MariaDB => match operator {
                ArrayOperator::Contains => self.build_custom_expression(
                    format!("JSON_CONTAINS({}, ?)", column_sql),
                    vec![db_sql::json_array_parameter(values)],
                ),
                ArrayOperator::ContainedBy => self.build_custom_expression(
                    format!("JSON_CONTAINS(?, {})", column_sql),
                    vec![db_sql::json_array_parameter(values)],
                ),
                ArrayOperator::Overlaps if values.is_empty() => Expr::cust("0 = 1".to_string()),
                ArrayOperator::Overlaps => self.build_custom_expression(
                    repeated_check(
                        format!("JSON_CONTAINS({}, ?)", column_sql),
                        values.len(),
                        " OR ",
                    ),
                    values.iter().map(db_sql::json_scalar_parameter).collect(),
                ),
            },
            DatabaseType::SQLite => {
                let element_matches = format!(
                    "EXISTS (SELECT 1 FROM json_each({}) WHERE value = ?)",
                    column_sql
                );
                match operator {
                    ArrayOperator::Contains if values.is_empty() => Expr::cust("1 = 1".to_string()),
                    ArrayOperator::Contains => self.build_custom_expression(
                        repeated_check(element_matches, values.len(), " AND "),
                        Self::sea_value_list(values),
                    ),
                    ArrayOperator::ContainedBy => {
                        let (listed, null_allowed) = split_nulls(values);
                        let operands = db_sql::placeholders(db_type, listed.len());
                        self.build_custom_expression(
                            db_sql::array_contained_by(
                                column_sql,
                                &format!("json_each({})", column_sql),
                                "value",
                                &operands,
                                null_allowed,
                            ),
                            Self::sea_value_list(&listed),
                        )
                    }
                    ArrayOperator::Overlaps if values.is_empty() => Expr::cust("0 = 1".to_string()),
                    ArrayOperator::Overlaps => self.build_custom_expression(
                        repeated_check(element_matches, values.len(), " OR "),
                        Self::sea_value_list(values),
                    ),
                }
            }
        }
    }
}

/// The non-NULL values of a list, and whether it also held a NULL.
fn split_nulls(values: &[serde_json::Value]) -> (Vec<serde_json::Value>, bool) {
    let listed: Vec<serde_json::Value> = values
        .iter()
        .filter(|value| !value.is_null())
        .cloned()
        .collect();
    let null_allowed = listed.len() < values.len();
    (listed, null_allowed)
}
