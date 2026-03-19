//! Full-Text Search Support for TideORM
//!
//! This module provides full-text search capabilities across PostgreSQL, MySQL, MariaDB, and SQLite.
//!
//! ## Features
//!
//! - **PostgreSQL**: Native `tsvector`/`tsquery` support with GIN/GiST indexes
//! - **MySQL/MariaDB**: FULLTEXT index support with natural language and boolean modes
//! - **SQLite**: FTS5 virtual table support
//! - **Search Ranking**: Result relevance scoring
//! - **Highlighting**: Mark matching terms in search results
//!
//! ## Example
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//!
//! #[derive(Model, Clone, Debug)]
//! #[tideorm(table = "articles")]
//! pub struct Article {
//!     #[tideorm(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub title: String,
//!     pub content: String,
//! }
//!
//! // Simple search
//! let results = Article::search(&["title", "content"], "rust programming")
//!     .await?;
//!
//! // Search with ranking
//! let results = Article::search_ranked(&["title", "content"], "rust programming")
//!     .limit(10)
//!     .get()
//!     .await?;
//!
//! // Search with highlighting
//! let results = Article::search_with_highlights(&["content"], "rust", "<b>", "</b>")
//!     .await?;
//! ```

use std::fmt;
use std::marker::PhantomData;

use crate::config::DatabaseType;
use crate::error::{Error, Result};
use crate::internal::{ConnectionTrait, FromQueryResult, Statement, Value};
use crate::model::Model;

// =============================================================================
// FULL-TEXT SEARCH CONFIGURATION
// =============================================================================

/// Full-text search configuration for different databases
#[derive(Debug, Clone, Default)]
pub struct FullTextConfig {
    /// Language for stemming/parsing (e.g., "english", "simple")
    pub language: Option<String>,
    /// Search mode
    pub mode: SearchMode,
    /// Minimum word length to index
    pub min_word_length: Option<u32>,
    /// Maximum word length to index
    pub max_word_length: Option<u32>,
    /// Custom stop words to exclude
    pub stop_words: Vec<String>,
    /// Weight configuration for ranked searches
    pub weights: Option<SearchWeights>,
}

impl FullTextConfig {
    /// Create a new full-text search configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the language for text analysis
    pub fn language(mut self, lang: impl Into<String>) -> Self {
        self.language = Some(lang.into());
        self
    }

    /// Set the search mode
    pub fn mode(mut self, mode: SearchMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set minimum word length
    pub fn min_word_length(mut self, len: u32) -> Self {
        self.min_word_length = Some(len);
        self
    }

    /// Set maximum word length
    pub fn max_word_length(mut self, len: u32) -> Self {
        self.max_word_length = Some(len);
        self
    }

    /// Add stop words to exclude from indexing
    pub fn stop_words(mut self, words: Vec<String>) -> Self {
        self.stop_words = words;
        self
    }

    /// Set search weights for ranking
    pub fn weights(mut self, weights: SearchWeights) -> Self {
        self.weights = Some(weights);
        self
    }
}

/// Search mode for full-text queries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    /// Natural language search (default)
    /// Finds rows that match the search terms naturally
    #[default]
    Natural,
    /// Boolean search mode
    /// Supports operators like +, -, *, "phrase"
    Boolean,
    /// Phrase search mode
    /// Matches exact phrases
    Phrase,
    /// Prefix search mode
    /// Matches words that start with the given prefix
    Prefix,
    /// Fuzzy search mode (PostgreSQL only)
    /// Matches similar words using trigrams
    Fuzzy,
    /// Proximity search
    /// Finds terms within a certain distance of each other
    Proximity(u32),
}

impl fmt::Display for SearchMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchMode::Natural => write!(f, "natural"),
            SearchMode::Boolean => write!(f, "boolean"),
            SearchMode::Phrase => write!(f, "phrase"),
            SearchMode::Prefix => write!(f, "prefix"),
            SearchMode::Fuzzy => write!(f, "fuzzy"),
            SearchMode::Proximity(d) => write!(f, "proximity({})", d),
        }
    }
}

/// Weight configuration for PostgreSQL tsvector ranking
#[derive(Debug, Clone)]
pub struct SearchWeights {
    /// Weight for 'A' category (highest priority, e.g., title)
    pub a: f32,
    /// Weight for 'B' category
    pub b: f32,
    /// Weight for 'C' category  
    pub c: f32,
    /// Weight for 'D' category (lowest priority, e.g., body)
    pub d: f32,
}

impl Default for SearchWeights {
    fn default() -> Self {
        Self {
            a: 1.0,
            b: 0.4,
            c: 0.2,
            d: 0.1,
        }
    }
}

impl SearchWeights {
    /// Create new weights
    pub fn new(a: f32, b: f32, c: f32, d: f32) -> Self {
        Self { a, b, c, d }
    }

