use super::*;

#[allow(missing_docs)]
impl<M: Model> QueryBuilder<M> {
    pub(crate) fn preview_values(
        &self,
        db_type: DatabaseType,
        values: &[serde_json::Value],
    ) -> Vec<String> {
        values
            .iter()
            .map(|value| self.format_preview_value(db_type, value))
            .collect()
    }

    pub(crate) fn pattern_value(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(text) => text.clone(),
            _ => value.to_string(),
        }
    }

    pub(in crate::query::sql) fn comparison_sql(operator: ComparisonOperator) -> &'static str {
        match operator {
            ComparisonOperator::Eq => "=",
            ComparisonOperator::NotEq => "!=",
            ComparisonOperator::Gt => ">",
            ComparisonOperator::Gte => ">=",
            ComparisonOperator::Lt => "<",
            ComparisonOperator::Lte => "<=",
        }
    }

    pub(crate) fn build_raw_condition_expression(
        &self,
        db_type: DatabaseType,
        column: &str,
        raw_sql: &str,
    ) -> SimpleExpr {
        if column.is_empty() {
            Expr::cust(raw_sql.to_string())
        } else {
            let column = self.format_column_for_db(db_type, column);
            Expr::cust(format!("{} {}", column, raw_sql))
        }
    }

    pub(crate) fn build_raw_condition_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        raw_sql: &str,
    ) -> String {
        if column.is_empty() {
            raw_sql.to_string()
        } else {
            format!("{} {}", self.format_column_for_db(db_type, column), raw_sql)
        }
    }

    pub(in crate::query::sql) fn build_compare_expression(
        &self,
        column_expr: SimpleExpr,
        operator: ComparisonOperator,
        value: &serde_json::Value,
    ) -> SimpleExpr {
        let value = crate::internal::json_to_db_value(value);
        match operator {
            ComparisonOperator::Eq => column_expr.eq(value),
            ComparisonOperator::NotEq => column_expr.ne(value),
            ComparisonOperator::Gt => column_expr.gt(value),
            ComparisonOperator::Gte => column_expr.gte(value),
            ComparisonOperator::Lt => column_expr.lt(value),
            ComparisonOperator::Lte => column_expr.lte(value),
        }
    }

    pub(in crate::query::sql) fn build_compare_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        operator: ComparisonOperator,
        value: &serde_json::Value,
    ) -> String {
        format!(
            "{} {} {}",
            column,
            Self::comparison_sql(operator),
            self.format_preview_value(db_type, value)
        )
    }

    pub(crate) fn build_pattern_expression(
        &self,
        db_type: DatabaseType,
        column_expr: SimpleExpr,
        column_sql: &str,
        negated: bool,
        escaped: bool,
        value: &serde_json::Value,
    ) -> SimpleExpr {
        let pattern = Self::pattern_value(value);
        if escaped {
            let operator = if negated { "NOT LIKE" } else { "LIKE" };
            let placeholder = match db_type {
                DatabaseType::Postgres => "$1",
                DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => "?",
            };
            self.build_custom_expression(
                format!(
                    "{} {} {}{}",
                    column_sql,
                    operator,
                    placeholder,
                    crate::columns::LIKE_ESCAPE_CLAUSE
                ),
                vec![Value::String(Some(pattern))],
            )
        } else if negated {
            column_expr.not_like(pattern)
        } else {
            column_expr.like(pattern)
        }
    }

    pub(crate) fn build_pattern_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        negated: bool,
        escaped: bool,
        value: &serde_json::Value,
    ) -> String {
        let mut sql = format!(
            "{} {}LIKE {}",
            column,
            if negated { "NOT " } else { "" },
            self.format_preview_value(db_type, value)
        );
        if escaped {
            sql.push_str(crate::columns::LIKE_ESCAPE_CLAUSE);
        }
        sql
    }

    pub(in crate::query::sql) fn build_list_expression(
        &self,
        column_expr: SimpleExpr,
        operator: ListOperator,
        values: &[serde_json::Value],
    ) -> SimpleExpr {
        let sea_values = Self::sea_value_list(values);
        match operator {
            ListOperator::In => column_expr.is_in(sea_values),
            ListOperator::NotIn => column_expr.is_not_in(sea_values),
            // An empty candidate set can never match, and rendering `ARRAY[]`
            // would make PostgreSQL reject the statement outright.
            ListOperator::EqAny if values.is_empty() => Expr::cust("0 = 1".to_string()),
            // No PostgreSQL special case: `col = ANY(ARRAY[a, b])` is just
            // `col IN (a, b)`, and sea-query binds `is_in` correctly on every
            // backend. Rendering the ARRAY form by hand cannot work here —
            // sea-query's fragment tokenizer treats `[` as a string delimiter
            // running to `]`, so placeholders inside the brackets are never
            // substituted and their values are dropped from the statement,
            // leaving `$1`/`$2` pointing at whatever else the query bound.
            ListOperator::EqAny => column_expr.is_in(sea_values),
            // Nothing to differ from, so an empty candidate set always matches.
            ListOperator::NeAll if values.is_empty() => Expr::cust("1 = 1".to_string()),
            // Same reasoning as `EqAny`: `<> ALL(ARRAY[..])` is `NOT IN (..)`.
            ListOperator::NeAll => column_expr.is_not_in(sea_values),
        }
    }

    pub(in crate::query::sql) fn build_list_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        operator: ListOperator,
        values: &[serde_json::Value],
    ) -> String {
        let rendered = self.preview_values(db_type, values);
        match operator {
            ListOperator::In => format!("{} IN ({})", column, rendered.join(", ")),
            ListOperator::NotIn => format!("{} NOT IN ({})", column, rendered.join(", ")),
            ListOperator::EqAny => db_sql::eq_any(db_type, column, &rendered),
            ListOperator::NeAll => db_sql::ne_all(db_type, column, &rendered),
        }
    }
}
