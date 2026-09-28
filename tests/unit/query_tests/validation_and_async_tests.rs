use super::*;

/// An unsafe raw fragment is refused when the query runs, before it reaches a
/// database: none is connected here.
#[tokio::test]
async fn unsafe_sql_is_rejected_before_db_lookup() {
    type Shape = fn(QueryBuilder<QueryTestUser>) -> QueryBuilder<QueryTestUser>;
    let cases: [(Shape, &str); 8] = [
        (
            |q| q.where_raw("1 = 1; DROP TABLE users"),
            "unsafe WHERE raw SQL",
        ),
        (
            |q| {
                q.begin_or()
                    .or_where_raw("1 = 1; DROP TABLE users")
                    .end_or()
            },
            "unsafe WHERE raw SQL",
        ),
        (
            |q| q.group_by("id").having("COUNT(*) > 0; DROP TABLE users"),
            "unsafe HAVING raw SQL",
        ),
        (
            |q| {
                q.group_by("id")
                    .having("1 = 1 OR (SELECT password FROM users LIMIT 1)::text = 'x'")
            },
            "unsafe HAVING raw SQL",
        ),
        (
            |q| q.select_raw("id; DROP TABLE users"),
            "unsafe SELECT raw SQL",
        ),
        (
            |q| q.select(vec!["COUNT(*); DROP TABLE users"]),
            "unsafe SELECT column",
        ),
        (
            |q| q.order_by("random(); DROP TABLE users", Order::Asc),
            "unsafe ORDER BY column",
        ),
        (
            |q| q.group_by("DATE_TRUNC('day', created_at); DROP TABLE users"),
            "unsafe GROUP BY column",
        ),
    ];

    for (shape, expected) in cases {
        let err = shape(QueryTestUser::query()).count().await.unwrap_err();
        assert!(err.to_string().contains(expected), "{expected}: {err}");
    }
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

#[test]
fn test_query_validation_allows_safe_expression_slots() {
    QueryBuilder::<QueryTestUser>::new()
        .select_raw("COUNT(*) AS total")
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
fn test_order_by_accepts_an_inline_direction_that_agrees() {
    let sql = QueryBuilder::<QueryTestUser>::new()
        .order_by("name DESC", Order::Desc)
        .build_select_sql_for_db(DatabaseType::Postgres);
    assert!(sql.ends_with("ORDER BY \"name\" DESC"), "sql: {sql}");

    let conflicting = QueryBuilder::<QueryTestUser>::new().order_by("name DESC", Order::Asc);
    let error = conflicting
        .validate()
        .expect_err("two directions for one column are refused");
    assert!(
        error.to_string().contains("names two directions"),
        "{error}"
    );
}

#[test]
fn test_sort_reads_a_request_sort_string() {
    let sql = QueryBuilder::<QueryTestUser>::new()
        .sort("-id, name, id asc,+name")
        .build_select_sql_for_db(DatabaseType::Postgres);
    assert!(
        sql.ends_with("ORDER BY \"id\" DESC, \"name\" ASC, \"id\" ASC, \"name\" ASC"),
        "sql: {sql}"
    );

    assert!(
        QueryBuilder::<QueryTestUser>::new()
            .sort("")
            .validate()
            .is_ok()
    );
    for unsafe_spec in ["name; DROP TABLE users", "-", "LOWER(name)"] {
        assert!(
            QueryBuilder::<QueryTestUser>::new()
                .sort(unsafe_spec)
                .validate()
                .is_err(),
            "{unsafe_spec}"
        );
    }
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
fn test_select_takes_columns_and_aliases_but_not_expressions() {
    QueryBuilder::<QueryTestUser>::new()
        .select(vec!["name AS display_name", "query_test_users.id", "*"])
        .ensure_query_is_valid()
        .expect("columns, qualified columns, aliases and * are the typed projection");

    for expression in [
        "CAST(name AS TEXT) AS display_name",
        "COUNT(*)",
        "(SELECT name FROM query_test_users LIMIT 1) AS leaked",
    ] {
        let err = QueryBuilder::<QueryTestUser>::new()
            .select(vec![expression])
            .ensure_query_is_valid()
            .expect_err("select() must not accept SQL expressions");
        assert!(
            err.to_string().contains("unsafe SELECT column"),
            "{expression}: {err}"
        );
    }

    let err = QueryBuilder::<QueryTestUser>::new()
        .select(vec!["name AS bad\"alias"])
        .ensure_query_is_valid()
        .expect_err("unsafe outer alias must still be rejected");
    assert!(err.to_string().contains("unsafe SELECT alias"));
}

const HOSTILE_COLUMNS: [&str; 6] = [
    "1=1 OR name",
    "name) OR 1=1 --",
    "(SELECT 1)",
    "name AS alias",
    "\"name\"",
    "lower(name)",
];

#[test]
fn test_where_columns_reject_sql_expressions() {
    for column in HOSTILE_COLUMNS {
        let queries = [
            QueryBuilder::<QueryTestUser>::new().where_eq(column, "x"),
            QueryBuilder::<QueryTestUser>::new().where_null(column),
            QueryBuilder::<QueryTestUser>::new().where_in(column, vec![1]),
            QueryBuilder::<QueryTestUser>::new().or_where_eq(column, "x"),
            QueryBuilder::<QueryTestUser>::new()
                .begin_or()
                .or_where_eq("name", "a")
                .and_where_eq(column, "x")
                .end_or(),
        ];
        for query in queries {
            let err = query
                .ensure_query_is_valid()
                .expect_err("a WHERE column must be a column reference");
            assert!(
                err.to_string().contains("unsafe WHERE column"),
                "{column}: {err}"
            );
        }
    }
}

#[tokio::test]
async fn test_batch_update_validates_its_where_columns_before_db_lookup() {
    for column in HOSTILE_COLUMNS {
        let err = QueryTestUser::update_all()
            .set("name", "x")
            .where_eq(column, 0)
            .execute()
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("unsafe WHERE column"),
            "{column}: {err}"
        );
    }
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

/// An unselected `Option` column decodes as `None`, and saving such a model
/// writes it back, so `get()` only takes a projection covering the model.
#[tokio::test]
async fn get_refuses_a_projection_that_leaves_model_columns_out() {
    let err = QueryTestUser::query()
        .select(vec!["id"])
        .get()
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("select() leaves out name"),
        "{err}"
    );

    // Covering projections get past the check and fail only for want of a
    // connection.
    for projection in [
        vec!["*"],
        vec!["query_test_users.*"],
        vec!["id", "query_test_users.name"],
        vec!["id", "name AS name"],
    ] {
        let err = QueryTestUser::query()
            .select(projection.clone())
            .get()
            .await
            .unwrap_err();
        assert!(
            !err.to_string().contains("leaves out"),
            "{projection:?}: {err}"
        );
    }
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
        .sum::<i64>("id")
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
        .having(crate::Aggregate::count().gt(3))
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
        )
        .expect("aggregate sql");

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
        )
        .expect("aggregate sql");

    assert_eq!(
        sql,
        "SELECT SUM(\"tideorm_aggregate_input_0\") AS \"agg_result\" FROM (SELECT \"id\" AS \"tideorm_aggregate_input_0\" FROM \"query_test_users\" LIMIT 10) AS \"tideorm_aggregate_subquery\""
    );
}

