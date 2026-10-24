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

    #[tideorm::model(table = "fulltext_test_notes", soft_delete)]
    pub struct FullTextTestNote {
        #[tideorm(primary_key, auto_increment)]
        pub id: i64,
        pub body: String,
        pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    }

    #[tideorm::model(table = "fulltext_rank_collision")]
    pub struct RankCollision {
        #[tideorm(primary_key)]
        pub id: i64,
        pub body: String,
        #[tideorm(column = "_fts_rank")]
        pub stored_rank: f64,
        #[tideorm(column = "_fts_rank_")]
        pub another_rank: f64,
    }
}

use builder_test_model::{FullTextTestArticle, FullTextTestNote};

#[test]
fn ranked_search_avoids_all_model_column_names_on_every_backend() {
    for backend in [
        DatabaseType::SQLite,
        DatabaseType::Postgres,
        DatabaseType::MySQL,
    ] {
        for query in ["alpha", ""] {
            let builder =
                FullTextSearchBuilder::<builder_test_model::RankCollision>::new(&["body"], query);
            let (sql, _) = builder.build_ranked_sql(backend).unwrap();
            assert!(sql.contains(" AS _fts_rank__"), "{sql}");
            if !query.is_empty() && backend != DatabaseType::SQLite {
                assert!(sql.contains("ORDER BY _fts_rank__ DESC"), "{sql}");
            }
        }
    }
}

/// Search SQL selects the model's columns by name and quotes each searched
/// column, doubling any quote inside it.
#[test]
fn test_search_sql_selects_model_columns_and_quotes_searched_ones() {
    let (postgres, _) =
        FullTextSearchBuilder::<FullTextTestArticle>::new(&["ti\"tle", "content"], "rust")
            .build_sql(DatabaseType::Postgres)
            .unwrap();
    assert!(
        postgres.starts_with(
            "SELECT \"id\", \"title\", \"content\" FROM \"fulltext_test_articles\" WHERE "
        ),
        "{postgres}"
    );
    assert!(
        postgres.contains("COALESCE(\"ti\"\"tle\", '') || ' ' || COALESCE(\"content\", '')"),
        "{postgres}"
    );

    let (mysql, _) =
        FullTextSearchBuilder::<FullTextTestArticle>::new(&["ti`tle", "content"], "rust")
            .build_sql(DatabaseType::MySQL)
            .unwrap();
    assert!(
        mysql.contains("MATCH(`ti``tle`, `content`) AGAINST(?)"),
        "{mysql}"
    );

    let (sqlite, _) =
        FullTextSearchBuilder::<FullTextTestArticle>::new(&["title", "content"], "rust")
            .build_sql(DatabaseType::SQLite)
            .unwrap();
    assert!(
        sqlite.starts_with(
            "SELECT \"t\".\"id\", \"t\".\"title\", \"t\".\"content\" FROM \"fulltext_test_articles\" t \
             INNER JOIN \"fulltext_test_articles_fts\" fts"
        ),
        "{sqlite}"
    );
    assert!(
        sqlite.contains("WHERE \"fulltext_test_articles_fts\" MATCH ?"),
        "{sqlite}"
    );
}

#[test]
fn test_search_mode_display() {
    assert_eq!(SearchMode::Natural.to_string(), "natural");
    assert_eq!(SearchMode::Boolean.to_string(), "boolean");
    assert_eq!(SearchMode::Phrase.to_string(), "phrase");
    assert_eq!(SearchMode::Prefix.to_string(), "prefix");
    assert_eq!(SearchMode::Proximity(3).to_string(), "proximity(3)");
}

#[test]
fn test_highlight_text() {
    let text = "The quick brown fox jumps over the lazy dog";
    let highlighted = highlight_text(text, "quick fox", "<b>", "</b>");
    assert!(highlighted.contains("<b>quick</b>"));
    assert!(highlighted.contains("<b>fox</b>"));

    // The tags it inserts are not searched again.
    assert_eq!(
        highlight_text("the mark", "mark b", "<b>", "</b>"),
        "the <b>mark</b>"
    );
    assert_eq!(highlight_text("no terms", "", "<b>", "</b>"), "no terms");
    // The longest term is tried first, so `rust-lang` is marked whole.
    assert_eq!(
        highlight_text("rust-lang and rust", "rust rust-lang", "[", "]"),
        "[rust-lang] and [rust]"
    );
}

