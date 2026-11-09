use super::structure::SubquerySelect;
use super::{ConditionValue, Correlation, Operator, QueryBuilder, Subquery, WhereCondition};
use crate::columns::IntoColumnName;
use crate::model::Model;
use crate::query::db_sql;

/// The alias a query `depth` (at least 1) levels deep in `where_has`
/// subqueries over its own table reads that table under.
pub(in crate::query) fn related_alias(depth: usize) -> String {
    match depth {
        1 => "tideorm_related".to_string(),
        depth => format!("tideorm_related_{}", depth),
    }
}

impl<M: Model> QueryBuilder<M> {
    fn push_condition(mut self, condition: WhereCondition) -> Self {
        self.clauses.conditions.push(condition);
        self
    }

    /// Push `<keyword> (subquery)` as a condition on `column` (empty for
    /// `EXISTS`), the subquery rendered with the statement.
    fn push_subquery_condition<N: Model>(
        mut self,
        method: &str,
        column: &str,
        keyword: &'static str,
        subquery: QueryBuilder<N>,
    ) -> Self {
        self.absorb_operand_error("subquery", method, &subquery);
        self.push_embedded(column, Subquery::select(keyword, subquery))
    }

    /// Push `subquery` as a condition on `column` (empty for a whole
    /// predicate).
    fn push_embedded(self, column: &str, subquery: Subquery) -> Self {
        self.push_condition(WhereCondition {
            column: column.to_string(),
            operator: Operator::Raw,
            value: ConditionValue::Subquery(subquery),
        })
    }

    /// How a correlated `EXISTS` ties the related rows' `foreign_key` to this
    /// query's row.
    fn correlation(&self, negated: bool, foreign_key: &str, local_key: &str) -> Correlation {
        Correlation {
            negated,
            foreign_key: foreign_key.to_string(),
            outer: self.own_table_ref().into_owned(),
            local_key: local_key.to_string(),
        }
    }

    /// Add a WHERE IN (subquery) condition.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().where_in_subquery(User::ID, User::query().select(vec!["id"])).validate().unwrap();
    /// ```
    pub fn where_in_subquery<N: Model>(
        self,
        column: impl IntoColumnName,
        subquery: QueryBuilder<N>,
    ) -> Self {
        let column = Self::own_column(&column);
        self.push_subquery_condition("where_in_subquery", &column, "IN", subquery)
    }

    /// Add a WHERE NOT IN (subquery) condition.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().where_not_in_subquery(User::ID, User::query().select(vec!["id"])).validate().unwrap();
    /// ```
    pub fn where_not_in_subquery<N: Model>(
        self,
        column: impl IntoColumnName,
        subquery: QueryBuilder<N>,
    ) -> Self {
        let column = Self::own_column(&column);
        self.push_subquery_condition("where_not_in_subquery", &column, "NOT IN", subquery)
    }

    /// Add a WHERE EXISTS (subquery) condition.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().where_exists(User::query().where_eq(User::ACTIVE, true)).validate().unwrap();
    /// ```
    pub fn where_exists<N: Model>(self, subquery: QueryBuilder<N>) -> Self {
        self.push_subquery_condition("where_exists", "", "EXISTS", subquery)
    }

    /// Add a WHERE NOT EXISTS (subquery) condition.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().where_not_exists(User::query().where_eq(User::ACTIVE, true)).validate().unwrap();
    /// ```
    pub fn where_not_exists<N: Model>(self, subquery: QueryBuilder<N>) -> Self {
        self.push_subquery_condition("where_not_exists", "", "NOT EXISTS", subquery)
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
        condition: Option<(&str, std::result::Result<crate::DbValue, String>)>,
    ) -> Self {
        let condition = match condition {
            Some((column, Ok(value))) => Some((column, value)),
            Some((_, Err(reason))) => {
                self.invalidate_query(format!("{}(): {}", method, reason));
                return self;
            }
            None => None,
        };

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

        // The model's own table is read under an alias, so the key on the
        // outer row stays reachable.
        let alias =
            (related_table == M::table_name()).then(|| related_alias(self.self_join_depth + 1));
        let condition = condition.map(|(column, value)| {
            // A NULL condition means a NULL column, as for `where_eq`: `= NULL`
            // is never true, which would turn `has_no_related` into a filter
            // that every row passes. Any other value is bound as the column's
            // type when a model maps the table, so a UUID or a timestamp is not
            // compared as text.
            let value = (!crate::query::is_sql_null(&value) && !crate::query::is_json_null(&value))
                .then(|| {
                    let column_type = crate::sync::registered_column_type(related_table, column);
                    crate::internal::column_value(&value, column_type.as_ref())
                });
            (column.to_string(), value)
        });
        let correlation = self.correlation(negated, foreign_key, local_key);
        self.push_embedded(
            "",
            Subquery::table(correlation, related_table.to_string(), alias, condition),
        )
    }

