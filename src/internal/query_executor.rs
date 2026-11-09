//! The typed read and insert paths the `Model` defaults run.

use crate::error::{Error, Result};

use super::{
    QueryOrder, QuerySelect, Select, SelectExt, batch_insert, build_count_select, count_to_u64,
    model_error_context, models_read_through, run_statement, scoped_find,
};

/// The typed reads the `Model` defaults run, each on the current connection.
///
/// The connection is resolved before the profiled statement, so a missing or
/// unreachable database keeps its `Error::Connection` class.
#[doc(hidden)]
pub struct QueryExecutor;

impl QueryExecutor {
    /// Every row in the model's soft-delete scope.
    pub async fn find_all<M>() -> Result<Vec<M>>
    where
        M: crate::model::Model,
    {
        let connection = crate::database::__current_connection()?;
        let rows = run_statement(scoped_find::<M>().all(&connection.executor()), || {
            model_error_context::<M>("find_all()")
        })
        .await?;
        Ok(models_read_through(&connection, rows))
    }

    /// The first row in scope, in the backend's natural order.
    pub async fn first<M>() -> Result<Option<M>>
    where
        M: crate::model::Model,
    {
        Self::first_of::<M>(scoped_find::<M>(), "first()".to_string()).await
    }

    /// The row in scope with the highest primary key; unordered when the model
    /// declares no primary key column.
    pub async fn last<M>() -> Result<Option<M>>
    where
        M: crate::model::Model,
    {
        let mut select = scoped_find::<M>();
        let mut query_label = String::from("last()");

        let pk_columns = M::primary_key_columns();
        if !pk_columns.is_empty() {
            for pk_col in pk_columns {
                select = select.order_by_desc(pk_col);
            }
            query_label = format!("last(order_by={} desc)", M::primary_key_names().join(", "));
        }

        Self::first_of::<M>(select, query_label).await
    }

    /// The first row `select` finds.
    async fn first_of<M>(select: Select<M::Entity>, query_label: String) -> Result<Option<M>>
    where
        M: crate::model::Model,
    {
        let connection = crate::database::__current_connection()?;
        let row = run_statement(select.one(&connection.executor()), || {
            model_error_context::<M>(query_label)
        })
        .await?;
        Ok(row.map(|row| row.finish_load(&connection)))
    }

    /// `COUNT(*)` over the model's scoped rows.
    ///
    /// The statement always returns exactly one row, so it is read with `all`,
    /// which adds no `LIMIT`, and a missing row is a decode failure rather than
    /// an empty table.
    pub async fn count<M>() -> Result<u64>
    where
        M: crate::model::Model,
    {
        let connection = crate::database::__current_connection()?;
        let rows: Vec<i64> = run_statement(
            build_count_select::<M>()
                .into_tuple::<i64>()
                .all(&connection.executor()),
            || model_error_context::<M>("count(*)"),
        )
        .await?;

        let count = rows
            .into_iter()
            .next()
            .ok_or_else(|| Error::query("Database returned no row for count(*)"))?;
        count_to_u64(count, "count(*)")
    }

    /// Whether any row is in the model's scope.
    pub async fn exists_any<M>() -> Result<bool>
    where
        M: crate::model::Model,
    {
        let connection = crate::database::__current_connection()?;
        run_statement(scoped_find::<M>().exists(&connection.executor()), || {
            model_error_context::<M>("exists_any()")
        })
        .await
    }

    /// Page `page` of `per_page` rows in scope, in primary key order, with
    /// the count of every row in scope.
    ///
    /// Without an order PostgreSQL returns rows in heap order, which an UPDATE
    /// changes: a row rewritten between two page requests moves, and the next
    /// page repeats it and skips another.
    pub async fn paginate<M>(page: u64, per_page: u64) -> Result<crate::query::Paginated<M>>
    where
        M: crate::model::Model,
    {
        // The same check `QueryBuilder::page` makes, so both refuse alike.
        // Every backend takes LIMIT and OFFSET as signed 64-bit integers,
        // which it already bounds both by.
        let offset = crate::query::page_offset(page, per_page)
            .map_err(|(field, message)| Error::validation(field, message))?;
        let total = Self::count::<M>().await?;
        let mut select = scoped_find::<M>().limit(per_page).offset(offset);
        for pk_col in M::primary_key_columns() {
            select = select.order_by_asc(pk_col);
        }
        let connection = crate::database::__current_connection()?;
        let rows = run_statement(select.all(&connection.executor()), || {
            model_error_context::<M>(format!("paginate(limit={per_page}, offset={offset})"))
        })
        .await?;
        Ok(crate::query::Paginated {
            items: models_read_through(&connection, rows),
            total,
            page,
            per_page,
        })
    }

    // Deletes deliberately do not live here: the macro-generated `delete`,
    // which `destroy` runs too, filters on `primary_key_condition`, which
    // binds the key through `bindable_value`, rather than deleting an
    // `ActiveModel`.

    /// Insert `models` on the current connection and return them as stored,
    /// in the order they were passed; see `batch_insert` for the statements
    /// each backend runs.
    pub async fn insert_many<M>(models: Vec<M>) -> Result<Vec<M>>
    where
        M: crate::model::Model,
    {
        let connection = crate::database::__current_connection()?;
        // Triggers and foreign keys can change tables beyond the inserted model.
        connection
            .invalidating(batch_insert::insert_many(&connection, models))
            .await
    }
}
