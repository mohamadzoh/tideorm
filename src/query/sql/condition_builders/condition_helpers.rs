use super::*;

impl<M: Model> QueryBuilder<M> {
    pub(crate) fn build_condition_for_db(&self, db_type: DatabaseType) -> Condition {
        let mut condition = self.filter_condition(db_type);
        if let Some(soft_delete_expression) = self.build_soft_delete_expression(db_type) {
            condition = condition.add(soft_delete_expression);
        }

        condition
    }

    /// The condition the query's filters build, without the soft-delete scope.
    pub(in crate::query::sql) fn filter_condition(&self, db_type: DatabaseType) -> Condition {
        self.add_filters(
            Condition::all(),
            &self.clauses.conditions,
            &self.clauses.or_groups,
            db_type,
        )
    }

    /// The backend the query renders for: the one its statement runs on.
    ///
    /// That is the enclosing transaction when there is one, even for a query
    /// with a handle of its own, since the handle executes inside it. A query
    /// with no database to run on has none.
    pub(crate) fn db_type_for_sql(&self) -> Result<DatabaseType> {
        Ok(self.current_db()?.execution_backend())
    }

    /// The type a model declares for a filter's column: the query's own
    /// model's, else a joined model's.
    pub(in crate::query) fn column_type(&self, column: &str) -> Option<crate::orm::ColumnType> {
        crate::internal::column_type_of::<M>(column)
            .or_else(|| self.joined_model_column_type(column))
    }

    /// Decode a projected output using the metadata captured by scalar subqueries.
    pub(in crate::query) fn projected_column_type(
        &self,
        output: &str,
    ) -> Option<crate::orm::ColumnType> {
        if let Some(subquery) = self
            .clauses
            .subquery_select_expressions
            .iter()
            .find(|item| item.alias == output)
        {
            return subquery.column_type.clone();
        }
        self.projection_outputs()
            .into_iter()
            .find(|(name, _)| name == output)
            .and_then(|(_, source)| source)
            .and_then(|(table, column)| self.column_type(&format!("{table}.{column}")))
    }

    /// The type a model declares for `qualifier.column` when the qualifier is
    /// a joined table or its alias, so a value compared with a joined table's
    /// UUID or timestamp column is bound as one, not as text.
    fn joined_model_column_type(&self, column: &str) -> Option<crate::orm::ColumnType> {
        let (qualifier, name) = column.rsplit_once('.')?;
        crate::sync::registered_column_type(&self.join_named(qualifier)?.table, name)
    }

    pub(crate) fn column_expr(&self, db_type: DatabaseType, column: &str) -> SimpleExpr {
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

    pub(in crate::query::sql) fn condition_spec<'a>(
        condition: &'a WhereCondition,
    ) -> Option<ConditionSpec<'a>> {
        let operator = &condition.operator;
        match (operator, &condition.value) {
            (Operator::Raw, ConditionValue::RawExpr(raw_sql)) => Some(ConditionSpec::Raw {
                raw_sql,
                values: &[],
            }),
            (Operator::Raw, ConditionValue::RawTemplate { sql, values }) => {
                Some(ConditionSpec::Raw {
                    raw_sql: sql,
                    values,
                })
            }
            (Operator::Raw, ConditionValue::Subquery(subquery)) => {
                Some(ConditionSpec::Subquery(subquery))
            }
            (_, ConditionValue::Column(other)) => Some(ConditionSpec::CompareColumns {
                operator: ComparisonOperator::of(operator)?,
                other,
            }),
            // `col = NULL` and `col != NULL` are UNKNOWN for every row, so binding
            // the NULL as a parameter would silently match nothing with no error
            // to explain it. Rewriting here makes both emit the null check
            // `where_null()` and `where_not_null()` build. A JSON null value is
            // a comparison, which a JSON column reads as the `null` document.
            (Operator::Eq | Operator::NotEq, ConditionValue::None) => {
                Some(ConditionSpec::NullCheck {
                    negated: matches!(operator, Operator::NotEq),
                })
            }
            (Operator::Like | Operator::NotLike, ConditionValue::Single(value)) => {
                Some(ConditionSpec::Pattern {
                    negated: matches!(operator, Operator::NotLike),
                    value,
                })
            }
            (
                Operator::Contains | Operator::StartsWith | Operator::EndsWith,
                ConditionValue::Single(value),
            ) => Some(ConditionSpec::Literal {
                leading: !matches!(operator, Operator::StartsWith),
                trailing: !matches!(operator, Operator::EndsWith),
                value,
            }),
            (Operator::JsonContains | Operator::JsonContainedBy, ConditionValue::Single(value)) => {
                Some(ConditionSpec::JsonValue {
                    containment: if matches!(operator, Operator::JsonContains) {
                        JsonContainment::Contains
                    } else {
                        JsonContainment::ContainedBy
                    },
                    value,
                })
            }
            (
                Operator::JsonKeyExists
                | Operator::JsonKeyNotExists
                | Operator::JsonPathExists
                | Operator::JsonPathNotExists,
                ConditionValue::Single(Value::String(Some(target))),
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
            (_, ConditionValue::List(values)) => match array_operator(operator) {
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

    /// Reject an ordering comparison or range bound whose value is NULL.
    ///
    /// `col > NULL` is UNKNOWN for every row, so such a filter matches nothing
    /// and says nothing about why. Equality is rewritten into a null check by
    /// `condition_spec`; `>`, `>=`, `<`, `<=` and `BETWEEN` have no null-safe
    /// reading at all, so they are refused instead of quietly emptying a result
    /// set.
    fn ensure_condition_has_no_null_bound(condition: &WhereCondition) -> Result<()> {
        let has_null_bound = match (&condition.operator, &condition.value) {
            (Operator::Gt | Operator::Gte | Operator::Lt | Operator::Lte, ConditionValue::None) => {
                true
            }
            (
                Operator::Gt | Operator::Gte | Operator::Lt | Operator::Lte,
                ConditionValue::Single(value),
            ) => is_null_value(value),
            (Operator::Between | Operator::NotBetween, ConditionValue::Range(low, high)) => {
                is_null_value(low) || is_null_value(high)
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

/// Whether `value` is SQL NULL or the JSON `null`, which no ordering compares.
fn is_null_value(value: &Value) -> bool {
    crate::query::is_sql_null(value) || crate::query::is_json_null(value)
}
