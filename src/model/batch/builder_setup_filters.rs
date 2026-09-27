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
impl<M: Model> crate::columns::ConditionOwner for BatchUpdateBuilder<M> {
    fn own_table() -> Option<&'static str> {
        Some(M::table_name())
    }
}

impl<M: Model> BatchUpdateBuilder<M> {
    /// Start an empty batch update for `M`.
    ///
    /// Prefer [`Model::update_all`](crate::model::Model::update_all), which
    /// calls this for you. Soft-deleted rows are left out, as a query leaves
    /// them out; see [`with_trashed`](Self::with_trashed).
    #[must_use]
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
            updates: std::collections::HashMap::new(),
            conditions: Vec::new(),
            or_group: OrGroup::new(),
            limit_value: None,
            include_trashed: None,
            base: None,
            invalid_reason: None,
        }
    }

    /// An update of the rows `query` matches, as
    /// [`QueryBuilder::update_all`](crate::query::QueryBuilder::update_all)
    /// starts one.
    pub(crate) fn from_query(query: crate::query::QueryBuilder<M>) -> Self {
        let mut builder = Self::new();
        builder.base = Some(query);
        builder
    }

    /// Restrict this update to rows that are not soft-deleted — the default,
    /// and what an update started from a `with_trashed()` query can return to.
    /// On a model without soft delete it changes nothing.
    #[must_use]
    pub fn without_trashed(mut self) -> Self {
        self.include_trashed = Some(false);
        self
    }

    /// Include soft-deleted rows in this update, which leaves them out by
    /// default: a backfill that should reach the trash too. To restore trashed
    /// rows, `Model::query().only_trashed().restore()` says so directly.
    #[must_use]
    pub fn with_trashed(mut self) -> Self {
        self.include_trashed = Some(true);
        self
    }

    /// Assign a literal value to a column.
    ///
    /// The value is bound as a parameter. Setting the same column twice keeps
    /// the last assignment.
    #[must_use]
    pub fn set(self, field: impl IntoColumnName, value: impl serde::Serialize) -> Self {
        match crate::query::checked_filter_value(value) {
            Ok(value) => self.assign(field, UpdateValue::Value(value)),
            Err(reason) => self.invalidate(format!("set(): {}", reason)),
        }
    }

    /// Keep the first reason the update cannot run.
    fn invalidate(mut self, reason: String) -> Self {
        self.invalid_reason.get_or_insert(reason);
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
    pub fn set_trusted_raw(self, field: impl IntoColumnName, expression: &str) -> Self {
        self.assign(field, UpdateValue::UnsafeRaw(expression.to_string()))
    }

    /// Assign a value only when `condition` holds, otherwise leave the column alone.
    ///
    /// Useful for assembling an update from optional inputs without breaking the
    /// call chain. Note that a builder whose every `set_if` was skipped has no
    /// assignments left, and executing it is a no-op that reports zero rows.
    #[must_use]
    pub fn set_if(
        self,
        field: impl IntoColumnName,
        value: impl serde::Serialize,
        condition: bool,
    ) -> Self {
        if condition {
            self.set(field, value)
        } else {
            self
        }
    }

    /// Add `by` to the column's current value in the database.
    ///
    /// The arithmetic happens server-side, so concurrent increments do not lose
    /// updates the way a read-modify-write from Rust would.
    #[must_use]
    pub fn increment(self, field: impl IntoColumnName, by: i64) -> Self {
        self.assign(field, UpdateValue::Increment(by))
    }

    /// Subtract `by` from the column's current value in the database.
    ///
    /// Nothing clamps the result: the column can go negative unless a check
    /// constraint or an added `where_gte` filter prevents it.
    #[must_use]
    pub fn decrement(self, field: impl IntoColumnName, by: i64) -> Self {
        self.assign(field, UpdateValue::Decrement(by))
    }

    /// Multiply the column's current value by `by` in the database.
    ///
    /// On an integer column the product is rounded to the nearest integer, as
    /// PostgreSQL and MySQL do when they store it.
    #[must_use]
    pub fn multiply(self, field: impl IntoColumnName, by: f64) -> Self {
        self.assign(field, UpdateValue::Multiply(by))
    }

    /// Divide the column's current value by `by` in the database.
    ///
    /// On an integer column the quotient is rounded to the nearest integer. A
    /// zero divisor is passed through to the backend, which normally raises a
    /// division-by-zero error for the whole statement.
    #[must_use]
    pub fn divide(self, field: impl IntoColumnName, by: f64) -> Self {
        self.assign(field, UpdateValue::Divide(by))
    }

    /// Append a value to an array or JSON array column.
    ///
    /// Renders to each backend's own function, so the column has to be a real
    /// array type on PostgreSQL and a JSON array elsewhere.
    #[must_use]
    pub fn array_append(
        self,
        field: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.assign(field, UpdateValue::ArrayAppend(value.into()))
    }

    /// Remove a value from an array or JSON array column.
    ///
    /// PostgreSQL drops every matching element; the JSON-based backends drop one
    /// match per row.
    #[must_use]
    pub fn array_remove(
        self,
        field: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.assign(field, UpdateValue::ArrayRemove(value.into()))
    }

    /// Set one path inside a JSON column, leaving the rest of the document intact.
    ///
    /// `path` must be `$.field` or `$.field.subfield`, with plain identifier
    /// segments; array indexes and wildcards are rejected when the statement is
    /// built. Prefer this over reading the document into Rust and writing it
    /// back, which would clobber concurrent edits to other keys.
    #[must_use]
    pub fn json_set(
        self,
        field: impl IntoColumnName,
        path: &str,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.assign(field, UpdateValue::JsonSet(path.to_string(), value.into()))
    }

    /// Fill the column with `default` only where it is currently `NULL`.
    ///
    /// This is the backfill setter: rows that already hold a value keep it, so
    /// the update is safe to re-run.
    #[must_use]
    pub fn coalesce(self, field: impl IntoColumnName, default: impl serde::Serialize) -> Self {
        match crate::query::checked_filter_value(default) {
            Ok(default) => self.assign(field, UpdateValue::Coalesce(default)),
            Err(reason) => self.invalidate(format!("coalesce(): {}", reason)),
        }
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

    fn assign(mut self, field: impl IntoColumnName, value: UpdateValue) -> Self {
        self.updates.insert(field.column_name().to_string(), value);
        self
    }

    fn push_condition(mut self, condition: WhereCondition) -> Self {
        self.conditions.push(condition);
        self
    }

    fn push_or_condition(mut self, condition: WhereCondition) -> Self {
        self.or_group.conditions.push(condition);
        self
    }
}

crate::query::condition_methods! {
    impl[M: Model] BatchUpdateBuilder<M> {
        where => push_condition, "The condition is ANDed with the update's other filters.";
        or_where => push_or_condition,
            "Every `or_where_*` call joins one shared OR group, which is ANDed with the plain `where_*` filters.";
    }
}
