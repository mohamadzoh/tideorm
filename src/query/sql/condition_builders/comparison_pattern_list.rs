use super::*;

impl<M: Model> QueryBuilder<M> {
    /// A raw condition's SQL: `raw_sql` behind its column, if it names one.
    pub(in crate::query::sql) fn raw_condition_sql(
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

    /// Whether `column` holds JSON, which compares as documents, not text.
    pub(in crate::query::sql) fn is_json_column(&self, column: &str) -> bool {
        is_json_type(self.column_type(column).as_ref())
    }

    /// `column` as the JSON and array conditions write it: on SQLite, one of
    /// the model's columns is always qualified with its table.
    pub(in crate::query::sql) fn condition_column_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
    ) -> String {
        self.format_column_for_db(
            db_type,
            &self.model_identifier(column, db_type == DatabaseType::SQLite),
        )
    }

    /// The JSON column `column_sql` holding the document `value`, or not when
    /// `negated`.
    pub(in crate::query::sql) fn build_json_equals_expression(
        db_type: DatabaseType,
        column_sql: &str,
        value: &serde_json::Value,
        negated: bool,
    ) -> SimpleExpr {
        let bound = db_type.dialect().json_equals(column_sql, value, negated);
        bound.into()
    }

    pub(crate) fn build_pattern_expression(
        &self,
        db_type: DatabaseType,
        column: &str,
        negated: bool,
        value: &Value,
    ) -> SimpleExpr {
        let pattern = pattern_text(value);
        if negated {
            self.column_expr(db_type, column).not_like(pattern)
        } else {
            self.column_expr(db_type, column).like(pattern)
        }
    }

    /// The text of `column` holding `value` literally, as the dialect tests
    /// it: anything before it when `leading` and after it when `trailing`.
    pub(in crate::query::sql) fn build_literal_expression(
        &self,
        db_type: DatabaseType,
        column: &str,
        leading: bool,
        trailing: bool,
        value: &Value,
    ) -> SimpleExpr {
        let column_sql = self.format_column_for_db(db_type, column);
        db_type
            .dialect()
            .literal_match(&column_sql, &pattern_text(value), leading, trailing)
            .map_or_else(Expr::Invalid, Into::into)
    }

    pub(in crate::query::sql) fn build_list_expression(
        &self,
        db_type: DatabaseType,
        column: &str,
        column_expr: SimpleExpr,
        operator: ListOperator,
        values: &[Value],
    ) -> SimpleExpr {
        // A NULL member reads as "or the column is NULL" (negated: "and it is
        // not"), the same reading `where_eq`/`where_not` give NULL. Taken
        // literally, `IN (.., NULL)` never matches a NULL row and
        // `NOT IN (.., NULL)` never matches any row at all. A JSON column
        // compares a JSON `null` member as the `null` document instead.
        let column_type = self.column_type(column);
        let json_column = is_json_type(column_type.as_ref());
        let (nulls, values): (Vec<&Value>, Vec<&Value>) = values.iter().partition(|value| {
            crate::query::is_sql_null(value) || (!json_column && crate::query::is_json_null(value))
        });
        let sql_null = !nulls.is_empty();
        let listed = self.build_non_null_list_expression(
            db_type,
            column,
            column_type.as_ref(),
            column_expr.clone(),
            operator,
            &values,
        );
        if !sql_null {
            return listed;
        }
        match operator {
            ListOperator::In if values.is_empty() => column_expr.is_null(),
            ListOperator::In => listed.or(column_expr.is_null()),
            ListOperator::NotIn if values.is_empty() => column_expr.is_not_null(),
            ListOperator::NotIn => listed.and(column_expr.is_not_null()),
        }
    }

    fn build_non_null_list_expression(
        &self,
        db_type: DatabaseType,
        column: &str,
        column_type: Option<&crate::orm::ColumnType>,
        column_expr: SimpleExpr,
        operator: ListOperator,
        values: &[&Value],
    ) -> SimpleExpr {
        // A JSON column compares documents, one per listed value, as
        // `where_eq`/`where_not` do.
        if is_json_type(column_type) {
            let negated = matches!(operator, ListOperator::NotIn);
            let column_sql = self.condition_column_sql(db_type, column);
            let mut documents = values.iter().map(|value| {
                let document = crate::internal::value_document(value);
                Self::build_json_equals_expression(db_type, &column_sql, &document, negated)
            });
            if let Some(first) = documents.next() {
                return documents.fold(first, |all, document| {
                    if negated {
                        all.and(document)
                    } else {
                        all.or(document)
                    }
                });
            }
        }

        let list_values: Vec<Value> = values
            .iter()
            .map(|value| crate::internal::column_value(value, column_type))
            .collect();
        db_type.dialect().in_list(
            column_expr,
            list_values,
            matches!(operator, ListOperator::NotIn),
        )
    }
}

/// `value` as the text a pattern or literal match compares.
fn pattern_text(value: &Value) -> String {
    match value {
        Value::String(Some(text)) => text.clone(),
        other => crate::internal::json_text(&crate::internal::value_document(other)),
    }
}
