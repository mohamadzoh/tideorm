use super::*;

mod mysql_sqlite;

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
        let statement = build_statement_with_values(backend, &sql, params);

        let results = match db.__get_connection()? {
            crate::database::ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(conn.connection().query_all_raw(statement)).await
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
        let statement = build_statement_with_values(backend, &sql, params);

        let results = match db.__get_connection()? {
            crate::database::ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(conn.connection().query_all_raw(statement)).await
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
        let statement = build_statement_with_values(backend, &sql, params);

        let result = match db.__get_connection()? {
            crate::database::ConnectionRef::Database(conn) => {
                crate::profiling::__profile_future(conn.connection().query_one_raw(statement)).await
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
        let language_placeholder = crate::internal::push_param(
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
        let language_placeholder = crate::internal::push_param(
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
            let min_rank_placeholder = crate::internal::push_param(
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
        let language_placeholder = crate::internal::push_param(
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
                let placeholder = crate::internal::push_param(
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
                let tsquery = sanitize_postgres_tsquery(&self.query, false);
                let use_plain = tsquery.is_empty();
                let placeholder = crate::internal::push_param(
                    DatabaseType::Postgres,
                    params,
                    Value::String(Some(if use_plain {
                        self.query.clone()
                    } else {
                        tsquery
                    })),
                );
                if use_plain {
                    format!(
                        "plainto_tsquery(CAST({} AS regconfig), {})",
                        language_placeholder, placeholder
                    )
                } else {
                    format!(
                        "to_tsquery(CAST({} AS regconfig), {})",
                        language_placeholder, placeholder
                    )
                }
            }
            SearchMode::Phrase => {
                let placeholder = crate::internal::push_param(
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
                let prefixed = sanitize_postgres_tsquery(&self.query, true);
                let use_plain = prefixed.is_empty();
                let placeholder = crate::internal::push_param(
                    DatabaseType::Postgres,
                    params,
                    Value::String(Some(if use_plain {
                        self.query.clone()
                    } else {
                        prefixed
                    })),
                );
                if use_plain {
                    format!(
                        "plainto_tsquery(CAST({} AS regconfig), {})",
                        language_placeholder, placeholder
                    )
                } else {
                    format!(
                        "to_tsquery(CAST({} AS regconfig), {})",
                        language_placeholder, placeholder
                    )
                }
            }
            SearchMode::Fuzzy => {
                let placeholder = crate::internal::push_param(
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
                let proximity = sanitize_postgres_proximity_tsquery(&self.query, distance);
                let use_plain = proximity.is_empty();
                let placeholder = crate::internal::push_param(
                    DatabaseType::Postgres,
                    params,
                    Value::String(Some(if use_plain {
                        self.query.clone()
                    } else {
                        proximity
                    })),
                );
                if use_plain {
                    format!(
                        "plainto_tsquery(CAST({} AS regconfig), {})",
                        language_placeholder, placeholder
                    )
                } else {
                    format!(
                        "to_tsquery(CAST({} AS regconfig), {})",
                        language_placeholder, placeholder
                    )
                }
            }
        }
    }
}
