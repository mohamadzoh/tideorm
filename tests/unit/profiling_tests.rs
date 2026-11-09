use std::time::Duration;

use super::*;
use crate::logging::QueryOperation;

#[test]
fn report_select_star_uses_sql_tokens_like_the_analyzer() {
    for (sql, expected) in [
        ("SELECT * FROM users", true),
        ("SELECT 'SELECT *' AS label", false),
        ("SELECT id /* SELECT * */ FROM users", false),
        ("SELECT\n /* columns */ * FROM users", true),
    ] {
        let mut profiler = Profiler::start();
        for _ in 0..6 {
            profiler.record(sql, Duration::ZERO);
        }
        assert_eq!(
            profiler
                .stop()
                .suggestions()
                .iter()
                .any(|s| s.contains("SELECT *")),
            expected,
            "{sql}"
        );
        assert_eq!(
            QueryAnalyzer::analyze(sql)
                .iter()
                .any(|s| s.title.contains("SELECT *")),
            expected,
            "{sql}"
        );
    }
}

#[test]
fn report_n_plus_one_counts_database_reads_only() {
    let mut profiler = Profiler::start();
    for _ in 0..11 {
        profiler.record_full(
            ProfiledQuery::new("SELECT id FROM users", Duration::ZERO)
                .with_table("users")
                .cached(),
        );
        profiler.record_full(
            ProfiledQuery::new("UPDATE users SET active = 1", Duration::ZERO).with_table("users"),
        );
    }
    for _ in 0..10 {
        profiler.record_full(
            ProfiledQuery::new("SELECT id FROM users", Duration::ZERO).with_table("users"),
        );
    }
    let mut report = profiler.stop();
    assert!(!report.suggestions().iter().any(|s| s.contains("N+1")));
    report
        .queries
        .push(ProfiledQuery::new("SELECT id FROM users", Duration::ZERO).with_table("users"));
    assert!(
        report
            .suggestions()
            .iter()
            .any(|s| s.contains("11 database reads"))
    );
}

#[test]
fn test_query_complexity() {
    let simple = QueryAnalyzer::estimate_complexity("SELECT id FROM users WHERE id = 1");
    assert_eq!(simple, QueryComplexity::Simple);

    let complex = QueryAnalyzer::estimate_complexity(
        "SELECT u.*, p.* FROM users u 
         JOIN posts p ON p.user_id = u.id 
         JOIN comments c ON c.post_id = p.id
         WHERE u.active = true
         ORDER BY p.created_at DESC",
    );
    assert!(matches!(
        complex,
        QueryComplexity::Complex | QueryComplexity::VeryComplex
    ));
}

#[test]
fn test_analyzer_flags_an_id_compared_with_a_string() {
    for sql in [
        "SELECT * FROM posts WHERE user_id = '5'",
        "select * from users where id = '5'",
    ] {
        assert!(
            QueryAnalyzer::analyze(sql)
                .iter()
                .any(|suggestion| suggestion.title == "Possible type mismatch"),
            "{sql}"
        );
    }
}

#[test]
fn test_analyzer_leaves_uuids_and_other_columns_alone() {
    for sql in [
        "SELECT * FROM users WHERE id = '550e8400-e29b-41d4-a716-446655440000'",
        "SELECT * FROM orders WHERE paid = 'yes'",
        "SELECT * FROM users WHERE uid = '5'",
    ] {
        assert!(
            !QueryAnalyzer::analyze(sql)
                .iter()
                .any(|suggestion| suggestion.title == "Possible type mismatch"),
            "{sql}"
        );
    }
}

