use super::*;

#[tokio::test]
async fn test_where_raw_rejects_unsafe_sql_before_db_lookup() {
    let err = QueryTestUser::query()
        .where_raw("1 = 1; DROP TABLE users")
        .count()
        .await
        .unwrap_err();

    assert!(err.to_string().contains("unsafe WHERE raw SQL"));
}

#[tokio::test]
async fn test_or_where_raw_rejects_unsafe_sql_before_db_lookup() {
    let err = QueryTestUser::query()
        .begin_or()
        .or_where_raw("1 = 1; DROP TABLE users")
        .end_or()
        .count()
        .await
        .unwrap_err();

    assert!(err.to_string().contains("unsafe WHERE raw SQL"));
}

#[tokio::test]
async fn test_having_rejects_unsafe_sql_before_db_lookup() {
    let err = QueryTestUser::query()
        .group_by("id")
        .having("COUNT(*) > 0; DROP TABLE users")
        .count()
        .await
        .unwrap_err();

    assert!(err.to_string().contains("unsafe HAVING raw SQL"));
}

#[tokio::test]
async fn test_having_rejects_subquery_like_sql_before_db_lookup() {
    let err = QueryTestUser::query()
        .group_by("id")
        .having("1 = 1 OR (SELECT password FROM users LIMIT 1)::text = 'x'")
        .count()
        .await
        .unwrap_err();

    assert!(err.to_string().contains("unsafe HAVING raw SQL"));
}

#[tokio::test]
async fn test_chunk_rejects_zero_batch_size_before_db_lookup() {
    let err = QueryTestUser::query()
        .chunk(0, |_| async { Ok::<(), crate::Error>(()) })
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("chunk() requires chunk_size to be greater than 0")
    );
}

#[tokio::test]
async fn test_chunk_rejects_offset_before_db_lookup() {
    let err = QueryTestUser::query()
        .offset(1)
        .chunk(10, |_| async { Ok::<(), crate::Error>(()) })
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("chunk() does not support offset()")
    );
}

#[tokio::test]
async fn test_chunk_rejects_non_primary_key_order_before_db_lookup() {
    let err = QueryTestUser::query()
        .order_by("name", Order::Asc)
        .chunk(10, |_| async { Ok::<(), crate::Error>(()) })
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("chunk() only supports explicit ordering by the single primary key 'id'")
    );
}

#[tokio::test]
async fn test_select_raw_rejects_unsafe_sql_before_db_lookup() {
    let err = QueryTestUser::query()
        .select_raw("id; DROP TABLE users")
        .count()
        .await
        .unwrap_err();

    assert!(err.to_string().contains("unsafe SELECT raw SQL"));
}

#[tokio::test]
async fn test_select_rejects_unsafe_expression_before_db_lookup() {
    let err = QueryTestUser::query()
        .select(vec!["COUNT(*); DROP TABLE users"])
        .count()
        .await
        .unwrap_err();

    assert!(err.to_string().contains("unsafe SELECT expression"));
}

#[tokio::test]
async fn test_order_by_rejects_unsafe_expression_before_db_lookup() {
    let err = QueryTestUser::query()
        .order_by("random(); DROP TABLE users", Order::Asc)
        .count()
        .await
        .unwrap_err();

    assert!(err.to_string().contains("unsafe ORDER BY column"));
}

#[tokio::test]
async fn test_group_by_rejects_unsafe_expression_before_db_lookup() {
    let err = QueryTestUser::query()
        .group_by("DATE_TRUNC('day', created_at); DROP TABLE users")
        .count()
        .await
        .unwrap_err();

    assert!(err.to_string().contains("unsafe GROUP BY column"));
}

#[test]
fn test_query_validation_allows_safe_expression_slots() {
    QueryBuilder::<QueryTestUser>::new()
        .select(vec!["COUNT(*) AS total"])
        .group_by("name")
        .order_by("name", Order::Asc)
        .ensure_query_is_valid()
        .expect("safe select/group/order slots should remain allowed");
}

