use super::*;
use crate::internal::sql_builder::SqlBuilder;

/// Predicate used when a SQLite full-text query carries no searchable terms.
const SQLITE_MATCH_NOTHING: &str = "1 = 0";

/// Render the MySQL/MariaDB `AGAINST(...)` search-mode modifier.
///
/// The row-returning, ranked, and counting builders must all render the *same*
/// modifier: a count taken in a different mode than the rows it paginates
/// reports a total that disagrees with the result set. The mapping therefore
/// lives here and nowhere else, and the match is deliberately exhaustive so a
/// new [`SearchMode`] variant has to be classified rather than silently falling
/// through to natural-language mode in some builders only.
fn mysql_against_mode_modifier(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Natural | SearchMode::Prefix | SearchMode::Fuzzy | SearchMode::Proximity(_) => {
            ""
        }
        SearchMode::Boolean => " IN BOOLEAN MODE",
        SearchMode::Phrase => " WITH QUERY EXPANSION",
    }
}

/// Build the FTS5 `MATCH` operand for a query, or `None` when it has no terms.
///
/// FTS5 rejects an empty operand outright (`fts5: syntax error near ""`), so a
/// whitespace-only or operator-only query must not reach `MATCH` at all; the
/// callers substitute a predicate that matches nothing instead.
fn sqlite_match_operand(query: &str) -> Option<String> {
    let operand = escape_fts5_query(query);
    if operand.is_empty() {
        None
    } else {
        Some(operand)
    }
}

/// Render the SQLite `WHERE` predicate, binding the FTS5 `MATCH` operand.
///
/// A `None` operand yields a match-nothing predicate so that a term-less query
/// returns no rows on every SQLite builder — rows, ranked rows, and count alike
/// — instead of failing at the driver with an FTS5 syntax error. Call this
/// before pushing any later parameter so the bound operand keeps its position.
fn sqlite_match_predicate(
    fts_table_name: &str,
    operand: Option<String>,
    params: &mut Vec<Value>,
) -> String {
    match operand {
        Some(operand) => SqlBuilder::new(DatabaseType::SQLite, params)
            .ident(fts_table_name)
            .raw(" MATCH ")
            .param(Value::String(Some(operand)))
            .into_sql(),
        None => SQLITE_MATCH_NOTHING.to_string(),
    }
}

