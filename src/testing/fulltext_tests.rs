use super::*;

#[test]
fn test_search_mode_display() {
    assert_eq!(SearchMode::Natural.to_string(), "natural");
    assert_eq!(SearchMode::Boolean.to_string(), "boolean");
    assert_eq!(SearchMode::Phrase.to_string(), "phrase");
    assert_eq!(SearchMode::Prefix.to_string(), "prefix");
    assert_eq!(SearchMode::Fuzzy.to_string(), "fuzzy");
    assert_eq!(SearchMode::Proximity(3).to_string(), "proximity(3)");
}

#[test]
fn test_search_weights() {
    let weights = SearchWeights::new(1.0, 0.5, 0.3, 0.1);
    assert_eq!(weights.to_pg_array(), "'{0.1,0.3,0.5,1}'");
}

#[test]
fn test_highlight_text() {
    let text = "The quick brown fox jumps over the lazy dog";
    let highlighted = highlight_text(text, "quick fox", "<b>", "</b>");
    assert!(highlighted.contains("<b>quick</b>"));
    assert!(highlighted.contains("<b>fox</b>"));
}

#[test]
fn test_generate_snippet() {
    let text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
               The quick brown fox jumps over the lazy dog. \
               Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.";
    let snippet = generate_snippet(text, "fox", 5, "<mark>", "</mark>");
    assert!(snippet.contains("<mark>fox</mark>"));
    assert!(snippet.contains("..."));
}

#[test]
fn test_fulltext_index_postgres() {
    let index = FullTextIndex::new(
        "idx_articles_search",
        "articles",
        vec!["title".to_string(), "content".to_string()],
    )
    .language("english")
    .pg_index_type(PgFullTextIndexType::GIN);

    let sql = index.to_postgres_sql();
    assert!(sql.contains("CREATE INDEX"));
    assert!(sql.contains("USING GIN"));
    assert!(sql.contains("to_tsvector"));
}

#[test]
fn test_fulltext_index_mysql() {
    let index = FullTextIndex::new(
        "idx_articles_search",
        "articles",
        vec!["title".to_string(), "content".to_string()],
    );
    let sql = index.to_mysql_sql();
    assert!(sql.contains("CREATE FULLTEXT INDEX"));
    assert!(sql.contains("`title`, `content`"));
}

#[test]
fn test_fulltext_index_mariadb() {
    let index = FullTextIndex::new(
        "idx_articles_search",
        "articles",
        vec!["title".to_string(), "content".to_string()],
    );
    let sqls = index.to_sql(DatabaseType::MariaDB);
    assert_eq!(sqls.len(), 1);
    let sql = &sqls[0];
    assert!(sql.contains("CREATE FULLTEXT INDEX"));
    assert!(sql.contains("`title`, `content`"));
}

#[test]
fn test_fulltext_index_sqlite() {
    let index = FullTextIndex::new(
        "idx_articles_search",
        "articles",
        vec!["title".to_string(), "content".to_string()],
    );
    let sqls = index.to_sqlite_sql();
    assert!(sqls.len() == 4);
    assert!(sqls[0].contains("CREATE VIRTUAL TABLE"));
    assert!(sqls[0].contains("fts5"));
}

#[test]
fn test_escape_string() {
    assert_eq!(escape_string("it's"), "it''s");
    assert_eq!(escape_string("back\\slash"), "back\\\\slash");
}

#[test]
fn test_fulltext_config() {
    let config = FullTextConfig::new()
        .language("german")
        .mode(SearchMode::Boolean)
        .min_word_length(3)
        .max_word_length(50);

    assert_eq!(config.language, Some("german".to_string()));
    assert_eq!(config.mode, SearchMode::Boolean);
    assert_eq!(config.min_word_length, Some(3));
    assert_eq!(config.max_word_length, Some(50));
}