#[test]
fn test_order_by_without_limit_reads_the_outer_statement() {
    let flagged = |sql: &str| {
        QueryAnalyzer::analyze(sql)
            .iter()
            .any(|suggestion| suggestion.title == "ORDER BY without LIMIT")
    };
    // A subquery's LIMIT does not bound the outer rows.
    assert!(flagged(
        "SELECT id FROM users WHERE id IN (SELECT user_id FROM posts LIMIT 5) ORDER BY id"
    ));
    // A window's ORDER BY sorts no rows of the result.
    assert!(!flagged(
        "SELECT id, ROW_NUMBER() OVER (ORDER BY id) AS n FROM users"
    ));
    assert!(!flagged("SELECT id FROM users ORDER BY id LIMIT 10"));
}

#[test]
fn test_report_slow_query_suggestion_uses_the_profiler_threshold() {
    GlobalProfiler::set_slow_threshold(Duration::from_millis(50));

    let mut profiler = Profiler::start();
    profiler.record("SELECT 1", Duration::from_millis(60));
    let suggestions = profiler.stop().suggestions();

    GlobalProfiler::set_slow_threshold(Duration::from_millis(100));

    assert!(
        suggestions
            .iter()
            .any(|suggestion| suggestion.starts_with("1 slow queries detected (>= 50ms)")),
        "{suggestions:?}"
    );
}

#[test]
fn a_leading_newline_does_not_hide_a_missing_where() {
    let suggestions = QueryAnalyzer::analyze("\n  DELETE FROM users");
    assert!(
        suggestions
            .iter()
            .any(|suggestion| suggestion.title == "Missing WHERE clause"),
        "{suggestions:?}"
    );
    assert_eq!(
        QueryAnalyzer::estimate_complexity("\n  DELETE FROM users"),
        QueryAnalyzer::estimate_complexity("DELETE FROM users")
    );
}

#[test]
fn diagnostics_inspect_structure_not_literal_text() {
    let warnings = QueryAnalyzer::analyze("UPDATE users SET name = 'WHERE'");
    assert!(
        warnings
            .iter()
            .any(|warning| warning.title == "Missing WHERE clause")
    );
    let warnings = QueryAnalyzer::analyze("SELECT LOWER(name) FROM users");
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.title == "Function in WHERE clause")
    );
    assert_eq!(
        crate::logging::QueryOperation::from_sql(
            "/* lead */ WITH c AS (SELECT 1) UPDATE users SET name = 'x'"
        ),
        crate::logging::QueryOperation::Update
    );
    assert_eq!(
        crate::logging::QueryOperation::from_sql("-- lead\nSELECT 1"),
        crate::logging::QueryOperation::Select
    );
}

#[test]
fn suggestions_take_a_threshold_and_list_the_most_read_table_first() {
    let mut profiler = Profiler::start();
    for (table, reads) in [("b_items", 11), ("c_items", 12), ("a_items", 11)] {
        for _ in 0..reads {
            profiler.record_full(
                ProfiledQuery::new("SELECT id FROM t", Duration::from_millis(1)).with_table(table),
            );
        }
    }
    profiler.record_full(ProfiledQuery::new(
        "UPDATE t SET a = 1",
        Duration::from_millis(9),
    ));
    let report = profiler.stop();

    let suggestions = report.suggestions_with(Duration::from_millis(5));
    let tables: Vec<_> = suggestions
        .iter()
        .filter_map(|s| s.split('\'').nth(1))
        .collect();
    assert_eq!(tables, ["c_items", "a_items", "b_items"], "{suggestions:?}");
    assert!(
        suggestions
            .iter()
            .any(|s| s.starts_with("1 slow queries detected (>= 5ms)")),
        "{suggestions:?}"
    );
    assert_eq!(
        report.suggestions_with(Duration::from_millis(5)),
        suggestions
    );

    assert_eq!(report.queries[0].operation_kind(), QueryOperation::Select);
    assert_eq!(
        report.queries.last().unwrap().operation_kind(),
        QueryOperation::Update
    );
    let mut labelled = ProfiledQuery::new("SELECT 1", Duration::ZERO);
    labelled.operation = "begin".into();
    assert_eq!(labelled.operation_kind(), QueryOperation::Unknown);
}
