use super::db_sql;
use super::{
    CTE, FrameBound, FrameType, Order, QueryBuilder, UnionClause, UnionType, WindowFunction,
    WindowFunctionType,
};
use crate::config::DatabaseType;
#[cfg(feature = "fulltext")]
use crate::fulltext::{FullTextSearchBuilder, SearchMode};
#[cfg(feature = "fulltext")]
use crate::internal::Value;

#[derive(tideorm::Model)]
#[tideorm(table = "query_test_users")]
struct QueryTestUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[test]
fn test_quote_char() {
    assert_eq!(db_sql::quote_char(DatabaseType::Postgres), '"');
    assert_eq!(db_sql::quote_char(DatabaseType::MySQL), '`');
    assert_eq!(db_sql::quote_char(DatabaseType::MariaDB), '`');
    assert_eq!(db_sql::quote_char(DatabaseType::SQLite), '"');
}

#[test]
fn test_quote_ident() {
    assert_eq!(
        db_sql::quote_ident(DatabaseType::Postgres, "column"),
        "\"column\""
    );
    assert_eq!(
        db_sql::quote_ident(DatabaseType::MySQL, "column"),
        "`column`"
    );
    assert_eq!(
        db_sql::quote_ident(DatabaseType::MariaDB, "column"),
        "`column`"
    );
    assert_eq!(
        db_sql::quote_ident(DatabaseType::SQLite, "column"),
        "\"column\""
    );
    assert_eq!(
        db_sql::quote_ident(DatabaseType::Postgres, "col\"umn"),
        "\"col\"\"umn\""
    );
    assert_eq!(
        db_sql::quote_ident(DatabaseType::MySQL, "col`umn"),
        "`col``umn`"
    );
}

