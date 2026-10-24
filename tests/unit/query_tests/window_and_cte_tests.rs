use super::*;

/// The WITH prefix precedes the base select, which precedes every union
/// operand, so PostgreSQL's numbered placeholders have to ascend in that same
/// order, each operand's values bound rather than inlined.
#[test]
fn test_postgres_compound_select_renumbers_placeholders_left_to_right() {
    let (sql, params) = QueryBuilder::<QueryTestUser>::new()
        .with_query(
            "recent_users",
            QueryBuilder::<QueryTestUser>::new().where_eq("name", "cte-value"),
        )
        .where_eq("name", "base-value")
        .union_all(QueryBuilder::<QueryTestUser>::new().where_eq("name", "union-value"))
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);

    let columns = "\"query_test_users\".\"id\", \"query_test_users\".\"name\"";
    assert_eq!(
        sql,
        format!(
            "WITH \"recent_users\" AS (SELECT {columns} FROM \"query_test_users\" WHERE \"name\" = $1) \
             SELECT {columns} FROM \"query_test_users\" WHERE \"name\" = $2 \
             UNION ALL (SELECT {columns} FROM \"query_test_users\" WHERE \"name\" = $3)"
        )
    );
    assert_eq!(
        params,
        vec![
            Value::String(Some("cte-value".to_string())),
            Value::String(Some("base-value".to_string())),
            Value::String(Some("union-value".to_string())),
        ]
    );
}

#[test]
fn test_positional_placeholder_compound_select_binds_values_left_to_right() {
    // MySQL and SQLite operands carry bare `?` markers that are never renumbered,
    // so only the order values are pushed keeps them matched to their markers.
    let bind = |value: &str| crate::internal::json_to_db_value(&serde_json::json!(value));

    let mut query = QueryBuilder::<QueryTestUser>::new().where_eq("name", "base-value");
    query.clauses.ctes.push(CTE::with_params(
        "recent_users",
        "SELECT `id` FROM `query_test_users` WHERE `name` = ?".to_string(),
        vec![bind("cte-value")],
    ));
    query.clauses.unions.push(UnionClause::with_params(
        UnionType::UnionAll,
        "SELECT `id` FROM `query_test_users` WHERE `name` = ?".to_string(),
        vec![bind("union-value")],
    ));

    let (sql, params) = query.build_select_sql_with_params_for_db(DatabaseType::MySQL);

    assert_eq!(sql.matches('?').count(), 3, "sql: {sql}");
    // MySQL quotes the CTE name with backticks, not double quotes.
    assert!(
        sql.starts_with("WITH `recent_users` AS (SELECT"),
        "sql: {sql}"
    );
    assert_eq!(
        params,
        vec![bind("cte-value"), bind("base-value"), bind("union-value")]
    );
}

#[test]
fn test_compound_operand_parenthesization_is_dialect_specific() {
    let build = |db_type| {
        QueryBuilder::<QueryTestUser>::new()
            .where_eq("name", "alice")
            .union_all_raw("SELECT id FROM query_test_users")
            .build_select_sql_with_params_for_db(db_type)
            .0
    };

    let postgres_sql = build(DatabaseType::Postgres);
    let mysql_sql = build(DatabaseType::MySQL);
    let sqlite_sql = build(DatabaseType::SQLite);

    assert!(
        postgres_sql.contains("UNION ALL (SELECT id FROM query_test_users)"),
        "sql: {postgres_sql}"
    );
    assert!(
        mysql_sql.contains("UNION ALL (SELECT id FROM query_test_users)"),
        "sql: {mysql_sql}"
    );
    // SQLite's compound-select grammar rejects a parenthesized select-core.
    assert!(
        sqlite_sql.contains("UNION ALL SELECT id FROM query_test_users"),
        "sql: {sqlite_sql}"
    );
    assert!(!sqlite_sql.contains("UNION ALL ("), "sql: {sqlite_sql}");
}

#[test]
fn test_union_operand_keeps_comment_like_values_out_of_the_sql() {
    let query = QueryBuilder::<QueryTestUser>::new().union_all(
        QueryBuilder::<QueryTestUser>::new().where_eq("name", "rob'; DROP TABLE users -- now"),
    );

    query
        .ensure_query_is_valid()
        .expect("a bound operand value must never be scanned as raw SQL");

    let (sql, params) = query.build_select_sql_with_params_for_db(DatabaseType::Postgres);

    assert!(!sql.contains("DROP TABLE"), "sql: {sql}");
    assert!(!sql.contains("--"), "sql: {sql}");
    assert!(
        matches!(params.first(), Some(Value::String(Some(value))) if value == "rob'; DROP TABLE users -- now")
    );
}