    /// Check if related records exist matching a condition.
    ///
    /// Every identifier must be a plain `table`/`column` name; anything else
    /// invalidates the query instead of being spliced into SQL. The table is
    /// named, not modeled, so its soft-deleted rows count too; to skip them,
    /// pass the related model's query to [`where_exists`](Self::where_exists).
    ///
    /// [`where_has`](Self::where_has) is the typed form: it names the related
    /// model, applies its soft-delete scope, and takes any filter.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().has_related("posts", "user_id", "id", "title", "Hello").validate().unwrap();
    /// ```
    pub fn has_related(
        self,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
        condition_column: &str,
        condition_value: impl Into<crate::DbValue>,
    ) -> Self {
        self.push_related_exists_condition(
            "has_related",
            false,
            related_table,
            foreign_key,
            local_key,
            Some((
                condition_column,
                crate::query::comparable_value(condition_value),
            )),
        )
    }

    /// Check that no related record matches a condition.
    ///
    /// A row with related records, none of them matching, passes, as does a
    /// row with none at all. Every identifier must be a plain `table`/`column`
    /// name; anything else invalidates the query instead of being spliced into
    /// SQL. The table is named, not modeled, so its soft-deleted rows count
    /// too; to skip them, pass the related model's query to
    /// [`where_not_exists`](Self::where_not_exists).
    ///
    /// [`where_doesnt_have`](Self::where_doesnt_have) is the typed form.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().has_no_related("posts", "user_id", "id", "title", "Hello").validate().unwrap();
    /// ```
    pub fn has_no_related(
        self,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
        condition_column: &str,
        condition_value: impl Into<crate::DbValue>,
    ) -> Self {
        self.push_related_exists_condition(
            "has_no_related",
            true,
            related_table,
            foreign_key,
            local_key,
            Some((
                condition_column,
                crate::query::comparable_value(condition_value),
            )),
        )
    }

    /// Keep the rows with at least one related `R` row that `constrain`
    /// keeps.
    ///
    /// `foreign_key` is `R`'s column that holds this model's `local_key`, and
    /// either may be a typed column. The related rows are `R`'s own query, so
    /// `R`'s soft-delete scope applies, its filter values bind as its columns'
    /// types, and every query method works inside `constrain`:
    ///
    /// ```ignore
    /// // Users with a published post
    /// User::query().where_has::<Post>(Post::columns.user_id, User::columns.id, |posts| {
    ///     posts.where_eq(Post::columns.published, true)
    /// });
    ///
    /// // Posts whose author is active: here the key sits on this side
    /// Post::query().where_has::<User>(User::columns.id, Post::columns.user_id, |users| {
    ///     users.where_eq(User::columns.active, true)
    /// });
    /// ```
    ///
    /// Pass `|q| q` to ask only that a related row exists. A related query
    /// that could not run fails this one; so does one that joins, pages or
    /// unions, which a correlated `EXISTS` cannot keep.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().where_has(User::ID, User::ID, |q: QueryBuilder<User>| q.where_eq(User::ACTIVE, true)).validate().unwrap();
    /// ```
    pub fn where_has<R: Model>(
        self,
        foreign_key: impl IntoColumnName,
        local_key: impl IntoColumnName,
        constrain: impl FnOnce(QueryBuilder<R>) -> QueryBuilder<R>,
    ) -> Self {
        let related = constrain(self.related_query::<R>());
        self.push_has_condition(
            "where_has",
            false,
            foreign_key.column_name(),
            local_key.column_name(),
            related,
        )
    }

    /// Keep the rows with no related `R` row that `constrain` keeps; the
    /// opposite of [`where_has`](Self::where_has), whose rules it follows. A
    /// row with no related rows at all passes.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().where_doesnt_have(User::ID, User::ID, |q: QueryBuilder<User>| q.where_eq(User::ACTIVE, true)).validate().unwrap();
    /// ```
    pub fn where_doesnt_have<R: Model>(
        self,
        foreign_key: impl IntoColumnName,
        local_key: impl IntoColumnName,
        constrain: impl FnOnce(QueryBuilder<R>) -> QueryBuilder<R>,
    ) -> Self {
        let related = constrain(self.related_query::<R>());
        self.push_has_condition(
            "where_doesnt_have",
            true,
            foreign_key.column_name(),
            local_key.column_name(),
            related,
        )
    }