    /// Convert to PostgreSQL weights array format
    pub fn to_pg_array(&self) -> String {
        format!("'{{{},{},{},{}}}'", self.d, self.c, self.b, self.a)
    }
}

// =============================================================================
// SEARCH RESULT TYPES
// =============================================================================

/// A search result with ranking information
#[derive(Debug, Clone)]
pub struct SearchResult<T> {
    /// The matched record
    pub record: T,
    /// Relevance score (higher = more relevant)
    pub rank: f64,
    /// Highlighted snippets (if requested)
    pub highlights: Vec<HighlightedField>,
}

impl<T> SearchResult<T> {
    /// Create a new search result
    pub fn new(record: T, rank: f64) -> Self {
        Self {
            record,
            rank,
            highlights: Vec::new(),
        }
    }

    /// Add highlighted fields
    pub fn with_highlights(mut self, highlights: Vec<HighlightedField>) -> Self {
        self.highlights = highlights;
        self
    }
}

/// A field with highlighted search matches
#[derive(Debug, Clone)]
pub struct HighlightedField {
    /// Field name
    pub field: String,
    /// Field value with highlighted matches
    pub highlighted: String,
    /// Original value
    pub original: String,
    /// Number of matches found
    pub match_count: usize,
}

impl HighlightedField {
    /// Create a new highlighted field
    pub fn new(
        field: impl Into<String>,
        highlighted: impl Into<String>,
        original: impl Into<String>,
    ) -> Self {
        let highlighted = highlighted.into();
        let original = original.into();
        // Count matches by looking for start tags
        let match_count = highlighted.matches("<mark>").count();
        Self {
            field: field.into(),
            highlighted,
            original,
            match_count,
        }
    }
}

// =============================================================================
// FULL-TEXT SEARCH TRAIT
// =============================================================================

/// Trait for models that support full-text search
pub trait FullTextSearch: Model + Sized {
    /// Perform a simple full-text search on specified columns
    fn search(columns: &[&str], query: &str) -> FullTextSearchBuilder<Self> {
        FullTextSearchBuilder::new(columns, query)
    }

    /// Perform a full-text search with custom configuration
    fn search_with_config(
        columns: &[&str],
        query: &str,
        config: FullTextConfig,
    ) -> FullTextSearchBuilder<Self> {
        FullTextSearchBuilder::new(columns, query).config(config)
    }

    /// Search with ranking (returns results ordered by relevance)
    fn search_ranked(columns: &[&str], query: &str) -> FullTextSearchBuilder<Self> {
        FullTextSearchBuilder::new(columns, query).with_ranking()
    }

    /// Search with highlighting
    fn search_highlighted(
        columns: &[&str],
        query: &str,
        start_tag: &str,
        end_tag: &str,
    ) -> FullTextSearchBuilder<Self> {
        FullTextSearchBuilder::new(columns, query).with_highlights(start_tag, end_tag)
    }
}

// Implement FullTextSearch for all Models
impl<T: Model> FullTextSearch for T {}

// =============================================================================
// FULL-TEXT SEARCH BUILDER
// =============================================================================

/// Builder for full-text search queries
pub struct FullTextSearchBuilder<T: Model> {
    columns: Vec<String>,
    query: String,
    config: FullTextConfig,
    with_ranking: bool,
    highlight_config: Option<HighlightConfig>,
    limit: Option<u64>,
    offset: Option<u64>,
    min_rank: Option<f64>,
    _marker: PhantomData<T>,
}

/// Configuration for search result highlighting
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
}

impl Default for HighlightConfig {
    fn default() -> Self {
        Self {
            start_tag: "<mark>".to_string(),
            end_tag: "</mark>".to_string(),
            max_length: None,
            fragment_words: Some(10),
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
            highlight_config: None,
            limit: None,
            offset: None,
            min_rank: None,
            _marker: PhantomData,
        }
    }

    /// Set the search configuration
    pub fn config(mut self, config: FullTextConfig) -> Self {
        self.config = config;
        self
    }

    /// Enable ranking for search results
    pub fn with_ranking(mut self) -> Self {
        self.with_ranking = true;
        self
    }

    /// Enable highlighting with default tags
    pub fn with_highlights(mut self, start_tag: &str, end_tag: &str) -> Self {
        self.highlight_config = Some(HighlightConfig {
            start_tag: start_tag.to_string(),
            end_tag: end_tag.to_string(),
            ..Default::default()
        });
        self
    }

    /// Set custom highlight configuration
    pub fn highlight_config(mut self, config: HighlightConfig) -> Self {
        self.highlight_config = Some(config);
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

    /// Filter results by minimum rank score
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
    pub async fn get(self) -> Result<Vec<T>>
    where
        T: FromQueryResult,
    {
        use crate::database::Connection;

        let db = crate::database::__current_db()?;
        let db_type = db.backend();
        let (sql, params) = self.build_sql(db_type)?;

        let backend = db.__internal_backend()?;
        let statement = Statement::from_sql_and_values(backend, &sql, params);

        let results = match db.__get_connection()? {
            crate::database::ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(conn.query_all_raw(statement)).await
            }
            crate::database::ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(tx.as_ref().query_all_raw(statement)).await
            }
        }
        .map_err(|e| Error::query(e.to_string()))?;

