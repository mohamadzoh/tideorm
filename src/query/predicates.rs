use super::structure::SubquerySelect;
use super::{ConditionValue, Operator, QueryBuilder, WhereCondition};
use crate::columns::IntoColumnName;
use crate::config::DatabaseType;
use crate::internal::Value;
use crate::model::Model;
use crate::query::db_sql;

impl<M: Model> QueryBuilder<M> {
    fn push_condition(mut self, condition: WhereCondition) -> Self {
        self.conditions.push(condition);
        self
    }

    /// Push `<keyword> (subquery)` as a parameterized condition on `column`
    /// (empty for `EXISTS`).
    ///
    /// The subquery is rendered with its values bound rather than inlined, so
    /// the fragment the crate executes never contains hand-escaped user data.
    /// Validation deliberately runs against that parameterized rendering: the
    /// operand handed to the raw-SQL scanner then holds placeholders only, which
    /// is why a legitimate value containing `--`, `;` or `#` cannot reject the
    /// whole query.
    fn push_subquery_condition<N: Model>(
        mut self,
        method: &str,
        column: &str,
        keyword: &str,
        subquery: &QueryBuilder<N>,
    ) -> Self {
        if let Err(err) = subquery.ensure_query_is_executable() {
            self.invalidate_query(format!("invalid subquery for {}(): {}", method, err));
        }

        let (sql, values) = subquery.to_subquery_sql_with_params(self.db_type_for_sql());
        if let Err(reason) = db_sql::validate_compound_subquery_sql(&sql) {
            self.invalidate_query(format!("invalid subquery for {}(): {}", method, reason));
        }

        self.conditions.push(WhereCondition {
            column: column.to_string(),
            operator: Operator::Raw,
            value: ConditionValue::RawExprWithValues {
                sql: format!("{} ({})", keyword, sql),
                values,
            },
        });
        self
    }

    /// Add a WHERE IN (subquery) condition.
    #[must_use]
    pub fn where_in_subquery<N: Model>(
        self,
        column: impl IntoColumnName,
        subquery: QueryBuilder<N>,
    ) -> Self {
        self.push_subquery_condition("where_in_subquery", column.column_name(), "IN", &subquery)
    }

    /// Add a WHERE NOT IN (subquery) condition.
    #[must_use]
    pub fn where_not_in_subquery<N: Model>(
        self,
        column: impl IntoColumnName,
        subquery: QueryBuilder<N>,
    ) -> Self {
        self.push_subquery_condition(
            "where_not_in_subquery",
            column.column_name(),
            "NOT IN",
            &subquery,
        )
    }

    /// Add a WHERE EXISTS (subquery) condition.
    #[must_use]
    pub fn where_exists<N: Model>(self, subquery: QueryBuilder<N>) -> Self {
        self.push_subquery_condition("where_exists", "", "EXISTS", &subquery)
    }

    /// Add a WHERE NOT EXISTS (subquery) condition.
    #[must_use]
    pub fn where_not_exists<N: Model>(self, subquery: QueryBuilder<N>) -> Self {
        self.push_subquery_condition("where_not_exists", "", "NOT EXISTS", &subquery)
    }

    /// Build and push the correlated `EXISTS` / `NOT EXISTS` condition shared by
    /// the `has_related` family, validating and backend-quoting every identifier.
    ///
    /// The comparison value is bound as a parameter rather than escaped into the
    /// SQL text.
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
        let mut sql = format!(
            "{}EXISTS (SELECT 1 FROM {} WHERE {}.{} = {}.{}",
            if negated { "NOT " } else { "" },
            related,
            related,
            db_sql::quote_ident(db_type, foreign_key),
            db_sql::quote_ident(db_type, M::table_name()),
            db_sql::quote_ident(db_type, local_key),
        );
        let mut values = Vec::new();

        if let Some((condition_column, value)) = condition {
            sql.push_str(&format!(
                " AND {}.{} = {}",
                related,
                db_sql::quote_ident(db_type, condition_column),
                db_sql::placeholder(db_type, 1),
            ));
            values.push(crate::internal::json_to_db_value(&value));
        }

        sql.push(')');