    /// Push `[NOT] EXISTS (SELECT 1 FROM R WHERE R.fk = M.lk AND <related>)`,
    /// the correlated form of `related`'s own filters and scope.
    fn push_has_condition<R: Model>(
        mut self,
        method: &str,
        negated: bool,
        foreign_key: &str,
        local_key: &str,
        related: QueryBuilder<R>,
    ) -> Self {
        self.absorb_operand_error("related query", method, &related);
        if let Some(part) = related.update_blocker() {
            self.invalidate_query(format!(
                "{}() correlates the related rows through their filters only; the related query cannot hold {}",
                method, part
            ));
        }
        let keys = (
            R::own_column_name(foreign_key),
            M::own_column_name(local_key),
        );
        let (Some(foreign_key), Some(local_key)) = keys else {
            self.invalidate_query(format!(
                "{}(): '{}' must be a column of {} and '{}' a column of {}",
                method,
                foreign_key,
                R::table_name(),
                local_key,
                M::table_name()
            ));
            return self;
        };

        let correlation = self.correlation(negated, foreign_key, local_key);
        self.push_embedded("", Subquery::related(correlation, related))
    }

    /// The query `where_has` hands its closure. Over this query's own table
    /// it reads its rows under an alias one level deeper than this query's, so
    /// a qualified column or a nested correlation inside the closure names the
    /// related row, and the correlation names this one.
    fn related_query<R: Model>(&self) -> QueryBuilder<R> {
        let mut related = QueryBuilder::<R>::new();
        if R::table_name() == M::table_name() {
            related.self_join_depth = self.self_join_depth + 1;
        }
        related
    }

    /// Check if any related records exist (without condition).
    ///
    /// Every identifier must be a plain `table`/`column` name; anything else
    /// invalidates the query instead of being spliced into SQL. The table is
    /// named, not modeled, so its soft-deleted rows count too; to skip them,
    /// pass the related model's query to [`where_exists`](Self::where_exists).
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().has_any_related("posts", "user_id", "id").validate().unwrap();
    /// ```
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
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().has_no_related_at_all("posts", "user_id", "id").validate().unwrap();
    /// ```
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

    /// Add a raw SELECT expression.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().select_raw("COUNT(*) AS total").validate().unwrap();
    /// ```
    pub fn select_raw(mut self, raw_select: &str) -> Self {
        if let Err(reason) = db_sql::validate_raw_sql_fragment("SELECT raw SQL", raw_select) {
            self.invalidate_query(reason);
        }

        self.clauses
            .raw_select_expressions
            .push(raw_select.to_string());
        self
    }

    /// Add a scalar subquery as a SELECT expression.
    ///
    /// The subquery keeps its values as bound parameters, rendered with this
    /// query for the backend it runs on.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().select_subquery(User::query().select(vec!["id"]).limit(1), "first_id").validate().unwrap();
    /// ```
    pub fn select_subquery<N: Model>(mut self, subquery: QueryBuilder<N>, alias: &str) -> Self {
        self.absorb_operand_error("subquery", "select_subquery", &subquery);

        if let Err(reason) = db_sql::validate_identifier("SELECT alias", alias) {
            self.invalidate_query(reason);
        }

        // What the one column it selects reads, captured while the subquery
        // still knows its model and joins: the outer query decodes and
        // decrypts the output, and checks comparisons of it, by these.
        let (source, column_type, encrypted_column) = match subquery.projection_outputs().as_slice()
        {
            [(output, source)] => {
                let encrypted = subquery.projected_encrypted_column(output, source.as_ref());
                (
                    source.clone(),
                    subquery.projected_column_type(output),
                    encrypted,
                )
            }
            _ => (None, None, None),
        };
        self.clauses
            .subquery_select_expressions
            .push(SubquerySelect {
                query: std::sync::Arc::new(subquery),
                alias: alias.to_string(),
                source,
                column_type,
                encrypted_column,
            });
        self
    }
}

impl<M: Model> crate::columns::ConditionOwner for QueryBuilder<M> {
    fn own_table() -> Option<(&'static str, Option<&'static str>)> {
        crate::columns::model_table::<M>()
    }
}

crate::query::condition_methods! {
    impl[M: Model] QueryBuilder<M> {
        where + raw => push_condition,
            "Accepts a column name, a Rust field name, or a typed column; the condition is ANDed with the query's other filters.";
    }
}