#[test]
fn test_term_filters_leave_words_out_of_the_search() {
    let config = FullTextConfig::new()
        .min_word_length(3)
        .max_word_length(10)
        .stop_words(vec!["The".to_string()]);
    let builder = FullTextSearchBuilder::<FullTextTestArticle>::new(
        &["title"],
        "the go +rust enormouslylong ownership",
    )
    .config(config.clone());
    assert_eq!(builder.query_text(), "+rust ownership");

    let (sql, params) = builder.build_sql(DatabaseType::SQLite).unwrap();
    assert!(sql.contains("MATCH ?"), "{sql}");
    assert_eq!(
        params[0],
        Value::String(Some("{\"title\"} : (\"+rust\" \"ownership\")".to_string()))
    );

    let nothing_left = FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "the go")
        .config(config.clone());
    let (sql, _) = nothing_left.build_sql(DatabaseType::SQLite).unwrap();
    assert!(
        sql.contains("0 = 1"),
        "a query with no term left matches nothing: {sql}"
    );

    let phrase = FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "the go")
        .config(config.clone().mode(SearchMode::Phrase));
    assert_eq!(
        phrase.query_text(),
        "the go",
        "a phrase is searched as written"
    );

    // A quoted phrase, and the operator before it, stay as written.
    let quoted = FullTextSearchBuilder::<FullTextTestArticle>::new(
        &["title"],
        "\"the rust book\" -the -\"go the\" +ownership",
    )
    .config(config.mode(SearchMode::Boolean));
    assert_eq!(
        quoted.query_text(),
        "\"the rust book\" -\"go the\" +ownership"
    );
}

#[test]
fn test_highlight_config_marks_a_snippet_cut_to_its_length() {
    let text = "one two three four five six seven eight nine fox ten eleven twelve";
    let fox = term_pattern("fox");
    let whole = HighlightConfig {
        fragment_words: None,
        ..HighlightConfig::default()
    };
    assert_eq!(
        whole.mark("the fox ran", fox.as_ref()),
        ("the <mark>fox</mark> ran".to_string(), 1)
    );

    let snippet = HighlightConfig {
        start_tag: "[".to_string(),
        end_tag: "]".to_string(),
        max_length: Some(20),
        fragment_words: Some(2),
        escape_html: false,
    };
    let (marked, matches) = snippet.mark(text, fox.as_ref());
    assert!(marked.contains("[fox]"), "{marked}");
    assert!(!marked.contains("one"), "{marked}");
    assert!(marked.ends_with("..."), "{marked}");
    assert_eq!(matches, 1);
}

#[test]
fn test_highlights_center_on_a_whole_word_match_and_escape_the_text() {
    // `art` is inside `Restart` too, which a substring search would centre
    // the snippet on and then find nothing to mark.
    let config = HighlightConfig {
        fragment_words: Some(1),
        ..HighlightConfig::default()
    };
    let text = "Restart one two three the art class";
    assert_eq!(
        config.mark(text, term_pattern("art").as_ref()),
        ("...the <mark>art</mark> class".to_string(), 1)
    );

    // The record's text is escaped around the tags, and a tag-like string
    // already in it is not counted as a match.
    let whole = HighlightConfig {
        fragment_words: None,
        ..HighlightConfig::default()
    };
    assert_eq!(
        whole.mark(
            "<script>x</script> <mark> fox",
            term_pattern("fox").as_ref()
        ),
        (
            "&lt;script&gt;x&lt;/script&gt; &lt;mark&gt; <mark>fox</mark>".to_string(),
            1
        )
    );

    assert!(term_pattern("  ").is_none());
}

#[test]
fn test_generate_snippet() {
    let text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
               The quick brown fox jumps over the lazy dog. \
               Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.";
    let snippet = generate_snippet(text, "fox", 5, "<mark>", "</mark>");
    assert!(snippet.contains("<mark>fox</mark>"));
    assert!(snippet.contains("..."));

    // The match is always in the snippet, with as many words on each side.
    assert_eq!(
        generate_snippet(text, "fox", 0, "<b>", "</b>"),
        "...<b>fox</b>..."
    );
    assert_eq!(
        generate_snippet(text, "fox", 1, "<b>", "</b>"),
        "...brown <b>fox</b> jumps..."
    );

    // A term is a whole word: `art` is not the end of `Restart`.
    assert_eq!(
        generate_snippet("Restart the art show now", "art", 1, "<b>", "</b>"),
        "...the <b>art</b> show..."
    );
}

