use super::*;

/// The alias of a SQLite JSON array's `json_each` rows.
const SQLITE_ELEMENT: &str = "tideorm_element";

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
        containment: db_sql::JsonContainment,
        value: &serde_json::Value,
    ) -> SimpleExpr {
        let bound = db_sql::json_containment_bound(db_type, column_sql, value, containment);

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
            None => Expr::cust(db_sql::MATCH_NOTHING),
        }
    }

    pub(in crate::query::sql) fn build_array_expression(
        &self,
        db_type: DatabaseType,
        column_sql: &str,
        operator: ArrayOperator,
        column: &str,
        values: &[serde_json::Value],
    ) -> SimpleExpr {
        if values.is_empty() && matches!(operator, ArrayOperator::Overlaps) {
            return Expr::cust(db_sql::MATCH_NOTHING);
        }
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
                        let json_elements = matches!(self.column_type(column),
                            Some(crate::orm::ColumnType::Array(element))
                                if matches!(element.as_ref(), crate::orm::ColumnType::Json | crate::orm::ColumnType::JsonBinary));
                        let (listed, null_allowed) = if json_elements {
                            (values.to_vec(), false)
                        } else {
                            split_nulls(values)
                        };
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
                self.build_custom_expression(
                    sql,
                    bound
                        .iter()
                        .map(|value| {
                            crate::internal::json_to_array_element(
                                value,
                                self.column_type(column).as_ref(),
                            )
                        })
                        .collect(),
                )
            }
            // A JSON array column is contained in or contains the list as a
            // JSON document, with the guards that make `JSON_CONTAINS` read
            // as PostgreSQL does: a column holding an object is contained by
            // no list. An overlap is a containment of any one value.
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                let list = serde_json::Value::Array(values.to_vec());
                let bound = match operator {
                    ArrayOperator::Contains => db_sql::json_containment_bound(
                        db_type,
                        column_sql,
                        &list,
                        db_sql::JsonContainment::Contains,
                    ),
                    ArrayOperator::ContainedBy => db_sql::json_containment_bound(
                        db_type,
                        column_sql,
                        &list,
                        db_sql::JsonContainment::ContainedBy,
                    ),
                    ArrayOperator::Overlaps => {
                        let check = format!("JSON_CONTAINS({column_sql}, ?)");
                        db_sql::BoundSql::new(
                            db_sql::any_element(&vec![check; values.len()]),
                            values.iter().map(db_sql::json_scalar_parameter).collect(),
                        )
                    }
                };
                self.build_custom_expression(
                    format!("(JSON_TYPE({column_sql}) = 'ARRAY' AND ({}))", bound.sql),
                    bound.values,
                )
            }
            // Elements are compared as JSON values, as `where_eq` compares a
            // document: `1` is not `true`, and the string `"{\"a\":1}"` is not
            // that object. A null in a contained-by list admits null elements,
            // and a NULL column has no elements to test, so it is contained by
            // nothing, as PostgreSQL's `<@` leaves it.
            DatabaseType::SQLite => {
                let mut checks = Vec::with_capacity(values.len());
                let mut bound = Vec::new();
                for value in values {
                    let equal = db_sql::sqlite_json_element_equals(SQLITE_ELEMENT, value);
                    checks.push(equal.sql);
                    bound.extend(equal.values);
                }
                let elements = format!("json_each({column_sql}) AS {SQLITE_ELEMENT}");
                let has_element =
                    |equal: &String| format!("EXISTS (SELECT 1 FROM {elements} WHERE {equal})");
                let sql = match operator {
                    ArrayOperator::Contains => db_sql::all_elements(
                        column_sql,
                        &checks.iter().map(has_element).collect::<Vec<_>>(),
                    ),
                    ArrayOperator::Overlaps => {
                        db_sql::any_element(&checks.iter().map(has_element).collect::<Vec<_>>())
                    }
                    ArrayOperator::ContainedBy => {
                        let offending =
                            (!checks.is_empty()).then(|| format!("NOT ({})", checks.join(" OR ")));
                        db_sql::array_contained_by(column_sql, &elements, offending.as_deref())
                    }
                };
                // json_each also enumerates objects and scalar documents. Array
                // operators must never widen reads or writes to those values.
                self.build_custom_expression(
                    format!("(json_type({column_sql}) = 'array' AND ({sql}))"),
                    bound,
                )
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
