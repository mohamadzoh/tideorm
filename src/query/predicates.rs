use super::{ConditionValue, Operator, QueryBuilder, WhereCondition};
use crate::config::DatabaseType;
use crate::internal::Value;
use crate::model::Model;
use crate::query::db_sql;

impl<M: Model> QueryBuilder<M> {
    /// Render a bound-parameter placeholder using the backend's own marker.
    ///
    /// `Expr::cust_with_values` substitutes only tokens matching the marker the
    /// target query builder emits — `$n` on PostgreSQL, `?` everywhere else — so
    /// a fragment written with the wrong marker is passed through verbatim and
    /// binds nothing.
    fn bound_parameter_placeholder(db_type: DatabaseType, index: usize) -> String {
        match db_type {
            DatabaseType::Postgres => format!("${}", index),
            DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => "?".to_string(),
        }
    }

    /// Render `subquery` as both an executable and a preview operand.
    ///
    /// The executable rendering keeps every bound value out of the SQL text, so
    /// the fragment the crate later executes never contains hand-escaped user
    /// data. Validation deliberately runs against that parameterized rendering:
    /// the operand handed to the raw-SQL scanner then holds placeholders only,
    /// which is why a legitimate value containing `--`, `;` or `#` can no longer
    /// reject the whole query.
    ///
    /// The second rendering is the inline-literal one and stays preview-only.
    fn render_subquery_operand<N: Model>(
        &mut self,
        method: &str,
        db_type: DatabaseType,
        subquery: &QueryBuilder<N>,
    ) -> (String, Vec<Value>, String) {
        let (sql, values) = subquery.to_subquery_sql_with_params(db_type);

        if let Err(reason) = db_sql::validate_compound_subquery_sql(&sql) {
            self.invalidate_query(format!("invalid subquery for {}(): {}", method, reason));
        }

        let preview_sql = subquery.build_select_sql_for_db(db_type);
        (sql, values, preview_sql)
    }

