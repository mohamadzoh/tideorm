use super::*;
use crate::fulltext::index_helpers::sqlite_rowid_guard;

const MYSQL: DatabaseType = DatabaseType::MySQL;
const SQLITE: DatabaseType = DatabaseType::SQLite;

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
/// — instead of failing at the driver with an FTS5 syntax error.
fn sqlite_match_predicate(fts_table_name: &str, operand: Option<String>) -> BoundSql {
    match operand {
        Some(operand) => {
            let mut matched =
                BoundSql::sql(format!("{} MATCH ", quote_ident(SQLITE, fts_table_name)));
            matched.push(Value::String(Some(operand)));
            matched
        }
        None => BoundSql::sql(MATCH_NOTHING),
    }
}

/// `operand` limited to `columns` with an FTS5 column filter: the FTS5 table
/// holds every column of the index, and a bare `MATCH` searches all of them.
fn fts5_in_columns(columns: &[String], operand: String) -> String {
    if columns.is_empty() {
        return operand;
    }
    let names: Vec<String> = columns
        .iter()
        .map(|column| quote_ident(SQLITE, column))
        .collect();
    format!("{{{}}} : ({operand})", names.join(" "))
}

impl<T: Model> FullTextSearchBuilder<T> {
    pub(super) fn build_mysql_sql(&self) -> (String, Vec<Value>) {
        self.select_where(MYSQL, self.mysql_predicate())
    }

    pub(super) fn build_mysql_ranked_sql(&self) -> (String, Vec<Value>) {
        let rank_alias = Self::rank_alias();
        let mut sql = BoundSql::sql(format!(
            "SELECT {}, ",
            crate::query::db_sql::model_columns_sql::<T>(MYSQL, None)
        ));
        sql.push(self.mysql_match())
            .push(format!(
                " AS {rank_alias} FROM {} WHERE ",
                crate::query::db_sql::quote_table::<T>(MYSQL)
            ))
            .push(self.mysql_predicate());
        if let Some(min_rank) = self.min_rank {
            sql.push(" AND ")
                .push(self.mysql_match())
                .push(" >= ")
                .push(Value::Double(Some(min_rank)));
        }
        sql.push(format!(" ORDER BY {rank_alias} DESC"))
            .push(MYSQL.dialect().limit_offset(self.limit, self.offset));
        sql.build(MYSQL.dialect())
    }

    pub(super) fn build_mysql_count_sql(&self) -> (String, Vec<Value>) {
        Self::count_where(MYSQL, self.mysql_predicate())
    }

