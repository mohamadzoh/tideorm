use super::*;
use crate::internal::{EntityTrait, FromQueryResult, translate_error};

mod mysql_sqlite;
mod postgres;

/// The predicate of a search whose text holds no word to search for.
use crate::query::db_sql::MATCH_NOTHING;

/// Builder for full-text search queries
pub struct FullTextSearchBuilder<T: Model> {
    columns: Vec<String>,
    query: String,
    config: FullTextConfig,
    with_ranking: bool,
    limit: Option<u64>,
    offset: Option<u64>,
    min_rank: Option<f64>,
    highlight: Option<HighlightConfig>,
    trashed: TrashedRows,
    _marker: PhantomData<T>,
}

/// Which soft-deleted rows a search reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrashedRows {
    Excluded,
    Included,
    Only,
}

/// How [`FullTextSearchBuilder::highlight`] marks the matches in each result.
#[derive(Debug, Clone)]
pub struct HighlightConfig {
    /// Start tag for highlighted text
    pub start_tag: String,
    /// End tag for highlighted text
    pub end_tag: String,
    /// Maximum length of highlighted snippet
    pub max_length: Option<usize>,
    /// Number of words around match to include
    pub fragment_words: Option<usize>,
    /// HTML-escape the record's text around the tags (default `true`), so a
    /// stored `<script>` reaches a page as text. Turn it off for tags that are
    /// not HTML.
    pub escape_html: bool,
}

impl Default for HighlightConfig {
    fn default() -> Self {
        Self {
            start_tag: "<mark>".to_string(),
            end_tag: "</mark>".to_string(),
            max_length: None,
            fragment_words: Some(10),
            escape_html: true,
        }
    }
}

impl HighlightConfig {
    /// `text` with each match of `pattern` marked, and the number marked: the
    /// words around the first match when `fragment_words` is set, cut to
    /// `max_length` characters, else the whole text.
    pub(crate) fn mark(&self, text: &str, pattern: Option<&regex::Regex>) -> (String, usize) {
        let plain = match self.fragment_words {
            Some(words) => snippet_around_first_match(text, pattern, words),
            None => text.to_string(),
        };
        let plain = match self.max_length {
            Some(max) if plain.chars().count() > max => {
                format!("{}...", plain.chars().take(max).collect::<String>())
            }
            _ => plain,
        };
        match pattern {
            Some(pattern) => mark_matches(
                &plain,
                pattern,
                &self.start_tag,
                &self.end_tag,
                self.escape_html,
            ),
            None if self.escape_html => (escape_html(&plain), 0),
            None => (plain, 0),
        }
    }
}

impl<T: Model> FullTextSearchBuilder<T> {
    /// Create a new search builder
    pub fn new(columns: &[&str], query: &str) -> Self {
        Self {
            columns: columns.iter().map(|s| s.to_string()).collect(),
            query: query.to_string(),
            config: FullTextConfig::default(),
            with_ranking: false,
            limit: None,
            offset: None,
            min_rank: None,
            highlight: None,
            trashed: TrashedRows::Excluded,
            _marker: PhantomData,
        }
    }

    /// Search soft-deleted rows too. A search on a soft-delete model leaves
    /// them out by default, as [`Model::query`](crate::model::Model::query)
    /// does.
    pub fn with_trashed(mut self) -> Self {
        self.trashed = TrashedRows::Included;
        self
    }

    /// Search only soft-deleted rows.
    pub fn only_trashed(mut self) -> Self {
        self.trashed = TrashedRows::Only;
        self
    }

    /// Set the search configuration
    pub fn config(mut self, config: FullTextConfig) -> Self {
        self.config = config;
        self
    }

    /// Order [`get`](Self::get) and [`first`](Self::first) results by relevance.
    ///
    /// Only PostgreSQL applies this; MySQL/MariaDB and SQLite ignore it.
    /// [`get_ranked`](Self::get_ranked) ranks on every backend.
    pub fn with_ranking(mut self) -> Self {
        self.with_ranking = true;
        self
    }

