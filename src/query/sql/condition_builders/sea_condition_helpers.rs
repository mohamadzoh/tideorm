use super::*;

use chrono::{DateTime, SecondsFormat, Utc};

impl<M: Model> QueryBuilder<M> {
    pub(crate) fn build_sea_condition_for_db(&self, db_type: DatabaseType) -> Condition {
        let mut condition = self.add_filters(
            Condition::all(),
            &self.clauses.conditions,
            &self.clauses.or_groups,
            db_type,
        );
        if let Some(soft_delete_expression) = self.build_soft_delete_expression(db_type) {
            condition = condition.add(soft_delete_expression);
        }

        condition
    }

    /// The backend the query renders for: the one its statement runs on.
    ///
    /// That is the enclosing transaction when there is one, even for a query
    /// with a handle of its own, since the handle executes inside it.
    pub(crate) fn db_type_for_sql(&self) -> DatabaseType {
        self.database
            .as_ref()
            .map(|db| db.execution_backend())
            .unwrap_or_else(Self::ambient_db_type)
    }

    /// The backend to render for when the query carries no connection of its
    /// own: the scope's, which is the enclosing transaction's if any.
    fn ambient_db_type() -> DatabaseType {
        crate::database::__current_db()
            .map(|db| db.backend())
            .unwrap_or(DatabaseType::Postgres)
    }

    /// The soft-delete stamp written by the query-level `soft_delete()`.
    ///
    /// This deliberately no longer emits `CURRENT_TIMESTAMP`. The instance-level
    /// [`SoftDelete::soft_delete`](crate::soft_delete::SoftDelete::soft_delete)
    /// stamps `Utc::now()`, and the macro requires `deleted_at` to be a
    /// `DateTime<Utc>` — but on MySQL/MariaDB `CURRENT_TIMESTAMP` is evaluated in
    /// the *session* time zone, so a non-UTC session stored an offset instant
    /// that was then read back as if it were UTC. Both paths now stamp the same
    /// clock, so retention jobs see one consistent instant.
    ///
    /// `db_type` has to be the backend the surrounding statement renders for —
    /// [`db_type_for_sql`](Self::db_type_for_sql), never the ambient default.
    /// The literal's shape is backend-specific, so a statement bound for MySQL
    /// that was handed the ambient PostgreSQL rendering would carry a `T`
    /// separator and a UTC offset that MySQL rejects outright.
    pub(crate) fn current_timestamp_sql(db_type: DatabaseType) -> String {
        Self::utc_timestamp_literal(db_type, Utc::now())
    }

    /// Render `timestamp` as a UTC datetime literal for `db_type`.
    ///
    /// The text is produced by `chrono`'s formatter, so it can only contain
    /// digits and `- : . + T`: there is no caller-controlled input here and
    /// nothing that could terminate the literal early.
    pub(in crate::query::sql) fn utc_timestamp_literal(
        db_type: DatabaseType,
        timestamp: DateTime<Utc>,
    ) -> String {
        let rendered = match db_type {
            // MySQL only accepts a time-zone offset inside a datetime literal
            // from 8.0.19 onwards, and MariaDB not at all, so the UTC wall clock
            // is written bare — which is exactly how the driver binds a
            // `DateTime<Utc>` on this backend.
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                timestamp.format("%Y-%m-%d %H:%M:%S%.6f").to_string()
            }
            // Postgres and SQLite both need the explicit offset: without it a
            // `timestamptz` assignment would be resolved in the session time
            // zone, reintroducing the very skew this avoids.
            DatabaseType::Postgres | DatabaseType::SQLite => {
                timestamp.to_rfc3339_opts(SecondsFormat::Micros, false)
            }
        };

