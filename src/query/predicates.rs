use super::structure::SubquerySelect;
use super::{ConditionValue, Operator, QueryBuilder, WhereCondition};
use crate::columns::IntoColumnName;
use crate::config::DatabaseType;
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
        self.absorb_operand_error("subquery", method, subquery);
        self.clauses.dependencies.extend(subquery.cache_tables());

        let db_type = self.db_type_for_sql();
        let (mut sql, values) = subquery.build_select_sql_with_params_for_db(db_type);
        // MySQL and MariaDB take no LIMIT in an IN subquery, but do one level
        // further down.
        if keyword.ends_with("IN")
            && matches!(db_type, DatabaseType::MySQL | DatabaseType::MariaDB)
            && (subquery.clauses.limit_value.is_some() || subquery.clauses.offset_value.is_some())
        {
            sql = db_sql::select_from_derived(db_type, "*", &sql, "tideorm_in_subquery");
        }
        if let Err(reason) = db_sql::validate_compound_subquery_sql(&sql) {
            self.invalidate_query(format!("invalid subquery for {}(): {}", method, reason));
        }

        self.push_bound_raw(column, format!("{} ({})", keyword, sql), values)
    }

    /// Push `sql`, a builder-rendered fragment with its bound `values`, as a
    /// condition on `column` (empty for a whole predicate).
    fn push_bound_raw(
        self,
        column: &str,
        sql: String,
        values: Vec<crate::internal::Value>,
    ) -> Self {
        self.push_condition(WhereCondition {
            column: column.to_string(),
            operator: Operator::Raw,
            value: ConditionValue::RawExprWithValues { sql, values },
        })
    }

    /// `[NOT ]EXISTS (SELECT 1 FROM <from> WHERE <related>.<fk> = <own>.<lk>`,
    /// correlating the related rows with this query's row; the caller adds
    /// its conditions and closes the parenthesis.
    fn correlated_exists_head(
        &self,
        db_type: DatabaseType,
        negated: bool,
        from: &str,
        related: &str,
        foreign_key: &str,
        local_key: &str,
    ) -> String {
        format!(
            "{}EXISTS (SELECT 1 FROM {} WHERE {}.{} = {}.{}",
            if negated { "NOT " } else { "" },
            from,
            related,
            db_sql::quote_ident(db_type, foreign_key),
            db_sql::quote_ident(db_type, &self.own_table_ref()),
            db_sql::quote_ident(db_type, local_key),
        )
    }

    /// Add a WHERE IN (subquery) condition.
    #[must_use]
    pub fn where_in_subquery<N: Model>(
        self,
        column: impl IntoColumnName,
        subquery: QueryBuilder<N>,
    ) -> Self {
        let column = crate::columns::column_reference(&column, Some(M::table_name()));
        self.push_subquery_condition("where_in_subquery", &column, "IN", &subquery)
    }

    /// Add a WHERE NOT IN (subquery) condition.
    #[must_use]
    pub fn where_not_in_subquery<N: Model>(
        self,
        column: impl IntoColumnName,
        subquery: QueryBuilder<N>,
    ) -> Self {
        let column = crate::columns::column_reference(&column, Some(M::table_name()));
        self.push_subquery_condition("where_not_in_subquery", &column, "NOT IN", &subquery)
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
        condition: Option<(&str, std::result::Result<serde_json::Value, String>)>,
    ) -> Self {
        let db_type = self.db_type_for_sql();
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
        let table = db_sql::quote_ident(db_type, related_table);
        let (from, related) = if related_table == M::table_name() {
            let alias = db_sql::quote_ident(db_type, &related_alias(self.self_join_depth + 1));
            (format!("{} AS {}", table, alias), alias)
        } else {
            (table.clone(), table)
        };
        let mut sql =
            self.correlated_exists_head(db_type, negated, &from, &related, foreign_key, local_key);
        let mut values = Vec::new();

        if let Some((condition_column, value)) = condition {
            let column = format!(
                "{}.{}",
                related,
                db_sql::quote_ident(db_type, condition_column)
            );
            // A NULL condition means a NULL column, as for `where_eq`: `= NULL`
            // is never true, which would turn `has_no_related` into a filter
            // that every row passes.
            if value.is_null() {
                sql.push_str(&format!(" AND {} IS NULL", column));
            } else {
                sql.push_str(&format!(
                    " AND {} = {}",
                    column,
                    db_sql::placeholder(db_type, 1)
                ));
                // Bound as the column's type when a model maps the table, so a
                // UUID or a timestamp is not compared as text.
                let column_type =
                    crate::sync::registered_column_type(related_table, condition_column);
                values.push(crate::internal::json_to_column_value(
                    &value,
                    column_type.as_ref(),
                ));
            }
        }

        sql.push(')');
        self.push_bound_raw("", sql, values)
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
    #[must_use]
    pub fn has_related(
        self,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
        condition_column: &str,
        condition_value: impl serde::Serialize,
    ) -> Self {
        self.push_related_exists_condition(
            "has_related",
            false,
            related_table,
            foreign_key,
            local_key,
            Some((
                condition_column,
                crate::query::checked_filter_value(condition_value),
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
    #[must_use]
    pub fn has_no_related(
        self,
        related_table: &str,
        foreign_key: &str,
        local_key: &str,
        condition_column: &str,
        condition_value: impl serde::Serialize,
    ) -> Self {
        self.push_related_exists_condition(
            "has_no_related",
            true,
            related_table,
            foreign_key,
            local_key,
            Some((
                condition_column,
                crate::query::checked_filter_value(condition_value),
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
    #[must_use]
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
            &related,
        )
    }

    /// Keep the rows with no related `R` row that `constrain` keeps; the
    /// opposite of [`where_has`](Self::where_has), whose rules it follows. A
    /// row with no related rows at all passes.
    #[must_use]
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
            &related,
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
        related: &QueryBuilder<R>,
    ) -> Self {
        self.absorb_operand_error("related query", method, related);
        self.clauses.dependencies.extend(related.cache_tables());
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

        let db_type = self.db_type_for_sql();
        let (where_sql, values) = related.build_where_clause_with_condition_for_db(db_type);
        // A model related to itself reads the related rows under an alias of
        // their own (see `related_query`), so the key on the outer row stays
        // reachable.
        let related_ref = db_sql::quote_ident(db_type, &related.own_table_ref());
        let from = if related.self_join_depth > 0 {
            format!("{} AS {}", db_sql::quote_table::<R>(db_type), related_ref)
        } else {
            db_sql::quote_table::<R>(db_type)
        };
        let mut sql = self.correlated_exists_head(
            db_type,
            negated,
            &from,
            &related_ref,
            foreign_key,
            local_key,
        );
        if !where_sql.is_empty() {
            sql.push_str(&format!(" AND ({})", where_sql));
        }
        sql.push(')');
        self.push_bound_raw("", sql, values)
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
    /// shows, without the banner, for display only.
    pub fn to_subquery_sql(&self) -> String {
        self.build_select_sql_for_db(self.db_type_for_sql())
    }

    /// Add a raw SELECT expression.
    #[must_use]
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
    /// The subquery keeps its values as bound parameters, rendered for this
    /// query's backend.
    #[must_use]
    pub fn select_subquery<N: Model>(mut self, subquery: QueryBuilder<N>, alias: &str) -> Self {
        self.absorb_operand_error("subquery", "select_subquery", &subquery);
        self.clauses.dependencies.extend(subquery.cache_tables());

        if let Err(reason) = db_sql::validate_identifier("SELECT alias", alias) {
            self.invalidate_query(reason);
        }

        let (query_sql, params) =
            subquery.build_select_sql_with_params_for_db(self.db_type_for_sql());
        self.clauses
            .subquery_select_expressions
            .push(SubquerySelect {
                query_sql,
                alias: alias.to_string(),
                params,
            });
        self
    }

    /// Add a WHERE condition using a strongly-typed column.
    #[must_use]
    pub fn where_col(self, mut condition: crate::columns::ColumnCondition) -> Self {
        // A column of this query's own model needs no qualifier.
        if let Some((table, name)) = condition.column.split_once('.')
            && table == M::table_name()
        {
            condition.column = name.to_string();
        }
        self.push_condition(condition)
    }
}

impl<M: Model> crate::columns::ConditionOwner for QueryBuilder<M> {
    fn own_table() -> Option<&'static str> {
        Some(M::table_name())
    }
}

crate::query::condition_methods! {
    impl[M: Model] QueryBuilder<M> {
        where + raw => push_condition,
            "Accepts a column name, a Rust field name, or a typed column; the condition is ANDed with the query's other filters.";
    }
}
