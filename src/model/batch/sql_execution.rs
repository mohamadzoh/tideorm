use super::*;

use crate::config::DatabaseType;
use crate::internal::Value;
use crate::orm::ColumnType;
use crate::orm::sql_builder::{
    BoundSql, Expr, ExprTrait, Returning, SelectStatement, UpdateStatement,
};
use crate::query::db_sql;

mod arithmetic;
mod assignments;

impl<M: Model> BatchUpdateBuilder<M> {
    pub(crate) fn ensure_backend_supports_returning(
        db_type: crate::config::DatabaseType,
    ) -> Result<()> {
        if !db_type.supports_returning() {
            return Err(Error::backend_not_supported(
                format!(
                    "execute_returning() is not supported on {}: it needs UPDATE .. RETURNING",
                    db_type
                ),
                db_type.to_string(),
            ));
        }

        Ok(())
    }

    fn build_where_query(&self) -> QueryBuilder<M> {
        // Live rows, as a query reads them, unless the update chose otherwise;
        // an update started from a query keeps that query's scope.
        let mut query = self.base.clone().unwrap_or_default();
        match self.include_trashed {
            Some(true) => query = query.with_trashed(),
            Some(false) => query = query.live_rows_only(),
            None => {}
        }

        query
            .clauses
            .conditions
            .extend(self.conditions.iter().cloned());
        // The update's `or_where_*` calls join the query's own OR group.
        for condition in &self.or_group.conditions {
            query = query.push_or_condition(condition.clone());
        }

        query
    }

    /// Report a value a setter refused, such as a NaN float; its assignment
    /// was never staged, so the update would otherwise run without it.
    fn ensure_values_are_bindable(&self) -> Result<()> {
        match &self.invalid_reason {
            Some(reason) => Err(Error::query(format!(
                "update of {}: {}",
                M::table_name(),
                reason
            ))),
            None => Ok(()),
        }
    }

    /// Refuse the parts of a query an `UPDATE` cannot keep: it takes the
    /// query's filters, scope and database, and nothing that reshapes rows.
    fn ensure_base_is_updatable(&self) -> Result<()> {
        match self.base.as_ref().and_then(QueryBuilder::update_blocker) {
            Some(part) => Err(Error::query(format!(
                "update_all() of a {} query keeps its filters and scope only; {} cannot be part of an UPDATE",
                M::table_name(),
                part
            ))),
            None => Ok(()),
        }
    }

    /// The database the update runs on: the one its query named, else the
    /// scope's.
    fn database(&self) -> Result<crate::database::Database> {
        self.base
            .as_ref()
            .map_or_else(crate::database::__current_db, QueryBuilder::current_db)
    }

    /// A batch update's filters must pass the query builder's validation, and
    /// at least one of them must be able to exclude a row; these are the same
    /// guards the `QueryBuilder` mutation terminals use, and the statement
    /// renders its filter through their `mutation_where_bound` as well.
    pub(crate) fn ensure_explicit_filters(&self, operation: &str) -> Result<()> {
        self.ensure_base_is_updatable()?;
        let query = self.build_where_query();
        query.ensure_query_is_executable()?;
        query.ensure_mutation_has_explicit_filters(operation)
    }