    /// Set maximum number of results
    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Set result offset
    pub fn offset(mut self, offset: u64) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Drop results scoring below `rank`; only
    /// [`get_ranked`](Self::get_ranked) applies it.
    pub fn min_rank(mut self, rank: f64) -> Self {
        self.min_rank = Some(rank);
        self
    }

    /// Set search mode
    pub fn mode(mut self, mode: SearchMode) -> Self {
        self.config.mode = mode;
        self
    }

    /// Set language for text analysis
    pub fn language(mut self, lang: impl Into<String>) -> Self {
        self.config.language = Some(lang.into());
        self
    }

    /// Execute the search and return results
    pub async fn get(self) -> Result<Vec<T>> {
        let db = crate::database::__current_db()?;
        let (sql, params) = self.build_sql(db.execution_backend())?;
        db.__raw_with_params::<T>(&sql, params).await
    }

    /// Execute the search and return ranked results
    pub async fn get_ranked(self) -> Result<Vec<SearchResult<T>>> {
        let db = crate::database::__current_db()?;
        let (sql, params) = self.build_ranked_sql(db.execution_backend())?;

        let pattern = term_pattern(&self.highlight_terms());
        db.fetch_rows(&sql, params)
            .await?
            .iter()
            .map(|row| {
                let model = <T::Entity as EntityTrait>::Model::from_query_result(row, "")
                    .map_err(translate_error)?;
                let rank = row.try_get("", "_fts_rank").map_err(translate_error)?;
                let mut result = SearchResult::new(T::try_from_entity_model(model)?, rank);
                if let Some(config) = &self.highlight {
                    result.highlights =
                        self.highlights_for(&result.record, config, pattern.as_ref());
                }
                Ok(result)
            })
            .collect()
    }

    /// Execute the search and return the first result
    pub async fn first(mut self) -> Result<Option<T>> {
        self.limit = Some(1);
        let results = self.get().await?;
        Ok(results.into_iter().next())
    }

    /// Count matching results
    pub async fn count(self) -> Result<u64> {
        let db = crate::database::__current_db()?;
        let (sql, params) = self.build_count_sql(db.execution_backend())?;

        let rows = db.fetch_rows(&sql, params).await?;
        let row = rows
            .first()
            .ok_or_else(|| Error::query("Database returned no row for the full-text count"))?;
        let count: i64 = row.try_get("", "count").map_err(translate_error)?;
        crate::internal::count_to_u64(count, "fulltext count")
    }

    /// Build the SQL query for the current database type
    pub(crate) fn build_sql(&self, db_type: DatabaseType) -> Result<(String, Vec<Value>)> {
        match db_type {
            DatabaseType::Postgres => self.build_postgres_sql(),
            DatabaseType::MySQL | DatabaseType::MariaDB => self.build_mysql_sql(),
            DatabaseType::SQLite => self.build_sqlite_sql(),
        }
    }

    /// Mark the matches in the searched columns of each
    /// [`get_ranked`](Self::get_ranked) result, filling
    /// [`SearchResult::highlights`].
    pub fn highlight(mut self, config: HighlightConfig) -> Self {
        self.highlight = Some(config);
        self
    }

