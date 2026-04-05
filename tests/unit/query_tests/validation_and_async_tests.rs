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
        .group_by("DATE(created_at)")
        .order_by("LOWER(name)", Order::Asc)
        .ensure_query_is_valid()
        .expect("safe select/group/order expressions should remain allowed");
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
