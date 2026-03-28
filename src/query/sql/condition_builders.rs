use super::*;

#[allow(missing_docs)]
impl<M: Model> QueryBuilder<M> {
    fn json_to_sea_value(value: &serde_json::Value) -> Value {
        match value {
            serde_json::Value::Null => Value::String(None),
            serde_json::Value::Bool(boolean) => Value::Bool(Some(*boolean)),
            serde_json::Value::Number(number) => {
                if let Some(integer) = number.as_i64() {
                    Value::BigInt(Some(integer))
                } else if let Some(float) = number.as_f64() {
                    Value::Double(Some(float))
                } else {
                    Value::String(Some(number.to_string()))
                }
            }
            serde_json::Value::String(text) => Value::String(Some(text.clone())),
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                Value::String(Some(value.to_string()))
            }
        }
    }

    pub(crate) fn build_sea_condition(&self) -> Condition {
        self.build_sea_condition_for_db(self.db_type_for_sql())
    }

    fn build_sea_condition_for_db(&self, db_type: DatabaseType) -> Condition {
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

    pub(super) fn current_timestamp_sql() -> &'static str {
        "CURRENT_TIMESTAMP"
    }

    fn sea_value_list(values: &[serde_json::Value]) -> Vec<Value> {
        values.iter().map(Self::json_to_sea_value).collect()
    }

    fn json_text_value(text: String) -> Value {
        Value::String(Some(text))
    }

    fn json_array_parameter(values: &[serde_json::Value]) -> Value {
        Self::json_text_value(
            serde_json::to_string(values)
                .expect("serializing array predicate values should not fail"),
        )
    }

    fn json_scalar_parameter(value: &serde_json::Value) -> Value {
        Self::json_text_value(
            serde_json::to_string(value)
                .expect("serializing scalar predicate value should not fail"),
        )
    }

    fn json_parameter(value: &serde_json::Value) -> Value {
        Value::Json(Some(Box::new(value.clone())))
    }

    fn sqlite_json_compare_value(value: &serde_json::Value) -> Value {
        match value {
            serde_json::Value::String(text) => Value::String(Some(text.clone())),
            serde_json::Value::Null => Value::String(Some("null".to_string())),
            serde_json::Value::Bool(boolean) => Value::Bool(Some(*boolean)),
            serde_json::Value::Number(number) => {
                if let Some(integer) = number.as_i64() {
                    Value::BigInt(Some(integer))
                } else if let Some(float) = number.as_f64() {
                    Value::Double(Some(float))
                } else {
                    Value::String(Some(number.to_string()))
                }
            }
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                Value::String(Some(value.to_string()))
            }
        }
    }

    fn placeholder_list(count: usize) -> String {
        std::iter::repeat_n("?", count)
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn sea_column_expr(&self, db_type: DatabaseType, column: &str) -> SimpleExpr {
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

    fn build_custom_expression(&self, sql: String, values: Vec<Value>) -> SimpleExpr {
        if values.is_empty() {
            Expr::cust(sql)
        } else {
            Expr::cust_with_values(sql, values)
        }
    }

    pub(super) fn condition_spec<'a>(condition: &'a WhereCondition) -> Option<ConditionSpec<'a>> {
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

    fn preview_values(&self, values: &[serde_json::Value]) -> Vec<String> {
        values
            .iter()
            .map(|value| self.format_preview_value(value))
            .collect()
    }

    fn pattern_value(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(text) => text.clone(),
            _ => value.to_string(),
        }
    }

    fn comparison_sql(operator: ComparisonOperator) -> &'static str {
        match operator {
            ComparisonOperator::Eq => "=",
            ComparisonOperator::NotEq => "!=",
            ComparisonOperator::Gt => ">",
            ComparisonOperator::Gte => ">=",
            ComparisonOperator::Lt => "<",
            ComparisonOperator::Lte => "<=",
        }
    }

    fn build_raw_condition_expression(
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

    pub(super) fn build_raw_condition_sql(
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

    fn build_compare_expression(
        &self,
        column_expr: SimpleExpr,
        operator: ComparisonOperator,
        value: &serde_json::Value,
    ) -> SimpleExpr {
        let value = Self::json_to_sea_value(value);
        match operator {
            ComparisonOperator::Eq => column_expr.eq(value),
            ComparisonOperator::NotEq => column_expr.ne(value),
            ComparisonOperator::Gt => column_expr.gt(value),
            ComparisonOperator::Gte => column_expr.gte(value),
            ComparisonOperator::Lt => column_expr.lt(value),
            ComparisonOperator::Lte => column_expr.lte(value),
        }
    }

    pub(super) fn build_compare_sql(
        &self,
        column: &str,
        operator: ComparisonOperator,
        value: &serde_json::Value,
    ) -> String {
        format!(
            "{} {} {}",
            column,
            Self::comparison_sql(operator),
            self.format_preview_value(value)
        )
    }

    fn build_pattern_expression(
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
            let escape_clause = match db_type {
                DatabaseType::Postgres => " ESCAPE '\\'",
                DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => {
                    " ESCAPE '\\\\'"
                }
            };
            self.build_custom_expression(
                format!(
                    "{} {} {}{}",
                    column_sql, operator, placeholder, escape_clause
                ),
                vec![Value::String(Some(pattern))],
            )
        } else if negated {
            column_expr.not_like(pattern)
        } else {
            column_expr.like(pattern)
        }
    }

    pub(super) fn build_pattern_sql(
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
            self.format_preview_value(value)
        );
        if escaped {
            sql.push_str(match db_type {
                DatabaseType::Postgres => " ESCAPE '\\'",
                DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => {
                    " ESCAPE '\\\\'"
                }
            });
        }
        sql
    }

    fn build_list_expression(
        &self,
        db_type: DatabaseType,
        column_expr: SimpleExpr,
        column_sql: &str,
        operator: ListOperator,
        values: &[serde_json::Value],
    ) -> SimpleExpr {
        let sea_values = Self::sea_value_list(values);
        match operator {
            ListOperator::In => column_expr.is_in(sea_values),
            ListOperator::NotIn => column_expr.is_not_in(sea_values),
            ListOperator::EqAny if matches!(db_type, DatabaseType::Postgres) => self
                .build_custom_expression(
                    format!(
                        "{} = ANY(ARRAY[{}])",
                        column_sql,
                        Self::placeholder_list(values.len())
                    ),
                    sea_values,
                ),
            ListOperator::EqAny => column_expr.is_in(sea_values),
            ListOperator::NeAll if matches!(db_type, DatabaseType::Postgres) => self
                .build_custom_expression(
                    format!(
                        "{} <> ALL(ARRAY[{}])",
                        column_sql,
                        Self::placeholder_list(values.len())
                    ),
                    sea_values,
                ),
            ListOperator::NeAll => column_expr.is_not_in(sea_values),
        }
    }

    pub(super) fn build_list_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        operator: ListOperator,
        values: &[serde_json::Value],
    ) -> String {
        let rendered = self.preview_values(values);
        match operator {
            ListOperator::In => format!("{} IN ({})", column, rendered.join(", ")),
            ListOperator::NotIn => format!("{} NOT IN ({})", column, rendered.join(", ")),
            ListOperator::EqAny => db_sql::eq_any(db_type, column, &rendered),
            ListOperator::NeAll => db_sql::ne_all(db_type, column, &rendered),
        }
    }

    fn build_null_check_expression(&self, column_expr: SimpleExpr, negated: bool) -> SimpleExpr {
        if negated {
            column_expr.is_not_null()
        } else {
            column_expr.is_null()
        }
    }

    pub(super) fn build_null_check_sql(&self, column: &str, negated: bool) -> String {
        format!("{} IS {}NULL", column, if negated { "NOT " } else { "" })
    }

    fn build_between_expression(
        &self,
        column_expr: SimpleExpr,
        low: &serde_json::Value,
        high: &serde_json::Value,
    ) -> SimpleExpr {
        column_expr.between(Self::json_to_sea_value(low), Self::json_to_sea_value(high))
    }

    pub(super) fn build_between_sql(
        &self,
        column: &str,
        low: &serde_json::Value,
        high: &serde_json::Value,
    ) -> String {
        format!(
            "{} BETWEEN {} AND {}",
            column,
            self.format_preview_value(low),
            self.format_preview_value(high)
        )
    }

    pub(super) fn build_json_value_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        operator: JsonValueOperator,
        value: &serde_json::Value,
    ) -> String {
        match operator {
            JsonValueOperator::Contains => {
                db_sql::json_contains(db_type, column, &value.to_string())
            }
            JsonValueOperator::ContainedBy => {
                db_sql::json_contained_by(db_type, column, &value.to_string())
            }
        }
    }

    fn build_json_value_expression(
        &self,
        db_type: DatabaseType,
        column_expr: SimpleExpr,
        column_sql: &str,
        operator: JsonValueOperator,
        value: &serde_json::Value,
    ) -> SimpleExpr {
        match db_type {
            DatabaseType::Postgres => {
                match operator {
                    JsonValueOperator::Contains => column_expr
                        .binary(PgBinOper::Contains, Expr::val(Self::json_parameter(value))),
                    JsonValueOperator::ContainedBy => column_expr
                        .binary(PgBinOper::Contained, Expr::val(Self::json_parameter(value))),
                }
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => match operator {
                JsonValueOperator::Contains => self.build_custom_expression(
                    format!("JSON_CONTAINS({}, CAST(? AS JSON))", column_sql),
                    vec![Self::json_scalar_parameter(value)],
                ),
                JsonValueOperator::ContainedBy => self.build_custom_expression(
                    format!("JSON_CONTAINS(CAST(? AS JSON), {})", column_sql),
                    vec![Self::json_scalar_parameter(value)],
                ),
            },
            DatabaseType::SQLite => match operator {
                JsonValueOperator::Contains => self.build_custom_expression(
                    format!(
                        "EXISTS (SELECT 1 FROM json_each({}) WHERE value = ?)",
                        column_sql
                    ),
                    vec![Self::sqlite_json_compare_value(value)],
                ),
                JsonValueOperator::ContainedBy => self.build_custom_expression(
                    format!(
                        "json_type({}) IS NOT NULL AND ? LIKE '%' || {} || '%'",
                        column_sql, column_sql
                    ),
                    vec![Self::json_scalar_parameter(value)],
                ),
            },
        }
    }

    pub(super) fn build_json_string_sql(
        &self,
        db_type: DatabaseType,
        column: &str,
        operator: JsonStringOperator,
        value: &str,
    ) -> String {
        match operator {
            JsonStringOperator::KeyPresent => db_sql::json_key_exists(db_type, column, value),
            JsonStringOperator::KeyAbsent => db_sql::json_key_not_exists(db_type, column, value),
            JsonStringOperator::PathPresent => db_sql::json_path_exists(db_type, column, value),
            JsonStringOperator::PathAbsent => db_sql::json_path_not_exists(db_type, column, value),
        }
    }

    fn build_json_string_expression(
        &self,
        db_type: DatabaseType,
        column_sql: &str,
        operator: JsonStringOperator,
        value: &str,
    ) -> SimpleExpr {
        match operator {
            JsonStringOperator::KeyPresent => match db_type {
                DatabaseType::Postgres => self.build_custom_expression(
                    format!("{} ? $1", column_sql),
                    vec![Value::String(Some(value.to_string()))],
                ),
                DatabaseType::MySQL | DatabaseType::MariaDB => self.build_custom_expression(
                    format!("JSON_CONTAINS_PATH({}, 'one', ?)", column_sql),
                    vec![Value::String(Some(db_sql::canonical_json_member_path(
                        value,
                    )))],
                ),
                DatabaseType::SQLite => self.build_custom_expression(
                    format!("json_extract({}, ?) IS NOT NULL", column_sql),
                    vec![Value::String(Some(db_sql::canonical_json_member_path(
                        value,
                    )))],
                ),
            },
            JsonStringOperator::KeyAbsent => match db_type {
                DatabaseType::Postgres => self.build_custom_expression(
                    format!("NOT ({} ? $1)", column_sql),
                    vec![Value::String(Some(value.to_string()))],
                ),
                DatabaseType::MySQL | DatabaseType::MariaDB => self.build_custom_expression(
                    format!("NOT JSON_CONTAINS_PATH({}, 'one', ?)", column_sql),
                    vec![Value::String(Some(db_sql::canonical_json_member_path(
                        value,
                    )))],
                ),
                DatabaseType::SQLite => self.build_custom_expression(
                    format!("json_extract({}, ?) IS NULL", column_sql),
                    vec![Value::String(Some(db_sql::canonical_json_member_path(
                        value,
                    )))],
                ),
            },
            JsonStringOperator::PathPresent => match db_type {
                DatabaseType::Postgres => self.build_custom_expression(
                    format!("{} @? ($1::jsonpath)", column_sql),
                    vec![Value::String(Some(value.to_string()))],
                ),
                DatabaseType::MySQL | DatabaseType::MariaDB => {
                    let Some(path) = db_sql::normalize_mysql_sqlite_json_path(value) else {
                        return Expr::cust(db_sql::invalid_json_path_predicate(true));
                    };
                    self.build_custom_expression(
                        format!("JSON_CONTAINS_PATH({}, 'one', ?)", column_sql),
                        vec![Value::String(Some(path))],
                    )
                }
                DatabaseType::SQLite => {
                    let Some(path) = db_sql::normalize_mysql_sqlite_json_path(value) else {
                        return Expr::cust(db_sql::invalid_json_path_predicate(true));
                    };
                    self.build_custom_expression(
                        format!("json_extract({}, ?) IS NOT NULL", column_sql),
                        vec![Value::String(Some(path))],
                    )
                }
            },
            JsonStringOperator::PathAbsent => match db_type {
                DatabaseType::Postgres => self.build_custom_expression(
                    format!("NOT ({} @? ($1::jsonpath))", column_sql),
                    vec![Value::String(Some(value.to_string()))],
                ),
                DatabaseType::MySQL | DatabaseType::MariaDB => {
                    let Some(path) = db_sql::normalize_mysql_sqlite_json_path(value) else {
                        return Expr::cust(db_sql::invalid_json_path_predicate(false));
                    };
                    self.build_custom_expression(
                        format!("NOT JSON_CONTAINS_PATH({}, 'one', ?)", column_sql),
                        vec![Value::String(Some(path))],
                    )
                }
                DatabaseType::SQLite => {
                    let Some(path) = db_sql::normalize_mysql_sqlite_json_path(value) else {
                        return Expr::cust(db_sql::invalid_json_path_predicate(false));
                    };
                    self.build_custom_expression(
                        format!("json_extract({}, ?) IS NULL", column_sql),
                        vec![Value::String(Some(path))],
                    )
                }
            },
        }
    }

    pub(super) fn build_array_sql(
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

    fn build_array_expression(
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
                Expr::cust(self.build_array_sql(db_type, column_sql, operator, values))
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

    fn build_subquery_expression(
        &self,
        column_sql: &str,
        negated: bool,
        query_sql: &str,
    ) -> SimpleExpr {
        Expr::cust(format!(
            "{} {}IN ({})",
            column_sql,
            if negated { "NOT " } else { "" },
            query_sql
        ))
    }

    pub(super) fn build_subquery_sql(
        &self,
        column: &str,
        negated: bool,
        query_sql: &str,
    ) -> String {
        format!(
            "{} {}IN ({})",
            column,
            if negated { "NOT " } else { "" },
            query_sql
        )
    }

    fn build_condition_expression(
        &self,
        condition: &WhereCondition,
        db_type: DatabaseType,
    ) -> Option<SimpleExpr> {
        let spec = Self::condition_spec(condition)?;

        if let ConditionSpec::Raw { column, raw_sql } = spec {
            return Some(self.build_raw_condition_expression(db_type, column, raw_sql));
        }

        let column_expr = self.sea_column_expr(db_type, &condition.column);
        let column_sql = self.format_column_for_db(db_type, &condition.column);

        match spec {
            ConditionSpec::Raw { .. } => None,
            ConditionSpec::Compare { operator, value } => {
                Some(self.build_compare_expression(column_expr, operator, value))
            }
            ConditionSpec::Pattern {
                negated,
                escaped,
                value,
            } => Some(self.build_pattern_expression(
                db_type,
                column_expr,
                &column_sql,
                negated,
                escaped,
                value,
            )),
            ConditionSpec::List { operator, values } => Some(self.build_list_expression(
                db_type,
                column_expr,
                &column_sql,
                operator,
                values,
            )),
            ConditionSpec::NullCheck { negated } => {
                Some(self.build_null_check_expression(column_expr, negated))
            }
            ConditionSpec::Between { low, high } => {
                Some(self.build_between_expression(column_expr, low, high))
            }
            ConditionSpec::JsonValue { operator, value } => Some(self.build_json_value_expression(
                db_type,
                column_expr,
                &column_sql,
                operator,
                value,
            )),
            ConditionSpec::JsonString { operator, value } => {
                Some(self.build_json_string_expression(db_type, &column_sql, operator, value))
            }
            ConditionSpec::Array { operator, values } => Some(self.build_array_expression(
                db_type,
                column_expr,
                &column_sql,
                operator,
                values,
            )),
            ConditionSpec::Subquery { negated, query_sql } => {
                Some(self.build_subquery_expression(&column_sql, negated, query_sql))
            }
        }
    }

    fn build_or_group_condition(&self, group: &OrGroup, db_type: DatabaseType) -> Condition {
        let mut condition = match group.combine_with {
            LogicalOp::And => Condition::all(),
            LogicalOp::Or => Condition::any(),
        };

        for filter in &group.conditions {
            if let Some(expression) = self.build_condition_expression(filter, db_type) {
                condition = condition.add(expression);
            }
        }

        for nested_group in &group.nested_groups {
            if !nested_group.is_empty() {
                condition = condition.add(self.build_or_group_condition(nested_group, db_type));
            }
        }

        condition
    }

    fn build_soft_delete_expression(&self, db_type: DatabaseType) -> Option<SimpleExpr> {
        match query_scope_for::<M>(self.include_trashed, self.only_trashed) {
            SoftDeleteScope::Disabled | SoftDeleteScope::WithTrashed => None,
            SoftDeleteScope::ActiveOnly => Some(
                self.sea_column_expr(db_type, M::deleted_at_column())
                    .is_null(),
            ),
            SoftDeleteScope::OnlyTrashed => Some(
                self.sea_column_expr(db_type, M::deleted_at_column())
                    .is_not_null(),
            ),
        }
    }

    pub(crate) fn build_where_clause_with_condition_for_db(
        &self,
        db_type: DatabaseType,
    ) -> (String, Vec<Value>) {
        let has_filters = !self.conditions.is_empty()
            || !self.or_groups.is_empty()
            || self.build_soft_delete_expression(db_type).is_some();
        if !has_filters {
            return (String::new(), Vec::new());
        }

        let mut query = Query::select();
        query.expr(Expr::cust("1"));
        query.cond_where(self.build_sea_condition_for_db(db_type));

        let (sql, values) = match db_type {
            DatabaseType::Postgres => query.build(PostgresQueryBuilder),
            DatabaseType::MySQL | DatabaseType::MariaDB => query.build(MysqlQueryBuilder),
            DatabaseType::SQLite => query.build(SqliteQueryBuilder),
        };

        match sql.split_once(" WHERE ") {
            Some((_, where_sql)) => (where_sql.to_string(), values.into_iter().collect()),
            None => (String::new(), Vec::new()),
        }
    }

    fn format_preview_value(&self, value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Null => "NULL".to_string(),
            serde_json::Value::Bool(boolean) => boolean.to_string(),
            serde_json::Value::Number(number) => number.to_string(),
            // This is only for the non-executable debug preview path. It renders
            // approximate inline literals for humans and must never be reused for
            // executable SQL; real query paths stay parameterized instead.
            serde_json::Value::String(text) => format!("'{}'", text.replace("'", "''")),
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                format!("'{}'", value.to_string().replace("'", "''"))
            }
        }
    }
}
