//! What decoding SQLite rows as JSON reads from the database itself.

use std::collections::HashMap;

use crate::error::Result;
use crate::internal::{QueryResult, translate_error};

use super::{ColumnTypeLookup, Database, SqliteSchema, sqlite_outputs};

/// The columns of a table and their declared types, in a database a
/// `pragma_database_list` join names, so an unknown one gives no rows rather
/// than an error.
const DECLARED_TYPES: &str = "SELECT x.name AS name, x.type AS type \
     FROM pragma_database_list AS d JOIN pragma_table_xinfo(?, d.name) AS x WHERE d.name = ?";

impl Database {
    /// The [`SqliteSchema`] of `rows`, which `sql` returned. It is read only
    /// for columns the driver names no type for and the model does not know,
    /// as a `JSON` column and a view's expression are: a table's declared
    /// types, one query per table, and the catalogs when such a column is
    /// read from a source the statement does not describe.
    pub(in crate::database) async fn sqlite_schema(
        &self,
        rows: &[QueryResult],
        sql: &str,
        model_type: ColumnTypeLookup<'_>,
    ) -> Result<SqliteSchema> {
        use crate::internal::sqlx::{Column, ColumnOrigin, Row, TypeInfo};

        let mut schema = SqliteSchema::default();
        let Some(row) = rows.first().and_then(QueryResult::try_as_sqlite_row) else {
            return Ok(schema);
        };
        let columns = row.columns();
        schema.documents = vec![false; columns.len()];
        let mut tables: HashMap<String, Vec<(String, String)>> = HashMap::new();
        // The expressions the driver names no type for, and whether the model
        // knows each.
        let mut expressions: Vec<(usize, bool)> = Vec::new();
        for (index, column) in columns.iter().enumerate() {
            if !column.type_info().is_null() {
                continue;
            }
            let known = model_type(column.name()).is_some();
            match column.origin() {
                ColumnOrigin::Table(origin) if !known => {
                    if !tables.contains_key(&*origin.table) {
                        let types = self.sqlite_declared_types(&origin.table).await?;
                        tables.insert(origin.table.to_string(), types);
                    }
                    schema.documents[index] =
                        tables[&*origin.table].iter().any(|(name, declared)| {
                            name.eq_ignore_ascii_case(&origin.name)
                                && matches!(
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

    /// The columns of `table` and their declared types. A table outside the
    /// main database comes as `schema.table`, as sqlx names it; a main table
    /// whose own name holds a dot is looked up whole when that finds nothing.
    async fn sqlite_declared_types(&self, table: &str) -> Result<Vec<(String, String)>> {
        let mut candidates = vec![("main", table)];
        if let Some(qualified) = table.split_once('.') {
            candidates.insert(0, qualified);
        }
        for (schema, bare) in candidates {
            let rows = self
                .fetch_rows(DECLARED_TYPES, vec![bare.into(), schema.into()])
                .await?;
            if !rows.is_empty() {
                return rows
                    .iter()
                    .map(|row| {
                        Ok((
                            row.try_get::<String>("", "name").map_err(translate_error)?,
                            row.try_get::<Option<String>>("", "type")
                                .map_err(translate_error)?
                                .unwrap_or_default(),
                        ))
                    })
                    .collect();
            }
        }
        Ok(Vec::new())
    }

    /// Objects in SQLite lookup order: temp, main, then attachment order.
    /// Include tables so a table can shadow a view in a later schema.
    async fn sqlite_catalog(&self) -> Result<Vec<sqlite_outputs::CatalogObject>> {
        let databases = self.fetch_rows(
            "SELECT name FROM pragma_database_list ORDER BY CASE name WHEN 'temp' THEN -2 WHEN 'main' THEN -1 ELSE seq END",
            Vec::new(),
        ).await?;
        let mut selects = Vec::with_capacity(databases.len());
        let mut params = Vec::with_capacity(databases.len());
        for (order, database) in databases.into_iter().enumerate() {
            let name = database
                .try_get::<String>("", "name")
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
        let rows = self.fetch_rows(&sql, params).await?;
        let mut objects = Vec::with_capacity(rows.len());
        for row in rows {
            objects.push(sqlite_outputs::CatalogObject {
                schema: row.try_get("", "schema_name").map_err(translate_error)?,
                name: row.try_get("", "name").map_err(translate_error)?,
                sql: row.try_get("", "sql").map_err(translate_error)?,
            });
        }
        Ok(objects)
    }
}
