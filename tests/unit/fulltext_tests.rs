use super::*;
use crate::config::DatabaseType;
use crate::internal::Value;

mod builder_test_model {
    #[tideorm::model(table = "fulltext_test_articles")]
    pub struct FullTextTestArticle {
        #[tideorm(primary_key, auto_increment)]
        pub id: i64,
        pub title: String,
        pub content: String,
    }
}

use builder_test_model::FullTextTestArticle;

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
fn test_fulltext_index_mysql_quotes_the_parser_name() {
    let mut index = FullTextIndex::new("idx", "articles", vec!["title".to_string()]);

    index.config.mysql_parser = Some("ngram".to_string());
    assert!(index.to_mysql_sql().contains(" WITH PARSER `ngram`"));

    // A parser name is an identifier, so it may not extend the statement.
    index.config.mysql_parser = Some("ngram`, KEY `injected".to_string());
    let sql = index.to_mysql_sql();
    assert!(sql.contains(" WITH PARSER `ngram``, KEY ``injected`"));
    assert!(!sql.contains(", KEY `injected`"));
}

#[test]
fn test_sqlite_fts_triggers_quote_the_virtual_table_consistently() {
    let index = FullTextIndex::new("idx", "art\"icles", vec!["title".to_string()]);
    let sqls = index.to_sqlite_sql();

    // The AFTER INSERT trigger used to write the quotes by hand, which broke on
    // any table name containing a double quote.
    assert!(sqls[1].contains("INSERT INTO \"art\"\"icles_fts\"(rowid, "));
    for sql in &sqls {
        assert!(
            !sql.contains("\"art\"icles_fts\""),
            "unescaped ident: {}",
            sql
        );
    }
}

#[test]
fn test_postgres_index_sql_escapes_literals_without_doubling_backslashes() {
    let index =
        FullTextIndex::new("idx", "articles", vec!["title".to_string()]).language("en'g\\ish");
    let sql = index.to_postgres_sql();

    assert!(sql.contains("to_tsvector('en''g\\ish'"));
    assert!(!sql.contains("\\\\"));
}

#[test]
fn test_pg_headline_sql_escapes_literals_and_quotes_identifiers() {
    let sql = pg_headline_sql("posts.body\"text", "don't panic", "en'g", "<b>'", "</b>'");

    assert!(sql.contains("\"posts\".\"body\"\"text\""));
    assert!(sql.contains("plainto_tsquery('en''g', 'don''t panic')"));
    assert!(sql.contains("'StartSel=\"<b>''\", StopSel=\"</b>''\", MaxWords=35, MinWords=15'"));
    assert!(!sql.contains("\\"));
}

#[test]
fn test_pg_headline_sql_tags_cannot_inject_headline_options() {
    let sql = pg_headline_sql("body", "term", "english", "<b>, MaxWords=1", "</b>");

    // The tag stays a single quoted option value instead of becoming another
    // `MaxWords` option that overrides the one below.
    assert!(sql.contains("StartSel=\"<b>, MaxWords=1\", StopSel=\"</b>\", MaxWords=35"));

    let sql = pg_headline_sql("body", "term", "english", "<span class=\"a\">", "</span>");
    assert!(sql.contains("StartSel=\"<span class=\"\"a\"\">\""));
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

#[test]
fn test_escape_fts5_query_quotes_each_literal_term() {
    assert_eq!(escape_fts5_query("test* OR 1"), "\"test*\" \"OR\" \"1\"");
    assert_eq!(
        escape_fts5_query("say \"hello world\" now"),
        "\"say\" \"hello world\" \"now\""
    );
}

#[test]
fn test_sanitizers_drop_terms_that_carry_no_lexeme() {
    // Operator-only input must not reach a query parser: FTS5 rejects an empty
    // MATCH operand and PostgreSQL rejects an empty quoted lexeme.
    assert_eq!(escape_fts5_query("   "), "");
    assert_eq!(escape_fts5_query("* ^ -"), "");
    assert_eq!(escape_fts5_query("rust *"), "\"rust\"");

    assert_eq!(sanitize_postgres_tsquery("", false), "");
    assert_eq!(sanitize_postgres_tsquery("&|!", false), "");
    assert_eq!(sanitize_postgres_tsquery("'''", false), "");
    assert_eq!(sanitize_postgres_tsquery("rust &", false), "'rust'");
}

#[test]
fn test_sanitize_postgres_tsquery_builds_literal_terms() {
    assert_eq!(
        sanitize_postgres_tsquery("test* OR 1", false),
        "'test' & 'OR' & '1'"
    );
    assert_eq!(
        sanitize_postgres_tsquery("quick \"brown fox\"", true),
        "'quick':* & ('brown':* <-> 'fox':*)"
    );
}

#[test]
fn test_postgres_fulltext_sql_parameterizes_runtime_values() {
    let builder =
        FullTextSearchBuilder::<FullTextTestArticle>::new(&["title", "content"], "rust safety")
            .language("custom_lang")
            .limit(10)
            .offset(5);

    let (sql, params) = builder.build_sql(DatabaseType::Postgres).unwrap();

    assert!(sql.contains("CAST($1 AS regconfig)"));
    assert!(sql.contains("plainto_tsquery(CAST($1 AS regconfig), $2)"));
    assert!(sql.contains("LIMIT $3"));
    assert!(sql.contains("OFFSET $4"));
    assert!(!sql.contains("rust safety"));
    assert!(!sql.contains("custom_lang"));
    assert_eq!(
        params,
        vec![
            Value::String(Some("custom_lang".to_string())),
            Value::String(Some("rust safety".to_string())),
            Value::BigInt(Some(10)),
            Value::BigInt(Some(5)),
        ]
    );
}

#[test]
fn test_postgres_ranked_fulltext_sql_parameterizes_weights_and_thresholds() {
    let builder = FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "ranked query")
        .config(
            FullTextConfig::new()
                .language("simple")
                .weights(SearchWeights::new(1.5, 0.7, 0.3, 0.05)),
        )
        .with_ranking()
        .min_rank(0.42)
        .limit(3)
        .offset(2);

    let (sql, params) = builder.build_ranked_sql(DatabaseType::Postgres).unwrap();

    assert!(sql.contains("ts_rank_cd(CAST($3 AS real[]),"));
    assert!(sql.contains(" >= $4"));
    assert!(sql.contains("LIMIT $5"));
    assert!(sql.contains("OFFSET $6"));
    assert!(!sql.contains("{0.05,0.3,0.7,1.5}"));
    assert_eq!(
        params,
        vec![
            Value::String(Some("simple".to_string())),
            Value::String(Some("ranked query".to_string())),
            Value::String(Some("{0.05,0.3,0.7,1.5}".to_string())),
            Value::Double(Some(0.42)),
            Value::BigInt(Some(3)),
            Value::BigInt(Some(2)),
        ]
    );
}

