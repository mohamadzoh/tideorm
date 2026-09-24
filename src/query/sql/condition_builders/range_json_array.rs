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
    ) -> SimpleExpr {
        column_expr.between(
            self.column_value(column, low),
            self.column_value(column, high),
        )
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
                let operands = db_sql::placeholders(db_type, values.len());
                let sql = match operator {
                    ArrayOperator::Contains => {
                        db_sql::postgres_array_contains(column_sql, &operands)
                    }
                    ArrayOperator::ContainedBy => {
                        db_sql::postgres_array_contained_by(column_sql, &operands)
                    }
                    ArrayOperator::Overlaps => {
                        db_sql::postgres_array_overlaps(column_sql, &operands)
                    }
                };
                self.build_custom_expression(sql, Self::sea_value_list(values))
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
                    ArrayOperator::ContainedBy if values.is_empty() => Expr::cust(format!(
                        "NOT EXISTS (SELECT 1 FROM json_each({}))",
                        column_sql
                    )),
                    ArrayOperator::ContainedBy => self.build_custom_expression(
                        format!(
                            "NOT EXISTS (SELECT 1 FROM json_each({}) WHERE value NOT IN ({}))",
                            column_sql,
                            db_sql::placeholders(db_type, values.len()).join(", ")
                        ),
                        Self::sea_value_list(values),
                    ),
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