#[test]
fn test_fulltext_index_sql_per_backend() {
    let index = FullTextIndex::new(
        "idx_articles_search",
        "articles",
        vec!["title".to_string(), "content".to_string()],
    )
    .language("english")
    .pg_index_type(PgFullTextIndexType::GIN);

    assert_eq!(
        index.to_postgres_sql(),
        "CREATE INDEX \"idx_articles_search\" ON \"articles\" USING GIN \
         ((to_tsvector('english', COALESCE(\"title\", '') || ' ' || COALESCE(\"content\", ''))))"
    );
    assert_eq!(
        index.to_sql(DatabaseType::Postgres),
        [index.to_postgres_sql()]
    );

    assert_eq!(
        index.to_mysql_sql(),
        "CREATE FULLTEXT INDEX `idx_articles_search` ON `articles`(`title`, `content`)"
    );
    for db_type in [DatabaseType::MySQL, DatabaseType::MariaDB] {
        assert_eq!(index.to_sql(db_type), [index.to_mysql_sql()], "{db_type:?}");
    }

    let sqlite = index.to_sqlite_sql();
    assert_eq!(index.to_sql(DatabaseType::SQLite), sqlite);
    assert_eq!(sqlite.len(), 7);
    assert!(
        sqlite[1].starts_with(
            "CREATE VIRTUAL TABLE IF NOT EXISTS \"articles_fts\" USING fts5(\"title\", \"content\""
        ),
        "{}",
        sqlite[1]
    );
    // Rows the table held before the index existed are indexed too, by the
    // rowids they have now.
    let rebuild = index.sqlite_rebuild_sql();
    assert_eq!(sqlite[6], rebuild);
    assert!(
        rebuild.starts_with(r#"INSERT INTO "articles_fts"("articles_fts") SELECT CASE WHEN "#),
        "{rebuild}"
    );
    assert!(
        rebuild.contains("pragma_table_xinfo('articles')"),
        "{rebuild}"
    );
    assert!(rebuild.ends_with(" THEN 'rebuild' END"), "{rebuild}");
}

#[test]
fn a_fulltext_index_names_a_qualified_table_part_by_part() {
    let index = FullTextIndex::new("posts_search", "tenant.posts", vec!["body".to_string()]);

    assert!(
        index
            .to_postgres_sql()
            .contains(r#" ON "tenant"."posts" USING GIN"#),
        "{}",
        index.to_postgres_sql()
    );
    assert!(
        index.to_mysql_sql().contains(" ON `tenant`.`posts`("),
        "{}",
        index.to_mysql_sql()
    );
    // SQLite creates the index in the attached database and names the tables
    // inside it bare, as FTS5 and its triggers require.
    let sqlite = index.to_sqlite_sql();
    assert!(
        sqlite[1].starts_with(r#"CREATE VIRTUAL TABLE IF NOT EXISTS "tenant"."posts_fts" USING fts5("body", content="posts""#),
        "{}",
        sqlite[1]
    );
    assert!(
        sqlite[3].starts_with(
            r#"CREATE TRIGGER IF NOT EXISTS "tenant"."posts_ai" AFTER INSERT ON "posts" BEGIN INSERT INTO "posts_fts"("#
        ),
        "{}",
        sqlite[3]
    );
    let rebuild = index.sqlite_rebuild_sql();
    assert!(
        rebuild.starts_with(r#"INSERT INTO "tenant"."posts_fts"("posts_fts") SELECT CASE WHEN "#)
    );
    assert!(rebuild.contains("pragma_table_xinfo('posts', 'tenant')"));
    assert!(rebuild.ends_with(" THEN 'rebuild' END"));
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
    assert!(sqls[3].contains("INSERT INTO \"art\"\"icles_fts\"(rowid, "));
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
fn test_escape_fts5_query_quotes_each_literal_term() {
    assert_eq!(escape_fts5_query("test* OR 1"), "\"test*\" \"OR\" \"1\"");
    assert_eq!(
        escape_fts5_query("say \"hello world\" now"),
        "\"say\" \"hello world\" \"now\""
    );
    // A term that carries no lexeme is dropped: FTS5 rejects an empty MATCH
    // operand.
    assert_eq!(escape_fts5_query("   "), "");
    assert_eq!(escape_fts5_query("* ^ -"), "");
    assert_eq!(escape_fts5_query("rust *"), "\"rust\"");
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
    // A term that carries no lexeme is dropped: PostgreSQL rejects an empty
    // quoted lexeme.
    assert_eq!(sanitize_postgres_tsquery("", false), "");
    assert_eq!(sanitize_postgres_tsquery("&|!", false), "");
    assert_eq!(sanitize_postgres_tsquery("'''", false), "");
    assert_eq!(sanitize_postgres_tsquery("rust &", false), "'rust'");
}

#[test]
fn test_postgres_fulltext_sql_parameterizes_runtime_values() {
    let builder =
        FullTextSearchBuilder::<FullTextTestArticle>::new(&["title", "content"], "rust safety")
            .language("custom_lang")
            .limit(10)
            .offset(5);

    let (sql, params) = builder.build_sql(DatabaseType::Postgres).unwrap();

    // The configuration is a constant, as in the index `FullTextIndex` builds,
    // so the query can use that index; the search text stays bound.
    assert!(
        sql.contains("to_tsvector('custom_lang', COALESCE("),
        "{sql}"
    );
    assert!(sql.contains("plainto_tsquery('custom_lang', $1)"), "{sql}");
    assert!(sql.contains("LIMIT 10 OFFSET $2"), "{sql}");
    assert!(!sql.contains("rust safety"));
    assert_eq!(
        params,
        vec![
            Value::String(Some("rust safety".to_string())),
            Value::BigInt(Some(5)),
        ]
    );

    let index = FullTextIndex::new(
        "idx",
        "fulltext_test_articles",
        vec!["title".to_string(), "content".to_string()],
    )
    .language("custom_lang");
    let indexed = index.to_postgres_sql();
    let expression = indexed
        .split_once("((")
        .and_then(|(_, rest)| rest.strip_suffix("))"))
        .expect("the index expression");
    assert!(sql.contains(expression), "{sql} does not use {expression}");

    for language in [
        "english'); DROP TABLE users; --",
        "en'g\"lish",
        "",
        "pg catalog",
    ] {
        let refused = FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "rust")
            .language(language)
            .build_sql(DatabaseType::Postgres);
        assert!(refused.is_err(), "{language:?} was accepted");
    }
    assert!(
        FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "rust")
            .language("pg_catalog.english")
            .build_sql(DatabaseType::Postgres)
            .is_ok()
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

    assert!(sql.contains("ts_rank_cd(CAST($2 AS real[]),"));
    assert!(sql.contains(" >= $3"));
    assert!(sql.contains("LIMIT 3 OFFSET $4"), "{sql}");
    assert!(!sql.contains("{0.05,0.3,0.7,1.5}"));
    assert_eq!(
        params,
        vec![
            Value::String(Some("ranked query".to_string())),
            Value::String(Some("{0.05,0.3,0.7,1.5}".to_string())),
            Value::Double(Some(0.42)),
            Value::BigInt(Some(2)),
        ]
    );
}

#[test]
fn test_fulltext_offset_without_limit_uses_each_dialects_open_ended_limit() {
    let builder =
        FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "portable query").offset(4);

    let (sqlite_sql, _) = builder.build_sql(DatabaseType::SQLite).unwrap();
    assert!(sqlite_sql.ends_with(" LIMIT -1 OFFSET ?"), "{sqlite_sql}");
    let (mysql_sql, _) = builder.build_sql(DatabaseType::MySQL).unwrap();
    assert!(
        mysql_sql.ends_with(" LIMIT 18446744073709551615 OFFSET ?"),
        "{mysql_sql}"
    );
    let (postgres_sql, _) = builder.build_sql(DatabaseType::Postgres).unwrap();
    assert!(!postgres_sql.contains(" LIMIT "), "{postgres_sql}");
    assert!(postgres_sql.contains(" OFFSET $"), "{postgres_sql}");
}

#[test]
fn test_mysql_and_sqlite_fulltext_sql_parameterize_pagination() {
    let builder = FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "portable query")
        .limit(7)
        .offset(4);

    let (mysql_sql, mysql_params) = builder.build_sql(DatabaseType::MySQL).unwrap();
    assert!(mysql_sql.contains("LIMIT 7 OFFSET ?"), "{mysql_sql}");
    assert_eq!(
        mysql_params,
        vec![
            Value::String(Some("portable query".to_string())),
            Value::BigInt(Some(4)),
        ]
    );

    let (sqlite_sql, sqlite_params) = builder.build_sql(DatabaseType::SQLite).unwrap();
    assert!(sqlite_sql.contains("LIMIT 7 OFFSET ?"), "{sqlite_sql}");
    assert_eq!(
        sqlite_params,
        vec![
            Value::String(Some("{\"title\"} : (\"portable\" \"query\")".to_string())),
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
        SearchMode::Proximity(2),
    ] {
        let expected = match mode {
            SearchMode::Natural => "AGAINST(?)",
            _ => "AGAINST(? IN BOOLEAN MODE)",
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
fn test_mysql_ranked_sql_binds_each_placeholder_in_statement_order() {
    let builder = FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "rust")
        .min_rank(0.5)
        .limit(3);

    let (sql, params) = builder.build_ranked_sql(DatabaseType::MySQL).unwrap();

    // MySQL binds `?` by position: the threshold `AGAINST` must receive the
    // query and the `>=` comparison the score, not the other way round.
    assert!(sql.contains("AND MATCH(`title`) AGAINST(?) >= ? ORDER BY _fts_rank DESC"));
    assert!(sql.ends_with(" LIMIT 3"), "{sql}");
    assert_eq!(
        params,
        vec![
            Value::String(Some("rust".to_string())),
            Value::String(Some("rust".to_string())),
            Value::String(Some("rust".to_string())),
            Value::Double(Some(0.5)),
        ]
    );
}

#[test]
fn test_postgres_ranked_sql_selects_a_double_precision_rank() {
    let builder = FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "rust");

    let (sql, _) = builder.build_ranked_sql(DatabaseType::Postgres).unwrap();

    // `ts_rank_cd` returns `real`, which the `f64` rank cannot decode.
    assert!(sql.contains(", CAST(ts_rank_cd(CAST($2 AS real[]), "));
    assert!(
        !sql.contains("SELECT *"),
        "ranked rows name their columns: {sql}"
    );
    assert!(sql.contains(" AS double precision) AS _fts_rank FROM "));
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
async fn sqlite_execute(db: &crate::database::Database, statements: &[impl AsRef<str>]) {
    for statement in statements {
        let statement = statement.as_ref();
        db.__execute_with_params(statement, Vec::new())
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

/// An in-memory SQLite database with `statements` run on it.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
async fn sqlite_database(statements: &[&str]) -> crate::database::Database {
    let db = crate::database::Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed");
    sqlite_execute(&db, statements).await;
    db
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn test_search_reports_matching_rows_that_fail_to_decode() {
    let db = sqlite_database(&[
        "CREATE TABLE fulltext_test_articles (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT, content TEXT)",
    ])
    .await;
    let index = FullTextIndex::new(
        "idx_fulltext_test_articles",
        "fulltext_test_articles",
        vec!["title".to_string(), "content".to_string()],
    );
    sqlite_execute(&db, &index.to_sqlite_sql()).await;
    // `title` is a `String` on the model, so a NULL title cannot decode.
    sqlite_execute(
        &db,
        &["INSERT INTO fulltext_test_articles (title, content) VALUES (NULL, 'Rust without a title')"],
    )
    .await;

    let rows = crate::database::__in_db_scope(&db, async {
        FullTextTestArticle::search(&["content"], "rust")
            .get()
            .await
    })
    .await;
    assert!(
        rows.is_err(),
        "an undecodable match must be reported, not dropped: {rows:?}"
    );

    let ranked = crate::database::__in_db_scope(&db, async {
        FullTextTestArticle::search(&["content"], "rust")
            .get_ranked()
            .await
    })
    .await;
    assert!(
        ranked.is_err(),
        "an undecodable ranked match must be reported, not dropped"
    );
}

/// A search with no word matches nothing on every backend rather than
/// handing the parser an empty operand: FTS5 rejects an empty MATCH, MySQL
/// errors on a boolean query of bare operators, and `plainto_tsquery('')`
/// reads the whole table to match nothing.
#[test]
fn test_a_search_without_words_matches_nothing_on_every_backend() {
    for query in ["   ", "* -"] {
        let builder =
            FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], query).min_rank(0.5);
        for db_type in [
            DatabaseType::Postgres,
            DatabaseType::MySQL,
            DatabaseType::SQLite,
        ] {
            for (sql, params) in [
                builder.build_sql(db_type).unwrap(),
                builder.build_ranked_sql(db_type).unwrap(),
                builder.build_count_sql(db_type).unwrap(),
            ] {
                assert!(sql.contains("WHERE 0 = 1"), "{db_type:?} {query:?}: {sql}");
                for search in ["MATCH", "AGAINST", "tsquery", "bm25("] {
                    assert!(!sql.contains(search), "{db_type:?} {query:?}: {sql}");
                }
                // No search text is bound; PostgreSQL binds nothing at all.
                assert!(
                    !params.iter().any(|param| matches!(param, Value::String(_))),
                    "{db_type:?} {query:?}: {params:?}"
                );
                if db_type == DatabaseType::Postgres {
                    assert!(params.is_empty(), "{query:?}: {params:?}");
                }
            }
        }
    }
}

#[test]
fn test_mysql_fulltext_operands_always_parse() {
    // Outside boolean mode every operator character becomes a separator.
    assert_eq!(
        sanitize_mysql_fulltext_query("rust* -async \"(x)\"", false),
        "rust async x"
    );
    // In boolean mode a term keeps one leading operator and a trailing star,
    // and any other operator character separates words.
    assert_eq!(
        sanitize_mysql_fulltext_query("+rust -java* ~old wi-fi", true),
        "+rust -java* ~old wi fi"
    );
    assert_eq!(
        sanitize_mysql_fulltext_query("\"exact phrase\" +(grouped)", true),
        "\"exact phrase\" +grouped"
    );
    // An operator written against a phrase's quote belongs to the phrase.
    assert_eq!(
        sanitize_mysql_fulltext_query("rust -\"java script\" +\"exact phrase\"", true),
        "rust -\"java script\" +\"exact phrase\""
    );
    // Nothing searchable leaves no operand at all.
    for junk in ["*", "-", "--", "()", "\"", "+ - ~", "'; --", ""] {
        assert_eq!(
            sanitize_mysql_fulltext_query(junk, true),
            "",
            "boolean {junk:?}"
        );
        assert_eq!(
            sanitize_mysql_fulltext_query(junk, false),
            "",
            "natural {junk:?}"
        );
    }
}

/// A boolean search of exclusions alone has nothing to subtract them from:
/// MySQL and FTS5 match no row for it, where PostgreSQL matched every row
/// without the excluded word.
#[test]
fn test_a_boolean_search_of_exclusions_alone_matches_nothing() {
    let builder =
        FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "-spam -\"junk mail\"")
            .mode(SearchMode::Boolean);
    for db_type in [
        DatabaseType::Postgres,
        DatabaseType::MySQL,
        DatabaseType::SQLite,
    ] {
        let (sql, _) = builder.build_sql(db_type).unwrap();
        assert!(sql.contains("WHERE 0 = 1"), "{db_type:?}: {sql}");
    }

    let builder = FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "rust -spam")
        .mode(SearchMode::Boolean);
    let (sql, _) = builder.build_sql(DatabaseType::Postgres).unwrap();
    assert!(!sql.contains("0 = 1"), "{sql}");
}

#[test]
fn test_boolean_exclusions_stay_exclusions() {
    // Stripping the operators used to turn `-javascript` into a required term.
    assert_eq!(
        sanitize_postgres_boolean_tsquery("+rust async -javascript -\"java script\""),
        "'rust' & 'async' & !'javascript' & !('java' <-> 'script')"
    );
    assert_eq!(
        fts5_boolean_query("+rust async -javascript -\"java script\""),
        "(\"+rust\" AND \"async\") NOT \"-javascript\" NOT \"java script\""
    );
    // FTS5's NOT needs something to subtract from.
    assert_eq!(fts5_boolean_query("-javascript"), "");
}

#[test]
fn test_each_search_mode_renders_its_own_operand() {
    let render = |mode: SearchMode, db_type: DatabaseType| {
        let builder =
            FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "rust async").mode(mode);
        let (sql, mut params) = builder.build_sql(db_type).unwrap();
        (sql, params.remove(0))
    };
    let text = |value: &str| Value::String(Some(value.to_string()));
    // FTS5 searches every column of the index unless the operand names them.
    let in_title = |value: &str| text(&format!("{{\"title\"}} : ({value})"));

    for (mode, db_type, operand) in [
        (
            SearchMode::Phrase,
            DatabaseType::MySQL,
            text("\"rust async\""),
        ),
        (
            SearchMode::Prefix,
            DatabaseType::MySQL,
            text("+rust* +async*"),
        ),
        (
            SearchMode::Proximity(3),
            DatabaseType::MySQL,
            text("\"rust async\" @3"),
        ),
        (
            SearchMode::Phrase,
            DatabaseType::SQLite,
            in_title("\"rust async\""),
        ),
        (
            SearchMode::Prefix,
            DatabaseType::SQLite,
            in_title("\"rust\"* \"async\"*"),
        ),
        (
            SearchMode::Proximity(3),
            DatabaseType::SQLite,
            in_title("NEAR(\"rust\" \"async\", 3)"),
        ),
    ] {
        assert_eq!(render(mode, db_type).1, operand, "{mode} on {db_type:?}");
    }

    // PostgreSQL parses each mode's operand with its own function.
    for (mode, function, operand) in [
        (SearchMode::Natural, "plainto_tsquery", "rust async"),
        (SearchMode::Phrase, "phraseto_tsquery", "rust async"),
        (SearchMode::Boolean, "to_tsquery", "'rust' & 'async'"),
        (SearchMode::Prefix, "to_tsquery", "'rust':* & 'async':*"),
    ] {
        let (sql, bound) = render(mode, DatabaseType::Postgres);
        assert!(
            sql.contains(&format!("@@ {function}('english', $1)")),
            "{mode}: {sql}"
        );
        assert_eq!(bound, text(operand), "{mode}");
    }
}

#[test]
fn test_postgres_proximity_finds_the_words_within_the_distance_either_way() {
    // `<n>` alone would mean exactly n apart, in that order.
    assert_eq!(
        sanitize_postgres_proximity_tsquery("rust tokio", 2),
        "('rust' <1> 'tokio' | 'tokio' <1> 'rust' | 'rust' <2> 'tokio' | 'tokio' <2> 'rust')"
    );
    assert_eq!(sanitize_postgres_proximity_tsquery("rust", 5), "'rust'");
    assert_eq!(
        sanitize_postgres_proximity_tsquery("a b c", 1),
        "('a' <1> 'b' | 'b' <1> 'a') & ('b' <1> 'c' | 'c' <1> 'b')"
    );
    // A distance past the limit is capped rather than written out in full.
    assert_eq!(
        sanitize_postgres_proximity_tsquery("rust tokio", u32::MAX)
            .matches(" | ")
            .count(),
        127
    );
}

#[test]
fn test_search_leaves_soft_deleted_rows_out_unless_asked() {
    let search = || FullTextSearchBuilder::<FullTextTestNote>::new(&["body"], "rust");
    for db_type in [
        DatabaseType::Postgres,
        DatabaseType::MySQL,
        DatabaseType::SQLite,
    ] {
        let deleted_at = crate::query::db_sql::quote_ident(db_type, "deleted_at");
        let rendered = |builder: FullTextSearchBuilder<FullTextTestNote>| {
            [
                builder.build_sql(db_type).unwrap().0,
                builder.build_ranked_sql(db_type).unwrap().0,
                builder.build_count_sql(db_type).unwrap().0,
            ]
        };
        for sql in rendered(search()) {
            assert!(sql.contains(&format!("{deleted_at} IS NULL")), "{sql}");
        }
        for sql in rendered(search().only_trashed()) {
            assert!(sql.contains(&format!("{deleted_at} IS NOT NULL")), "{sql}");
        }
        for sql in rendered(search().with_trashed()) {
            assert!(!sql.contains(&format!("{deleted_at} IS")), "{sql}");
        }
    }

    // A model without soft delete has no scope to add.
    let (sql, _) = FullTextSearchBuilder::<FullTextTestArticle>::new(&["title"], "rust")
        .build_sql(DatabaseType::SQLite)
        .unwrap();
    assert!(!sql.contains("deleted_at"), "{sql}");
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn a_second_sqlite_index_over_other_columns_fails_instead_of_keeping_the_first() {
    let db = sqlite_database(&[
        "CREATE TABLE fts_posts (id INTEGER PRIMARY KEY, title TEXT, body TEXT)",
    ])
    .await;
    let by_title = FullTextIndex::new("posts_title", "fts_posts", vec!["title".to_string()]);
    sqlite_execute(&db, &by_title.to_sqlite_sql()).await;

    let by_body = FullTextIndex::new("posts_body", "fts_posts", vec!["body".to_string()]);
    let mut failure = None;
    for statement in by_body.to_sqlite_sql() {
        if let Err(error) = db.__execute_with_params(&statement, Vec::new()).await {
            failure = Some(error.to_string());
            break;
        }
    }
    let failure = failure.expect("the second index should be refused");
    assert!(
        failure.contains("full-text trigger name conflict"),
        "{failure}"
    );
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn test_sqlite_search_reads_earlier_rows_and_only_the_named_columns() {
    let db = sqlite_database(&[
        "CREATE TABLE fulltext_test_articles (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT, content TEXT)",
        "INSERT INTO fulltext_test_articles (title, content) VALUES ('Rust ORMs', 'Tomatoes')",
        "INSERT INTO fulltext_test_articles (title, content) VALUES ('Gardening', 'rust on the shovel')",
    ])
    .await;
    let index = FullTextIndex::new(
        "idx_fulltext_test_articles",
        "fulltext_test_articles",
        vec!["title".to_string(), "content".to_string()],
    );
    sqlite_execute(&db, &index.to_sqlite_sql()).await;

    crate::database::__in_db_scope(&db, async {
        // Both rows predate the index.
        let both = FullTextTestArticle::search(&["title", "content"], "rust")
            .count()
            .await?;
        assert_eq!(both, 2);

        let in_title = FullTextTestArticle::search(&["title"], "rust")
            .get()
            .await?;
        assert_eq!(in_title.len(), 1);
        assert_eq!(in_title[0].title, "Rust ORMs");
        assert_eq!(
            FullTextTestArticle::search(&["content"], "rust")
                .count()
                .await?,
            1
        );
        assert_eq!(
            FullTextTestArticle::search(&["content"], "gardening")
                .count()
                .await?,
            0
        );

        // Applying the index again keeps each row indexed once.
        sqlite_execute(&db, &index.to_sqlite_sql()).await;
        assert_eq!(
            FullTextTestArticle::search(&["title", "content"], "rust")
                .count()
                .await?,
            2
        );
        Ok(())
    })
    .await
    .expect("searching rows written before the index should succeed");
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn test_sqlite_search_skips_soft_deleted_rows() {
    let db = sqlite_database(&[
        "CREATE TABLE fulltext_test_notes (id INTEGER PRIMARY KEY AUTOINCREMENT, body TEXT NOT NULL, deleted_at TEXT)",
        "INSERT INTO fulltext_test_notes (body) VALUES ('rust is live')",
        "INSERT INTO fulltext_test_notes (body, deleted_at) VALUES ('rust is trashed', '2026-01-01T00:00:00Z')",
    ])
    .await;
    let index = FullTextIndex::new(
        "idx_fulltext_test_notes",
        "fulltext_test_notes",
        vec!["body".to_string()],
    );
    sqlite_execute(&db, &index.to_sqlite_sql()).await;

    crate::database::__in_db_scope(&db, async {
        let live = FullTextTestNote::search(&["body"], "rust").get().await?;
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].body, "rust is live");
        assert_eq!(
            FullTextTestNote::search(&["body"], "rust")
                .get_ranked()
                .await?
                .len(),
            1
        );

        let trashed = FullTextTestNote::search(&["body"], "rust")
            .only_trashed()
            .get()
            .await?;
        assert_eq!(trashed.len(), 1);
        assert_eq!(trashed[0].body, "rust is trashed");
        assert_eq!(
            FullTextTestNote::search(&["body"], "rust")
                .with_trashed()
                .count()
                .await?,
            2
        );
        Ok(())
    })
    .await
    .expect("searching a soft-delete model should succeed");
}
