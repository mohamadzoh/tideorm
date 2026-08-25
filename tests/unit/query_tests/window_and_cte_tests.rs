use super::*;

#[test]
fn test_union_type_sql() {
    assert_eq!(UnionType::Union.as_sql(), "UNION");
    assert_eq!(UnionType::UnionAll.as_sql(), "UNION ALL");
}

#[test]
fn test_union_clause_creation() {
    let clause = UnionClause::new(
        UnionType::Union,
        "SELECT * FROM users WHERE active = true".to_string(),
    );
    assert_eq!(clause.union_type, UnionType::Union);
    assert!(clause.query_sql.contains("active = true"));
    assert!(clause.params.is_empty());
}

#[test]
fn test_union_operand_binds_values_instead_of_inlining_them() {
    let (sql, params) = QueryBuilder::<QueryTestUser>::new()
        .where_eq("name", "alice")
        .union_all(QueryBuilder::<QueryTestUser>::new().where_eq("name", "bob"))
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);

    assert!(sql.contains("WHERE \"name\" = $1"), "sql: {sql}");
    assert!(sql.contains("UNION ALL (SELECT"), "sql: {sql}");
    assert!(sql.contains("WHERE \"name\" = $2)"), "sql: {sql}");
    assert!(!sql.contains("'bob'"), "sql: {sql}");
    assert_eq!(params.len(), 2);
    assert!(matches!(params.first(), Some(Value::String(Some(value))) if value == "alice"));
    assert!(matches!(params.get(1), Some(Value::String(Some(value))) if value == "bob"));
}

#[test]
fn test_cte_body_binds_values_instead_of_inlining_them() {
    let (sql, params) = QueryBuilder::<QueryTestUser>::new()
        .with_query(
            "recent_users",
            QueryBuilder::<QueryTestUser>::new().where_eq("name", "carol"),
        )
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);

    assert!(
        sql.starts_with("WITH \"recent_users\" AS (SELECT"),
        "sql: {sql}"
    );
    assert!(sql.contains("WHERE \"name\" = $1)"), "sql: {sql}");
    assert!(!sql.contains("'carol'"), "sql: {sql}");
    assert_eq!(params.len(), 1);
    assert!(matches!(params.first(), Some(Value::String(Some(value))) if value == "carol"));
}

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

    let cte_placeholder = sql
        .find("$1")
        .expect("CTE body should bind the first value");
    let base_placeholder = sql
        .find("$2")
        .expect("base select should bind the second value");
    let union_placeholder = sql
        .find("$3")
        .expect("union operand should bind the third value");

    // The WITH prefix precedes the base select, which precedes every union
    // operand, so the placeholder numbers have to ascend in that same order.
    assert!(
        sql.starts_with("WITH \"recent_users\" AS (SELECT"),
        "sql: {sql}"
    );
    assert!(cte_placeholder < base_placeholder, "sql: {sql}");
    assert!(base_placeholder < union_placeholder, "sql: {sql}");
    assert!(!sql.contains("$4"), "sql: {sql}");

    assert_eq!(params.len(), 3);
    assert!(matches!(params.first(), Some(Value::String(Some(value))) if value == "cte-value"));
    assert!(matches!(params.get(1), Some(Value::String(Some(value))) if value == "base-value"));
    assert!(matches!(params.get(2), Some(Value::String(Some(value))) if value == "union-value"));
}

