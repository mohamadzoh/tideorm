use super::*;

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
    let column = format_identifier_reference(DatabaseType::Postgres, column)
        .unwrap_or_else(|| quote_ident(DatabaseType::Postgres, column));
    let language = escape_string(language);
    let query = escape_string(query);
    let start_tag = escape_string(start_tag);
    let end_tag = escape_string(end_tag);

    format!(
        "ts_headline('{}', {}, plainto_tsquery('{}', '{}'), \
         'StartSel={}, StopSel={}, MaxWords=35, MinWords=15')",
        language, column, language, query, start_tag, end_tag
    )
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Escape a string for SQL queries
fn escape_string(s: &str) -> String {
    escape_sql_literal(s).replace('\\', "\\\\")
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
#[path = "../../tests/unit/fulltext_tests.rs"]
mod tests;