#[test]
fn test_a_limited_aggregate_reads_a_joined_column_from_its_table() {
    let (sql, _params) = QueryTestUser::query()
        .inner_join("profiles", "query_test_users.id", "profiles.user_id")
        .limit(10)
        .build_aggregate_sql_with_params_for_db(
            DatabaseType::Postgres,
            "profiles.score",
            "agg_result",
            |column: &str| format!("SUM({})", column),
        )
        .expect("aggregate sql");

    assert!(
        sql.contains("(SELECT \"profiles\".\"score\" AS \"tideorm_aggregate_input_0\" FROM"),
        "{sql}"
    );

    let refused = QueryTestUser::query()
        .inner_join("profiles", "query_test_users.id", "profiles.user_id")
        .distinct()
        .build_aggregate_sql_with_params_for_db(
            DatabaseType::Postgres,
            "profiles.score",
            "agg_result",
            |column: &str| format!("SUM({})", column),
        )
        .expect_err("a distinct query does not select profiles.score");
    assert!(
        refused.to_string().contains("'profiles.score'"),
        "{refused}"
    );
}

#[test]
fn test_a_joined_query_qualifies_its_model_columns() {
    let (sql, _params) = QueryTestUser::query()
        .inner_join("profiles", "query_test_users.id", "profiles.user_id")
        .build_aggregate_sql_with_params_for_db(
            DatabaseType::Postgres,
            "id",
            "agg_result",
            |column: &str| format!("COUNT(DISTINCT {})", column),
        )
        .expect("aggregate sql");
    assert!(
        sql.starts_with("SELECT COUNT(DISTINCT \"query_test_users\".\"id\")"),
        "{sql}"
    );

    let (where_sql, _) = QueryTestUser::query()
        .inner_join("profiles", "query_test_users.id", "profiles.user_id")
        .where_eq("name", "a")
        .build_where_clause_with_condition_for_db(DatabaseType::Postgres);
    assert_eq!(where_sql, "\"query_test_users\".\"name\" = $1");
}

