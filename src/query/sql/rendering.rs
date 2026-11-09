mod select;

use super::*;
use crate::orm::sql_builder::SelectStatement;

impl<M: Model> QueryBuilder<M> {
    /// `SELECT <projection>` over the query's table, joins and filters alone,
    /// for a count or an aggregate that no other clause shapes.
    pub(in crate::query) fn plain_select(
        &self,
        db_type: DatabaseType,
        projection: impl Into<String>,
    ) -> Result<SelectStatement> {
        let mut select = SelectStatement::new();
        select.expr(Expr::cust(projection.into()));
        self.add_tables_and_filter(&mut select, db_type)?;
        Ok(select)
    }

    pub(crate) fn build_count_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
    ) -> Result<(String, Vec<Value>)> {
        let mut count_query = self.clone();
        count_query.clauses.order_by.clear();
        count_query.clauses.limit_value = None;
        count_query.clauses.offset_value = None;
        // A grouped query counts its groups, one row each, so the inner query
        // selects a constant unless the caller chose a projection: selecting
        // the grouping columns would name two of one name (`orders.status`,
        // `customers.status`) alike in the derived table, which MySQL refuses.
        if !count_query.clauses.group_by.is_empty() && !count_query.has_explicit_projection() {
            count_query.clauses.select_columns = None;
            count_query.clauses.raw_select_expressions = vec!["1".to_string()];
        }

        if count_query.clauses.unions.is_empty()
            && count_query.clauses.ctes.is_empty()
            && count_query.clauses.group_by.is_empty()
            && count_query.clauses.having_conditions.is_empty()
            && count_query.clauses.raw_select_expressions.is_empty()
            && !count_query.clauses.lock_for_update
        {
            return db_sql::built(
                db_type,
                &count_query.plain_select(db_type, "COUNT(*) AS count")?,
            );
        }

        let select = db_sql::select_from_derived(
            "COUNT(*) AS count",
            count_query.select_statement(db_type)?,
            "tideorm_count_subquery",
        );
        db_sql::built(db_type, &select)
    }

    pub(super) fn build_count_sql_with_params(&self) -> Result<(String, Vec<Value>)> {
        self.build_count_sql_with_params_for_db(self.db_type_for_sql()?)
    }

    pub(crate) fn build_exists_sql_with_params_for_db(
        &self,
        db_type: DatabaseType,
    ) -> Result<(String, Vec<Value>)> {
        let mut exists_query = self.clone();
        exists_query.clauses.order_by.clear();
        exists_query.clauses.limit_value = None;
        exists_query.clauses.offset_value = None;

        if exists_query.clauses.unions.is_empty() {
            // Raw projections may aggregate empty input into one row. Keep
            // them without trying to recognize aggregate functions in SQL.
            // HAVING can also refer to aliases from an explicit projection.
            let preserve_projection = exists_query.raw_projection().next().is_some()
                || (!exists_query.clauses.having_conditions.is_empty()
                    && exists_query.has_explicit_projection());
            if !preserve_projection {
                exists_query.clauses.select_columns = None;
                exists_query.clauses.raw_select_expressions = vec!["1".to_string()];
                exists_query.clauses.subquery_select_expressions.clear();
                exists_query.clauses.window_functions.clear();
            }

            if exists_query.clauses.ctes.is_empty() && !exists_query.clauses.lock_for_update {
                let mut inner = exists_query.select_statement(db_type)?;
                inner.limit(1);
                let mut select = SelectStatement::new();
                select.expr_as(Expr::exists(inner), "exists_result");
                return db_sql::built(db_type, &select);
            }
        }

        let mut select = db_sql::select_from_derived(
            "1",
            exists_query.select_statement(db_type)?,
            "tideorm_exists_subquery",
        );
        select.limit(1);
        db_sql::built(db_type, &select)
    }

    pub(super) fn build_exists_sql_with_params(&self) -> Result<(String, Vec<Value>)> {
        self.build_exists_sql_with_params_for_db(self.db_type_for_sql()?)
    }
}