    /// The primary-key column used to cap an `UPDATE` on backends that
    /// cannot take `LIMIT` directly.
    fn limit_scope_primary_key(db_type: DatabaseType) -> Result<&'static str> {
        match M::primary_key_names() {
            [column] => Ok(column),
            columns => Err(Error::query(format!(
                "limit() is not supported for '{}' on {}: that backend cannot cap an UPDATE \
                 directly, so the row limit has to be scoped through a primary-key subquery, \
                 which needs a single primary key column (found {})",
                M::table_name(),
                db_type,
                columns.len()
            ))),
        }
    }

    /// The `UPDATE` statement, without any `RETURNING` clause, for `db_type`.
    fn update_statement(&self, db_type: DatabaseType) -> Result<UpdateStatement> {
        let table = || db_sql::table_name(M::schema_name(), M::table_name());
        let mut update = UpdateStatement::new();
        update.table(table());
        self.add_assignments(&mut update, db_type)?;

        let query = self.build_where_query();
        let filter = Expr::from(query.mutation_where_bound(db_type, "update", false)?);
        // The rows a limit keeps are the first in the query's order, as a
        // read of them would have it.
        match self.limit_value {
            // Where an `UPDATE` takes no `LIMIT`, the cap is enforced by
            // scoping the update to a bounded primary-key subquery. `limit()`
            // is a blast-radius control; dropping it on those backends would
            // widen exactly what the caller asked to contain.
            Some(limit) if !db_type.dialect().update_limit() => {
                let primary_key = Self::limit_scope_primary_key(db_type)?;
                let mut rows = SelectStatement::new();
                rows.column(primary_key).from(table()).and_where(filter);
                query.add_ordering(&mut rows, db_type);
                rows.limit(limit);
                update.and_where(Expr::col(primary_key).in_subquery(rows));
            }
            limit => {
                update.and_where(filter);
                if let Some(limit) = limit {
                    query.add_ordering(&mut update, db_type);
                    update.limit(limit);
                }
            }
        }
        Ok(update)
    }

    /// [`update_statement`](Self::update_statement), rendered.
    fn build_update_statement(&self, db_type: DatabaseType) -> Result<(String, Vec<Value>)> {
        db_sql::built(db_type, &self.update_statement(db_type)?)
    }

    /// Run the update and report how many rows it changed.
    ///
    /// This is a single `UPDATE` statement: model callbacks, validations, and
    /// automatic timestamp columns are **not** applied, and no rows are loaded.
    /// Use it for bulk maintenance writes; use `model.save()` when the model's
    /// own lifecycle matters.
    ///
    /// Returns `Ok(0)` without touching the database when no assignment was
    /// staged, and errors when the builder carries no explicit filter. Use
    /// [`execute_returning`](Self::execute_returning) for the rows.
    ///
    /// On success the query cache for this table is invalidated.
    pub async fn execute(self) -> Result<u64> {
        self.ensure_values_are_bindable()?;
        if self.updates.is_empty() {
            return Ok(0);
        }

        self.ensure_explicit_filters("update")?;

        // Resolve the dialect from the very handle that will run the statement:
        // inside `some_db.transaction(..)` that is `some_db`, whatever the
        // global connection is, or whether there is one.
        let db = self.database()?;
        let db_type = db.execution_backend();

        let (sql, params) = self.build_update_statement(db_type)?;

        QueryBuilder::<M>::invalidating_model_state(
            &db,
            db.__execute_with_params(&sql, params),
            |rows| *rows,
        )
        .await
    }

    /// Run the update and return the rows it wrote.
    ///
    /// Appends `RETURNING`, so it needs a backend whose `UPDATE` takes one:
    /// PostgreSQL and SQLite. MySQL has no `RETURNING`, and MariaDB has
    /// `UPDATE .. RETURNING` only from 13.0, so both are refused with an error
    /// before anything runs, whatever the server's version.
    ///
    /// Returns an empty vector without touching the database when no assignment
    /// was staged, and errors when the builder carries no explicit filter. Like
    /// [`execute`](Self::execute), no callbacks or validations run.
    ///
    /// The update and the decoding of the rows it returns are not one unit: if
    /// a row does not decode into the model, the error comes back but the rows
    /// are already written, and retrying repeats a non-idempotent change such
    /// as `increment`. Run it inside `Database::transaction` when that matters.
    pub async fn execute_returning(self) -> Result<Vec<M>> {
        self.ensure_values_are_bindable()?;
        if self.updates.is_empty() {
            return Ok(vec![]);
        }

        self.ensure_explicit_filters("update")?;

        let db = self.database()?;
        let db_type = db.execution_backend();
        Self::ensure_backend_supports_returning(db_type)?;

        let mut update = self.update_statement(db_type)?;
        update.returning(Returning::new().columns(M::column_names().iter().copied()));
        let (sql, params) = db_sql::built(db_type, &update)?;

        // A row that fails to decode was still written, so the table's cached
        // reads are dropped whether or not the decoding succeeds.
        QueryBuilder::<M>::invalidating_model_state(
            &db,
            db.raw_models::<M>(&sql, params, crate::logging::EngineLog::On),
            |models| models.len() as u64,
        )
        .await
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/model_batch_sql_execution_tests.rs"]
mod tests;