#[test]
fn test_having_aggregate_helpers_qualify_table_columns() {
    let sql = QueryTestUser::query()
        .inner_join("profiles", "query_test_users.id", "profiles.user_id")
        .group_by("query_test_users.id")
        .having(crate::Aggregate::sum("profiles.score").gt(10.0))
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert!(
        sql.contains("HAVING COALESCE(SUM(\"profiles\".\"score\"), 0) > 10"),
        "{}",
        sql
    );
}

#[test]
fn test_having_takes_typed_aggregate_comparisons_and_raw_sql() {
    let (sql, params) = QueryTestUser::query()
        .group_by("name")
        .having(crate::Aggregate::count().gte(2))
        .having(crate::Aggregate::max("id").lt(100))
        .having("COUNT(*) < 50")
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);

    assert!(
        sql.contains(
            "HAVING COUNT(*) >= $1 AND MAX(\"query_test_users\".\"id\") < $2 AND COUNT(*) < 50"
        ),
        "{sql}"
    );
    assert_eq!(params.len(), 2);
}

#[test]
fn test_where_raw_with_binds_its_values_per_backend() {
    let query = || {
        QueryTestUser::query()
            .where_eq("id", 1)
            .where_raw_with("LOWER(name) = LOWER(?) AND name <> '?'", vec!["Ann".into()])
    };

    let (sql, params) = query().build_select_sql_with_params_for_db(DatabaseType::Postgres);
    assert!(
        sql.contains("LOWER(name) = LOWER($2) AND name <> '?'"),
        "{sql}"
    );
    assert_eq!(params.len(), 2);

    let (sql, _) = query().build_select_sql_with_params_for_db(DatabaseType::MySQL);
    assert!(
        sql.contains("LOWER(name) = LOWER(?) AND name <> '?'"),
        "{sql}"
    );

    let mismatched = QueryTestUser::query().where_raw_with("id = ? OR id = ?", vec![1.into()]);
    assert!(mismatched.ensure_query_is_executable().is_err());
}

#[test]
fn test_validate_and_debug_report_why_a_query_would_fail() {
    let valid = QueryTestUser::query()
        .where_eq("name", "a")
        .or_where_eq("id", 1)
        .or_where_eq("id", 2);
    assert!(valid.validate().is_ok());
    let info = valid.debug();
    assert_eq!(info.error, None);
    assert!(
        info.conditions
            .iter()
            .any(|condition| condition.contains(" OR ")),
        "the OR group is listed: {:?}",
        info.conditions
    );

    let invalid = QueryTestUser::query().order_by("name; DROP TABLE users", Order::Asc);
    let error = invalid.validate().expect_err("an unsafe order is refused");
    let info = invalid.debug();
    assert_eq!(info.error.as_deref(), Some(error.to_string().as_str()));
    assert!(info.to_string().contains("Invalid:"), "{info}");
}

#[test]
fn test_a_cte_body_may_hold_a_union() {
    let query = QueryTestUser::query()
        .with_cte(CTE::new(
            "names",
            "SELECT name FROM query_test_users UNION SELECT 'guest'".to_string(),
        ))
        .where_raw("name IN (SELECT name FROM names)");
    query
        .validate()
        .expect("a UNION inside a CTE's parentheses stays inside it");
}

#[test]
fn test_where_raw_with_counts_no_placeholder_inside_brackets() {
    // The engine reads `[..]` as quoted and would never bind a `?` there.
    assert_eq!(
        db_sql::count_template_placeholders("tags && ARRAY[?] AND id = ?"),
        1
    );
    let query = QueryTestUser::query().where_raw_with("name = ANY(ARRAY[?])", vec!["a".into()]);
    assert!(query.validate().is_err());
}

#[test]
fn test_a_mismatched_raw_template_renders_without_panicking() {
    let query = QueryTestUser::query().where_raw_with("id IN (?, ?)", vec![1.into()]);
    assert!(query.validate().is_err());
    // Rendering paths that run before validation must not panic.
    let info = query.debug();
    assert!(info.error.is_some());
    let _ = query.build_select_sql_with_params_for_db(DatabaseType::Postgres);
    let _ = QueryTestUser::query()
        .where_in_subquery("id", query)
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);
}

#[test]
fn test_reorder_replaces_every_earlier_order() {
    let sql = QueryTestUser::query()
        .order_desc("id")
        .order_asc("name")
        .reorder("name", Order::Desc)
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert!(sql.ends_with("ORDER BY \"name\" DESC"), "{sql}");
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