#[test]
fn test_json_contains_postgres() {
    let sql = db_sql::json_contains(DatabaseType::Postgres, "metadata", r#"{"key": "value"}"#);
    assert!(sql.contains("@>"));
    assert!(sql.contains("\"metadata\""));
}

#[test]
fn test_json_contains_mysql() {
    let sql = db_sql::json_contains(DatabaseType::MySQL, "metadata", r#"{"key": "value"}"#);
    assert!(sql.contains("JSON_CONTAINS"));
    assert!(sql.contains("`metadata`"));

    let sql = db_sql::json_contains(DatabaseType::MariaDB, "metadata", r#"{"key": "value"}"#);
    assert!(sql.contains("JSON_CONTAINS"));
    assert!(sql.contains("`metadata`"));
}

#[test]
fn test_json_contains_sqlite() {
    let sql = db_sql::json_contains(DatabaseType::SQLite, "metadata", "test_value");
    assert!(sql.contains("json_each"));
    assert!(sql.contains("\"metadata\""));
}

#[test]
fn test_json_key_exists_postgres() {
    let sql = db_sql::json_key_exists(DatabaseType::Postgres, "data", "email");
    assert_eq!(sql, "\"data\" ? 'email'");
}

#[test]
fn test_json_key_exists_mysql() {
    let sql = db_sql::json_key_exists(DatabaseType::MySQL, "data", "email");
    assert!(sql.contains("JSON_CONTAINS_PATH"));
    assert!(sql.contains("$.email"));

    let sql = db_sql::json_key_exists(DatabaseType::MariaDB, "data", "email");
    assert!(sql.contains("JSON_CONTAINS_PATH"));
    assert!(sql.contains("$.email"));
}

#[test]
fn test_json_key_exists_sqlite() {
    let sql = db_sql::json_key_exists(DatabaseType::SQLite, "data", "email");
    assert!(sql.contains("json_extract"));
    assert!(sql.contains("$.email"));
    assert!(sql.contains("IS NOT NULL"));
}

#[test]
fn test_json_path_exists_postgres() {
    let sql = db_sql::json_path_exists(DatabaseType::Postgres, "data", "$.user.name");
    assert!(sql.contains("@?"));
}

#[test]
fn test_json_path_exists_mysql() {
    let sql = db_sql::json_path_exists(DatabaseType::MySQL, "data", "$.user.name");
    assert!(sql.contains("JSON_CONTAINS_PATH"));

    let sql = db_sql::json_path_exists(DatabaseType::MariaDB, "data", "$.user.name");
    assert!(sql.contains("JSON_CONTAINS_PATH"));
}

#[test]
fn test_json_path_exists_sqlite() {
    let sql = db_sql::json_path_exists(DatabaseType::SQLite, "data", "$.user.name");
    assert!(sql.contains("json_extract"));
}

#[test]
fn test_array_contains_postgres() {
    let values = vec!["'admin'".to_string(), "'user'".to_string()];
    let sql = db_sql::array_contains(DatabaseType::Postgres, "roles", &values);
    assert!(sql.contains("@>"));
    assert!(sql.contains("ARRAY["));
}

#[test]
fn test_array_contains_mysql() {
    let values = vec!["'admin'".to_string(), "'user'".to_string()];
    let sql = db_sql::array_contains(DatabaseType::MySQL, "roles", &values);
    assert!(sql.contains("JSON_CONTAINS"));

    let sql = db_sql::array_contains(DatabaseType::MariaDB, "roles", &values);
    assert!(sql.contains("JSON_CONTAINS"));
}

#[test]
fn test_array_contains_sqlite() {
    let values = vec!["'admin'".to_string(), "'user'".to_string()];
    let sql = db_sql::array_contains(DatabaseType::SQLite, "roles", &values);
    assert!(sql.contains("json_each"));
}

#[test]
fn test_array_overlaps_postgres() {
    let values = vec!["'a'".to_string(), "'b'".to_string()];
    let sql = db_sql::array_overlaps(DatabaseType::Postgres, "tags", &values);
    assert!(sql.contains("&&"));
    assert!(sql.contains("ARRAY["));
}

#[test]
fn test_array_overlaps_mysql() {
    let values = vec!["'a'".to_string(), "'b'".to_string()];
    let sql = db_sql::array_overlaps(DatabaseType::MySQL, "tags", &values);
    assert!(sql.contains(" OR "));

    let sql = db_sql::array_overlaps(DatabaseType::MariaDB, "tags", &values);
    assert!(sql.contains(" OR "));
}

#[test]
fn test_array_overlaps_sqlite() {
    let values = vec!["'a'".to_string(), "'b'".to_string()];
    let sql = db_sql::array_overlaps(DatabaseType::SQLite, "tags", &values);
    assert!(sql.contains(" OR "));
}

#[test]
fn test_format_column_simple() {
    assert_eq!(
        db_sql::format_column(DatabaseType::Postgres, "name"),
        "\"name\""
    );
    assert_eq!(db_sql::format_column(DatabaseType::MySQL, "name"), "`name`");
    assert_eq!(
        db_sql::format_column(DatabaseType::MariaDB, "name"),
        "`name`"
    );
}

#[test]
fn test_format_column_dotted() {
    assert_eq!(
        db_sql::format_column(DatabaseType::Postgres, "users.name"),
        "\"users\".\"name\""
    );
    assert_eq!(
        db_sql::format_column(DatabaseType::MySQL, "users.name"),
        "`users`.`name`"
    );
    assert_eq!(
        db_sql::format_column(DatabaseType::MariaDB, "users.name"),
        "`users`.`name`"
    );
}

#[test]
fn test_format_column_expression() {
    assert_eq!(
        db_sql::format_column(DatabaseType::Postgres, "COUNT(*)"),
        "COUNT(*)"
    );
}

#[test]
fn test_cast_to_float() {
    assert_eq!(
        db_sql::cast_to_float(DatabaseType::Postgres, "value"),
        "CAST(value AS FLOAT8)"
    );
    assert_eq!(
        db_sql::cast_to_float(DatabaseType::MySQL, "value"),
        "CAST(value AS DOUBLE)"
    );
    assert_eq!(
        db_sql::cast_to_float(DatabaseType::MariaDB, "value"),
        "CAST(value AS DOUBLE)"
    );
    assert_eq!(
        db_sql::cast_to_float(DatabaseType::SQLite, "value"),
        "CAST(value AS REAL)"
    );
}

#[test]
fn test_sql_injection_prevention() {
    let sql = db_sql::json_contains(DatabaseType::Postgres, "data", "O'Brien");
    assert!(sql.contains("O''Brien"));

    let sql = db_sql::json_key_exists(DatabaseType::MySQL, "data", "key'; DROP TABLE--");
    assert!(sql.contains("key''; DROP TABLE--"));

    let sql = db_sql::json_key_exists(DatabaseType::MariaDB, "data", "key'; DROP TABLE--");
    assert!(sql.contains("key''; DROP TABLE--"));
}

#[test]
fn test_join_identifier_validation_accepts_safe_values() {
    assert!(db_sql::validate_identifier("JOIN table", "users").is_ok());
    assert!(db_sql::validate_identifier("JOIN alias", "author_1").is_ok());
    assert!(db_sql::validate_join_column("posts.user_id").is_ok());
}

#[test]
fn test_join_identifier_validation_rejects_injection() {
    let table_err =
        db_sql::validate_identifier("JOIN table", "users; DROP TABLE users; --").unwrap_err();
    assert!(table_err.contains("unsafe JOIN table"));

    let alias_err = db_sql::validate_identifier("JOIN alias", "author --").unwrap_err();
    assert!(alias_err.contains("unsafe JOIN alias"));

    let column_err = db_sql::validate_join_column("posts.user_id OR 1=1").unwrap_err();
    assert!(column_err.contains("unsafe JOIN column reference"));
}

#[test]
fn test_union_type_sql() {
    assert_eq!(UnionType::Union.as_sql(), "UNION");
    assert_eq!(UnionType::UnionAll.as_sql(), "UNION ALL");
}

#[test]
fn test_union_clause_creation() {
    let clause = UnionClause {
        union_type: UnionType::Union,
        query_sql: "SELECT * FROM users WHERE active = true".to_string(),
    };
    assert_eq!(clause.union_type, UnionType::Union);
    assert!(clause.query_sql.contains("active = true"));
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

#[test]
fn test_build_where_sql_includes_or_groups() {
    let query = QueryBuilder::<QueryTestUser>::new()
        .where_eq("status", "active")
        .or_where(|q| q.where_eq("role", "admin").where_eq("role", "moderator"));

    let sql = query.build_where_sql_for_db(DatabaseType::Postgres);

    assert_eq!(
        sql,
        "\"status\" = 'active' AND (\"role\" = 'admin' OR \"role\" = 'moderator')"
    );
}

#[test]
fn test_build_where_sql_escapes_inner_quotes_in_column_names() {
    let query = QueryBuilder::<QueryTestUser>::new().where_eq("profile.na\"me", "active");

    let sql = query.build_where_sql_for_db(DatabaseType::Postgres);

    assert_eq!(sql, "\"profile\".\"na\"\"me\" = 'active'");
}

#[test]
fn test_build_select_sql_with_params_parameterizes_read_filters() {
    let query = QueryBuilder::<QueryTestUser>::new()
        .select_raw("COUNT(*) as total")
        .where_eq("status", "active")
        .or_where(|q| q.where_eq("role", "admin").where_eq("role", "moderator"))
        .where_in("department", vec!["engineering", "design"])
        .order_by("name", Order::Asc)
        .limit(10);

    let (sql, params) = query.build_select_sql_with_params_for_db(DatabaseType::Postgres);

    assert_eq!(
        sql,
        "SELECT COUNT(*) as total FROM \"query_test_users\" WHERE \"status\" = $1 AND \"department\" IN ($2, $3) AND (\"role\" = $4 OR \"role\" = $5) ORDER BY \"name\" ASC LIMIT 10"
    );
    assert_eq!(params.len(), 5);
}

#[test]
fn test_build_select_sql_with_params_uses_mysql_identifier_quoting() {
    let query = QueryBuilder::<QueryTestUser>::new()
        .select(vec!["id", "name"])
        .where_eq("status", "active")
        .order_by("name", Order::Asc)
        .limit(5);

    let (sql, params) = query.build_select_sql_with_params_for_db(DatabaseType::MySQL);

    assert_eq!(
        sql,
        "SELECT `query_test_users`.`id`, `query_test_users`.`name` FROM `query_test_users` WHERE `status` = ? ORDER BY `name` ASC LIMIT 5"
    );
    assert_eq!(params.len(), 1);
}

#[cfg(feature = "fulltext")]
#[test]
fn test_fulltext_build_postgres_sql_parameterizes_query_and_escapes_identifiers() {
    let builder = FullTextSearchBuilder::<QueryTestUser>::new(&["na\"me", "bio"], "o'hai")
        .language("en'g\"lish");

    let (sql, params) = builder.build_sql(DatabaseType::Postgres).unwrap();

    assert!(sql.contains("SELECT * FROM \"query_test_users\""));
    assert!(sql.contains("COALESCE(\"na\"\"me\", '')"));
    assert!(sql.contains("plainto_tsquery('en''g\"lish', $1)"));
    assert!(matches!(params.first(), Some(Value::String(Some(query))) if query == "o'hai"));
}

#[cfg(feature = "fulltext")]
#[test]
fn test_fulltext_build_postgres_ranked_sql_binds_prefix_query_and_min_rank() {
    let builder = FullTextSearchBuilder::<QueryTestUser>::new(&["name"], "quick fox")
        .mode(SearchMode::Prefix)
        .with_ranking()
        .min_rank(0.75);

    let (sql, params) = builder.build_ranked_sql(DatabaseType::Postgres).unwrap();

    assert!(sql.contains("to_tsquery('english', $1)"));
    assert!(sql.contains(" >= $2"));
    assert!(
        matches!(params.first(), Some(Value::String(Some(query))) if query == "quick:* & fox:*")
    );
    assert!(
        matches!(params.get(1), Some(Value::Double(Some(rank))) if (*rank - 0.75).abs() < f64::EPSILON)
    );
}

#[cfg(feature = "fulltext")]
#[test]
fn test_fulltext_build_mysql_ranked_sql_uses_bound_values_for_all_dynamic_inputs() {
    let builder = FullTextSearchBuilder::<QueryTestUser>::new(&["na`me", "bio"], "+urgent term")
        .mode(SearchMode::Boolean)
        .with_ranking()
        .min_rank(0.5);

    let (sql, params) = builder.build_ranked_sql(DatabaseType::MySQL).unwrap();

    assert!(sql.contains("MATCH(`na``me`, `bio`) AGAINST(? IN BOOLEAN MODE)"));
    assert!(sql.contains("AND MATCH(`na``me`, `bio`) AGAINST(? IN BOOLEAN MODE) >= ?"));
    assert_eq!(params.len(), 4);
    assert!(matches!(params.first(), Some(Value::String(Some(query))) if query == "+urgent term"));
    assert!(matches!(params.get(1), Some(Value::String(Some(query))) if query == "+urgent term"));
    assert!(
        matches!(params.get(2), Some(Value::Double(Some(rank))) if (*rank - 0.5).abs() < f64::EPSILON)
    );
    assert!(matches!(params.get(3), Some(Value::String(Some(query))) if query == "+urgent term"));
}

#[cfg(feature = "fulltext")]
#[test]
fn test_fulltext_build_sqlite_sql_binds_escaped_fts_query() {
    let builder =
        FullTextSearchBuilder::<QueryTestUser>::new(&["name", "bio"], "say \"hello\" to it's")
            .limit(5)
            .offset(2);

    let (sql, params) = builder.build_sql(DatabaseType::SQLite).unwrap();

    assert!(sql.contains("SELECT t.* FROM \"query_test_users\" t"));
    assert!(sql.contains("INNER JOIN \"query_test_users_fts\" fts"));
    assert!(sql.contains("WHERE \"query_test_users_fts\" MATCH ?"));
    assert!(sql.contains("LIMIT 5 OFFSET 2"));
    assert!(
        matches!(params.first(), Some(Value::String(Some(query))) if query == "say \"\"hello\"\" to it''s")
    );
}