        let mut records = Vec::new();
        for row in results {
            if let Ok(record) = T::from_query_result(&row, "") {
                records.push(record);
            }
        }

        Ok(records)
    }

    /// Execute the search and return ranked results
    pub async fn get_ranked(self) -> Result<Vec<SearchResult<T>>>
    where
        T: FromQueryResult,
    {
        use crate::database::Connection;

        let db = crate::database::__current_db()?;
        let db_type = db.backend();
        let (sql, params) = self.build_ranked_sql(db_type)?;

        let backend = db.__internal_backend()?;
        let statement = Statement::from_sql_and_values(backend, &sql, params);

        let results = match db.__get_connection()? {
            crate::database::ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(conn.query_all_raw(statement)).await
            }
            crate::database::ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(tx.as_ref().query_all_raw(statement)).await
            }
        }
        .map_err(|e| Error::query(e.to_string()))?;

        let mut records = Vec::new();
        for row in results {
            if let Ok(record) = T::from_query_result(&row, "") {
                // Try to get rank from the row
                let rank = row.try_get::<f64>("", "_fts_rank").unwrap_or(0.0);
                records.push(SearchResult::new(record, rank));
            }
        }

        Ok(records)
    }

    /// Execute the search and return the first result
    pub async fn first(mut self) -> Result<Option<T>>
    where
        T: FromQueryResult,
    {
        self.limit = Some(1);
        let results = self.get().await?;
        Ok(results.into_iter().next())
    }

    /// Count matching results
    pub async fn count(self) -> Result<u64> {
        use crate::database::Connection;

        let db = crate::database::__current_db()?;
        let db_type = db.backend();
        let (sql, params) = self.build_count_sql(db_type)?;

        let backend = db.__internal_backend()?;
        let statement = Statement::from_sql_and_values(backend, &sql, params);

        let result = match db.__get_connection()? {
            crate::database::ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(conn.query_one_raw(statement)).await
            }
            crate::database::ConnectionRef::Transaction(tx) => {
                crate::profiling::__profile_future(tx.as_ref().query_one_raw(statement)).await
            }
        }
        .map_err(|e| Error::query(e.to_string()))?;

        if let Some(row) = result {
            let count: i64 = row.try_get("", "count").unwrap_or(0);
            crate::internal::count_to_u64(count, "fulltext count")
        } else {
            Ok(0)
        }
    }

    /// Build the SQL query for the current database type
    pub(crate) fn build_sql(&self, db_type: DatabaseType) -> Result<(String, Vec<Value>)> {
        match db_type {
            DatabaseType::Postgres => self.build_postgres_sql(),
            DatabaseType::MySQL | DatabaseType::MariaDB => self.build_mysql_sql(),
            DatabaseType::SQLite => self.build_sqlite_sql(),
        }
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

    // =========================================================================
    // POSTGRESQL IMPLEMENTATION
    // =========================================================================

    fn build_postgres_sql(&self) -> Result<(String, Vec<Value>)> {
        let table = quote_ident(DatabaseType::Postgres, T::table_name());
        let mut params = Vec::new();
        let language_placeholder = self.push_param(
            DatabaseType::Postgres,
            &mut params,
            Value::String(Some(
                self.config
                    .language
                    .clone()
                    .unwrap_or_else(|| "english".to_string()),
            )),
        );

        // Build tsvector expression for columns
        let tsvector_expr = self.build_pg_tsvector_expr(&language_placeholder);

        // Build tsquery based on search mode
        let tsquery_expr = self.build_pg_tsquery_expr(&language_placeholder, &mut params);

        let mut sql = format!(
            "SELECT * FROM {} WHERE {} @@ {}",
            table, tsvector_expr, tsquery_expr
        );

        if self.with_ranking {
            let weights_placeholder = self.pg_weights_placeholder(&mut params);
            sql = format!(
                "SELECT *, ts_rank_cd(CAST({} AS real[]), {}, {}) AS _fts_rank FROM {} WHERE {} @@ {} ORDER BY _fts_rank DESC",
                weights_placeholder,
                tsvector_expr,
                tsquery_expr,
                table,
                tsvector_expr,
                tsquery_expr
            );
        }

        self.append_limit_offset(DatabaseType::Postgres, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    fn build_postgres_ranked_sql(&self) -> Result<(String, Vec<Value>)> {
        let table = quote_ident(DatabaseType::Postgres, T::table_name());
        let mut params = Vec::new();
        let language_placeholder = self.push_param(
            DatabaseType::Postgres,
            &mut params,
            Value::String(Some(
                self.config
                    .language
                    .clone()
                    .unwrap_or_else(|| "english".to_string()),
            )),
        );

        let tsvector_expr = self.build_pg_tsvector_expr(&language_placeholder);
        let tsquery_expr = self.build_pg_tsquery_expr(&language_placeholder, &mut params);

        let weights_placeholder = self.pg_weights_placeholder(&mut params);

        let mut sql = format!(
            "SELECT *, ts_rank_cd(CAST({} AS real[]), {}, {}) AS _fts_rank FROM {} WHERE {} @@ {}",
            weights_placeholder, tsvector_expr, tsquery_expr, table, tsvector_expr, tsquery_expr
        );

        if let Some(min_rank) = self.min_rank {
            let min_rank_placeholder = self.push_param(
                DatabaseType::Postgres,
                &mut params,
                Value::Double(Some(min_rank)),
            );
            sql.push_str(&format!(
                " AND ts_rank_cd(CAST({} AS real[]), {}, {}) >= {}",
                weights_placeholder, tsvector_expr, tsquery_expr, min_rank_placeholder
            ));
        }

        sql.push_str(" ORDER BY _fts_rank DESC");

        self.append_limit_offset(DatabaseType::Postgres, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    fn build_postgres_count_sql(&self) -> Result<(String, Vec<Value>)> {
        let table = quote_ident(DatabaseType::Postgres, T::table_name());
        let mut params = Vec::new();
        let language_placeholder = self.push_param(
            DatabaseType::Postgres,
            &mut params,
            Value::String(Some(
                self.config
                    .language
                    .clone()
                    .unwrap_or_else(|| "english".to_string()),
            )),
        );

        let tsvector_expr = self.build_pg_tsvector_expr(&language_placeholder);
        let tsquery_expr = self.build_pg_tsquery_expr(&language_placeholder, &mut params);

        Ok((
            format!(
                "SELECT COUNT(*) as count FROM {} WHERE {} @@ {}",
                table, tsvector_expr, tsquery_expr
            ),
            params,
        ))
    }

    fn build_pg_tsvector_expr(&self, language_placeholder: &str) -> String {
        if self.columns.len() == 1 {
            format!(
                "to_tsvector(CAST({} AS regconfig), COALESCE({}, ''))",
                language_placeholder,
                quote_ident(DatabaseType::Postgres, &self.columns[0])
            )
        } else {
            let cols: Vec<String> = self
                .columns
                .iter()
                .map(|c| format!("COALESCE({}, '')", quote_ident(DatabaseType::Postgres, c)))
                .collect();
            format!(
                "to_tsvector(CAST({} AS regconfig), {})",
                language_placeholder,
                cols.join(" || ' ' || ")
            )
        }
    }

    fn build_pg_tsquery_expr(&self, language_placeholder: &str, params: &mut Vec<Value>) -> String {
        match self.config.mode {
            SearchMode::Natural => {
                let placeholder = self.push_param(
                    DatabaseType::Postgres,
                    params,
                    Value::String(Some(self.query.clone())),
                );
                format!(
                    "plainto_tsquery(CAST({} AS regconfig), {})",
                    language_placeholder, placeholder
                )
            }
            SearchMode::Boolean => {
                let placeholder = self.push_param(
                    DatabaseType::Postgres,
                    params,
                    Value::String(Some(self.query.clone())),
                );
                format!(
                    "to_tsquery(CAST({} AS regconfig), {})",
                    language_placeholder, placeholder
                )
            }
            SearchMode::Phrase => {
                let placeholder = self.push_param(
                    DatabaseType::Postgres,
                    params,
                    Value::String(Some(self.query.clone())),
                );
                format!(
                    "phraseto_tsquery(CAST({} AS regconfig), {})",
                    language_placeholder, placeholder
                )
            }
            SearchMode::Prefix => {
                let words: Vec<&str> = self.query.split_whitespace().collect();
                let prefixed: Vec<String> = words.iter().map(|w| format!("{}:*", w)).collect();
                let placeholder = self.push_param(
                    DatabaseType::Postgres,
                    params,
                    Value::String(Some(prefixed.join(" & "))),
                );
                format!(
                    "to_tsquery(CAST({} AS regconfig), {})",
                    language_placeholder, placeholder
                )
            }
            SearchMode::Fuzzy => {
                let placeholder = self.push_param(
                    DatabaseType::Postgres,
                    params,
                    Value::String(Some(self.query.clone())),
                );
                format!(
                    "plainto_tsquery(CAST({} AS regconfig), {})",
                    language_placeholder, placeholder
                )
            }
            SearchMode::Proximity(distance) => {
                let words: Vec<&str> = self.query.split_whitespace().collect();
                let proximity: Vec<String> = words.iter().map(|w| w.to_string()).collect();
                let placeholder = self.push_param(
                    DatabaseType::Postgres,
                    params,
                    Value::String(Some(proximity.join(&format!(" <{}> ", distance)))),
                );
                format!(
                    "to_tsquery(CAST({} AS regconfig), {})",
                    language_placeholder, placeholder
                )
            }
        }
    }

    // =========================================================================
    // MYSQL IMPLEMENTATION
    // =========================================================================

    fn build_mysql_sql(&self) -> Result<(String, Vec<Value>)> {
        let table = quote_ident(DatabaseType::MySQL, T::table_name());
        let mut params = Vec::new();

        let columns_str = self
            .columns
            .iter()
            .map(|c| quote_ident(DatabaseType::MySQL, c))
            .collect::<Vec<_>>()
            .join(", ");

        let mode_modifier = match self.config.mode {
            SearchMode::Natural => "",
            SearchMode::Boolean => " IN BOOLEAN MODE",
            SearchMode::Phrase => " WITH QUERY EXPANSION",
            _ => "",
        };

        let query_placeholder = self.push_param(
            DatabaseType::MySQL,
            &mut params,
            Value::String(Some(self.query.clone())),
        );
        let mut sql = format!(
            "SELECT * FROM {} WHERE MATCH({}) AGAINST({}{}) ",
            table, columns_str, query_placeholder, mode_modifier
        );

        self.append_limit_offset(DatabaseType::MySQL, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    fn build_mysql_ranked_sql(&self) -> Result<(String, Vec<Value>)> {
        let table = quote_ident(DatabaseType::MySQL, T::table_name());
        let mut params = Vec::new();

        let columns_str = self
            .columns
            .iter()
            .map(|c| quote_ident(DatabaseType::MySQL, c))
            .collect::<Vec<_>>()
            .join(", ");

        let mode_modifier = match self.config.mode {
            SearchMode::Natural => "",
            SearchMode::Boolean => " IN BOOLEAN MODE",
            SearchMode::Phrase => " WITH QUERY EXPANSION",
            _ => "",
        };

        let rank_placeholder = self.push_param(
            DatabaseType::MySQL,
            &mut params,
            Value::String(Some(self.query.clone())),
        );
        let where_placeholder = self.push_param(
            DatabaseType::MySQL,
            &mut params,
            Value::String(Some(self.query.clone())),
        );
        let mut sql = format!(
            "SELECT *, MATCH({}) AGAINST({}{}) AS _fts_rank FROM {} \
             WHERE MATCH({}) AGAINST({}{}) ",
            columns_str,
            rank_placeholder,
            mode_modifier,
            table,
            columns_str,
            where_placeholder,
            mode_modifier
        );

        if let Some(min_rank) = self.min_rank {
            let min_rank_placeholder = self.push_param(
                DatabaseType::MySQL,
                &mut params,
                Value::Double(Some(min_rank)),
            );
            let against_placeholder = self.push_param(
                DatabaseType::MySQL,
                &mut params,
                Value::String(Some(self.query.clone())),
            );
            sql.push_str(&format!(
                "AND MATCH({}) AGAINST({}{}) >= {} ",
                columns_str, against_placeholder, mode_modifier, min_rank_placeholder
            ));
        }

        sql.push_str("ORDER BY _fts_rank DESC ");

        self.append_limit_offset(DatabaseType::MySQL, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    fn build_mysql_count_sql(&self) -> Result<(String, Vec<Value>)> {
        let table = quote_ident(DatabaseType::MySQL, T::table_name());
        let mut params = Vec::new();

        let columns_str = self
            .columns
            .iter()
            .map(|c| quote_ident(DatabaseType::MySQL, c))
            .collect::<Vec<_>>()
            .join(", ");

        let mode_modifier = match self.config.mode {
            SearchMode::Natural => "",
            SearchMode::Boolean => " IN BOOLEAN MODE",
            _ => "",
        };

        let query_placeholder = self.push_param(
            DatabaseType::MySQL,
            &mut params,
            Value::String(Some(self.query.clone())),
        );
        Ok((
            format!(
                "SELECT COUNT(*) as count FROM {} WHERE MATCH({}) AGAINST({}{})",
                table, columns_str, query_placeholder, mode_modifier
            ),
            params,
        ))
    }

    // =========================================================================
    // SQLITE IMPLEMENTATION (FTS5)
    // =========================================================================

    fn build_sqlite_sql(&self) -> Result<(String, Vec<Value>)> {
        let table_name = T::table_name();
        let table = quote_ident(DatabaseType::SQLite, table_name);
        let fts_table_name = format!("{}_fts", table_name);
        let fts_table = quote_ident(DatabaseType::SQLite, &fts_table_name);
        let mut params = Vec::new();
        let query_placeholder = self.push_param(
            DatabaseType::SQLite,
            &mut params,
            Value::String(Some(escape_fts5_query(&self.query))),
        );

        // SQLite FTS5 requires a separate virtual table
        // This assumes the FTS5 table exists with the same columns
        let mut sql = format!(
            "SELECT t.* FROM {} t \
             INNER JOIN {} fts ON t.rowid = fts.rowid \
             WHERE {} MATCH {} ",
            table, fts_table, fts_table, query_placeholder
        );

        self.append_limit_offset(DatabaseType::SQLite, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    fn build_sqlite_ranked_sql(&self) -> Result<(String, Vec<Value>)> {
        let table_name = T::table_name();
        let table = quote_ident(DatabaseType::SQLite, table_name);
        let fts_table_name = format!("{}_fts", table_name);
        let fts_table = quote_ident(DatabaseType::SQLite, &fts_table_name);
        let mut params = Vec::new();
        let query_placeholder = self.push_param(
            DatabaseType::SQLite,
            &mut params,
            Value::String(Some(escape_fts5_query(&self.query))),
        );

        let mut sql = format!(
            "SELECT t.*, bm25({}) AS _fts_rank FROM {} t \
             INNER JOIN {} fts ON t.rowid = fts.rowid \
             WHERE {} MATCH {} ",
            fts_table, table, fts_table, fts_table, query_placeholder
        );

        if let Some(min_rank) = self.min_rank {
            // Note: BM25 returns negative values, lower is better
            let min_rank_placeholder = self.push_param(
                DatabaseType::SQLite,
                &mut params,
                Value::Double(Some(-min_rank)),
            );
            sql.push_str(&format!(
                "AND bm25({}) <= {} ",
                fts_table, min_rank_placeholder
            ));
        }

        // BM25 returns negative values, so ORDER BY ASC for best matches
        sql.push_str(&format!("ORDER BY bm25({}) ", fts_table));

        self.append_limit_offset(DatabaseType::SQLite, &mut sql, &mut params)?;

        Ok((sql, params))
    }

    fn build_sqlite_count_sql(&self) -> Result<(String, Vec<Value>)> {
        let table_name = T::table_name();
        let table = quote_ident(DatabaseType::SQLite, table_name);
        let fts_table_name = format!("{}_fts", table_name);
        let fts_table = quote_ident(DatabaseType::SQLite, &fts_table_name);
        let mut params = Vec::new();
        let query_placeholder = self.push_param(
            DatabaseType::SQLite,
            &mut params,
            Value::String(Some(escape_fts5_query(&self.query))),
        );

        Ok((
            format!(
                "SELECT COUNT(*) as count FROM {} t \
                 INNER JOIN {} fts ON t.rowid = fts.rowid \
                 WHERE {} MATCH {}",
                table, fts_table, fts_table, query_placeholder
            ),
            params,
        ))
    }

    fn push_param(&self, db_type: DatabaseType, params: &mut Vec<Value>, value: Value) -> String {
        let placeholder = match db_type {
            DatabaseType::Postgres => format!("${}", params.len() + 1),
            DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => "?".to_string(),
        };
        params.push(value);
        placeholder
    }

    fn pg_weights_placeholder(&self, params: &mut Vec<Value>) -> String {
        let weights = self
            .config
            .weights
            .as_ref()
            .map(|w| w.to_pg_array().trim_matches('\'').to_string())
            .unwrap_or_else(|| "{0.1,0.2,0.4,1.0}".to_string());

        self.push_param(DatabaseType::Postgres, params, Value::String(Some(weights)))
    }

    fn append_limit_offset(
        &self,
        db_type: DatabaseType,
        sql: &mut String,
        params: &mut Vec<Value>,
    ) -> Result<()> {
        if let Some(limit) = self.limit {
            let limit_value = i64::try_from(limit)
                .map_err(|_| Error::query("Full-text search limit exceeds i64 range"))?;
            let placeholder = self.push_param(db_type, params, Value::BigInt(Some(limit_value)));
            sql.push_str(&format!(" LIMIT {}", placeholder));
        }
        if let Some(offset) = self.offset {
            let offset_value = i64::try_from(offset)
                .map_err(|_| Error::query("Full-text search offset exceeds i64 range"))?;
            let placeholder = self.push_param(db_type, params, Value::BigInt(Some(offset_value)));
            sql.push_str(&format!(" OFFSET {}", placeholder));
        }
        Ok(())
    }
}

// =============================================================================
// INDEX GENERATION HELPERS
// =============================================================================

/// Full-text index definition
#[derive(Debug, Clone)]
pub struct FullTextIndex {
    /// Index name
    pub name: String,
    /// Table name
    pub table: String,
    /// Columns to index
    pub columns: Vec<String>,
    /// Index configuration
    pub config: FullTextIndexConfig,
}

/// Configuration for full-text indexes
#[derive(Debug, Clone, Default)]
pub struct FullTextIndexConfig {
    /// Language configuration (PostgreSQL)
    pub language: Option<String>,
    /// Index type: GIN or GiST (PostgreSQL)
    pub pg_index_type: PgFullTextIndexType,
    /// Parser type (MySQL)
    pub mysql_parser: Option<String>,
}

/// PostgreSQL full-text index type
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PgFullTextIndexType {
    /// GIN index - faster lookups, slower updates
    #[default]
    GIN,
    /// GiST index - slower lookups, faster updates, supports ranking
    GiST,
}

impl FullTextIndex {
    /// Create a new full-text index
    pub fn new(name: impl Into<String>, table: impl Into<String>, columns: Vec<String>) -> Self {
        Self {
            name: name.into(),
            table: table.into(),
            columns,
            config: FullTextIndexConfig::default(),
        }
    }

    /// Set the language
    pub fn language(mut self, lang: impl Into<String>) -> Self {
        self.config.language = Some(lang.into());
        self
    }

    /// Set PostgreSQL index type
    pub fn pg_index_type(mut self, index_type: PgFullTextIndexType) -> Self {
        self.config.pg_index_type = index_type;
        self
    }

    /// Generate CREATE INDEX statement for PostgreSQL
    pub fn to_postgres_sql(&self) -> String {
        let language = self.config.language.as_deref().unwrap_or("english");
        let index_type = match self.config.pg_index_type {
            PgFullTextIndexType::GIN => "GIN",
            PgFullTextIndexType::GiST => "GiST",
        };

        let tsvector_expr = if self.columns.len() == 1 {
            format!(
                "to_tsvector('{}', COALESCE({}, ''))",
                language,
                quote_ident(DatabaseType::Postgres, &self.columns[0])
            )
        } else {
            let cols: Vec<String> = self
                .columns
                .iter()
                .map(|c| format!("COALESCE({}, '')", quote_ident(DatabaseType::Postgres, c)))
                .collect();
            format!("to_tsvector('{}', {})", language, cols.join(" || ' ' || "))
        };

        format!(
            "CREATE INDEX {} ON {} USING {} (({}))",
            quote_ident(DatabaseType::Postgres, &self.name),
            quote_ident(DatabaseType::Postgres, &self.table),
            index_type,
            tsvector_expr
        )
    }

    /// Generate CREATE FULLTEXT INDEX statement for MySQL
    pub fn to_mysql_sql(&self) -> String {
        let columns_str = self
            .columns
            .iter()
            .map(|c| quote_ident(DatabaseType::MySQL, c))
            .collect::<Vec<_>>()
            .join(", ");

        let parser = self
            .config
            .mysql_parser
            .as_ref()
            .map(|p| format!(" WITH PARSER {}", p))
            .unwrap_or_default();

        format!(
            "CREATE FULLTEXT INDEX {} ON {}({}){}",
            quote_ident(DatabaseType::MySQL, &self.name),
            quote_ident(DatabaseType::MySQL, &self.table),
            columns_str,
            parser
        )
    }

    /// Generate CREATE VIRTUAL TABLE statement for SQLite FTS5
    pub fn to_sqlite_sql(&self) -> Vec<String> {
        let fts_table = format!("{}_fts", self.table);
        let columns_str = self
            .columns
            .iter()
            .map(|column| quote_ident(DatabaseType::SQLite, column))
            .collect::<Vec<_>>()
            .join(", ");

        vec![
            // Create FTS5 virtual table
            format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS {} USING fts5({}, content={}, content_rowid={})",
                quote_ident(DatabaseType::SQLite, &fts_table),
                columns_str,
                quote_ident(DatabaseType::SQLite, &self.table),
                quote_ident(DatabaseType::SQLite, "rowid")
            ),
            // Create triggers to keep FTS table in sync
            format!(
                "CREATE TRIGGER IF NOT EXISTS {} AFTER INSERT ON {} BEGIN \
                 INSERT INTO \"{}\"(rowid, {}) VALUES (new.rowid, {}); \
                 END",
                quote_ident(DatabaseType::SQLite, &format!("{}_ai", self.table)),
                quote_ident(DatabaseType::SQLite, &self.table),
                quote_ident(DatabaseType::SQLite, &fts_table),
                columns_str,
                self.columns
                    .iter()
                    .map(|c| format!("new.{}", quote_ident(DatabaseType::SQLite, c)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            format!(
                "CREATE TRIGGER IF NOT EXISTS {} AFTER DELETE ON {} BEGIN \
                 INSERT INTO {}({}, rowid, {}) VALUES('delete', old.rowid, {}); \
                 END",
                quote_ident(DatabaseType::SQLite, &format!("{}_ad", self.table)),
                quote_ident(DatabaseType::SQLite, &self.table),
                quote_ident(DatabaseType::SQLite, &fts_table),
                quote_ident(DatabaseType::SQLite, &fts_table),
                columns_str,
                self.columns
                    .iter()
                    .map(|c| format!("old.{}", quote_ident(DatabaseType::SQLite, c)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            format!(
                "CREATE TRIGGER IF NOT EXISTS {} AFTER UPDATE ON {} BEGIN \
                 INSERT INTO {}({}, rowid, {}) VALUES('delete', old.rowid, {}); \
                 INSERT INTO {}(rowid, {}) VALUES (new.rowid, {}); \
                 END",
                quote_ident(DatabaseType::SQLite, &format!("{}_au", self.table)),
                quote_ident(DatabaseType::SQLite, &self.table),
                quote_ident(DatabaseType::SQLite, &fts_table),
                quote_ident(DatabaseType::SQLite, &fts_table),
                columns_str,
                self.columns
                    .iter()
                    .map(|c| format!("old.{}", quote_ident(DatabaseType::SQLite, c)))
                    .collect::<Vec<_>>()
                    .join(", "),
                quote_ident(DatabaseType::SQLite, &fts_table),
                columns_str,
                self.columns
                    .iter()
                    .map(|c| format!("new.{}", quote_ident(DatabaseType::SQLite, c)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ]
    }

    /// Generate CREATE INDEX for the current database type
    pub fn to_sql(&self, db_type: DatabaseType) -> Vec<String> {
        match db_type {
            DatabaseType::Postgres => vec![self.to_postgres_sql()],
            DatabaseType::MySQL | DatabaseType::MariaDB => vec![self.to_mysql_sql()],
            DatabaseType::SQLite => self.to_sqlite_sql(),
        }
    }
}

// =============================================================================
// HIGHLIGHTING UTILITIES
// =============================================================================

/// Highlight search terms in text
pub fn highlight_text(text: &str, query: &str, start_tag: &str, end_tag: &str) -> String {
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut result = text.to_string();

    // Pre-compile all regex patterns outside the loop to avoid regex_creation_in_loops
    let patterns: Vec<regex::Regex> = words
        .iter()
        .filter_map(|word| regex::Regex::new(&format!(r"(?i)\b{}\b", regex::escape(word))).ok())
        .collect();

    for pattern in &patterns {
        result = pattern
            .replace_all(&result, |caps: &regex::Captures| {
                format!("{}{}{}", start_tag, &caps[0], end_tag)
            })
            .to_string();
    }

    result
}

/// Generate highlighted snippets from text
pub fn generate_snippet(
    text: &str,
    query: &str,
    fragment_words: usize,
    start_tag: &str,
    end_tag: &str,
) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let query_words_owned: Vec<String> =
        query.split_whitespace().map(|w| w.to_lowercase()).collect();

    // Find the first matching word position
    let mut match_pos = None;
    for (i, word) in words.iter().enumerate() {
        let word_lower = word.to_lowercase();
        if query_words_owned.iter().any(|q| word_lower.contains(q)) {
            match_pos = Some(i);
            break;
        }
    }

    if let Some(pos) = match_pos {
        let start = pos.saturating_sub(fragment_words);
        let end = (pos + fragment_words).min(words.len());

        let snippet_words: Vec<String> = words[start..end]
            .iter()
            .map(|w| {
                let word_lower = w.to_lowercase();
                if query_words_owned.iter().any(|q| word_lower.contains(q)) {
                    format!("{}{}{}", start_tag, w, end_tag)
                } else {
                    w.to_string()
                }
            })
            .collect();

        let mut snippet = snippet_words.join(" ");
        if start > 0 {
            snippet = format!("...{}", snippet);
        }
        if end < words.len() {
            snippet = format!("{}...", snippet);
        }
        snippet
    } else {
        // No match found, return beginning of text
        let end = fragment_words.min(words.len());
        let snippet = words[..end].join(" ");
        if end < words.len() {
            format!("{}...", snippet)
        } else {
            snippet
        }
    }
}

/// PostgreSQL-specific highlighting using ts_headline
pub fn pg_headline_sql(
    column: &str,
    query: &str,
    language: &str,
    start_tag: &str,
    end_tag: &str,
) -> String {
    format!(
        "ts_headline('{}', \"{}\", plainto_tsquery('{}', '{}'), \
         'StartSel={}, StopSel={}, MaxWords=35, MinWords=15')",
        language,
        column,
        language,
        escape_string(query),
        start_tag,
        end_tag
    )
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Escape a string for SQL queries
fn escape_string(s: &str) -> String {
    s.replace('\'', "''").replace('\\', "\\\\")
}

/// Escape a query for SQLite FTS5
fn escape_fts5_query(s: &str) -> String {
    // FTS5 uses double quotes for phrases
    // Escape special characters
    s.replace('"', "\"\"").replace('\'', "''")
}

fn quote_ident(db_type: DatabaseType, name: &str) -> String {
    let quote = match db_type {
        DatabaseType::Postgres | DatabaseType::SQLite => '"',
        DatabaseType::MySQL | DatabaseType::MariaDB => '`',
    };
    let escaped = name.replace(quote, &format!("{quote}{quote}"));
    format!("{}{}{}", quote, escaped, quote)
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
#[path = "testing/fulltext_tests.rs"]
mod tests;