#[test]
fn test_order_and_group_by_reject_sql_expressions_from_strings() {
    let order_err = QueryBuilder::<QueryTestUser>::new()
        .order_by(
            "(CASE WHEN (SELECT name FROM query_test_users LIMIT 1) = 'a' THEN id ELSE name END)",
            Order::Asc,
        )
        .ensure_query_is_valid()
        .expect_err("ORDER BY must not accept arbitrary SQL expressions");
    assert!(
        order_err.to_string().contains("unsafe ORDER BY column"),
        "unexpected error: {order_err}"
    );

    let group_err = QueryBuilder::<QueryTestUser>::new()
        .group_by("DATE(name)")
        .ensure_query_is_valid()
        .expect_err("GROUP BY must not accept arbitrary SQL expressions");
    assert!(
        group_err.to_string().contains("unsafe GROUP BY column"),
        "unexpected error: {group_err}"
    );
}

#[test]
fn test_order_by_accepts_inline_direction_suffix() {
    QueryBuilder::<QueryTestUser>::new()
        .order_by("name DESC", Order::Asc)
        .ensure_query_is_valid()
        .expect("a column with an inline ASC/DESC suffix should remain allowed");

    let sql = QueryBuilder::<QueryTestUser>::new()
        .order_by("name DESC", Order::Asc)
        .build_select_sql_for_db(DatabaseType::Postgres);
    assert!(sql.ends_with("ORDER BY \"name\" DESC"), "sql: {sql}");
}

#[test]
fn test_order_by_raw_is_the_explicit_escape_hatch() {
    let query =
        QueryBuilder::<QueryTestUser>::new().order_by_raw("COALESCE(name, '')", Order::Desc);

    query
        .ensure_query_is_valid()
        .expect("order_by_raw() should accept a trusted expression");

    let sql = query.build_select_sql_for_db(DatabaseType::Postgres);
    assert!(
        sql.ends_with("ORDER BY COALESCE(name, '') DESC"),
        "sql: {sql}"
    );
}

