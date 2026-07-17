use super::*;

#[allow(missing_docs)]
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

    pub(crate) fn build_null_check_sql(&self, column: &str, negated: bool) -> String {
        format!("{} IS {}NULL", column, if negated { "NOT " } else { "" })
    }

    pub(crate) fn build_between_expression(
        &self,
        column_expr: SimpleExpr,
        low: &serde_json::Value,
        high: &serde_json::Value,
    ) -> SimpleExpr {
        column_expr.between(
            crate::internal::json_to_db_value(low),
            crate::internal::json_to_db_value(high),
        )
    }

    pub(crate) fn build_between_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        low: &serde_json::Value,
        high: &serde_json::Value,
    ) -> String {
        format!(
            "{} BETWEEN {} AND {}",
            column,
            self.format_preview_value(db_type, low),
            self.format_preview_value(db_type, high)
        )
    }

    pub(in crate::query::sql) fn build_json_value_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        operator: JsonValueOperator,
        value: &serde_json::Value,
    ) -> String {
        match operator {
            JsonValueOperator::Contains => {
                db_sql::preview_json_contains(db_type, column, &value.to_string())
            }
            JsonValueOperator::ContainedBy => {
                db_sql::preview_json_contained_by(db_type, column, &value.to_string())
            }
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

    pub(in crate::query::sql) fn build_json_string_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        operator: JsonStringOperator,
        value: &str,
    ) -> String {
        match operator {
            JsonStringOperator::KeyPresent => {
                db_sql::preview_json_key_exists(db_type, column, value)
            }
            JsonStringOperator::KeyAbsent => {
                db_sql::preview_json_key_not_exists(db_type, column, value)
            }
            JsonStringOperator::PathPresent => {
                db_sql::preview_json_path_exists(db_type, column, value)
            }
            JsonStringOperator::PathAbsent => {
                db_sql::preview_json_path_not_exists(db_type, column, value)
            }
        }
    }

    pub(in crate::query::sql) fn build_json_string_expression(
        &self,
        db_type: DatabaseType,
        column_sql: &str,
        operator: JsonStringOperator,
        value: &str,
    ) -> SimpleExpr {
        let maybe_bound = match operator {
            JsonStringOperator::KeyPresent => {
                Some(db_sql::json_key_exists_bound(db_type, column_sql, value))
            }
            JsonStringOperator::KeyAbsent => Some(db_sql::json_key_not_exists_bound(
                db_type, column_sql, value,
            )),
            JsonStringOperator::PathPresent => {
                db_sql::json_path_exists_bound(db_type, column_sql, value)
            }
            JsonStringOperator::PathAbsent => {
                db_sql::json_path_not_exists_bound(db_type, column_sql, value)
            }
        };

        let Some(bound) = maybe_bound else {
            return Expr::cust(db_sql::invalid_json_path_predicate());
        };

        self.build_custom_expression(bound.sql, bound.values)
    }

    pub(in crate::query::sql) fn build_array_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        operator: ArrayOperator,
        values: &[serde_json::Value],
    ) -> String {
        let rendered = self.render_array_values(values);
        match operator {
            ArrayOperator::Contains => db_sql::array_contains(db_type, column, &rendered),
            ArrayOperator::ContainedBy => db_sql::array_contained_by(db_type, column, &rendered),
            ArrayOperator::Overlaps => db_sql::array_overlaps(db_type, column, &rendered),
        }
    }

    pub(in crate::query::sql) fn build_array_expression(
        &self,
        db_type: DatabaseType,
        column_expr: SimpleExpr,
        column_sql: &str,
        operator: ArrayOperator,
        values: &[serde_json::Value],
    ) -> SimpleExpr {
        match db_type {
            DatabaseType::Postgres => {
                let _ = column_expr;
                let rendered = self.render_array_values(values);
                let sql = match operator {
                    ArrayOperator::Contains => {
                        format!("{} @> ARRAY[{}]", column_sql, rendered.join(","))
                    }
                    ArrayOperator::ContainedBy => {
                        format!("{} <@ ARRAY[{}]", column_sql, rendered.join(","))
                    }
                    ArrayOperator::Overlaps => {
                        format!("{} && ARRAY[{}]", column_sql, rendered.join(","))
                    }
                };
                Expr::cust(sql)
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => match operator {
                ArrayOperator::Contains => self.build_custom_expression(
                    format!("JSON_CONTAINS({}, CAST(? AS JSON))", column_sql),
                    vec![Self::json_array_parameter(values)],
                ),
                ArrayOperator::ContainedBy => self.build_custom_expression(
                    format!("JSON_CONTAINS(CAST(? AS JSON), {})", column_sql),
                    vec![Self::json_array_parameter(values)],
                ),
                ArrayOperator::Overlaps => {
                    if values.is_empty() {
                        Expr::cust("0 = 1".to_string())
                    } else {
                        let sql = std::iter::repeat_n(
                            format!("JSON_CONTAINS({}, CAST(? AS JSON))", column_sql),
                            values.len(),
                        )
                        .collect::<Vec<_>>()
                        .join(" OR ");
                        let params = values.iter().map(Self::json_scalar_parameter).collect();
                        self.build_custom_expression(format!("({})", sql), params)
                    }
                }
            },
            DatabaseType::SQLite => match operator {
                ArrayOperator::Contains => {
                    if values.is_empty() {
                        Expr::cust("1 = 1".to_string())
                    } else {
                        let sql = std::iter::repeat_n(
                            format!(
                                "EXISTS (SELECT 1 FROM json_each({}) WHERE value = ?)",
                                column_sql
                            ),
                            values.len(),
                        )
                        .collect::<Vec<_>>()
                        .join(" AND ");
                        self.build_custom_expression(
                            format!("({})", sql),
                            Self::sea_value_list(values),
                        )
                    }
                }
                ArrayOperator::ContainedBy => {
                    if values.is_empty() {
                        Expr::cust(format!(
                            "NOT EXISTS (SELECT 1 FROM json_each({}))",
                            column_sql
                        ))
                    } else {
                        self.build_custom_expression(
                            format!(
                                "NOT EXISTS (SELECT 1 FROM json_each({}) WHERE value NOT IN ({}))",
                                column_sql,
                                Self::placeholder_list(values.len())
                            ),
                            Self::sea_value_list(values),
                        )
                    }
                }
                ArrayOperator::Overlaps => {
                    if values.is_empty() {
                        Expr::cust("0 = 1".to_string())
                    } else {
                        let sql = std::iter::repeat_n(
                            format!(
                                "EXISTS (SELECT 1 FROM json_each({}) WHERE value = ?)",
                                column_sql
                            ),
                            values.len(),
                        )
                        .collect::<Vec<_>>()
                        .join(" OR ");
                        self.build_custom_expression(
                            format!("({})", sql),
                            Self::sea_value_list(values),
                        )
                    }
                }
            },
        }
    }
}