#[test]
fn test_frame_clauses_render_their_keywords() {
    for (bound, expected) in [
        (FrameBound::UnboundedPreceding, "UNBOUNDED PRECEDING"),
        (FrameBound::UnboundedFollowing, "UNBOUNDED FOLLOWING"),
        (FrameBound::CurrentRow, "CURRENT ROW"),
        (FrameBound::Preceding(5), "5 PRECEDING"),
        (FrameBound::Following(3), "3 FOLLOWING"),
    ] {
        assert_eq!(bound.as_sql(), expected);
    }
    for (frame, expected) in [
        (FrameType::Rows, "ROWS"),
        (FrameType::Range, "RANGE"),
        (FrameType::Groups, "GROUPS"),
    ] {
        assert_eq!(frame.as_sql(), expected);
    }
}

#[test]
fn test_window_function_types_render_their_calls() {
    for (function, expected) in [
        (WindowFunctionType::RowNumber, "ROW_NUMBER()"),
        (WindowFunctionType::Rank, "RANK()"),
        (WindowFunctionType::DenseRank, "DENSE_RANK()"),
        (WindowFunctionType::Ntile(4), "NTILE(4)"),
        (
            WindowFunctionType::Lag("price".to_string(), Some(1), Some("0".to_string())),
            "LAG(\"price\", 1, 0)",
        ),
        // SQL cannot pass a default without an offset, so it is dropped.
        (
            WindowFunctionType::Lag("price".to_string(), None, Some("0".to_string())),
            "LAG(\"price\")",
        ),
        (
            WindowFunctionType::Lead("date".to_string(), Some(1), None),
            "LEAD(\"date\", 1)",
        ),
        (
            WindowFunctionType::FirstValue("amount".to_string()),
            "FIRST_VALUE(\"amount\")",
        ),
        (
            WindowFunctionType::LastValue("total".to_string()),
            "LAST_VALUE(\"total\")",
        ),
        (
            WindowFunctionType::NthValue("total".to_string(), 2),
            "NTH_VALUE(\"total\", 2)",
        ),
        (
            WindowFunctionType::Sum("amount".to_string()),
            "SUM(\"amount\")",
        ),
        (
            WindowFunctionType::Avg("amount".to_string()),
            "AVG(\"amount\")",
        ),
        (WindowFunctionType::Count(None), "COUNT(*)"),
        (
            WindowFunctionType::Count(Some("id".to_string())),
            "COUNT(\"id\")",
        ),
        (
            WindowFunctionType::Min("amount".to_string()),
            "MIN(\"amount\")",
        ),
        (
            WindowFunctionType::Max("amount".to_string()),
            "MAX(\"amount\")",
        ),
    ] {
        assert_eq!(function.as_sql_for_db(DatabaseType::Postgres), expected);
    }
}

#[test]
fn test_window_function_renders_its_over_clause() {
    for (window, expected) in [
        (
            WindowFunction::new(WindowFunctionType::RowNumber, "row_num"),
            "ROW_NUMBER() OVER () AS \"row_num\"",
        ),
        (
            WindowFunction::new(
                WindowFunctionType::Sum("amount".to_string()),
                "running_total",
            )
            .order_by("date", Order::Asc)
            .frame(
                FrameType::Rows,
                FrameBound::UnboundedPreceding,
                FrameBound::CurrentRow,
            ),
            "SUM(\"amount\") OVER (ORDER BY \"date\" ASC ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS \"running_total\"",
        ),
        (
            WindowFunction::new(WindowFunctionType::Sum("sales".to_string()), "total_sales")
                .partition_by("region")
                .order_by("month", Order::Asc)
                .frame(
                    FrameType::Range,
                    FrameBound::UnboundedPreceding,
                    FrameBound::CurrentRow,
                ),
            "SUM(\"sales\") OVER (PARTITION BY \"region\" ORDER BY \"month\" ASC RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS \"total_sales\"",
        ),
    ] {
        assert_eq!(window.to_sql_for_db(DatabaseType::Postgres), expected);
    }
}

