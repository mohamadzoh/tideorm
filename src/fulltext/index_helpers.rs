use super::*;

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

        let mut params = Vec::new();
        SqlBuilder::new(DatabaseType::Postgres, &mut params)
            .raw("CREATE INDEX ")
            .ident(&self.name)
            .raw(" ON ")
            .ident(&self.table)
            .raw(" USING ")
            .raw(index_type)
            .raw(" ((to_tsvector('")
            .raw(&escape_sql_literal_for_db(DatabaseType::Postgres, language))
            .raw("', ")
            .raw(&pg_search_document(&self.columns))
            .raw(")))")
            .into_sql()
    }

    /// Generate CREATE FULLTEXT INDEX statement for MySQL
    pub fn to_mysql_sql(&self) -> String {
        let mut params = Vec::new();
        let mut builder = SqlBuilder::new(DatabaseType::MySQL, &mut params)
            .raw("CREATE FULLTEXT INDEX ")
            .ident(&self.name)
            .raw(" ON ")
            .ident(&self.table)
            .raw("(")
            .raw(&column_list(DatabaseType::MySQL, &self.columns, ""))
            .raw(")");
        // `WITH PARSER` takes an identifier, so it goes through the same quoting
        // path as every other identifier in this module rather than being pasted
        // in raw. A name that is not a real parser plugin then fails loudly on
        // the server instead of silently extending the statement.
        if let Some(parser) = &self.config.mysql_parser {
            builder = builder.raw(" WITH PARSER ").ident(parser);
        }
        builder.into_sql()
    }

    /// Generate CREATE VIRTUAL TABLE statement for SQLite FTS5, plus the
    /// triggers that keep it in sync with the table
    pub fn to_sqlite_sql(&self) -> Vec<String> {
        let mut params = Vec::new();
        let fts_table = format!("{}_fts", self.table);
        let columns = column_list(DatabaseType::SQLite, &self.columns, "");

        let insert_new = SqlBuilder::new(DatabaseType::SQLite, &mut params)
            .raw("INSERT INTO ")
            .ident(&fts_table)
            .raw("(rowid, ")
            .raw(&columns)
            .raw(") VALUES (new.rowid, ")
            .raw(&column_list(DatabaseType::SQLite, &self.columns, "new."))
            .raw(");")
            .into_sql();
        let delete_old = SqlBuilder::new(DatabaseType::SQLite, &mut params)
            .raw("INSERT INTO ")
            .ident(&fts_table)
            .raw("(")
            .ident(&fts_table)
            .raw(", rowid, ")
            .raw(&columns)
            .raw(") VALUES('delete', old.rowid, ")
            .raw(&column_list(DatabaseType::SQLite, &self.columns, "old."))
            .raw(");")
            .into_sql();

        vec![
            SqlBuilder::new(DatabaseType::SQLite, &mut params)
                .raw("CREATE VIRTUAL TABLE IF NOT EXISTS ")
                .ident(&fts_table)
                .raw(" USING fts5(")
                .raw(&columns)
                .raw(", content=")
                .ident(&self.table)
                .raw(", content_rowid=")
                .ident("rowid")
                .raw(")")
                .into_sql(),
            self.sqlite_trigger("ai", "INSERT", &insert_new),
            self.sqlite_trigger("ad", "DELETE", &delete_old),
            self.sqlite_trigger("au", "UPDATE", &format!("{delete_old} {insert_new}")),
        ]
    }

    /// Render the `AFTER <event>` trigger named `<table>_<suffix>`.
    fn sqlite_trigger(&self, suffix: &str, event: &str, body: &str) -> String {
        let mut params = Vec::new();
        SqlBuilder::new(DatabaseType::SQLite, &mut params)
            .raw("CREATE TRIGGER IF NOT EXISTS ")
            .ident(&format!("{}_{}", self.table, suffix))
            .raw(" AFTER ")
            .raw(event)
            .raw(" ON ")
            .ident(&self.table)
            .raw(" BEGIN ")
            .raw(body)
            .raw(" END")
            .into_sql()
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

/// Highlight search terms in text
///
/// Each whole word of `query` is marked, regardless of case. `text` is not
/// HTML-escaped; [`HighlightConfig`] escapes it around the tags it inserts.
pub fn highlight_text(text: &str, query: &str, start_tag: &str, end_tag: &str) -> String {
    match term_pattern(query) {
        Some(pattern) => mark_matches(text, &pattern, start_tag, end_tag, false).0,
        None => text.to_string(),
    }
}

/// The words of `query` as one case-insensitive whole-word pattern, or `None`
/// when there are none (or too many to compile).
///
/// One pattern marks every term in one pass, so a term cannot match inside the
/// tags an earlier one inserted (`b` inside `<b>`), and the longest terms come
/// first, so `rust-lang` is marked whole rather than as `rust`.
pub(crate) fn term_pattern(query: &str) -> Option<regex::Regex> {
    let mut words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    words.sort_by_key(|word| std::cmp::Reverse(word.chars().count()));
    let alternation: Vec<String> = words.into_iter().map(regex::escape).collect();
    regex::Regex::new(&format!(r"(?i)\b(?:{})\b", alternation.join("|"))).ok()
}

/// `text` with every match of `pattern` between the tags, HTML-escaped around
/// them when `escape` is set, and how many matches were marked.
pub(crate) fn mark_matches(
    text: &str,
    pattern: &regex::Regex,
    start_tag: &str,
    end_tag: &str,
    escape: bool,
) -> (String, usize) {
    let plain = |part: &str| {
        if escape {
            escape_html(part)
        } else {
            part.to_string()
        }
    };
    let mut marked = String::with_capacity(text.len());
    let mut matches = 0;
    let mut last = 0;
    for found in pattern.find_iter(text) {
        marked.push_str(&plain(&text[last..found.start()]));
        marked.push_str(start_tag);
        marked.push_str(&plain(found.as_str()));
        marked.push_str(end_tag);
        last = found.end();
        matches += 1;
    }
    marked.push_str(&plain(&text[last..]));
    (marked, matches)
}

/// `text` with the five characters HTML gives a meaning replaced by entities.
pub(crate) fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// The `fragment_words` words either side of the word holding the first match
/// of `pattern`, or the first words of `text` when nothing matches, with `...`
/// where words were left out.
pub(crate) fn snippet_around_first_match(
    text: &str,
    pattern: Option<&regex::Regex>,
    fragment_words: usize,
) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let center = pattern
        .and_then(|pattern| pattern.find(text))
        .map_or(0, |found| {
            let before = &text[..found.start()];
            let index = before.split_whitespace().count();
            // A match that starts inside a word belongs to that word.
            if before
                .chars()
                .next_back()
                .is_some_and(|ch| !ch.is_whitespace())
            {
                index - 1
            } else {
                index
            }
        });
    let start = center.saturating_sub(fragment_words);
    let end = center
        .saturating_add(fragment_words)
        .saturating_add(1)
        .min(words.len());

    let mut snippet = words[start..end].join(" ");
    if start > 0 {
        snippet.insert_str(0, "...");
    }
    if end < words.len() {
        snippet.push_str("...");
    }
    snippet
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
        let end = pos.saturating_add(fragment_words).min(words.len());

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
///
/// `start_tag` and `end_tag` end up inside `ts_headline`'s comma-separated
/// option string, so they are double-quoted there (with embedded `"` doubled)
/// to keep a caller-supplied tag from being read as another option such as
/// `MaxWords` or `MaxFragments`.
pub fn pg_headline_sql(
    column: &str,
    query: &str,
    language: &str,
    start_tag: &str,
    end_tag: &str,
) -> String {
    let column = format_identifier_reference(DatabaseType::Postgres, column)
        .unwrap_or_else(|| quote_ident(DatabaseType::Postgres, column));
    let options = format!(
        "StartSel={}, StopSel={}, MaxWords=35, MinWords=15",
        quote_ts_headline_option(start_tag),
        quote_ts_headline_option(end_tag),
    );

    let mut params = Vec::new();
    SqlBuilder::new(DatabaseType::Postgres, &mut params)
        .raw("ts_headline('")
        .raw(&escape_sql_literal_for_db(DatabaseType::Postgres, language))
        .raw("', ")
        .raw(&column)
        .raw(", plainto_tsquery('")
        .raw(&escape_sql_literal_for_db(DatabaseType::Postgres, language))
        .raw("', '")
        .raw(&escape_sql_literal_for_db(DatabaseType::Postgres, query))
        .raw("'), '")
        .raw(&escape_sql_literal_for_db(DatabaseType::Postgres, &options))
        .raw("')")
        .into_sql()
}

/// Quote a `ts_headline` option value.
///
/// PostgreSQL lets an option value be wrapped in double quotes, with an
/// embedded double quote written twice. Quoting unconditionally means a value
/// containing `,` or `=` stays one value instead of introducing extra options.
fn quote_ts_headline_option(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