    /// The `AGAINST(..)` operand for the search mode, empty when the search
    /// has no word: the words as a quoted phrase, each word as a required
    /// prefix, or the phrase with InnoDB's `@distance`.
    fn mysql_operand(&self) -> String {
        if !self.has_search_terms() {
            return String::new();
        }
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

    /// The `WHERE` predicate: the match, within the soft-delete scope.
    fn mysql_predicate(&self) -> BoundSql {
        self.scoped(MYSQL, self.mysql_match(), None)
    }

    /// `MATCH(<columns>) AGAINST(? <mode>)`, binding the query, or a
    /// match-nothing predicate when the query has no searchable terms.
    fn mysql_match(&self) -> BoundSql {
        let operand = self.mysql_operand();
        if operand.is_empty() {
            return BoundSql::sql(MATCH_NOTHING);
        }
        let mut matched = BoundSql::sql(format!(
            "MATCH({}) AGAINST(",
            column_list(MYSQL, &self.column_names(), "")
        ));
        matched
            .push(Value::String(Some(operand)))
            .push(mysql_against_mode_modifier(self.config.mode))
            .push(")");
        matched
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
        (!operand.is_empty()).then(|| fts5_in_columns(&self.column_names(), operand))
    }

    pub(super) fn build_sqlite_sql(&self) -> (String, Vec<Value>) {
        let mut sql = BoundSql::sql(format!(
            "SELECT {}",
            crate::query::db_sql::model_columns_sql::<T>(SQLITE, Some("t"))
        ));
        sql.push(self.sqlite_from(self.sqlite_match_operand()))
            .push(self.sqlite_guarded_limit());
        sql.build(SQLITE.dialect())
    }

    pub(super) fn build_sqlite_ranked_sql(&self) -> (String, Vec<Value>) {
        let operand = self.sqlite_match_operand();

        // `bm25()` is only usable in a query that carries a `MATCH` operator, so
        // a term-less query reports a constant rank over its empty result set
        // instead of referencing the ranking function at all. It scores a
        // better match lower, below zero, so the rank is its negation: higher
        // is more relevant, as on the other backends.
        let rank = operand
            .as_ref()
            .map(|_| format!("-bm25({})", quote_ident(SQLITE, &Self::sqlite_fts_table())));
        let mut sql = BoundSql::sql(format!(
            "SELECT {}, {} AS {}",
            crate::query::db_sql::model_columns_sql::<T>(SQLITE, Some("t")),
            rank.as_deref().unwrap_or("0.0"),
            Self::rank_alias()
        ));
        sql.push(self.sqlite_from(operand));
        if let Some(rank) = &rank {
            if let Some(min_rank) = self.min_rank {
                sql.push(format!(" AND {rank} >= "))
                    .push(Value::Double(Some(min_rank)));
            }
            sql.push(format!(" ORDER BY {rank} DESC"));
        }
        sql.push(self.sqlite_guarded_limit());
        sql.build(SQLITE.dialect())
    }

    pub(super) fn build_sqlite_count_sql(&self) -> (String, Vec<Value>) {
        let mut sql = BoundSql::sql("SELECT COUNT(*) as count");
        sql.push(self.sqlite_from(self.sqlite_match_operand()))
            .push(format!(
                " LIMIT {}",
                sqlite_rowid_guard(T::table_name(), T::schema_name(), -1)
            ));
        sql.build(SQLITE.dialect())
    }

    /// ` LIMIT <rowid guard> [OFFSET ?]`. The guard checks the live schema,
    /// columns the model leaves out included, and is the limit because SQLite
    /// evaluates a limit once, before the scan, even when no row matches: a
    /// search needs no statement of its own for it.
    fn sqlite_guarded_limit(&self) -> BoundSql {
        let limit = self
            .limit
            .and_then(|limit| i64::try_from(limit).ok())
            .unwrap_or(-1);
        let mut sql = BoundSql::sql(format!(
            " LIMIT {}",
            sqlite_rowid_guard(T::table_name(), T::schema_name(), limit)
        ));
        if let Some(offset) = self.offset {
            sql.push(" OFFSET ").push(Value::from(offset));
        }
        sql
    }

    /// The FTS5 table an index built by [`FullTextIndex::to_sqlite_sql`] keeps
    /// in sync with the model's table.
    fn sqlite_fts_table() -> String {
        format!("{}_fts", T::table_name())
    }

    /// ` FROM <table> t INNER JOIN <fts table> fts ... WHERE <match>`, binding
    /// the `MATCH` operand.
    fn sqlite_from(&self, operand: Option<String>) -> BoundSql {
        let fts_table = Self::sqlite_fts_table();
        // The index lives beside the content table. An unqualified join can
        // resolve to a same-named index in main or another attached database.
        // MATCH and bm25 still name the FTS table's hidden column, unqualified.
        let fts_reference = quote_qualified(SQLITE, T::schema_name(), &fts_table);
        let mut from = BoundSql::sql(format!(
            " FROM {} t INNER JOIN {fts_reference} fts ON t.rowid = fts.rowid WHERE ",
            crate::query::db_sql::quote_table::<T>(SQLITE)
        ));
        let predicate = sqlite_match_predicate(&fts_table, operand);
        from.push(self.scoped(SQLITE, predicate, Some("t")));
        from
    }
}
