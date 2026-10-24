use super::*;

impl<M: Model> QueryBuilder<M> {
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
        &self,
        db_type: DatabaseType,
        column_sql: &str,
        value: &serde_json::Value,
        negated: bool,
    ) -> SimpleExpr {
        let bound = db_sql::json_equals_bound(db_type, column_sql, value, negated);
        self.build_custom_expression(bound.sql, bound.values)
    }

    pub(crate) fn build_pattern_expression(
        &self,
        db_type: DatabaseType,
        column: &str,
        negated: bool,
        escaped: bool,
        value: &serde_json::Value,
    ) -> SimpleExpr {
        let pattern = crate::internal::json_text(value);
        if escaped {
            if db_type == DatabaseType::SQLite
                && let Some((literal, leading, trailing)) = sqlite_literal_pattern(&pattern)
            {
                // LIKE stops at a stored NUL. Blob slicing/search after ASCII
                // case folding preserves the entire string, including NULs.
                let test = match (leading, trailing) {
                    (true, true) => "instr(hay, needle) > 0",
                    (false, true) => "substr(hay, 1, length(needle)) = needle",
                    (true, false) => "length(needle) = 0 OR substr(hay, -length(needle)) = needle",
                    (false, false) => "hay = needle",
                };
                let sql = format!(
                    "(SELECT CASE WHEN hay IS NOT NULL THEN {}({test}) END FROM \
                     (SELECT CAST(CASE WHEN 'a' LIKE 'A' THEN lower(raw_hay) ELSE raw_hay END AS BLOB) AS hay, \
                     CAST(CASE WHEN 'a' LIKE 'A' THEN lower(raw_needle) ELSE raw_needle END AS BLOB) AS needle \
                     FROM (SELECT {} AS raw_hay, ? AS raw_needle)))",
                    if negated { "NOT " } else { "" },
                    self.format_column_for_db(db_type, column),
                );
                return self.build_custom_expression(sql, vec![Value::String(Some(literal))]);
            }
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
        documents: Option<bool>,
    ) -> SimpleExpr {
        // A NULL member reads as "or the column is NULL" (negated: "and it is
        // not"), the same reading `where_eq`/`where_not` give NULL. Taken
        // literally, `IN (.., NULL)` never matches a NULL row and
        // `NOT IN (.., NULL)` never matches any row at all. A JSON column
        // compares a JSON `null` member as the `null` document instead.
        let column_type = self.column_type(column);
        let (sql_null, values): (bool, Vec<&serde_json::Value>) = match documents {
            Some(sql_null) if is_json_type(column_type.as_ref()) => {
                (sql_null, values.iter().collect())
            }
            documents => {
                let (nulls, values): (Vec<&serde_json::Value>, Vec<&serde_json::Value>) =
                    values.iter().partition(|value| value.is_null());
                (documents.unwrap_or(false) || !nulls.is_empty(), values)
            }
        };
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
        values: &[&serde_json::Value],
    ) -> SimpleExpr {
        // A JSON column compares documents, one per listed value, as
        // `where_eq`/`where_not` do.
        if is_json_type(column_type) {
            let negated = matches!(operator, ListOperator::NotIn);
            let column_sql = self.condition_column_sql(db_type, column);
            let mut documents = values.iter().map(|value| {
                self.build_json_equals_expression(db_type, &column_sql, value, negated)
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

        let sea_values: Vec<Value> = values
            .iter()
            .map(|value| crate::internal::json_to_column_value(value, column_type))
            .collect();
        // PostgreSQL takes a long list as one array parameter, so any length
        // fits in a statement and every length shares one prepared statement.
        #[cfg(feature = "postgres")]
        if db_type == DatabaseType::Postgres
            && sea_values.len() > INLINE_INTEGER_LIST_AFTER
            && let Some(array_type) = shared_array_type(&sea_values)
        {
            use crate::orm::sea_query::extension::postgres::PgFunc;

            let array = Expr::val(Value::Array(array_type, Some(Box::new(sea_values))));
            return match operator {
                ListOperator::In => column_expr.eq(PgFunc::any(array)),
                ListOperator::NotIn => column_expr.ne(PgFunc::all(array)),
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
                    Value::String(Some(text)) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
        {
            let operator_sql = match operator {
                ListOperator::In => "IN",
                ListOperator::NotIn => "NOT IN",
            };
            return self.build_custom_expression(
                format!(
                    "{} {} (SELECT value FROM json_each({}))",
                    self.format_column_for_db(db_type, column),
                    operator_sql,
                    db_sql::placeholder(db_type, 1)
                ),
                vec![db_sql::json_array_parameter(&texts)],
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
                ListOperator::In => column_expr.is_in(literals),
                ListOperator::NotIn => column_expr.is_not_in(literals),
            };
        }
        match operator {
            ListOperator::In => column_expr.is_in(sea_values),
            ListOperator::NotIn => column_expr.is_not_in(sea_values),
        }
    }
}

/// Lists longer than this bind no parameters when every value is an integer,
/// and one array parameter on PostgreSQL.
const INLINE_INTEGER_LIST_AFTER: usize = 1_000;

/// Decode the escaped patterns made by the literal contains/starts/ends helpers.
fn sqlite_literal_pattern(pattern: &str) -> Option<(String, bool, bool)> {
    let mut chars = pattern.chars();
    let mut tokens = Vec::new();
    while let Some(ch) = chars.next() {
        tokens.push(match ch {
            '!' => (chars.next()?, false),
            '_' => return None,
            '%' => ('%', true),
            other => (other, false),
        });
    }
    let leading = tokens.first().is_some_and(|(_, wildcard)| *wildcard);
    let trailing = tokens.last().is_some_and(|(_, wildcard)| *wildcard);
    let start = usize::from(leading);
    let end = tokens
        .len()
        .saturating_sub(usize::from(trailing))
        .max(start);
    let middle = &tokens[start..end];
    if middle.iter().any(|(_, wildcard)| *wildcard) {
        return None;
    }
    Some((middle.iter().map(|(ch, _)| ch).collect(), leading, trailing))
}

/// The PostgreSQL array type of `values`, when they share one.
#[cfg(feature = "postgres")]
fn shared_array_type(values: &[Value]) -> Option<crate::orm::sea_query::ArrayType> {
    let array_type = values.first()?.array_type();
    values
        .iter()
        .all(|value| value.array_type() == array_type)
        .then_some(array_type)
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