    /// Build and push a parameterized `IN (subquery)` / `NOT IN (subquery)`
    /// condition shared by `where_in_subquery` and `where_not_in_subquery`.
    fn push_subquery_membership_condition<N: Model>(
        mut self,
        method: &str,
        column: &str,
        negated: bool,
        subquery: &QueryBuilder<N>,
    ) -> Self {
        if let Err(err) = subquery.ensure_query_is_valid() {
            self.invalidate_query(format!("invalid subquery for {}(): {}", method, err));
        }

        let db_type = self.db_type_for_sql();
        let (sql, values, preview_sql) = self.render_subquery_operand(method, db_type, subquery);
        let prefix = if negated { "NOT " } else { "" };

        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::Raw,
            value: ConditionValue::RawExprWithValues {
                sql: format!("{}IN ({})", prefix, sql),
                values,
                preview_sql: format!("{}IN ({})", prefix, preview_sql),
            },
        });
        self
    }

    /// Build and push a parameterized `EXISTS` / `NOT EXISTS` condition shared
    /// by `where_exists` and `where_not_exists`.
    fn push_subquery_exists_condition<N: Model>(
        mut self,
        method: &str,
        negated: bool,
        subquery: &QueryBuilder<N>,
    ) -> Self {
        if let Err(err) = subquery.ensure_query_is_valid() {
            self.invalidate_query(format!("invalid subquery for {}(): {}", method, err));
        }

        let db_type = self.db_type_for_sql();
        let (sql, values, preview_sql) = self.render_subquery_operand(method, db_type, subquery);
        let prefix = if negated { "NOT " } else { "" };

        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExprWithValues {
                sql: format!("{}EXISTS ({})", prefix, sql),
                values,
                preview_sql: format!("{}EXISTS ({})", prefix, preview_sql),
            },
        });
        self
    }

    /// Add a WHERE IN (subquery) condition.
    #[must_use]
    pub fn where_in_subquery<N: Model>(self, column: &str, subquery: QueryBuilder<N>) -> Self {
        self.push_subquery_membership_condition("where_in_subquery", column, false, &subquery)
    }

    /// Add a WHERE NOT IN (subquery) condition.
    #[must_use]
    pub fn where_not_in_subquery<N: Model>(self, column: &str, subquery: QueryBuilder<N>) -> Self {
        self.push_subquery_membership_condition("where_not_in_subquery", column, true, &subquery)
    }

    /// Add a WHERE EXISTS (subquery) condition.
    #[must_use]
    pub fn where_exists<N: Model>(self, subquery: QueryBuilder<N>) -> Self {
        self.push_subquery_exists_condition("where_exists", false, &subquery)
    }

    /// Add a WHERE NOT EXISTS (subquery) condition.
    #[must_use]
    pub fn where_not_exists<N: Model>(self, subquery: QueryBuilder<N>) -> Self {
        self.push_subquery_exists_condition("where_not_exists", true, &subquery)
    }

    /// Render a comparison literal for the correlated `EXISTS` helpers.
    ///
    /// This feeds the preview rendering only — the executable rendering binds
    /// the same value as a parameter — so the literal is escaped with the shared
    /// backend-aware escaper instead of a local `'` replacement.
    fn related_condition_literal_sql(db_type: DatabaseType, value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Null => "NULL".to_string(),
            serde_json::Value::Bool(boolean) => boolean.to_string(),
            serde_json::Value::Number(number) => number.to_string(),
            serde_json::Value::String(text) => {
                format!("'{}'", db_sql::escape_sql_literal(db_type, text))
            }
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => format!(
                "'{}'",
                db_sql::escape_sql_literal(db_type, &value.to_string())
            ),
        }
    }

    /// Build and push the correlated `EXISTS` / `NOT EXISTS` condition shared by
    /// the `has_related` family, validating and backend-quoting every identifier.
    ///
    /// The comparison value is bound as a parameter rather than escaped into the
    /// SQL text; only the parallel preview rendering keeps an inline literal.
    fn push_related_exists_condition(
        mut self,
        method: &str,
        negated: bool,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
        condition: Option<(&str, serde_json::Value)>,
    ) -> Self {
        let db_type = self.db_type_for_sql();

        let mut identifiers = vec![
            ("related table", related_table),
            ("foreign key", foreign_key),
            ("local key", local_key),
        ];
        if let Some((condition_column, _)) = &condition {
            identifiers.push(("condition column", *condition_column));
        }

        for (kind, identifier) in identifiers {
            if let Err(reason) =
                db_sql::validate_identifier(&format!("{}() {}", method, kind), identifier)
            {
                self.invalidate_query(reason);
            }
        }

        let related = db_sql::quote_ident(db_type, related_table);
        let mut exists_sql = format!(
            "{}EXISTS (SELECT 1 FROM {} WHERE {}.{} = {}.{}",
            if negated { "NOT " } else { "" },
            related,
            related,
            db_sql::quote_ident(db_type, foreign_key),
            db_sql::quote_ident(db_type, M::table_name()),
            db_sql::quote_ident(db_type, local_key),
        );
        let mut preview_sql = exists_sql.clone();
        let mut values = Vec::new();

        if let Some((condition_column, value)) = condition {
            let condition_column = db_sql::quote_ident(db_type, condition_column);
            exists_sql.push_str(&format!(
                " AND {}.{} = {}",
                related,
                condition_column,
                Self::bound_parameter_placeholder(db_type, values.len() + 1),
            ));
            preview_sql.push_str(&format!(
                " AND {}.{} = {}",
                related,
                condition_column,
                Self::related_condition_literal_sql(db_type, &value),
            ));
            values.push(crate::internal::json_to_db_value(&value));
        }

        exists_sql.push(')');
        preview_sql.push(')');

        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExprWithValues {
                sql: exists_sql,
                values,
                preview_sql,
            },
        });
        self
    }

    /// Check if related records exist matching a condition.
    ///
    /// Every identifier must be a plain `table`/`column` name; anything else
    /// invalidates the query instead of being spliced into SQL.
    #[must_use]
    pub fn has_related(
        self,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
        condition_column: &str,
        condition_value: impl Into<serde_json::Value>,
    ) -> Self {
        self.push_related_exists_condition(
            "has_related",
            false,
            related_table,
            foreign_key,
            local_key,
            Some((condition_column, condition_value.into())),
        )
    }

    /// Check if related records do NOT exist matching a condition.
    ///
    /// Every identifier must be a plain `table`/`column` name; anything else
    /// invalidates the query instead of being spliced into SQL.
    #[must_use]
    pub fn has_no_related(
        self,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
        condition_column: &str,
        condition_value: impl Into<serde_json::Value>,
    ) -> Self {
        self.push_related_exists_condition(
            "has_no_related",
            true,
            related_table,
            foreign_key,
            local_key,
            Some((condition_column, condition_value.into())),
        )
    }

    /// Check if any related records exist (without condition).
    ///
    /// Every identifier must be a plain `table`/`column` name; anything else
    /// invalidates the query instead of being spliced into SQL.
    #[must_use]
    pub fn has_any_related(self, related_table: &str, foreign_key: &str, local_key: &str) -> Self {
        self.push_related_exists_condition(
            "has_any_related",
            false,
            related_table,
            foreign_key,
            local_key,
            None,
        )
    }

    /// Check if no related records exist.
    ///
    /// Every identifier must be a plain `table`/`column` name; anything else
    /// invalidates the query instead of being spliced into SQL.
    #[must_use]
    pub fn has_no_related_at_all(
        self,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
    ) -> Self {
        self.push_related_exists_condition(
            "has_no_related_at_all",
            true,
            related_table,
            foreign_key,
            local_key,
            None,
        )
    }

    /// Convert this query builder to a subquery SQL string.
    ///
    /// The rendering inlines every bound value as an escaped SQL literal, so it
    /// is only safe for display. Use [`Self::to_subquery_sql_with_params`] for
    /// anything that ends up being executed.
    pub fn to_subquery_sql(&self) -> String {
        self.build_select_sql()
    }

    /// Convert this query builder to a parameterized subquery operand.
    ///
    /// Returns the SQL together with the values bound to it. The placeholders
    /// use `db_type`'s own marker (`$1..$n` on PostgreSQL, `?` elsewhere), which
    /// is what `Expr::cust_with_values` renumbers into a surrounding statement,
    /// so the operand must be rendered for the same backend that will execute
    /// it.
    pub fn to_subquery_sql_with_params(&self, db_type: DatabaseType) -> (String, Vec<Value>) {
        self.build_select_sql_with_params_for_db(db_type)
    }

    /// Add a raw WHERE condition.
    #[must_use]
    pub fn where_raw(mut self, raw_sql: &str) -> Self {
        if let Err(reason) =
            crate::query::db_sql::validate_raw_sql_fragment("WHERE raw SQL", raw_sql)
        {
            self.invalidate_query(reason);
        }

        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(raw_sql.to_string()),
        });
        self
    }

    /// Add a raw WHERE condition with a column comparison.
    #[must_use]
    pub fn where_column_raw(mut self, column: &str, raw_expr: &str) -> Self {
        if let Err(reason) =
            crate::query::db_sql::validate_raw_sql_fragment("WHERE raw column expression", raw_expr)
        {
            self.invalidate_query(reason);
        }

        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(raw_expr.to_string()),
        });
        self
    }

    /// Add a raw SELECT expression.
    #[must_use]
    pub fn select_raw(mut self, raw_select: &str) -> Self {
        if let Err(reason) =
            crate::query::db_sql::validate_raw_sql_fragment("SELECT raw SQL", raw_select)
        {
            self.invalidate_query(reason);
        }

        self.raw_select_expressions.push(raw_select.to_string());
        self
    }

    /// Add a scalar subquery as a SELECT expression.
    #[must_use]
    pub fn select_subquery<N: Model>(mut self, subquery: QueryBuilder<N>, alias: &str) -> Self {
        if let Err(err) = subquery.ensure_query_is_valid() {
            self.invalidate_query(format!("invalid subquery for select_subquery(): {}", err));
        }

        if let Err(reason) = crate::query::db_sql::validate_identifier("SELECT alias", alias) {
            self.invalidate_query(reason);
        }

        let subquery_sql = subquery.to_subquery_sql();
        self.subquery_select_expressions
            .push((subquery_sql, alias.to_string()));
        self
    }

    /// Add a where IS NULL condition.
    #[must_use]
    pub fn where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNull,
            value: ConditionValue::None,
        });
        self
    }

    /// Add a where IS NOT NULL condition.
    #[must_use]
    pub fn where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::IsNotNull,
            value: ConditionValue::None,
        });
        self
    }

    /// Add a where BETWEEN condition.
    #[must_use]
    pub fn where_between(
        mut self,
        column: impl crate::columns::IntoColumnName,
        low: impl Into<serde_json::Value>,
        high: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.column_name().to_string(),
            operator: Operator::Between,
            value: ConditionValue::Range(low.into(), high.into()),
        });
        self
    }

    /// Add a JSON contains condition (column @> value).
    #[must_use]
    pub fn where_json_contains(
        mut self,
        column: &str,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonContains,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a JSON contained by condition (column <@ value).
    #[must_use]
    pub fn where_json_contained_by(
        mut self,
        column: &str,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonContainedBy,
            value: ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a JSON key exists condition (column ? key).
    #[must_use]
    pub fn where_json_key_exists(mut self, column: &str, key: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonKeyExists,
            value: ConditionValue::Single(serde_json::Value::String(key.to_string())),
        });
        self
    }

    /// Add a JSON key does not exist condition.
    #[must_use]
    pub fn where_json_key_not_exists(mut self, column: &str, key: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonKeyNotExists,
            value: ConditionValue::Single(serde_json::Value::String(key.to_string())),
        });
        self
    }

    /// Add a JSON path exists condition.
    #[must_use]
    pub fn where_json_path_exists(mut self, column: &str, path: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonPathExists,
            value: ConditionValue::Single(serde_json::Value::String(path.to_string())),
        });
        self
    }

    /// Add a JSON path does not exist condition.
    #[must_use]
    pub fn where_json_path_not_exists(mut self, column: &str, path: &str) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::JsonPathNotExists,
            value: ConditionValue::Single(serde_json::Value::String(path.to_string())),
        });
        self
    }

    /// Add an array contains condition (column @> value).
    #[must_use]
    pub fn where_array_contains<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        value: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::ArrayContains,
            value: ConditionValue::List(value.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add an array contained by condition (column <@ value).
    #[must_use]
    pub fn where_array_contained_by<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        value: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::ArrayContainedBy,
            value: ConditionValue::List(value.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add an array overlaps condition (column && value).
    #[must_use]
    pub fn where_array_overlaps<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        value: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::ArrayOverlaps,
            value: ConditionValue::List(value.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add an array contains any element condition.
    #[must_use]
    pub fn where_array_contains_any<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        value: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::ArrayContainsAny,
            value: ConditionValue::List(value.into_iter().map(|v| v.into()).collect()),
        });
        self
    }

    /// Add an array contains all elements condition.
    #[must_use]
    pub fn where_array_contains_all<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        value: Vec<V>,
    ) -> Self {
        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::ArrayContainsAll,
            value: ConditionValue::List(value.into_iter().map(|v| v.into()).collect()),
        });
        self
    }
}
