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
    fn own_table() -> Option<(&'static str, Option<&'static str>)> {
        crate::columns::model_table::<M>()
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
    /// A typed column takes the values a filter on it takes (see
    /// [`ColumnValue`](crate::columns::ColumnValue)), so
    /// `.set(User::ACTIVE, "yes")` does not compile; a column named by a
    /// string takes any value, and [`set_value`](Self::set_value) any
    /// [`UpdateValue`] for either.
    ///
    /// The value is bound as the column's type, as a filter value is: a
    /// `None` of any type writes SQL NULL, and a `serde_json::Value` or text
    /// is read as the column's type (a UUID or timestamp written as text).
    /// A JSON column takes the value as a document, so text stays a JSON
    /// string and the JSON `null` writes the `null` document.
    /// Setting the same column twice keeps the last assignment.
    /// Boolean, UUID and binary columns require a value of their type, and
    /// `f32` fields one within their finite range. Date, time, timestamp and
    /// decimal values must parse as their destination type; on SQLite a
    /// decimal must stay within the model's readable range after conversion
    /// to REAL, and an integer within the field's own range. A NaN or
    /// infinite float is refused. Invalid values fail before any rows change.
    #[must_use]
    pub fn set<C, V>(self, field: C, value: V) -> Self
    where
        C: crate::columns::ColumnValue<V>,
    {
        self.assign_checked("set", field, C::convert_value(value), UpdateValue::Value)
    }

    /// Keep the first reason the update cannot run.
    fn invalidate(mut self, reason: String) -> Self {
        self.invalid_reason.get_or_insert(reason);
        self
    }

    /// Assign what `update` builds from `value`, refusing a NaN or infinite
    /// float, which no column can hold.
    fn assign_checked(
        self,
        setter: &str,
        field: impl IntoColumnName,
        value: impl Into<crate::DbValue>,
        update: impl FnOnce(crate::DbValue) -> UpdateValue,
    ) -> Self {
        match crate::query::comparable_value(value) {
            Ok(value) => self.assign(field, update(value)),
            Err(reason) => self.invalidate(format!("{setter}(): {reason}")),
        }
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

    /// Assign a prebuilt [`UpdateValue`], for an assignment chosen at run
    /// time; each named setter builds one variant.
    ///
    /// The value goes through the same checks as its setter's. An
    /// [`UpdateValue::UnsafeRaw`] is spliced in verbatim, as with
    /// [`set_trusted_raw`](Self::set_trusted_raw).
    ///
    /// ```ignore
    /// let change = if reset { UpdateValue::Value(0.into()) } else { UpdateValue::Increment(1) };
    /// Counter::update_all().set_value(Counter::HITS, change).where_eq(Counter::ID, 1);
    /// ```
    #[must_use]
    pub fn set_value(self, field: impl IntoColumnName, value: UpdateValue) -> Self {
        match value {
            UpdateValue::Value(value) => {
                self.assign_checked("set", field, value, UpdateValue::Value)
            }
            UpdateValue::ArrayAppend(value) => self.array_append(field, value),
            UpdateValue::ArrayRemove(value) => self.array_remove(field, value),
            UpdateValue::JsonSet(path, value) => self.json_set(field, &path, value),
            UpdateValue::Coalesce(value) => {
                self.assign_checked("coalesce", field, value, UpdateValue::Coalesce)
            }
            value => self.assign(field, value),
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
    /// On an integer column the product is exact, taking `by` as the decimal
    /// it is written as, and rounded half away from zero on every backend.
    /// Non-finite factors and factors SQLite cannot apply exactly to integers
    /// are rejected before executing the update.
    #[must_use]
    pub fn multiply(self, field: impl IntoColumnName, by: f64) -> Self {
        self.assign(field, UpdateValue::Multiply(by))
    }

    /// Divide the column's current value by `by` in the database.
    ///
    /// On an integer column the quotient is exact, taking `by` as the decimal
    /// it is written as, and rounded half away from zero on every backend.
    /// Zero, non-finite divisors and factors SQLite cannot apply exactly to
    /// integers are rejected before executing the update.
    #[must_use]
    pub fn divide(self, field: impl IntoColumnName, by: f64) -> Self {
        self.assign(field, UpdateValue::Divide(by))
    }

    /// Append a value to an array or JSON array column.
    ///
    /// Renders to each backend's own function, so the column has to be a real
    /// array type on PostgreSQL and a JSON array elsewhere. The element is
    /// bound as the array's element type on PostgreSQL and as its JSON
    /// document elsewhere, where a `None` appends `null`.
    #[must_use]
    pub fn array_append(
        self,
        field: impl IntoColumnName,
        value: impl Into<crate::DbValue>,
    ) -> Self {
        self.assign_checked("array_append", field, value, UpdateValue::ArrayAppend)
    }

    /// Remove a value from an array or JSON array column.
    ///
    /// Every backend drops all matching elements. Non-array JSON documents and
    /// SQL NULL stay unchanged. Non-finite floats are rejected.
    #[must_use]
    pub fn array_remove(
        self,
        field: impl IntoColumnName,
        value: impl Into<crate::DbValue>,
    ) -> Self {
        self.assign_checked("array_remove", field, value, UpdateValue::ArrayRemove)
    }

    /// Set one path inside a JSON column, leaving the rest of the document intact.
    ///
    /// `path` is `$` followed by `.key`, `."any key"`, `['any key']` or
    /// `[index]` steps, as `where_json_path_exists` takes it; wildcards are
    /// rejected when the statement is built. Prefer this over reading the
    /// document into Rust and writing it back, which would clobber concurrent
    /// edits to other keys. The value is written as its JSON document, so a
    /// `None` sets `null`.
    #[must_use]
    pub fn json_set(
        self,
        field: impl IntoColumnName,
        path: &str,
        value: impl Into<crate::DbValue>,
    ) -> Self {
        self.assign_checked("json_set", field, value, |value| {
            UpdateValue::JsonSet(path.to_string(), value)
        })
    }

    /// Fill the column with `default` only where it is currently `NULL`.
    ///
    /// This is the backfill setter: rows that already hold a value keep it, so
    /// the update is safe to re-run.
    /// Defaults are typed, bound and checked as [`set`](Self::set) types and
    /// binds its value.
    #[must_use]
    pub fn coalesce<C, V>(self, field: C, default: V) -> Self
    where
        C: crate::columns::ColumnValue<V>,
    {
        self.assign_checked(
            "coalesce",
            field,
            C::convert_value(default),
            UpdateValue::Coalesce,
        )
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

    /// Keyed by the column, so a later assignment through the field's name
    /// or the column's replaces an earlier one through the other.
    /// Typed columns must belong to the model being updated.
    fn assign(mut self, field: impl IntoColumnName, value: UpdateValue) -> Self {
        if let Some(table) = field.column_table()
            && (table != M::table_name() || field.column_schema() != M::schema_name())
        {
            return self.invalidate(format!(
                "cannot update {} (schema {:?}) with typed column {}.{} (schema {:?})",
                M::table_name(),
                M::schema_name(),
                table,
                field.column_name(),
                field.column_schema()
            ));
        }
        let name = field.column_name();
        let column = M::column_named(name);
        self.updates.insert(column.to_string(), value);
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