#[test]
fn test_order_by_rejects_forged_raw_expression_marker() {
    let err = QueryBuilder::<QueryTestUser>::new()
        .order_by("\u{1}tideorm_raw_order_by\u{1}(SELECT 1)", Order::Asc)
        .ensure_query_is_valid()
        .expect_err("the raw-expression marker must not be forgeable through order_by()");

    assert!(
        err.to_string()
            .contains("raw-expression marker is reserved"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_standalone_offset_renders_a_portable_limit() {
    let query = QueryBuilder::<QueryTestUser>::new().offset(20);

    let postgres_sql = query.build_select_sql_for_db(DatabaseType::Postgres);
    assert!(postgres_sql.ends_with(" OFFSET 20"), "sql: {postgres_sql}");
    assert!(!postgres_sql.contains("LIMIT"), "sql: {postgres_sql}");

    let sqlite_sql = query.build_select_sql_for_db(DatabaseType::SQLite);
    assert!(
        sqlite_sql.ends_with(" LIMIT -1 OFFSET 20"),
        "sql: {sqlite_sql}"
    );

    let mysql_sql = query.build_select_sql_for_db(DatabaseType::MySQL);
    assert!(
        mysql_sql.ends_with(" LIMIT 18446744073709551615 OFFSET 20"),
        "sql: {mysql_sql}"
    );
}

#[test]
fn test_query_validation_splits_select_alias_at_outer_as_only() {
    QueryBuilder::<QueryTestUser>::new()
        .select(vec!["CAST(name AS TEXT) AS display_name"])
        .ensure_query_is_valid()
        .expect("outer SELECT alias must not be confused with an inner CAST(... AS ...)");

    QueryBuilder::<QueryTestUser>::new()
        .select(vec!["CAST(name AS TEXT)"])
        .ensure_query_is_valid()
        .expect("SELECT expression without an outer alias must still validate");

    let err = QueryBuilder::<QueryTestUser>::new()
        .select(vec!["CAST(name AS TEXT) AS bad\"alias"])
        .ensure_query_is_valid()
        .expect_err("unsafe outer alias must still be rejected");
    assert!(err.to_string().contains("unsafe SELECT alias"));
}

#[test]
fn test_window_validation_uses_final_join_qualifiers() {
    let window = WindowFunction::new(
        WindowFunctionType::Sum("profiles.score".to_string()),
        "profile_score_sum",
    )
    .partition_by("profiles.user_id")
    .order_by("profiles.created_at", Order::Asc);

    QueryBuilder::<QueryTestUser>::new()
        .window(window)
        .inner_join("profiles", "query_test_users.id", "profiles.user_id")
        .ensure_query_is_valid()
        .expect("window columns may reference joins added later in the chain");

    QueryBuilder::<QueryTestUser>::new()
        .lag(
            "previous_profile_score",
            "profiles.score",
            1,
            Some("0"),
            "profiles.user_id",
            "profiles.created_at",
            Order::Asc,
        )
        .inner_join("profiles", "query_test_users.id", "profiles.user_id")
        .ensure_query_is_valid()
        .expect("lag columns may reference joins added later in the chain");
}

#[test]
fn test_window_validation_rejects_unknown_qualifier() {
    let window = WindowFunction::new(
        WindowFunctionType::Sum("profiles.score".to_string()),
        "profile_score_sum",
    )
    .partition_by("profiles.user_id")
    .order_by("profiles.created_at", Order::Asc);

    let err = QueryBuilder::<QueryTestUser>::new()
        .window(window)
        .ensure_query_is_valid()
        .expect_err("unknown window column qualifiers should be rejected");

    assert!(
        err.to_string()
            .contains("unknown window PARTITION BY column qualifier 'profiles'")
            || err
                .to_string()
                .contains("unknown window function column qualifier 'profiles'")
    );
}

#[tokio::test]
async fn test_where_in_subquery_rejects_invalid_nested_query_before_db_lookup() {
    let err = QueryTestUser::query()
        .where_in_subquery(
            "id",
            QueryTestUser::query()
                .select(vec!["id"])
                .where_raw("1 = 1; DROP TABLE users"),
        )
        .count()
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("invalid subquery for where_in_subquery()")
    );
}

#[tokio::test]
async fn test_select_subquery_rejects_invalid_nested_query_before_db_lookup() {
    let err = QueryTestUser::query()
        .select_subquery(
            QueryTestUser::query()
                .select(vec!["id"])
                .where_raw("1 = 1; DROP TABLE users"),
            "nested_id",
        )
        .count()
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("invalid subquery for select_subquery()")
    );
}

#[tokio::test]
async fn test_select_subquery_rejects_unsafe_alias_before_db_lookup() {
    let err = QueryTestUser::query()
        .select_subquery(QueryTestUser::query().select(vec!["id"]), "bad\"alias")
        .count()
        .await
        .unwrap_err();

    assert!(err.to_string().contains("unsafe SELECT alias"));
}

#[test]
fn test_select_subquery_uses_backend_specific_alias_quoting() {
    let query = QueryTestUser::query().select_subquery(
        QueryTestUser::query().select(vec!["id"]).limit(1),
        "nested_id",
    );

    let postgres_sql = query.build_select_sql_for_db(DatabaseType::Postgres);
    assert!(postgres_sql.contains("AS \"nested_id\""));

    let mysql_sql = query.build_select_sql_for_db(DatabaseType::MySQL);
    assert!(mysql_sql.contains("AS `nested_id`"));
}

#[tokio::test]
async fn test_union_raw_rejects_invalid_subquery_before_db_lookup() {
    let err = QueryTestUser::query()
        .union_raw("DELETE FROM users")
        .count()
        .await
        .unwrap_err();

    assert!(err.to_string().contains("invalid subquery for union_raw()"));
}

#[tokio::test]
async fn test_union_raw_rejects_top_level_compound_subquery_before_db_lookup() {
    let err = QueryTestUser::query()
        .union_raw("SELECT id FROM query_test_users UNION SELECT password FROM users")
        .count()
        .await
        .unwrap_err();

    assert!(err.to_string().contains("invalid subquery for union_raw()"));
    assert!(
        err.to_string()
            .contains("top-level 'union' queries are not allowed here")
    );
}

#[tokio::test]
async fn test_union_all_raw_rejects_invalid_subquery_before_db_lookup() {
    let err = QueryTestUser::query()
        .union_all_raw("DELETE FROM users")
        .count()
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("invalid subquery for union_all_raw()")
    );
}

