use super::*;

impl<M: Model> QueryBuilder<M> {
    pub(crate) fn build_null_check_expression(
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
        low: &Value,
        high: &Value,
        negated: bool,
    ) -> SimpleExpr {
        let column_type = self.column_type(column);
        let (low, high) = (
            crate::internal::column_value(low, column_type.as_ref()),
            crate::internal::column_value(high, column_type.as_ref()),
        );
        if negated {
            column_expr.not_between(low, high)
        } else {
            column_expr.between(low, high)
        }
    }

    pub(in crate::query::sql) fn build_json_value_expression(
        db_type: DatabaseType,
        column_sql: &str,
        containment: JsonContainment,
        value: &Value,
    ) -> SimpleExpr {
        let document = crate::internal::value_document(value);
        let bound = db_type
            .dialect()
            .json_containment(column_sql, &document, containment);
        bound.into()
    }

    pub(in crate::query::sql) fn build_json_exists_expression(
        &self,
        db_type: DatabaseType,
        column: &str,
        existence: JsonExistence,
        negated: bool,
        target: &str,
    ) -> SimpleExpr {
        let column_sql = self.condition_column_sql(db_type, column);
        match db_type
            .dialect()
            .json_exists(&column_sql, existence, target, negated)
        {
            Ok(bound) => bound.into(),
            Err(error) => Expr::Invalid(RenderError::Unsupported(format!(
                "WHERE condition on '{}' for model '{}': {}",
                column,
                M::table_name(),
                error
            ))),
        }
    }

    /// An array predicate. Where arrays are the database's own, each value is
    /// bound as an element of the column's array type; elsewhere the array is
    /// a JSON document and each value a JSON value.
    pub(in crate::query::sql) fn build_array_expression(
        &self,
        db_type: DatabaseType,
        column_sql: &str,
        operator: ArrayOperator,
        column: &str,
        values: &[Value],
    ) -> SimpleExpr {
        if values.is_empty() && matches!(operator, ArrayOperator::Overlaps) {
            return Expr::cust(db_sql::MATCH_NOTHING);
        }
        let dialect = db_type.dialect();
        let (elements, null_allowed) = if dialect.native_arrays() {
            let column_type = self.column_type(column);
            let json_elements = matches!(&column_type,
                Some(crate::orm::ColumnType::Array(element)) if is_json_type(Some(element.as_ref())));
            let (listed, null_allowed) = match operator {
                ArrayOperator::ContainedBy if !json_elements => split_nulls(values),
                _ => (values.iter().collect(), false),
            };
            let elements = listed
                .into_iter()
                .map(|value| crate::internal::array_element(value, column_type.as_ref()))
                .collect();
            (elements, null_allowed)
        } else {
            let elements = values
                .iter()
                .map(|value| Value::from(crate::internal::value_document(value)))
                .collect();
            (elements, false)
        };
        let bound = dialect.array_predicate(column_sql, operator, elements, null_allowed);
        bound.into()
    }
}

/// The non-NULL values of a list, and whether it also held a NULL: SQL NULL
/// or, in an array of anything but JSON, a JSON `null`.
fn split_nulls(values: &[Value]) -> (Vec<&Value>, bool) {
    let listed: Vec<&Value> = values
        .iter()
        .filter(|value| !crate::query::is_sql_null(value) && !crate::query::is_json_null(value))
        .collect();
    let null_allowed = listed.len() < values.len();
    (listed, null_allowed)
}