#[test]
fn test_positional_placeholder_compound_select_binds_values_left_to_right() {
    // MySQL and SQLite operands carry bare `?` markers that are never renumbered,
    // so only the order values are pushed keeps them matched to their markers.
    let bind = |value: &str| crate::internal::json_to_db_value(&serde_json::json!(value));

    let mut query = QueryBuilder::<QueryTestUser>::new().where_eq("name", "base-value");
    query.ctes.push(CTE::with_params(
        "recent_users",
        "SELECT `id` FROM `query_test_users` WHERE `name` = ?".to_string(),
        vec![bind("cte-value")],
    ));
    query.unions.push(UnionClause::with_params(
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
fn test_frame_bound_sql() {
    assert_eq!(
        FrameBound::UnboundedPreceding.as_sql(),
        "UNBOUNDED PRECEDING"
    );
    assert_eq!(
        FrameBound::UnboundedFollowing.as_sql(),
        "UNBOUNDED FOLLOWING"
    );
    assert_eq!(FrameBound::CurrentRow.as_sql(), "CURRENT ROW");
    assert_eq!(FrameBound::Preceding(5).as_sql(), "5 PRECEDING");
    assert_eq!(FrameBound::Following(3).as_sql(), "3 FOLLOWING");
}

#[test]
fn test_frame_type_sql() {
    assert_eq!(FrameType::Rows.as_sql(), "ROWS");
    assert_eq!(FrameType::Range.as_sql(), "RANGE");
    assert_eq!(FrameType::Groups.as_sql(), "GROUPS");
}

#[test]
fn test_window_function_type_row_number() {
    assert_eq!(WindowFunctionType::RowNumber.as_sql(), "ROW_NUMBER()");
}

#[test]
fn test_window_function_type_rank() {
    assert_eq!(WindowFunctionType::Rank.as_sql(), "RANK()");
}

#[test]
fn test_window_function_type_dense_rank() {
    assert_eq!(WindowFunctionType::DenseRank.as_sql(), "DENSE_RANK()");
}

#[test]
fn test_window_function_type_ntile() {
    assert_eq!(WindowFunctionType::Ntile(4).as_sql(), "NTILE(4)");
}

#[test]
fn test_window_function_type_lag() {
    let sql = WindowFunctionType::Lag("price".to_string(), Some(1), Some("0".to_string())).as_sql();
    assert!(sql.contains("LAG"));
    assert!(sql.contains("\"price\""));
    assert!(sql.contains("1"));
}

#[test]
fn test_window_function_type_lead() {
    let sql = WindowFunctionType::Lead("date".to_string(), Some(1), None).as_sql();
    assert!(sql.contains("LEAD"));
    assert!(sql.contains("\"date\""));
}

#[test]
fn test_window_function_type_first_value() {
    assert_eq!(
        WindowFunctionType::FirstValue("amount".to_string()).as_sql(),
        "FIRST_VALUE(\"amount\")"
    );
}

#[test]
fn test_window_function_type_last_value() {
    assert_eq!(
        WindowFunctionType::LastValue("total".to_string()).as_sql(),
        "LAST_VALUE(\"total\")"
    );
}

#[test]
fn test_window_function_type_sum() {
    assert_eq!(
        WindowFunctionType::Sum("amount".to_string()).as_sql(),
        "SUM(\"amount\")"
    );
}

#[test]
fn test_window_function_type_count() {
    assert_eq!(WindowFunctionType::Count(None).as_sql(), "COUNT(*)");
    assert_eq!(
        WindowFunctionType::Count(Some("id".to_string())).as_sql(),
        "COUNT(\"id\")"
    );
}

#[test]
fn test_window_function_basic() {
    let sql = WindowFunction::new(WindowFunctionType::RowNumber, "row_num").to_sql();
    assert!(sql.contains("ROW_NUMBER()"));
    assert!(sql.contains("OVER"));
    assert!(sql.contains("AS \"row_num\""));
}

#[test]
fn test_window_function_with_partition() {
    let sql = WindowFunction::new(WindowFunctionType::RowNumber, "row_num")
        .partition_by("category")
        .to_sql();
    assert!(sql.contains("PARTITION BY \"category\""));
}

#[test]
fn test_window_function_with_order() {
    let sql = WindowFunction::new(WindowFunctionType::Rank, "rank")
        .order_by("score", Order::Desc)
        .to_sql();
    assert!(sql.contains("ORDER BY \"score\" DESC"));
}

#[test]
fn test_window_function_with_frame() {
    let sql = WindowFunction::new(
        WindowFunctionType::Sum("amount".to_string()),
        "running_total",
    )
    .order_by("date", Order::Asc)
    .frame(
        FrameType::Rows,
        FrameBound::UnboundedPreceding,
        FrameBound::CurrentRow,
    )
    .to_sql();
    assert!(sql.contains("ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW"));
}

#[test]
fn test_window_function_full() {
    let sql = WindowFunction::new(WindowFunctionType::Sum("sales".to_string()), "total_sales")
        .partition_by("region")
        .order_by("month", Order::Asc)
        .frame(
            FrameType::Range,
            FrameBound::UnboundedPreceding,
            FrameBound::CurrentRow,
        )
        .to_sql();
    assert!(sql.contains("SUM(\"sales\")"));
    assert!(sql.contains("PARTITION BY \"region\""));
    assert!(sql.contains("ORDER BY \"month\" ASC"));
    assert!(sql.contains("RANGE BETWEEN"));
    assert!(sql.contains("AS \"total_sales\""));
}

#[test]
fn test_cte_basic() {
    let cte = CTE::new(
        "active_users",
        "SELECT * FROM users WHERE active = true".to_string(),
    );
    let sql = cte.to_sql();
    assert!(sql.contains("\"active_users\""));
    assert!(sql.contains("AS ("));
    assert!(sql.contains("active = true"));
}

#[test]
fn test_cte_with_columns() {
    let cte = CTE::with_columns(
        "user_stats",
        vec!["user_id", "total", "count"],
        "SELECT user_id, SUM(amount), COUNT(*) FROM orders GROUP BY user_id".to_string(),
    );
    let sql = cte.to_sql();
    assert!(sql.contains("\"user_stats\""));
    assert!(sql.contains("(\"user_id\", \"total\", \"count\")"));
    assert!(sql.contains("GROUP BY"));
}

#[test]
fn test_cte_recursive() {
    let cte = CTE::new("tree", "SELECT 1 UNION ALL SELECT 2".to_string()).recursive();
    assert!(cte.recursive);
}

#[test]
fn test_cte_name_quoting() {
    let cte = CTE::new("my_cte", "SELECT 1".to_string());
    let sql = cte.to_sql();
    assert!(sql.starts_with("\"my_cte\""));
}