        format!("'{}'", rendered)
    }

    /// Bind `value` with the type of `column` when it is one of `M`'s columns,
    /// or a column of a joined model's table.
    pub(crate) fn column_value(&self, column: &str, value: &serde_json::Value) -> Value {
        crate::internal::json_to_column_value(value, self.column_type(column).as_ref())
    }

    /// The type a model declares for a filter's column: the query's own
    /// model's, else a joined model's.
    pub(in crate::query) fn column_type(&self, column: &str) -> Option<crate::orm::ColumnType> {
        crate::internal::column_type_of::<M>(column)
            .or_else(|| self.joined_model_column_type(column))
    }

    /// The type a model declares for `qualifier.column` when the qualifier is
    /// a joined table or its alias, so a value compared with a joined table's
    /// UUID or timestamp column is bound as one, not as text.
    fn joined_model_column_type(&self, column: &str) -> Option<crate::orm::ColumnType> {
        let (qualifier, name) = column.split_once('.')?;
        let join = self
            .clauses
            .joins
            .iter()
            .find(|join| join.qualifier() == qualifier)?;
        crate::sync::registered_column_type(join.bare_table(), name)
    }

    pub(crate) fn sea_value_list(values: &[serde_json::Value]) -> Vec<Value> {
        values
            .iter()
            .map(crate::internal::json_to_db_value)
            .collect()
    }

    pub(crate) fn sea_column_expr(&self, db_type: DatabaseType, column: &str) -> SimpleExpr {
        // Both shapes canonicalize, so `where_eq("users.display_name", ..)`
        // addresses the same column `where_eq("display_name", ..)` does.
        // Validation already resolves a self-qualified reference through the
        // field-name map, so rendering has to agree or a name that validates
        // would be emitted as a column that does not exist.
        match M::canonical_column_parts(column) {
            (Some(table), field)
                if db_sql::is_safe_identifier_segment(table)
                    && db_sql::is_safe_identifier_segment(field) =>
            {
                let table = if table == M::table_name() {
                    self.own_table_ref()
                } else {
                    std::borrow::Cow::Borrowed(table)
                };
                return Expr::col((Alias::new(table.into_owned()), Alias::new(field)));
            }
            (None, field) if db_sql::is_safe_identifier_segment(field) => {
                if self.qualifies_model_column(field) {
                    return Expr::col((
                        Alias::new(self.own_table_ref().into_owned()),
                        Alias::new(field),
                    ));
                }
                return Expr::col(Alias::new(field));
            }
            _ => {}
        }

        Expr::cust(self.format_column_for_db(db_type, column))
    }

    pub(crate) fn build_custom_expression(&self, sql: String, values: Vec<Value>) -> SimpleExpr {
        if values.is_empty() {
            Expr::cust(sql)
        } else {
            Expr::cust_with_values(sql, values)
        }
    }

    pub(in crate::query::sql) fn condition_spec<'a>(
        condition: &'a WhereCondition,
    ) -> Option<ConditionSpec<'a>> {
        let operator = &condition.operator;
        match (operator, &condition.value) {
            (Operator::Raw, ConditionValue::RawExpr(raw_sql)) => Some(ConditionSpec::Raw {
                raw_sql,
                values: &[],
                template: false,
            }),
            (Operator::Raw, ConditionValue::RawExprWithValues { sql, values }) => {
                Some(ConditionSpec::Raw {
                    raw_sql: sql,
                    values,
                    template: false,
                })
            }
            (Operator::Raw, ConditionValue::RawTemplate { sql, values }) => {
                Some(ConditionSpec::Raw {
                    raw_sql: sql,
                    values,
                    template: true,
                })
            }
            (_, ConditionValue::Column(other)) => Some(ConditionSpec::CompareColumns {
                operator: ComparisonOperator::of(operator)?,
                other,
            }),
            // `col = NULL` and `col != NULL` are UNKNOWN for every row, so binding
            // the JSON null as a parameter would silently match nothing with no
            // error to explain it. Rewriting here makes both emit the null check
            // `where_null()` and `where_not_null()` build.
            (Operator::Eq | Operator::NotEq, ConditionValue::Single(serde_json::Value::Null)) => {
                Some(ConditionSpec::NullCheck {
                    negated: matches!(operator, Operator::NotEq),
                })
            }
            (
                Operator::Like | Operator::LikeEscaped | Operator::NotLike,
                ConditionValue::Single(value),
            ) => Some(ConditionSpec::Pattern {
                negated: matches!(operator, Operator::NotLike),
                escaped: matches!(operator, Operator::LikeEscaped),
                value,
            }),
            (Operator::JsonContains | Operator::JsonContainedBy, ConditionValue::Single(value)) => {
                Some(ConditionSpec::JsonValue {
                    containment: if matches!(operator, Operator::JsonContains) {
                        db_sql::JsonContainment::Contains
                    } else {
                        db_sql::JsonContainment::ContainedBy
                    },
                    value,
                })
            }
            (
                Operator::JsonKeyExists
                | Operator::JsonKeyNotExists
                | Operator::JsonPathExists
                | Operator::JsonPathNotExists,
                ConditionValue::Single(serde_json::Value::String(target)),
            ) => Some(ConditionSpec::JsonExists {
                existence: match operator {
                    Operator::JsonKeyExists | Operator::JsonKeyNotExists => JsonExistence::Key,
                    _ => JsonExistence::Path,
                },
                negated: matches!(
                    operator,
                    Operator::JsonKeyNotExists | Operator::JsonPathNotExists
                ),
                target,
            }),
            (_, ConditionValue::Single(value)) => Some(ConditionSpec::Compare {
                operator: ComparisonOperator::of(operator)?,
                value,
            }),
            (Operator::IsNull | Operator::IsNotNull, ConditionValue::None) => {
                Some(ConditionSpec::NullCheck {
                    negated: matches!(operator, Operator::IsNotNull),
                })
            }
            (Operator::Between | Operator::NotBetween, ConditionValue::Range(low, high)) => {
                Some(ConditionSpec::Between {
                    low,
                    high,
                    negated: matches!(operator, Operator::NotBetween),
                })
            }
            (_, ConditionValue::List(values)) => match ArrayOperator::of(operator) {
                Some(operator) => Some(ConditionSpec::Array { operator, values }),
                None => Some(ConditionSpec::List {
                    operator: ListOperator::of(operator)?,
                    values,
                }),
            },
            _ => None,
        }
    }

    /// Reject any condition whose operator/value pairing has no SQL rendering.
    ///
    /// `condition_spec` returns `None` for an unrepresentable pair and the WHERE
    /// renderer skips a `None`, so such a condition would silently disappear from
    /// the rendered predicate — widening a targeted mutation into a full-table
    /// one. Surfacing it as a query error at render time keeps an unrenderable
    /// filter from ever becoming a missing filter.
    pub(in crate::query::sql) fn ensure_conditions_are_representable(&self) -> Result<()> {
        for condition in self.all_conditions() {
            Self::ensure_condition_is_representable(condition)?;
        }

        Ok(())
    }

    /// Reject an ordering comparison or range bound whose value is JSON null.
    ///
    /// `col > NULL` is UNKNOWN for every row, so such a filter matches nothing
    /// and says nothing about why. Equality is rewritten into a null check by
    /// `condition_spec`; `>`, `>=`, `<`, `<=` and `BETWEEN` have no null-safe
    /// reading at all, so they are refused instead of quietly emptying a result
    /// set.
    fn ensure_condition_has_no_null_bound(condition: &WhereCondition) -> Result<()> {
        let has_null_bound = match (&condition.operator, &condition.value) {
            (
                Operator::Gt | Operator::Gte | Operator::Lt | Operator::Lte,
                ConditionValue::Single(serde_json::Value::Null),
            ) => true,
            (Operator::Between | Operator::NotBetween, ConditionValue::Range(low, high)) => {
                low.is_null() || high.is_null()
            }
            _ => false,
        };

        if !has_null_bound {
            return Ok(());
        }

        Err(Error::query(format!(
            "WHERE condition on '{}' for model '{}' compares against NULL with operator {:?}; a NULL comparison is never true — use where_null() or where_not_null() instead",
            condition.column,
            M::table_name(),
            condition.operator
        )))
    }

    fn ensure_condition_is_representable(condition: &WhereCondition) -> Result<()> {
        if let ConditionValue::Invalid(reason) = &condition.value {
            return Err(Error::query(format!(
                "WHERE condition on '{}' for model '{}': {}",
                condition.column,
                M::table_name(),
                reason
            )));
        }
        Self::ensure_condition_has_no_null_bound(condition)?;

        if Self::condition_spec(condition).is_some() {
            return Ok(());
        }

        let column = if condition.column.is_empty() {
            "<raw>"
        } else {
            condition.column.as_str()
        };

        Err(Error::query(format!(
            "WHERE condition on '{}' for model '{}' pairs operator {:?} with an incompatible value {:?} and cannot be rendered as SQL",
            column,
            M::table_name(),
            condition.operator,
            condition.value
        )))
    }
}