#[test]
fn test_mysql_and_sqlite_fulltext_sql_parameterize_pagination() {
    let builder = FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "portable query")
        .limit(7)
        .offset(4);

    let (mysql_sql, mysql_params) = builder.build_sql(DatabaseType::MySQL).unwrap();
    assert!(mysql_sql.contains("LIMIT ?"));
    assert!(mysql_sql.contains("OFFSET ?"));
    assert_eq!(
        mysql_params,
        vec![
            Value::String(Some("portable query".to_string())),
            Value::BigInt(Some(7)),
            Value::BigInt(Some(4)),
        ]
    );

    let (sqlite_sql, sqlite_params) = builder.build_sql(DatabaseType::SQLite).unwrap();
    assert!(sqlite_sql.contains("LIMIT ?"));
    assert!(sqlite_sql.contains("OFFSET ?"));
    assert_eq!(
        sqlite_params,
        vec![
            Value::String(Some("\"portable\" \"query\"".to_string())),
            Value::BigInt(Some(7)),
            Value::BigInt(Some(4)),
        ]
    );
}

#[test]
fn test_mysql_against_mode_matches_across_row_ranked_and_count_builders() {
    for mode in [
        SearchMode::Natural,
        SearchMode::Boolean,
        SearchMode::Phrase,
        SearchMode::Prefix,
        SearchMode::Fuzzy,
        SearchMode::Proximity(2),
    ] {
        let expected = match mode {
            SearchMode::Boolean => "AGAINST(? IN BOOLEAN MODE)",
            SearchMode::Phrase => "AGAINST(? WITH QUERY EXPANSION)",
            _ => "AGAINST(?)",
        };

        let builder =
            FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "shared mode").mode(mode);

        for sql in [
            builder.build_sql(DatabaseType::MySQL).unwrap().0,
            builder.build_ranked_sql(DatabaseType::MySQL).unwrap().0,
            builder.build_count_sql(DatabaseType::MySQL).unwrap().0,
        ] {
            assert!(sql.contains(expected), "{} for {}: {}", expected, mode, sql);
        }
    }
}

#[test]
fn test_sqlite_termless_query_matches_nothing_instead_of_empty_fts5_operand() {
    let builder =
        FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "   ").min_rank(0.5);

    for (sql, params) in [
        builder.build_sql(DatabaseType::SQLite).unwrap(),
        builder.build_ranked_sql(DatabaseType::SQLite).unwrap(),
        builder.build_count_sql(DatabaseType::SQLite).unwrap(),
    ] {
        assert!(!sql.contains("MATCH"), "empty MATCH operand: {}", sql);
        assert!(!sql.contains("bm25("), "bm25 without MATCH: {}", sql);
        assert!(sql.contains("WHERE 1 = 0"), "{}", sql);
        assert!(!params.contains(&Value::String(Some(String::new()))));
    }

    // A query with terms is unaffected.
    let builder = FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "rust");
    let (sql, params) = builder.build_sql(DatabaseType::SQLite).unwrap();
    assert!(sql.contains("\"fulltext_test_articles_fts\" MATCH ?"));
    assert_eq!(params, vec![Value::String(Some("\"rust\"".to_string()))]);
}