        self.conditions.push(WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExprWithValues { sql, values },
        });
        self
    }

    /// Check if related records exist matching a condition.
    ///
    /// Every identifier must be a plain `table`/`column` name; anything else
    /// invalidates the query instead of being spliced into SQL. The table is
    /// named, not modeled, so its soft-deleted rows count too; to skip them,
    /// pass the related model's query to [`where_exists`](Self::where_exists).
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

    /// Check if any related records exist (without condition).
    ///
    /// Every identifier must be a plain `table`/`column` name; anything else
    /// invalidates the query instead of being spliced into SQL. The table is
    /// named, not modeled, so its soft-deleted rows count too; to skip them,
    /// pass the related model's query to [`where_exists`](Self::where_exists).
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
    /// invalidates the query instead of being spliced into SQL. The table is
    /// named, not modeled, so its soft-deleted rows count too; to skip them,
    /// pass the related model's query to
    /// [`where_not_exists`](Self::where_not_exists).
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

    /// Render this query with its bound values inlined, for display only.
    ///
    /// This is the statement [`build_sql_preview()`](Self::build_sql_preview)
    /// shows, without the banner. Use
    /// [`to_subquery_sql_with_params`](Self::to_subquery_sql_with_params) for
    /// anything that ends up being executed.
    pub fn to_subquery_sql(&self) -> String {
        self.build_select_sql_for_db(self.db_type_for_sql())
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
    ///
    /// **Trusted SQL only.** The fragment is checked by the shared raw-fragment
    /// validator as soon as it is added, and a rejected fragment invalidates the
    /// query.
    #[must_use]
    pub fn where_raw(mut self, raw_sql: &str) -> Self {
        if let Err(reason) = db_sql::validate_raw_sql_fragment("WHERE raw SQL", raw_sql) {
            self.invalidate_query(reason);
        }

        self.push_condition(WhereCondition::new(
            "",
            Operator::Raw,
            ConditionValue::RawExpr(raw_sql.to_string()),
        ))
    }

    /// Add a raw SELECT expression.
    #[must_use]
    pub fn select_raw(mut self, raw_select: &str) -> Self {
        if let Err(reason) = db_sql::validate_raw_sql_fragment("SELECT raw SQL", raw_select) {
            self.invalidate_query(reason);
        }

        self.raw_select_expressions.push(raw_select.to_string());
        self
    }

    /// Add a scalar subquery as a SELECT expression.
    ///
    /// The subquery keeps its values as bound parameters, rendered for this
    /// query's backend.
    #[must_use]
    pub fn select_subquery<N: Model>(mut self, subquery: QueryBuilder<N>, alias: &str) -> Self {
        if let Err(err) = subquery.ensure_query_is_executable() {
            self.invalidate_query(format!("invalid subquery for select_subquery(): {}", err));
        }

        if let Err(reason) = db_sql::validate_identifier("SELECT alias", alias) {
            self.invalidate_query(reason);
        }

        let (query_sql, params) = subquery.to_subquery_sql_with_params(self.db_type_for_sql());
        self.subquery_select_expressions.push(SubquerySelect {
            query_sql,
            alias: alias.to_string(),
            params,
        });
        self
    }

    /// Add a WHERE column = ANY(array) condition.
    ///
    /// Rendered as `column IN (..)`, which is what `= ANY(ARRAY[..])` means and
    /// which binds correctly on every backend.
    #[must_use]
    pub fn eq_any<V: serde::Serialize>(self, column: impl IntoColumnName, values: Vec<V>) -> Self {
        self.push_condition(WhereCondition::new(
            column,
            Operator::EqAny,
            ConditionValue::List(values.into_iter().map(crate::query::filter_value).collect()),
        ))
    }

    /// Add a WHERE column <> ALL(array) condition, rendered as `column NOT IN (..)`.
    #[must_use]
    pub fn ne_all<V: serde::Serialize>(self, column: impl IntoColumnName, values: Vec<V>) -> Self {
        self.push_condition(WhereCondition::new(
            column,
            Operator::NeAll,
            ConditionValue::List(values.into_iter().map(crate::query::filter_value).collect()),
        ))
    }

    /// Add a WHERE condition using a strongly-typed column.
    #[must_use]
    pub fn where_col(mut self, condition: crate::columns::ColumnCondition) -> Self {
        let crate::columns::ColumnCondition {
            column,
            operator,
            value,
        } = condition;

        let value = match (operator, value) {
            (Operator::IsNull | Operator::IsNotNull, _) => ConditionValue::None,
            (Operator::In | Operator::NotIn, serde_json::Value::Array(values)) => {
                ConditionValue::List(values)
            }
            (Operator::In | Operator::NotIn, value) => ConditionValue::List(vec![value]),
            (Operator::Between, serde_json::Value::Array(values)) if values.len() >= 2 => {
                let mut bounds = values.into_iter();
                ConditionValue::Range(
                    bounds.next().unwrap_or(serde_json::Value::Null),
                    bounds.next().unwrap_or(serde_json::Value::Null),
                )
            }
            (Operator::Between, serde_json::Value::Array(_)) => {
                ConditionValue::Single(serde_json::Value::Null)
            }
            (_, value) => ConditionValue::Single(value),
        };

        self.conditions.push(WhereCondition {
            column,
            operator,
            value,
        });
        self
    }

    /// Add a JSON contains condition (column @> value).
    #[must_use]
    pub fn where_json_contains(
        self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.push_condition(WhereCondition::new(
            column,
            Operator::JsonContains,
            ConditionValue::Single(value.into()),
        ))
    }

    /// Add a JSON contained by condition (column <@ value).
    #[must_use]
    pub fn where_json_contained_by(
        self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.push_condition(WhereCondition::new(
            column,
            Operator::JsonContainedBy,
            ConditionValue::Single(value.into()),
        ))
    }

    /// Add a JSON key exists condition (column ? key).
    #[must_use]
    pub fn where_json_key_exists(self, column: impl IntoColumnName, key: &str) -> Self {
        self.push_condition(WhereCondition::new(
            column,
            Operator::JsonKeyExists,
            json_text(key),
        ))
    }

    /// Add a JSON key does not exist condition.
    #[must_use]
    pub fn where_json_key_not_exists(self, column: impl IntoColumnName, key: &str) -> Self {
        self.push_condition(WhereCondition::new(
            column,
            Operator::JsonKeyNotExists,
            json_text(key),
        ))
    }

    /// Add a JSON path exists condition.
    ///
    /// On MySQL, MariaDB and SQLite a path that cannot be expressed there
    /// renders a condition that matches nothing.
    #[must_use]
    pub fn where_json_path_exists(self, column: impl IntoColumnName, path: &str) -> Self {
        self.push_condition(WhereCondition::new(
            column,
            Operator::JsonPathExists,
            json_text(path),
        ))
    }

    /// Add an array contains condition (column @> value).
    #[must_use]
    pub fn where_array_contains<V: Into<serde_json::Value>>(
        self,
        column: impl IntoColumnName,
        value: Vec<V>,
    ) -> Self {
        self.push_condition(WhereCondition::new(
            column,
            Operator::ArrayContains,
            json_list(value),
        ))
    }

    /// Add an array contained by condition (column <@ value).
    #[must_use]
    pub fn where_array_contained_by<V: Into<serde_json::Value>>(
        self,
        column: impl IntoColumnName,
        value: Vec<V>,
    ) -> Self {
        self.push_condition(WhereCondition::new(
            column,
            Operator::ArrayContainedBy,
            json_list(value),
        ))
    }

    /// Add an array overlaps condition (column && value).
    #[must_use]
    pub fn where_array_overlaps<V: Into<serde_json::Value>>(
        self,
        column: impl IntoColumnName,
        value: Vec<V>,
    ) -> Self {
        self.push_condition(WhereCondition::new(
            column,
            Operator::ArrayOverlaps,
            json_list(value),
        ))
    }

    /// Add an array contains any element condition; the same test as
    /// [`where_array_overlaps`](Self::where_array_overlaps).
    #[must_use]
    pub fn where_array_contains_any<V: Into<serde_json::Value>>(
        self,
        column: impl IntoColumnName,
        value: Vec<V>,
    ) -> Self {
        self.where_array_overlaps(column, value)
    }

    /// Add an array contains all elements condition; the same test as
    /// [`where_array_contains`](Self::where_array_contains).
    #[must_use]
    pub fn where_array_contains_all<V: Into<serde_json::Value>>(
        self,
        column: impl IntoColumnName,
        value: Vec<V>,
    ) -> Self {
        self.where_array_contains(column, value)
    }
}

fn json_text(text: &str) -> ConditionValue {
    ConditionValue::Single(serde_json::Value::String(text.to_string()))
}

fn json_list<V: Into<serde_json::Value>>(values: Vec<V>) -> ConditionValue {
    ConditionValue::List(values.into_iter().map(Into::into).collect())
}

crate::query::condition_methods! {
    impl[M: Model] QueryBuilder<M> {
        where => push_condition,
            "Accepts a column name, a Rust field name, or a typed column; the condition is ANDed with the query's other filters.";
    }
}