#[test]
fn test_cte_sql_uses_backend_identifier_quoting() {
    let body = "SELECT id, name FROM query_test_users";

    for (db_type, expected) in [
        (
            DatabaseType::Postgres,
            "\"recent\" (\"id\", \"name\") AS (SELECT id, name FROM query_test_users)",
        ),
        (
            DatabaseType::MySQL,
            "`recent` (`id`, `name`) AS (SELECT id, name FROM query_test_users)",
        ),
    ] {
        let cte = CTE::with_columns("recent", vec!["id", "name"], body.to_string());
        assert_eq!(cte.to_sql_for_db(db_type), expected);
    }
    assert_eq!(
        CTE::new("recent", body.to_string()).to_sql_for_db(DatabaseType::Postgres),
        "\"recent\" AS (SELECT id, name FROM query_test_users)"
    );
}

/// One recursive CTE makes the statement's whole prefix `WITH RECURSIVE`.
#[test]
fn a_recursive_cte_promotes_the_with_keyword() {
    let query = QueryTestUser::query().with_recursive_cte(
        "numbers",
        vec!["n"],
        "SELECT 1",
        "SELECT n + 1 FROM numbers WHERE n < 3",
    );
    query
        .validate()
        .expect("a recursive body is a top-level UNION ALL");

    let sql = query.build_select_sql_for_db(DatabaseType::Postgres);
    assert!(
        sql.starts_with(
            "WITH RECURSIVE \"numbers\" (\"n\") AS (SELECT 1 UNION ALL SELECT n + 1 FROM numbers WHERE n < 3) SELECT"
        ),
        "{sql}"
    );

    let plain = QueryTestUser::query()
        .with_cte(CTE::new("one", "SELECT 1".to_string()))
        .build_select_sql_for_db(DatabaseType::Postgres);
    assert!(
        plain.starts_with("WITH \"one\" AS (SELECT 1) SELECT"),
        "{plain}"
    );
}

#[test]
fn a_cte_body_keeps_its_ordering_and_limit() {
    let (sql, params) = QueryBuilder::<QueryTestUser>::new()
        .with_query(
            "top_users",
            QueryBuilder::<QueryTestUser>::new()
                .order_desc("id")
                .limit(10)
                .offset(20),
        )
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);

    assert!(
        sql.contains("ORDER BY \"id\" DESC LIMIT 10 OFFSET $1)"),
        "sql: {sql}"
    );
    assert_eq!(params, vec![Value::BigInt(Some(20))]);
}

#[test]
fn a_union_operand_keeps_its_ordering_and_limit() {
    let operand = || {
        QueryBuilder::<QueryTestUser>::new()
            .order_desc("id")
            .limit(10)
    };

    let (sql, _) = QueryBuilder::<QueryTestUser>::new()
        .union(operand())
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);
    assert!(
        sql.contains("UNION (SELECT") && sql.ends_with("DESC LIMIT 10)"),
        "sql: {sql}"
    );

    // SQLite takes no parenthesized operand, so a limited one is read through
    // a derived table. (An operand renders for the connected backend, and
    // this test has none, so the operand is asked for SQLite directly.)
    let (sql, _) = operand().build_compound_operand_sql_for_db(DatabaseType::SQLite);
    assert!(
        sql.starts_with("SELECT * FROM (SELECT")
            && sql.ends_with("DESC LIMIT 10) AS \"tideorm_union_operand\""),
        "sql: {sql}"
    );
}

#[test]
fn a_lag_default_is_chosen_by_a_case_on_the_mysql_family() {
    let window = WindowFunction::new(
        WindowFunctionType::Lag("amount".to_string(), Some(1), Some("0".to_string())),
        "prev",
    )
    .partition_by("region")
    .order_by("id", Order::Asc);

    assert_eq!(
        window.to_sql_for_db(DatabaseType::Postgres),
        r#"LAG("amount", 1, 0) OVER (PARTITION BY "region" ORDER BY "id" ASC) AS "prev""#
    );
    // MariaDB's LAG takes no default.
    for db_type in [DatabaseType::MySQL, DatabaseType::MariaDB] {
        assert_eq!(
            window.to_sql_for_db(db_type),
            "CASE WHEN LAG(1, 1) OVER (PARTITION BY `region` ORDER BY `id` ASC) IS NULL THEN 0 \
             ELSE LAG(`amount`, 1) OVER (PARTITION BY `region` ORDER BY `id` ASC) END AS `prev`"
        );
    }

    let lead = WindowFunction::new(
        WindowFunctionType::Lead("amount".to_string(), Some(2), None),
        "next",
    );
    assert_eq!(
        lead.to_sql_for_db(DatabaseType::MariaDB),
        "LEAD(`amount`, 2) OVER () AS `next`"
    );
}
