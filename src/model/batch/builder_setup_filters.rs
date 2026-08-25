use super::*;

/// Assignments and filters for [`BatchUpdateBuilder`].
///
/// Two rules apply to everything in this file:
///
/// - Column arguments accept a typed column (`User::columns.name`), the
///   database column name, or the Rust field name. A name the model does not
///   know is rejected when the statement is built, not at the call site.
/// - Filters are combined with `AND`, except the `or_where_*` family, which
///   collects into one `OR` group that is then `AND`ed with the rest. At least
///   one filter is required: an unfiltered batch update is refused.
impl<M: Model> BatchUpdateBuilder<M> {
    /// Start an empty batch update for `M`.
    ///
    /// Prefer [`Model::update_all`](crate::model::Model::update_all), which
    /// calls this for you. Soft-deleted rows are in scope by default; see the
    /// type-level docs.
    #[must_use]
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
            updates: std::collections::HashMap::new(),
            conditions: Vec::new(),
            returning: false,
            limit_value: None,
            // Batch updates are scoped like `with_trashed()` unless the caller
            // opts out; see the type-level docs on `BatchUpdateBuilder`.
            include_trashed: true,
        }
    }

    /// Restrict this update to rows that are not soft-deleted.
    ///
    /// A batch update includes trashed rows by default, which is what makes a
    /// bulk restore possible. Call this when the update should behave like a
    /// normal query and skip soft-deleted rows. On a model without soft delete
    /// it changes nothing.
    #[must_use]
    pub fn without_trashed(mut self) -> Self {
        self.include_trashed = false;
        self
    }

    /// Explicitly include soft-deleted rows in this update.
    ///
    /// This is already the default; it exists so the intent can be written down
    /// at the call site.
    #[must_use]
    pub fn with_trashed(mut self) -> Self {
        self.include_trashed = true;
        self
    }

    /// Assign a literal value to a column.
    ///
    /// The value is bound as a parameter. Setting the same column twice keeps
    /// the last assignment.
    #[must_use]
    pub fn set(mut self, field: impl IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::Value(value.into()),
        );
        self
    }

    /// Assign a raw SQL expression, spliced into the `SET` clause verbatim.
    ///
    /// This is the one escape hatch in the builder that is not parameterized:
    /// the expression is not escaped, validated, or dialect-translated. Pass
    /// only literals you wrote yourself — never user input, and never a string
    /// you built by formatting one in. Reach for
    /// [`increment`](Self::increment), [`json_set`](Self::json_set), or the
    /// other computed setters first; they cover the common cases safely and
    /// portably.
    #[must_use]
    pub fn set_trusted_raw(mut self, field: impl IntoColumnName, expression: &str) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::UnsafeRaw(expression.to_string()),
        );
        self
    }

    /// Assign a value only when `condition` holds, otherwise leave the column alone.
    ///
    /// Useful for assembling an update from optional inputs without breaking the
    /// call chain. Note that a builder whose every `set_if` was skipped has no
    /// assignments left, and executing it is a no-op that reports zero rows.
    #[must_use]
    pub fn set_if(
        mut self,
        field: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
        condition: bool,
    ) -> Self {
        if condition {
            self.updates.insert(
                field.column_name().to_string(),
                UpdateValue::Value(value.into()),
            );
        }
        self
    }

    /// Add `by` to the column's current value in the database.
    ///
    /// The arithmetic happens server-side, so concurrent increments do not lose
    /// updates the way a read-modify-write from Rust would.
    #[must_use]
    pub fn increment(mut self, field: impl IntoColumnName, by: i64) -> Self {
        self.updates
            .insert(field.column_name().to_string(), UpdateValue::Increment(by));
        self
    }

    /// Subtract `by` from the column's current value in the database.
    ///
    /// Nothing clamps the result: the column can go negative unless a check
    /// constraint or an added `where_gte` filter prevents it.
    #[must_use]
    pub fn decrement(mut self, field: impl IntoColumnName, by: i64) -> Self {
        self.updates
            .insert(field.column_name().to_string(), UpdateValue::Decrement(by));
        self
    }

    /// Multiply the column's current value by `by` in the database.
    #[must_use]
    pub fn multiply(mut self, field: impl IntoColumnName, by: f64) -> Self {
        self.updates
            .insert(field.column_name().to_string(), UpdateValue::Multiply(by));
        self
    }

    /// Divide the column's current value by `by` in the database.
    ///
    /// A zero divisor is passed through to the backend, which normally raises a
    /// division-by-zero error for the whole statement.
    #[must_use]
    pub fn divide(mut self, field: impl IntoColumnName, by: f64) -> Self {
        self.updates
            .insert(field.column_name().to_string(), UpdateValue::Divide(by));
        self
    }

    /// Append a value to an array or JSON array column.
    ///
    /// Renders to each backend's own function, so the column has to be a real
    /// array type on PostgreSQL and a JSON array elsewhere.
    #[must_use]
    pub fn array_append(
        mut self,
        field: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::ArrayAppend(value.into()),
        );
        self
    }

    /// Remove a value from an array or JSON array column.
    ///
    /// PostgreSQL drops every matching element; the JSON-based backends drop one
    /// match per row.
    #[must_use]
    pub fn array_remove(
        mut self,
        field: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::ArrayRemove(value.into()),
        );
        self
    }

    /// Set one path inside a JSON column, leaving the rest of the document intact.
    ///
    /// `path` must be `$.field` or `$.field.subfield`, with plain identifier
    /// segments; array indexes and wildcards are rejected when the statement is
    /// built. Prefer this over reading the document into Rust and writing it
    /// back, which would clobber concurrent edits to other keys.
    #[must_use]
    pub fn json_set(
        mut self,
        field: impl IntoColumnName,
        path: &str,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::JsonSet(path.to_string(), value.into()),
        );
        self
    }

    /// Fill the column with `default` only where it is currently `NULL`.
    ///
    /// This is the backfill setter: rows that already hold a value keep it, so
    /// the update is safe to re-run.
    #[must_use]
    pub fn coalesce(
        mut self,
        field: impl IntoColumnName,
        default: impl Into<serde_json::Value>,
    ) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::Coalesce(default.into()),
        );
        self
    }

    /// Cap how many rows the update may touch.
    ///
    /// The cap is always enforced. MySQL and MariaDB take `LIMIT` on the
    /// `UPDATE` directly; Postgres and SQLite cannot, so the update is scoped
    /// to a bounded primary-key subquery instead. That rewrite needs a
    /// single-column primary key — on those backends a model with a composite
    /// primary key fails at execution rather than silently updating every
    /// matching row.
    #[must_use]
    pub fn limit(mut self, n: u64) -> Self {
        self.limit_value = Some(n);
        self
    }

    /// Ask for the updated rows to be returned.
    ///
    /// Only [`BatchUpdateBuilder::execute_returning`] can hand rows back;
    /// [`BatchUpdateBuilder::execute`] reports the affected row count and
    /// therefore rejects a builder that was marked with `returning()`.
    #[must_use]
    pub fn returning(mut self) -> Self {
        self.returning = true;
        self
    }

    /// Match rows where `column` equals `value`.
    #[must_use]
    pub fn where_eq(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Eq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Match rows where `column` differs from `value`.
    ///
    /// This is SQL `<>`, so rows where the column is `NULL` do not match. Add
    /// [`or_where_null`](Self::or_where_null) when they should.
    #[must_use]
    pub fn where_not(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::NotEq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Match rows where `column` is greater than `value`.
    #[must_use]
    pub fn where_gt(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Gt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Match rows where `column` is greater than or equal to `value`.
    #[must_use]
    pub fn where_gte(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Gte,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Match rows where `column` is less than `value`.
    #[must_use]
    pub fn where_lt(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Lt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Match rows where `column` is less than or equal to `value`.
    #[must_use]
    pub fn where_lte(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Lte,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Match rows where `column` is one of `values`.
    ///
    /// Every value is bound as its own parameter, so a very large list can hit
    /// the backend's parameter limit; chunk the update if that happens.
    #[must_use]
    pub fn where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::In,
            value: crate::query::ConditionValue::List(
                values.into_iter().map(|v| v.into()).collect(),
            ),
        });
        self
    }

    /// Match rows where `column` is none of `values`.
    ///
    /// Like `NOT IN` in SQL, rows where the column is `NULL` do not match.
    #[must_use]
    pub fn where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::NotIn,
            value: crate::query::ConditionValue::List(
                values.into_iter().map(|v| v.into()).collect(),
            ),
        });
        self
    }

    /// Match rows where `column` is `NULL`.
    #[must_use]
    pub fn where_null(mut self, column: impl IntoColumnName) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::IsNull,
            value: crate::query::ConditionValue::None,
        });
        self
    }

    /// Match rows where `column` holds a value.
    #[must_use]
    pub fn where_not_null(mut self, column: impl IntoColumnName) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::IsNotNull,
            value: crate::query::ConditionValue::None,
        });
        self
    }

    /// Match rows where `column` falls between `min` and `max`, inclusive.
    #[must_use]
    pub fn where_between(
        mut self,
        column: impl IntoColumnName,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Between,
            value: crate::query::ConditionValue::Range(min.into(), max.into()),
        });
        self
    }

    /// Match rows against a raw `LIKE` pattern.
    ///
    /// `pattern` is used as written, so `%` and `_` in it stay wildcards. When
    /// the text comes from a user, reach for [`where_contains`](Self::where_contains),
    /// [`where_starts_with`](Self::where_starts_with), or
    /// [`where_ends_with`](Self::where_ends_with) instead — those escape the
    /// wildcards for you.
    #[must_use]
    pub fn where_like(mut self, column: impl IntoColumnName, pattern: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Like,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(
                pattern.to_string(),
            )),
        });
        self
    }

    /// Match rows where `column` contains `value` as a literal substring.
    ///
    /// Wildcards in `value` are escaped, so it is safe for user input.
    #[must_use]
    pub fn where_contains(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "%{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    /// Match rows where `column` starts with `value` as a literal prefix.
    ///
    /// Wildcards in `value` are escaped, so it is safe for user input.
    #[must_use]
    pub fn where_starts_with(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    /// Match rows where `column` ends with `value` as a literal suffix.
    ///
    /// Wildcards in `value` are escaped, so it is safe for user input.
    #[must_use]
    pub fn where_ends_with(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "%{}",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    /// Add `column = value` to this update's `OR` group.
    ///
    /// Every `or_where_*` call joins one shared `OR` group, and that group is
    /// `AND`ed with the plain `where_*` filters.
    #[must_use]
    pub fn or_where_eq(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::Eq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add `column <> value` to this update's `OR` group.
    #[must_use]
    pub fn or_where_not(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::NotEq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add `column > value` to this update's `OR` group.
    #[must_use]
    pub fn or_where_gt(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::Gt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add `column < value` to this update's `OR` group.
    #[must_use]
    pub fn or_where_lt(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::Lt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add `column IN (values)` to this update's `OR` group.
    #[must_use]
    pub fn or_where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::In,
            value: crate::query::ConditionValue::List(
                values.into_iter().map(|v| v.into()).collect(),
            ),
        });
        self
    }

    /// Add `column IS NULL` to this update's `OR` group.
    #[must_use]
    pub fn or_where_null(mut self, column: impl IntoColumnName) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::IsNull,
            value: crate::query::ConditionValue::None,
        });
        self
    }

    /// Add a raw `LIKE` pattern to this update's `OR` group.
    ///
    /// The pattern is used as written; see [`where_like`](Self::where_like).
    #[must_use]
    pub fn or_where_like(mut self, column: impl IntoColumnName, pattern: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::Like,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(
                pattern.to_string(),
            )),
        });
        self
    }

    /// Add a literal substring match to this update's `OR` group.
    #[must_use]
    pub fn or_where_contains(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "%{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    /// Add a literal prefix match to this update's `OR` group.
    #[must_use]
    pub fn or_where_starts_with(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    /// Add a literal suffix match to this update's `OR` group.
    #[must_use]
    pub fn or_where_ends_with(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "%{}",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }
}