    /// The search text with the configured term filters applied. A quoted
    /// phrase, and a search in [`SearchMode::Phrase`], is searched as written.
    pub(crate) fn query_text(&self) -> String {
        if self.config.mode == SearchMode::Phrase || !self.config.filters_terms() {
            return self.query.clone();
        }
        search_tokens(&self.query)
            .into_iter()
            .filter(|token| token.contains('"') || self.config.keeps_term(token))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Whether the search text holds a word to search for.
    fn has_search_terms(&self) -> bool {
        has_searchable_char(&self.query_text())
    }

    /// The words to mark in a result: the search terms without their
    /// operators, and without the ones a boolean `-` excludes.
    fn highlight_terms(&self) -> String {
        let boolean = self.config.mode == SearchMode::Boolean;
        search_tokens(&self.query_text())
            .into_iter()
            .filter(|token| !(boolean && token.starts_with('-')))
            .flat_map(search_words)
            .map(|word| word.trim_matches(|character: char| !character.is_alphanumeric()))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The searched columns of `record` that hold text, with the matches of
    /// `pattern` marked as `config` says.
    fn highlights_for(
        &self,
        record: &T,
        config: &HighlightConfig,
        pattern: Option<&regex::Regex>,
    ) -> Vec<HighlightedField> {
        self.columns
            .iter()
            .filter_map(|column| {
                let value = record.field_json_value(column).ok()??;
                let original = value.as_str()?.to_string();
                let (highlighted, match_count) = config.mark(&original, pattern);
                Some(HighlightedField {
                    field: column.clone(),
                    match_count,
                    highlighted,
                    original,
                })
            })
            .collect()
    }

    /// Build ranked SQL query
    pub(crate) fn build_ranked_sql(&self, db_type: DatabaseType) -> Result<(String, Vec<Value>)> {
        match db_type {
            DatabaseType::Postgres => self.build_postgres_ranked_sql(),
            DatabaseType::MySQL | DatabaseType::MariaDB => self.build_mysql_ranked_sql(),
            DatabaseType::SQLite => self.build_sqlite_ranked_sql(),
        }
    }

    /// Build count SQL query
    pub(crate) fn build_count_sql(&self, db_type: DatabaseType) -> Result<(String, Vec<Value>)> {
        match db_type {
            DatabaseType::Postgres => self.build_postgres_count_sql(),
            DatabaseType::MySQL | DatabaseType::MariaDB => self.build_mysql_count_sql(),
            DatabaseType::SQLite => self.build_sqlite_count_sql(),
        }
    }

    /// `predicate` restricted to the rows the soft-delete scope keeps, with
    /// the deleted-at column qualified by `table` when one is given.
    fn scoped(&self, db_type: DatabaseType, predicate: String, table: Option<&str>) -> String {
        if self.trashed == TrashedRows::Included {
            return predicate;
        }
        let only_trashed = self.trashed == TrashedRows::Only;
        match crate::relations::soft_delete_clause::<T>(db_type, table, only_trashed) {
            Some(test) => format!("{predicate} AND {test}"),
            None => predicate,
        }
    }

    /// The searched columns as the database names them, so a Rust field name
    /// searches its column.
    fn column_names(&self) -> Vec<String> {
        self.columns
            .iter()
            .map(|column| {
                T::canonical_column_name(column)
                    .map(str::to_string)
                    .unwrap_or_else(|| column.clone())
            })
            .collect()
    }

    fn append_limit_offset(
        &self,
        db_type: DatabaseType,
        sql: &mut String,
        params: &mut Vec<Value>,
    ) {
        crate::query::db_sql::append_limit_offset(sql, db_type, self.limit, self.offset, params);
    }

    /// `SELECT <model columns> FROM <table> WHERE <predicate>`, paged.
    fn select_where(
        &self,
        db_type: DatabaseType,
        predicate: &str,
        mut params: Vec<Value>,
    ) -> (String, Vec<Value>) {
        let mut sql = format!(
            "SELECT {} FROM {} WHERE {}",
            crate::query::db_sql::model_columns_sql::<T>(db_type, None),
            crate::query::db_sql::quote_table::<T>(db_type),
            predicate
        );
        self.append_limit_offset(db_type, &mut sql, &mut params);
        (sql, params)
    }

    /// `SELECT COUNT(*) as count FROM <table> WHERE <predicate>`.
    fn count_where(
        db_type: DatabaseType,
        predicate: &str,
        params: Vec<Value>,
    ) -> (String, Vec<Value>) {
        let sql = format!(
            "SELECT COUNT(*) as count FROM {} WHERE {}",
            crate::query::db_sql::quote_table::<T>(db_type),
            predicate
        );
        (sql, params)
    }
}

/// `text` split on whitespace outside double quotes, so a quoted phrase and
/// the operator written before it stay one token.
fn search_tokens(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut in_quotes = false;
    for (index, character) in text.char_indices() {
        if character == '"' {
            in_quotes = !in_quotes;
        }
        if character.is_whitespace() && !in_quotes {
            if let Some(begin) = start.take() {
                tokens.push(&text[begin..index]);
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(begin) = start {
        tokens.push(&text[begin..]);
    }
    tokens
}