#[tokio::test]
async fn test_with_cte_rejects_non_select_sql_before_db_lookup() {
    let err = QueryTestUser::query()
        .with_cte(CTE::new(
            "active_users",
            "DELETE FROM users RETURNING id".to_string(),
        ))
        .count()
        .await
        .unwrap_err();

    assert!(err.to_string().contains("invalid CTE for with_cte()"));
}

#[tokio::test]
async fn test_with_cte_columns_rejects_non_select_sql_before_db_lookup() {
    let err = QueryTestUser::query()
        .with_cte_columns("active_users", vec!["id"], "DELETE FROM users RETURNING id")
        .count()
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("invalid subquery for with_cte_columns()")
    );
}

#[tokio::test]
async fn test_with_recursive_cte_rejects_non_select_sql_before_db_lookup() {
    let err = QueryTestUser::query()
        .with_recursive_cte(
            "user_tree",
            vec!["id"],
            "SELECT id FROM users",
            "DELETE FROM users RETURNING id",
        )
        .count()
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("invalid subquery for with_recursive_cte() recursive query")
    );
}

#[tokio::test]
async fn test_recursive_cte_accepts_compound_query_body_before_db_lookup() {
    let err = QueryTestUser::query()
        .with_cte(CTE::new("user_tree", "SELECT 1 UNION ALL SELECT 2".to_string()).recursive())
        .count()
        .await
        .unwrap_err();

    assert!(!err.to_string().contains("invalid CTE for with_cte()"));
}

#[tokio::test]
async fn test_lag_rejects_unsafe_default_expression_before_db_lookup() {
    let err = QueryTestUser::query()
        .lag(
            "previous_id",
            "id",
            1,
            Some("0; DROP TABLE users"),
            "name",
            "id",
            Order::Asc,
        )
        .count()
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("unsafe LAG/LEAD default expression")
    );
}

#[tokio::test]
async fn test_sum_rejects_grouped_query_before_db_lookup() {
    let err = QueryTestUser::query()
        .group_by("name")
        .sum("id")
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("sum() returns a single scalar and does not support group_by()"),
        "{}",
        err
    );
}

#[tokio::test]
async fn test_count_distinct_rejects_having_before_db_lookup() {
    let err = QueryTestUser::query()
        .having_count_gt(3)
        .count_distinct("name")
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("count_distinct() returns a single scalar and does not support having()"),
        "{}",
        err
    );
}

#[test]
fn test_aggregate_sql_qualifies_columns_and_keeps_joins() {
    let (sql, _params) = QueryTestUser::query()
        .inner_join("profiles", "query_test_users.id", "profiles.user_id")
        .build_aggregate_sql_with_params_for_db(
            DatabaseType::Postgres,
            "profiles.score",
            "agg_result",
            |column: &str| format!("SUM({})", column),
        );

    assert!(sql.contains("SUM(\"profiles\".\"score\")"), "{}", sql);
    assert!(sql.contains("INNER JOIN \"profiles\""), "{}", sql);
}

#[test]
fn test_aggregate_sql_wraps_limited_query_in_derived_table() {
    let (sql, _params) = QueryTestUser::query()
        .limit(10)
        .build_aggregate_sql_with_params_for_db(
            DatabaseType::Postgres,
            "id",
            "agg_result",
            |column: &str| format!("SUM({})", column),
        );

    assert!(
        sql.starts_with("SELECT SUM(\"id\") AS \"agg_result\" FROM ("),
        "{}",
        sql
    );
    assert!(sql.contains("LIMIT 10"), "{}", sql);
    assert!(
        sql.ends_with("AS \"tideorm_aggregate_subquery\""),
        "{}",
        sql
    );
}

#[test]
fn test_having_aggregate_helpers_qualify_table_columns() {
    let sql = QueryTestUser::query()
        .inner_join("profiles", "query_test_users.id", "profiles.user_id")
        .group_by("query_test_users.id")
        .having_sum_gt("profiles.score", 10.0)
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert!(sql.contains("SUM(\"profiles\".\"score\") >"), "{}", sql);
}

#[tokio::test]
async fn test_custom_window_expression_rejects_unsafe_sql_before_db_lookup() {
    let err = QueryTestUser::query()
        .window(WindowFunction::new(
            WindowFunctionType::Custom("SUM(id); DROP TABLE users".to_string()),
            "unsafe_sum",
        ))
        .count()
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("unsafe window function expression")
    );
}