impl<T: Model> FullTextSearchBuilder<T> {
    pub(super) fn build_mysql_sql(&self) -> Result<(String, Vec<Value>)> {
        let columns_str = self
            .columns
            .iter()
            .map(|c| quote_ident(DatabaseType::MySQL, c))
            .collect::<Vec<_>>()
            .join(", ");

        let mode_modifier = mysql_against_mode_modifier(self.config.mode);

        let mut params = Vec::new();
        let mut sql = SqlBuilder::new(DatabaseType::MySQL, &mut params)
            .raw("SELECT * FROM ")
            .ident(T::table_name())
            .raw(" WHERE MATCH(")
            .raw(&columns_str)
            .raw(") AGAINST(")
            .param(Value::String(Some(self.query.clone())))
            .raw(mode_modifier)
            .raw(") ")
            .into_sql();

        self.append_limit_offset(DatabaseType::MySQL, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_mysql_ranked_sql(&self) -> Result<(String, Vec<Value>)> {
        let columns_str = self
            .columns
            .iter()
            .map(|c| quote_ident(DatabaseType::MySQL, c))
            .collect::<Vec<_>>()
            .join(", ");

        let mode_modifier = mysql_against_mode_modifier(self.config.mode);

        let mut params = Vec::new();
        let rank_placeholder = crate::internal::push_param(
            DatabaseType::MySQL,
            &mut params,
            Value::String(Some(self.query.clone())),
        );
        let where_placeholder = crate::internal::push_param(
            DatabaseType::MySQL,
            &mut params,
            Value::String(Some(self.query.clone())),
        );

        let mut sql = SqlBuilder::new(DatabaseType::MySQL, &mut params)
            .raw("SELECT *, MATCH(")
            .raw(&columns_str)
            .raw(") AGAINST(")
            .placeholder(&rank_placeholder)
            .raw(mode_modifier)
            .raw(") AS _fts_rank FROM ")
            .ident(T::table_name())
            .raw(" WHERE MATCH(")
            .raw(&columns_str)
            .raw(") AGAINST(")
            .placeholder(&where_placeholder)
            .raw(mode_modifier)
            .raw(") ")
            .into_sql();

        if let Some(min_rank) = self.min_rank {
            let min_rank_placeholder = crate::internal::push_param(
                DatabaseType::MySQL,
                &mut params,
                Value::Double(Some(min_rank)),
            );
            let against_placeholder = crate::internal::push_param(
                DatabaseType::MySQL,
                &mut params,
                Value::String(Some(self.query.clone())),
            );
            sql.push_str("AND MATCH(");
            sql.push_str(&columns_str);
            sql.push_str(") AGAINST(");
            sql.push_str(&against_placeholder);
            sql.push_str(mode_modifier);
            sql.push_str(") >= ");
            sql.push_str(&min_rank_placeholder);
            sql.push(' ');
        }

        sql.push_str("ORDER BY _fts_rank DESC ");

        self.append_limit_offset(DatabaseType::MySQL, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_mysql_count_sql(&self) -> Result<(String, Vec<Value>)> {
        let columns_str = self
            .columns
            .iter()
            .map(|c| quote_ident(DatabaseType::MySQL, c))
            .collect::<Vec<_>>()
            .join(", ");

        let mode_modifier = mysql_against_mode_modifier(self.config.mode);

        let mut params = Vec::new();
        let sql = SqlBuilder::new(DatabaseType::MySQL, &mut params)
            .raw("SELECT COUNT(*) as count FROM ")
            .ident(T::table_name())
            .raw(" WHERE MATCH(")
            .raw(&columns_str)
            .raw(") AGAINST(")
            .param(Value::String(Some(self.query.clone())))
            .raw(mode_modifier)
            .raw(")")
            .into_sql();

        Ok((sql, params))
    }

    // =========================================================================
    // SQLITE IMPLEMENTATION (FTS5)
    // =========================================================================

    pub(super) fn build_sqlite_sql(&self) -> Result<(String, Vec<Value>)> {
        let table_name = T::table_name();
        let fts_table_name = format!("{}_fts", table_name);

        let mut params = Vec::new();
        let operand = sqlite_match_operand(&self.query);
        let predicate = sqlite_match_predicate(&fts_table_name, operand, &mut params);
        let mut sql = SqlBuilder::new(DatabaseType::SQLite, &mut params)
            .raw("SELECT t.* FROM ")
            .ident(table_name)
            .raw(" t INNER JOIN ")
            .ident(&fts_table_name)
            .raw(" fts ON t.rowid = fts.rowid WHERE ")
            .raw(&predicate)
            .raw(" ")
            .into_sql();

        self.append_limit_offset(DatabaseType::SQLite, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_sqlite_ranked_sql(&self) -> Result<(String, Vec<Value>)> {
        let table_name = T::table_name();
        let fts_table_name = format!("{}_fts", table_name);

        let mut params = Vec::new();
        let operand = sqlite_match_operand(&self.query);

        // `bm25()` is only usable in a query that carries a `MATCH` operator, so
        // a term-less query reports a constant rank over its empty result set
        // instead of referencing the ranking function at all.
        let rank_expr = operand.as_ref().map(|_| {
            format!(
                "bm25({})",
                quote_ident(DatabaseType::SQLite, &fts_table_name)
            )
        });
        let predicate = sqlite_match_predicate(&fts_table_name, operand, &mut params);

        let mut sql = SqlBuilder::new(DatabaseType::SQLite, &mut params)
            .raw("SELECT t.*, ")
            .raw(rank_expr.as_deref().unwrap_or("0.0"))
            .raw(" AS _fts_rank FROM ")
            .ident(table_name)
            .raw(" t INNER JOIN ")
            .ident(&fts_table_name)
            .raw(" fts ON t.rowid = fts.rowid WHERE ")
            .raw(&predicate)
            .raw(" ")
            .into_sql();

        if let (Some(rank_expr), Some(min_rank)) = (rank_expr.as_deref(), self.min_rank) {
            let min_rank_placeholder = crate::internal::push_param(
                DatabaseType::SQLite,
                &mut params,
                Value::Double(Some(-min_rank)),
            );
            sql.push_str("AND ");
            sql.push_str(rank_expr);
            sql.push_str(" <= ");
            sql.push_str(&min_rank_placeholder);
            sql.push(' ');
        }

        if let Some(rank_expr) = rank_expr.as_deref() {
            sql.push_str("ORDER BY ");
            sql.push_str(rank_expr);
            sql.push(' ');
        }

        self.append_limit_offset(DatabaseType::SQLite, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_sqlite_count_sql(&self) -> Result<(String, Vec<Value>)> {
        let table_name = T::table_name();
        let fts_table_name = format!("{}_fts", table_name);

        let mut params = Vec::new();
        let operand = sqlite_match_operand(&self.query);
        let predicate = sqlite_match_predicate(&fts_table_name, operand, &mut params);
        let sql = SqlBuilder::new(DatabaseType::SQLite, &mut params)
            .raw("SELECT COUNT(*) as count FROM ")
            .ident(table_name)
            .raw(" t INNER JOIN ")
            .ident(&fts_table_name)
            .raw(" fts ON t.rowid = fts.rowid WHERE ")
            .raw(&predicate)
            .into_sql();

        Ok((sql, params))
    }

    pub(super) fn pg_weights_placeholder(&self, params: &mut Vec<Value>) -> String {
        let weights = self
            .config
            .weights
            .as_ref()
            .map(|w| w.to_pg_array().trim_matches('\'').to_string())
            .unwrap_or_else(|| "{0.1,0.2,0.4,1.0}".to_string());

        crate::internal::push_param(DatabaseType::Postgres, params, Value::String(Some(weights)))
    }

    pub(super) fn append_limit_offset(
        &self,
        db_type: DatabaseType,
        sql: &mut String,
        params: &mut Vec<Value>,
    ) -> Result<()> {
        if let Some(limit) = self.limit {
            let limit_value = i64::try_from(limit)
                .map_err(|_| Error::query("Full-text search limit exceeds i64 range"))?;
            let placeholder =
                crate::internal::push_param(db_type, params, Value::BigInt(Some(limit_value)));
            sql.push_str(" LIMIT ");
            sql.push_str(&placeholder);
        }
        if let Some(offset) = self.offset {
            let offset_value = i64::try_from(offset)
                .map_err(|_| Error::query("Full-text search offset exceeds i64 range"))?;
            let placeholder =
                crate::internal::push_param(db_type, params, Value::BigInt(Some(offset_value)));
            sql.push_str(" OFFSET ");
            sql.push_str(&placeholder);
        }
        Ok(())
    }
}
