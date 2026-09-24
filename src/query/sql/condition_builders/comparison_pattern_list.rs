use super::*;

impl<M: Model> QueryBuilder<M> {
    fn pattern_value(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(text) => text.clone(),
            _ => value.to_string(),
        }
    }

    /// A raw fragment, optionally prefixed with the column it constrains.
    ///
    /// A fragment carrying `values` already uses the backend's own placeholder
    /// marker, so sea-query renumbers those tokens into the surrounding
    /// statement and binds `values` against them.
    pub(crate) fn build_raw_condition_expression(
        &self,
        db_type: DatabaseType,
        column: &str,
        raw_sql: &str,
        values: Vec<Value>,
    ) -> SimpleExpr {
        let sql = if column.is_empty() {
            raw_sql.to_string()
        } else {
            format!("{} {}", self.format_column_for_db(db_type, column), raw_sql)
        };

        self.build_custom_expression(sql, values)
    }

    pub(in crate::query::sql) fn build_compare_expression(
        &self,
        column: &str,
        column_expr: SimpleExpr,
        operator: ComparisonOperator,
        value: &serde_json::Value,
    ) -> SimpleExpr {
        let value = self.column_value(column, value);
        match operator {
            ComparisonOperator::Eq => column_expr.eq(value),
            ComparisonOperator::NotEq => column_expr.ne(value),
            ComparisonOperator::Gt => column_expr.gt(value),
            ComparisonOperator::Gte => column_expr.gte(value),
            ComparisonOperator::Lt => column_expr.lt(value),
            ComparisonOperator::Lte => column_expr.lte(value),
        }
    }

    pub(crate) fn build_pattern_expression(
        &self,
        db_type: DatabaseType,
        column: &str,
        negated: bool,
        escaped: bool,
        value: &serde_json::Value,
    ) -> SimpleExpr {
        let pattern = Self::pattern_value(value);
        if escaped {
            let operator = if negated { "NOT LIKE" } else { "LIKE" };
            self.build_custom_expression(
                format!(
                    "{} {} {}{}",
                    self.format_column_for_db(db_type, column),
                    operator,
                    db_sql::placeholder(db_type, 1),
                    crate::columns::LIKE_ESCAPE_CLAUSE
                ),
                vec![Value::String(Some(pattern))],
            )
        } else if negated {
            self.sea_column_expr(db_type, column).not_like(pattern)
        } else {
            self.sea_column_expr(db_type, column).like(pattern)
        }
    }

    pub(in crate::query::sql) fn build_list_expression(
        &self,
        db_type: DatabaseType,
        column: &str,
        column_expr: SimpleExpr,
        operator: ListOperator,
        values: &[serde_json::Value],
    ) -> SimpleExpr {
        // A NULL member reads as "or the column is NULL" (negated: "and it is
        // not"), the same reading `where_eq`/`where_not` give NULL. Taken
        // literally, `IN (.., NULL)` never matches a NULL row and
        // `NOT IN (.., NULL)` never matches any row at all.
        let (nulls, values): (Vec<&serde_json::Value>, Vec<&serde_json::Value>) =
            values.iter().partition(|value| value.is_null());
        let listed = self.build_non_null_list_expression(
            db_type,
            column,
            column_expr.clone(),
            operator,
            &values,
        );
        if nulls.is_empty() {
            return listed;
        }
        match operator {
            ListOperator::In | ListOperator::EqAny if values.is_empty() => column_expr.is_null(),
            ListOperator::In | ListOperator::EqAny => listed.or(column_expr.is_null()),
            ListOperator::NotIn | ListOperator::NeAll if values.is_empty() => {
                column_expr.is_not_null()
            }
            ListOperator::NotIn | ListOperator::NeAll => listed.and(column_expr.is_not_null()),
        }
    }

    fn build_non_null_list_expression(
        &self,
        db_type: DatabaseType,
        column: &str,
        column_expr: SimpleExpr,
        operator: ListOperator,
        values: &[&serde_json::Value],
    ) -> SimpleExpr {
        let sea_values: Vec<Value> = values
            .iter()
            .map(|value| self.column_value(column, value))
            .collect();
        // PostgreSQL takes a long list as one array parameter, so any length
        // fits in a statement and every length shares one prepared statement.
        #[cfg(feature = "postgres")]
        if db_type == DatabaseType::Postgres
            && sea_values.len() > INLINE_INTEGER_LIST_AFTER
            && let Some(array) = postgres_array(&sea_values)
        {
            use crate::orm::sea_query::extension::postgres::PgFunc;

            let array = Expr::val(array);
            return match operator {
                ListOperator::In | ListOperator::EqAny => column_expr.eq(PgFunc::any(array)),
                ListOperator::NotIn | ListOperator::NeAll => column_expr.ne(PgFunc::all(array)),
            };
        }
        // SQLite reads a long text list from one JSON parameter. Other types
        // keep their bound form: a UUID is stored as a blob, and a timestamp
        // as text in a format its JSON form does not share.
        if db_type == DatabaseType::SQLite
            && sea_values.len() > INLINE_INTEGER_LIST_AFTER
            && let Some(texts) = sea_values
                .iter()
                .map(|value| match value {
                    Value::String(Some(text)) => Some(serde_json::Value::String(text.clone())),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
        {
            let operator_sql = match operator {
                ListOperator::In | ListOperator::EqAny => "IN",
                ListOperator::NotIn | ListOperator::NeAll => "NOT IN",
            };
            return self.build_custom_expression(
                format!(
                    "{} {} (SELECT value FROM json_each({}))",
                    self.format_column_for_db(db_type, column),
                    operator_sql,
                    db_sql::placeholder(db_type, 1)
                ),
                vec![Value::String(Some(
                    serde_json::Value::Array(texts).to_string(),
                ))],
            );
        }
        // Every backend caps one statement at 32,766 or 65,535 bind parameters,
        // so a long id list could not be sent at all. Integers rendered by
        // Rust carry nothing but digits and a sign, so a long all-integer list
        // is written as literals instead.
        if sea_values.len() > INLINE_INTEGER_LIST_AFTER
            && let Some(literals) = sea_values
                .iter()
                .map(integer_literal)
                .collect::<Option<Vec<_>>>()
        {
            let literals = literals.into_iter().map(Expr::cust);
            return match operator {
                ListOperator::In | ListOperator::EqAny => column_expr.is_in(literals),
                ListOperator::NotIn | ListOperator::NeAll => column_expr.is_not_in(literals),
            };
        }
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
}

/// Lists longer than this bind no parameters when every value is an integer,
/// and one array parameter on PostgreSQL.
const INLINE_INTEGER_LIST_AFTER: usize = 1_000;

/// `values` as one PostgreSQL array value, when they share a type.
#[cfg(feature = "postgres")]
fn postgres_array(values: &[Value]) -> Option<Value> {
    let array_type = values.first()?.array_type();
    values
        .iter()
        .all(|value| value.array_type() == array_type)
        .then(|| Value::Array(array_type, Some(Box::new(values.to_vec()))))
}

/// The SQL literal of an integer value, or `None` for anything else.
fn integer_literal(value: &Value) -> Option<String> {
    match value {
        Value::TinyInt(Some(n)) => Some(n.to_string()),
        Value::SmallInt(Some(n)) => Some(n.to_string()),
        Value::Int(Some(n)) => Some(n.to_string()),
        Value::BigInt(Some(n)) => Some(n.to_string()),
        Value::TinyUnsigned(Some(n)) => Some(n.to_string()),
        Value::SmallUnsigned(Some(n)) => Some(n.to_string()),
        Value::Unsigned(Some(n)) => Some(n.to_string()),
        Value::BigUnsigned(Some(n)) => Some(n.to_string()),
        _ => None,
    }
}
