use super::*;

#[test]
fn test_query_analyzer() {
    let suggestions = QueryAnalyzer::analyze("SELECT * FROM users");
    assert!(!suggestions.is_empty());
    assert!(suggestions.iter().any(|s| s.title.contains("SELECT *")));
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
fn test_profiler() {
    let mut profiler = Profiler::start();
    profiler.record("SELECT 1", Duration::from_millis(10));
    profiler.record("SELECT 2", Duration::from_millis(20));

    let report = profiler.stop();
    assert_eq!(report.query_count(), 2);
}

#[test]
fn test_global_stats() {
    GlobalProfiler::enable();
    GlobalProfiler::reset();

    GlobalProfiler::record(Duration::from_millis(50));
    GlobalProfiler::record(Duration::from_millis(150));

    let stats = GlobalProfiler::stats();
    assert_eq!(stats.total_queries, 2);
    assert_eq!(stats.slow_queries, 1);

    GlobalProfiler::disable();
}

#[tokio::test]
async fn test_profile_future_records_enabled_queries() {
    GlobalProfiler::enable();
    GlobalProfiler::reset();

    let value = __profile_future(async { 42_u64 }).await;

    assert_eq!(value, 42);
    assert_eq!(GlobalProfiler::stats().total_queries, 1);

    GlobalProfiler::disable();
    GlobalProfiler::reset();
}

#[tokio::test]
async fn test_profile_future_does_not_record_when_disabled() {
    GlobalProfiler::disable();
    GlobalProfiler::reset();

    let value = __profile_future(async { 7_u64 }).await;

    assert_eq!(value, 7);
    assert_eq!(GlobalProfiler::stats().total_queries, 0);
}

#[test]
fn test_missing_where_detection() {
    let suggestions = QueryAnalyzer::analyze("DELETE FROM users");
    assert!(
        suggestions
            .iter()
            .any(|s| s.level == SuggestionLevel::Critical)
    );
}