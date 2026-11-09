//! What decoding SQLite rows as JSON reads from the database itself.

use crate::error::Result;
use crate::internal::{QueryResult, translate_error};
use crate::logging::EngineLog;

use super::{ColumnTypeLookup, Database, SqliteSchema, sqlite_outputs};

impl Database {
    /// The [`SqliteSchema`] of `rows`, which `sql` returned, for the columns
    /// the driver names no type for and the model does not know. A table
    /// column is a document when the driver reports it declared `JSON` or
    /// `JSONB`; an expression is classified from the statement, reading the
    /// catalogs only when it comes from a source the statement does not
    /// describe.
    pub(in crate::database) async fn sqlite_schema(
        &self,
        rows: &[QueryResult],
        sql: &str,
        model_type: ColumnTypeLookup<'_>,
    ) -> Result<SqliteSchema> {
        use crate::internal::driver::{Column, ColumnOrigin, Row, TypeInfo};

        let mut schema = SqliteSchema::default();
        let Some(row) = rows.first().and_then(QueryResult::try_as_sqlite_row) else {
            return Ok(schema);
        };
        let columns = row.columns();
        schema.documents = vec![false; columns.len()];
        // The expressions the driver names no type for, and whether the model
        // knows each.
        let mut expressions: Vec<(usize, bool)> = Vec::new();
        for (index, column) in columns.iter().enumerate() {
            if !column.type_info().is_null() {
                continue;
            }
            let known = model_type(column.name()).is_some();
            match column.origin() {
                ColumnOrigin::Table(_) if !known => {
                    schema.documents[index] = column.declared_type().is_some_and(|declared| {
                        matches!(
                            declared.trim().to_ascii_uppercase().as_str(),
                            "JSON" | "JSONB"
                        )
                    });
                }
                ColumnOrigin::Table(_) => {}
                _ => expressions.push((index, known)),
            }
        }

        if !expressions.is_empty() {
            let names: Vec<&str> = columns.iter().map(Column::name).collect();
            let mut classified = sqlite_outputs::classify(sql, &names, &[]);
            if expressions
                .iter()
                .any(|&(index, known)| !known && classified[index].1)
            {
                classified = sqlite_outputs::classify(sql, &names, &self.sqlite_catalog().await?);
            }
            for (index, _) in expressions {
                schema.documents[index] = classified[index].0;
            }
        }
        Ok(schema)
    }

    /// Objects in SQLite lookup order: temp, main, then attachment order.
    /// Include tables so a table can shadow a view in a later schema.
    async fn sqlite_catalog(&self) -> Result<Vec<sqlite_outputs::CatalogObject>> {
        let databases = self.fetch_rows_logged(
            "SELECT name FROM pragma_database_list ORDER BY CASE name WHEN 'temp' THEN -2 WHEN 'main' THEN -1 ELSE seq END",
            Vec::new(),
            EngineLog::Off,
        ).await?;
        let mut selects = Vec::with_capacity(databases.len());
        let mut params = Vec::with_capacity(databases.len());
        for (order, database) in databases.into_iter().enumerate() {
            let name = database
                .try_get::<String>("name")
                .map_err(translate_error)?;
            let quoted = crate::internal::sql_safety::quote_ident(
                crate::config::DatabaseType::SQLite,
                &name,
            );
            selects.push(format!(
                "SELECT {order} AS schema_order, ? AS schema_name, name, CASE WHEN type = 'view' THEN sql END AS sql FROM {quoted}.sqlite_master WHERE type IN ('table', 'view')"
            ));
            params.push(name.into());
        }
        if selects.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!("{} ORDER BY schema_order", selects.join(" UNION ALL "));
        let rows = self.fetch_rows_logged(&sql, params, EngineLog::Off).await?;
        let mut objects = Vec::with_capacity(rows.len());
        for row in rows {
            objects.push(sqlite_outputs::CatalogObject {
                schema: row.try_get("schema_name").map_err(translate_error)?,
                name: row.try_get("name").map_err(translate_error)?,
                sql: row.try_get("sql").map_err(translate_error)?,
            });
        }
        Ok(objects)
    }
}
