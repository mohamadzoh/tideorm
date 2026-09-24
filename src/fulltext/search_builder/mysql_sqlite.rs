use super::*;

/// Render the MySQL/MariaDB `AGAINST(...)` search-mode modifier.
///
/// The row-returning, ranked, and counting builders must all render the *same*
/// modifier: a count taken in a different mode than the rows it paginates
/// reports a total that disagrees with the result set. The mapping therefore
/// lives here and nowhere else, and the match is deliberately exhaustive so a
/// new [`SearchMode`] variant has to be classified rather than silently falling
/// through to natural-language mode in some builders only. Every mode but the
/// natural one is written with boolean-mode operators (see `mysql_operand`).
fn mysql_against_mode_modifier(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Natural => "",
        SearchMode::Boolean
        | SearchMode::Phrase
        | SearchMode::Prefix
        | SearchMode::Proximity(_) => " IN BOOLEAN MODE",
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
        None => MATCH_NOTHING.to_string(),
    }
}

// MySQL and SQLite placeholders bind by position, so every builder here renders
// its fragments — and pushes their values — in the order they appear in the
// statement.
impl<T: Model> FullTextSearchBuilder<T> {
    pub(super) fn build_mysql_sql(&self) -> Result<(String, Vec<Value>)> {
        let mut params = Vec::new();
        let predicate = self.mysql_match(&mut params);
        let mut sql = SqlBuilder::new(DatabaseType::MySQL, &mut params)
            .raw("SELECT ")
            .raw(&crate::query::db_sql::model_columns_sql::<T>(
                DatabaseType::MySQL,
                None,
            ))
            .raw(" FROM ")
            .raw(&crate::query::db_sql::quote_table::<T>(DatabaseType::MySQL))
            .raw(" WHERE ")
            .raw(&predicate)
            .raw(" ")
            .into_sql();

        self.append_limit_offset(DatabaseType::MySQL, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_mysql_ranked_sql(&self) -> Result<(String, Vec<Value>)> {
        let mut params = Vec::new();
        let rank = self.mysql_match(&mut params);
        let predicate = self.mysql_match(&mut params);
        let mut sql = SqlBuilder::new(DatabaseType::MySQL, &mut params)
            .raw("SELECT ")
            .raw(&crate::query::db_sql::model_columns_sql::<T>(
                DatabaseType::MySQL,
                None,
            ))
            .raw(", ")
            .raw(&rank)
            .raw(" AS _fts_rank FROM ")
            .raw(&crate::query::db_sql::quote_table::<T>(DatabaseType::MySQL))
            .raw(" WHERE ")
            .raw(&predicate)
            .raw(" ")
            .into_sql();

        if let Some(min_rank) = self.min_rank {
            let threshold_rank = self.mysql_match(&mut params);
            let threshold = push_param(
                DatabaseType::MySQL,
                &mut params,
                Value::Double(Some(min_rank)),
            );
            sql.push_str("AND ");
            sql.push_str(&threshold_rank);
            sql.push_str(" >= ");
            sql.push_str(&threshold);
            sql.push(' ');
        }

        sql.push_str("ORDER BY _fts_rank DESC ");

        self.append_limit_offset(DatabaseType::MySQL, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_mysql_count_sql(&self) -> Result<(String, Vec<Value>)> {
        let mut params = Vec::new();
        let predicate = self.mysql_match(&mut params);
        let sql = SqlBuilder::new(DatabaseType::MySQL, &mut params)
            .raw("SELECT COUNT(*) as count FROM ")
            .raw(&crate::query::db_sql::quote_table::<T>(DatabaseType::MySQL))
            .raw(" WHERE ")
            .raw(&predicate)
            .into_sql();

        Ok((sql, params))
    }

    /// The `AGAINST(..)` operand for the search mode, empty when the search
    /// has no word: the words as a quoted phrase, each word as a required
    /// prefix, or the phrase with InnoDB's `@distance`.
    fn mysql_operand(&self) -> String {
        let text = self.query_text();
        if self.config.mode == SearchMode::Boolean {
            return sanitize_mysql_fulltext_query(&text, true);
        }
        let words = sanitize_mysql_fulltext_query(&text, false);
        if words.is_empty() {
            return words;
        }
        match self.config.mode {
            SearchMode::Natural | SearchMode::Boolean => words,
            SearchMode::Phrase => format!("\"{words}\""),
            SearchMode::Prefix => words
                .split(' ')
                .map(|word| format!("+{word}*"))
                .collect::<Vec<_>>()
                .join(" "),
            SearchMode::Proximity(distance) => format!("\"{words}\" @{distance}"),
        }
    }

    /// Render `MATCH(<columns>) AGAINST(? <mode>)`, binding the query for it,
    /// or a match-nothing predicate when the query has no searchable terms.
    fn mysql_match(&self, params: &mut Vec<Value>) -> String {
        let operand = self.mysql_operand();
        if operand.is_empty() {
            return MATCH_NOTHING.to_string();
        }
        SqlBuilder::new(DatabaseType::MySQL, params)
            .raw("MATCH(")
            .raw(&column_list(DatabaseType::MySQL, &self.columns, ""))
            .raw(") AGAINST(")
            .param(Value::String(Some(operand)))
            .raw(mysql_against_mode_modifier(self.config.mode))
            .raw(")")
            .into_sql()
    }

    /// The FTS5 `MATCH` operand for the search mode, or `None` when it has no
    /// terms.
    ///
    /// FTS5 rejects an empty operand outright (`fts5: syntax error near ""`),
    /// so a whitespace-only or operator-only query must not reach `MATCH` at
    /// all; the callers substitute a predicate that matches nothing instead.
    fn sqlite_match_operand(&self) -> Option<String> {
        let text = self.query_text();
        let operand = match self.config.mode {
            SearchMode::Natural => escape_fts5_query(&text),
            SearchMode::Boolean => fts5_boolean_query(&text),
            SearchMode::Phrase => fts5_phrase_query(&text),
            SearchMode::Prefix => fts5_prefix_query(&text),
            SearchMode::Proximity(distance) => fts5_near_query(&text, distance),
        };
        (!operand.is_empty()).then_some(operand)
    }

    pub(super) fn build_sqlite_sql(&self) -> Result<(String, Vec<Value>)> {
        let mut params = Vec::new();
        let from = Self::sqlite_from(self.sqlite_match_operand(), &mut params);
        let mut sql = SqlBuilder::new(DatabaseType::SQLite, &mut params)
            .raw("SELECT ")
            .raw(&crate::query::db_sql::model_columns_sql::<T>(
                DatabaseType::SQLite,
                Some("t"),
            ))
            .raw(&from)
            .raw(" ")
            .into_sql();

        self.append_limit_offset(DatabaseType::SQLite, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_sqlite_ranked_sql(&self) -> Result<(String, Vec<Value>)> {
        let mut params = Vec::new();
        let operand = self.sqlite_match_operand();

        // `bm25()` is only usable in a query that carries a `MATCH` operator, so
        // a term-less query reports a constant rank over its empty result set
        // instead of referencing the ranking function at all. It scores a
        // better match lower, below zero, so the rank is its negation: higher
        // is more relevant, as on the other backends.
        let rank = operand.as_ref().map(|_| {
            format!(
                "-bm25({})",
                quote_ident(DatabaseType::SQLite, &Self::sqlite_fts_table())
            )
        });
        let from = Self::sqlite_from(operand, &mut params);

        let mut sql = SqlBuilder::new(DatabaseType::SQLite, &mut params)
            .raw("SELECT ")
            .raw(&crate::query::db_sql::model_columns_sql::<T>(
                DatabaseType::SQLite,
                Some("t"),
            ))
            .raw(", ")
            .raw(rank.as_deref().unwrap_or("0.0"))
            .raw(" AS _fts_rank")
            .raw(&from)
            .raw(" ")
            .into_sql();

        if let Some(rank) = rank.as_deref() {
            if let Some(min_rank) = self.min_rank {
                let threshold = push_param(
                    DatabaseType::SQLite,
                    &mut params,
                    Value::Double(Some(min_rank)),
                );
                sql.push_str("AND ");
                sql.push_str(rank);
                sql.push_str(" >= ");
                sql.push_str(&threshold);
                sql.push(' ');
            }

            sql.push_str("ORDER BY ");
            sql.push_str(rank);
            sql.push_str(" DESC ");
        }

        self.append_limit_offset(DatabaseType::SQLite, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    pub(super) fn build_sqlite_count_sql(&self) -> Result<(String, Vec<Value>)> {
        let mut params = Vec::new();
        let from = Self::sqlite_from(self.sqlite_match_operand(), &mut params);
        let sql = SqlBuilder::new(DatabaseType::SQLite, &mut params)
            .raw("SELECT COUNT(*) as count")
            .raw(&from)
            .into_sql();

        Ok((sql, params))
    }

    /// The FTS5 table an index built by [`FullTextIndex::to_sqlite_sql`] keeps
    /// in sync with the model's table.
    fn sqlite_fts_table() -> String {
        format!("{}_fts", T::table_name())
    }

    /// Render ` FROM <table> t INNER JOIN <fts table> fts ... WHERE <match>`,
    /// binding the `MATCH` operand.
    fn sqlite_from(operand: Option<String>, params: &mut Vec<Value>) -> String {
        let fts_table = Self::sqlite_fts_table();
        let predicate = sqlite_match_predicate(&fts_table, operand, params);
        SqlBuilder::new(DatabaseType::SQLite, params)
            .raw(" FROM ")
            .raw(&crate::query::db_sql::quote_table::<T>(
                DatabaseType::SQLite,
            ))
            .raw(" t INNER JOIN ")
            .ident(&fts_table)
            .raw(" fts ON t.rowid = fts.rowid WHERE ")
            .raw(&predicate)
            .into_sql()
    }
}
