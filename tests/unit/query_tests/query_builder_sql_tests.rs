use super::*;

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
fn test_build_where_sql_includes_typed_columns_in_or_groups() {
    let query = QueryBuilder::<QueryTestUser>::new().or_where(|q| {
        q.where_eq(QueryTestUser::columns.name, "alice")
            .where_eq(QueryTestUser::columns.id, 7)
    });

    let sql = query.build_where_sql_for_db(DatabaseType::Postgres);

    assert_eq!(sql, "(\"name\" = 'alice' OR \"id\" = 7)");
}

#[test]
fn test_begin_or_where_eq_accepts_typed_columns() {
    let query = QueryBuilder::<QueryTestUser>::new()
        .begin_or_where_eq(QueryTestUser::columns.name, "alice")
        .and_where_eq(QueryTestUser::columns.id, 7)
        .end_or();

    let sql = query.build_where_sql_for_db(DatabaseType::Postgres);

    assert_eq!(sql, "((\"name\" = 'alice' AND \"id\" = 7))");
}

#[test]
fn test_build_where_sql_escapes_inner_quotes_in_column_names() {
    let query = QueryBuilder::<QueryTestUser>::new().where_eq("profile.na\"me", "active");

    let sql = query.build_where_sql_for_db(DatabaseType::Postgres);

    assert_eq!(sql, "\"profile\".\"na\"\"me\" = 'active'");
}

#[test]
fn test_query_validation_rejects_unknown_model_column_in_where() {
    let err = QueryBuilder::<QueryTestUser>::new()
        .where_eq("naem", "alice")
        .ensure_query_is_valid()
        .expect_err("unknown where column should invalidate query");

    assert!(err.to_string().contains("unknown WHERE column 'naem'"));
    assert!(err.to_string().contains("known columns: id, name"));
}

#[test]
fn test_query_validation_rejects_unknown_self_qualified_column() {
    let err = QueryBuilder::<QueryTestUser>::new()
        .where_eq("query_test_users.naem", "alice")
        .ensure_query_is_valid()
        .expect_err("unknown self-qualified column should invalidate query");

    assert!(
        err.to_string()
            .contains("unknown WHERE column 'query_test_users.naem'")
    );
}

#[test]
fn test_query_validation_allows_joined_table_column_references() {
    QueryBuilder::<QueryTestUser>::new()
        .inner_join("profiles", "query_test_users.id", "profiles.user_id")
        .where_eq("profiles.active", true)
        .order_by("profiles.created_at", Order::Desc)
        .ensure_query_is_valid()
        .expect("joined-table column references should remain allowed");
}

#[test]
fn test_query_validation_rejects_unknown_model_column_in_order_by() {
    let err = QueryBuilder::<QueryTestUser>::new()
        .order_by("naem", Order::Asc)
        .ensure_query_is_valid()
        .expect_err("unknown order-by column should invalidate query");

    assert!(err.to_string().contains("unknown ORDER BY column 'naem'"));
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

#[test]
fn test_build_select_sql_with_params_inlines_postgres_array_predicates() {
    let query = QueryBuilder::<QueryTestUser>::new()
        .where_array_contains("tags", vec!["ops'", "core"])
        .where_array_contained_by("tags", vec!["ops'", "core"])
        .where_array_overlaps("tags", vec!["ops'", "core"]);

    let (sql, params) = query.build_select_sql_with_params_for_db(DatabaseType::Postgres);

    assert!(
        sql.contains("\"tags\" @> ARRAY['ops''','core']"),
        "sql: {sql}"
    );
    assert!(
        sql.contains("\"tags\" <@ ARRAY['ops''','core']"),
        "sql: {sql}"
    );
    assert!(
        sql.contains("\"tags\" && ARRAY['ops''','core']"),
        "sql: {sql}"
    );
    assert!(!sql.contains("$1"), "sql: {sql}");
    assert!(params.is_empty(), "params: {:?}", params);
}

#[test]
fn test_build_select_sql_with_params_inlines_postgres_integer_array_predicates() {
    let query = QueryBuilder::<QueryTestUser>::new()
        .where_array_contains("scores", vec![5, 7])
        .where_array_overlaps("scores", vec![3, 5]);

    let (sql, params) = query.build_select_sql_with_params_for_db(DatabaseType::Postgres);

    assert!(sql.contains("\"scores\" @> ARRAY[5,7]"), "sql: {sql}");
    assert!(sql.contains("\"scores\" && ARRAY[3,5]"), "sql: {sql}");
    assert!(!sql.contains("$1"), "sql: {sql}");
    assert!(params.is_empty(), "params: {:?}", params);
}

#[test]
fn test_build_select_sql_with_params_parameterizes_postgres_json_predicates() {
    let query = QueryBuilder::<QueryTestUser>::new()
        .where_json_contains("data", serde_json::json!({"role": "admin'"}))
        .where_json_key_exists("data", "unsafe'key")
        .where_json_path_exists("data", "$.user.name");

    let (sql, params) = query.build_select_sql_with_params_for_db(DatabaseType::Postgres);

    assert!(sql.contains("\"data\" @> $1"));
    assert!(sql.contains("\"data\" ? $2"));
    assert!(sql.contains("\"data\" @? ($3::jsonpath)"));
    assert!(!sql.contains("admin'"));
    assert!(!sql.contains("unsafe'key"));
    assert_eq!(params.len(), 3);
}

#[test]
fn test_build_select_sql_with_params_parameterizes_mysql_json_predicates() {
    let query = QueryBuilder::<QueryTestUser>::new()
        .where_json_contains("data", serde_json::json!({"role": "admin'"}))
        .where_json_key_exists("data", "unsafe'key")
        .where_json_path_exists("data", "$.user.name");

    let (sql, params) = query.build_select_sql_with_params_for_db(DatabaseType::MySQL);

    assert!(sql.contains("JSON_CONTAINS(`data`, CAST(? AS JSON))"));
    assert!(sql.contains("JSON_CONTAINS_PATH(`data`, 'one', ?)"));
    assert!(!sql.contains("admin'"));
    assert!(!sql.contains("unsafe'key"));
    assert_eq!(params.len(), 3);
    assert!(
        matches!(params.first(), Some(Value::String(Some(json))) if json == "{\"role\":\"admin'\"}")
    );
}

#[test]
fn test_build_select_sql_with_params_parameterizes_sqlite_json_predicates() {
    let query = QueryBuilder::<QueryTestUser>::new()
        .where_json_contains("data", serde_json::json!("admin'"))
        .where_json_path_exists("data", "$.user.name");

    let (sql, params) = query.build_select_sql_with_params_for_db(DatabaseType::SQLite);

    assert!(sql.contains("EXISTS (SELECT 1 FROM json_each(\"data\") WHERE value = ?)"));
    assert!(sql.contains("json_extract(\"data\", ?) IS NOT NULL"));
    assert!(!sql.contains("admin'"));
    assert_eq!(params.len(), 2);
    assert!(matches!(params.first(), Some(Value::String(Some(value))) if value == "admin'"));
}

#[test]
fn test_build_select_sql_with_params_parameterizes_mysql_array_predicates() {
    let query = QueryBuilder::<QueryTestUser>::new()
        .where_array_contains("tags", vec!["ops'", "core"])
        .where_array_overlaps("tags", vec!["ops'", "core"]);

    let (sql, params) = query.build_select_sql_with_params_for_db(DatabaseType::MySQL);

    assert!(sql.contains("JSON_CONTAINS(`tags`, CAST(? AS JSON))"));
    assert!(sql.contains(
        "(JSON_CONTAINS(`tags`, CAST(? AS JSON)) OR JSON_CONTAINS(`tags`, CAST(? AS JSON)))"
    ));
    assert!(!sql.contains("ops'"));
    assert_eq!(params.len(), 3);
    assert!(
        matches!(params.first(), Some(Value::String(Some(json))) if json == "[\"ops'\",\"core\"]")
    );
    assert!(matches!(params.get(1), Some(Value::String(Some(json))) if json == "\"ops'\""));
    assert!(matches!(params.get(2), Some(Value::String(Some(json))) if json == "\"core\""));
}

#[test]
fn test_build_select_sql_with_params_parameterizes_sqlite_array_predicates() {
    let query = QueryBuilder::<QueryTestUser>::new()
        .where_array_contained_by("tags", vec!["ops'", "core"])
        .where_array_overlaps("tags", vec!["ops'", "core"]);

    let (sql, params) = query.build_select_sql_with_params_for_db(DatabaseType::SQLite);

    assert!(
        sql.contains("NOT EXISTS (SELECT 1 FROM json_each(\"tags\") WHERE value NOT IN (?, ?))")
    );
    assert!(sql.contains("(EXISTS (SELECT 1 FROM json_each(\"tags\") WHERE value = ?) OR EXISTS (SELECT 1 FROM json_each(\"tags\") WHERE value = ?))"));
    assert!(!sql.contains("ops'"));
    assert_eq!(params.len(), 4);
}

#[test]
fn test_build_select_sql_with_params_quotes_reserved_identifiers() {
    let query = QueryBuilder::<QueryTestUser>::new()
        .select(vec!["order as group"])
        .where_eq("group", "active")
        .group_by("group")
        .order_by("order", Order::Desc)
        .limit(5);

    let (postgres_sql, postgres_params) = query
        .clone()
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);
    assert_eq!(
        postgres_sql,
        "SELECT \"query_test_users\".\"order\" AS \"group\" FROM \"query_test_users\" WHERE \"group\" = $1 GROUP BY \"group\" ORDER BY \"order\" DESC LIMIT 5"
    );
    assert_eq!(postgres_params.len(), 1);

    let (mysql_sql, mysql_params) = query.build_select_sql_with_params_for_db(DatabaseType::MySQL);
    assert_eq!(
        mysql_sql,
        "SELECT `query_test_users`.`order` AS `group` FROM `query_test_users` WHERE `group` = ? GROUP BY `group` ORDER BY `order` DESC LIMIT 5"
    );
    assert_eq!(mysql_params.len(), 1);
}

#[test]
fn test_build_select_sql_with_params_uses_escape_clause_for_typed_literal_like_helpers() {
    let name = crate::columns::Column::<String>::new("name");
    let query = QueryBuilder::<QueryTestUser>::new().where_col(name.contains(r"100%_\done"));

    let (postgres_sql, postgres_params) = query
        .clone()
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);
    assert!(
        postgres_sql.contains(" LIKE "),
        "postgres sql: {postgres_sql}"
    );
    assert!(
        postgres_sql.contains("ESCAPE '\\'"),
        "postgres sql: {postgres_sql}"
    );
    assert!(postgres_sql.contains("$1"), "postgres sql: {postgres_sql}");
    assert_eq!(
        postgres_params.len(),
        1,
        "postgres params: {:?}",
        postgres_params
    );
    assert!(matches!(
        postgres_params.first(),
        Some(Value::String(Some(value))) if value == r"%100\%\_\\done%"
    ));

    let (mysql_sql, mysql_params) = query.build_select_sql_with_params_for_db(DatabaseType::MySQL);
    assert!(mysql_sql.contains(" LIKE "), "mysql sql: {mysql_sql}");
    assert!(
        mysql_sql.contains("ESCAPE '\\\\'"),
        "mysql sql: {mysql_sql}"
    );
    assert!(mysql_sql.contains("?"), "mysql sql: {mysql_sql}");
    assert_eq!(mysql_params.len(), 1, "mysql params: {:?}", mysql_params);
    assert!(matches!(
        mysql_params.first(),
        Some(Value::String(Some(value))) if value == r"%100\%\_\\done%"
    ));
}

#[test]
fn test_build_select_sql_with_params_uses_escape_clause_for_query_contains_helpers() {
    let query = QueryBuilder::<QueryTestUser>::new()
        .where_contains("name", r"100%_\done")
        .or_where_starts_with("name", r"lead%_")
        .begin_or_where_ends_with("name", r"tail%_")
        .end_or();

    let (sql, params) = query.build_select_sql_with_params_for_db(DatabaseType::Postgres);

    assert!(sql.contains("$1 ESCAPE '\\'"), "sql: {sql}");
    assert!(sql.contains("$2 ESCAPE '\\'"), "sql: {sql}");
    assert!(sql.contains("$3 ESCAPE '\\'"), "sql: {sql}");
    assert_eq!(params.len(), 3, "params: {:?}", params);
    assert!(
        matches!(params.first(), Some(Value::String(Some(value))) if value == r"%100\%\_\\done%")
    );
    assert!(matches!(params.get(1), Some(Value::String(Some(value))) if value == r"lead\%\_%"));
    assert!(matches!(params.get(2), Some(Value::String(Some(value))) if value == r"%tail\%\_"));
}

#[test]
fn test_consolidate_preserves_full_query_fragment_state() {
    let original = QueryBuilder::<QueryTestUser>::new()
        .where_eq("name", "alice")
        .or_where_eq("name", "bob")
        .select(vec!["id", "name"])
        .select_raw("COUNT(*) AS total_count")
        .order_desc("id")
        .limit(5)
        .offset(10)
        .union_all(QueryBuilder::<QueryTestUser>::new().where_eq("name", "carol"))
        .window(
            WindowFunction::new(WindowFunctionType::RowNumber, "row_num")
                .order_by("id", Order::Asc),
        )
        .with_cte(CTE::new(
            "active_users",
            "SELECT id FROM query_test_users WHERE name IS NOT NULL".to_string(),
        ))
        .cache_with_key("fragment-key", Duration::from_secs(30));

    let fragment = original.consolidate();

    assert_eq!(fragment.condition_count(), 2);
    assert_eq!(fragment.or_groups.len(), 1);
    assert_eq!(
        fragment.select_columns.as_deref(),
        Some(&["id".to_string(), "name".to_string()][..])
    );
    assert_eq!(
        fragment.raw_select_expressions,
        vec!["COUNT(*) AS total_count"]
    );
    assert_eq!(fragment.limit_value, Some(5));
    assert_eq!(fragment.offset_value, Some(10));
    assert_eq!(fragment.unions.len(), 1);
    assert_eq!(fragment.window_functions.len(), 1);
    assert_eq!(fragment.ctes.len(), 1);
    assert_eq!(fragment.cache_key.as_deref(), Some("fragment-key"));
    let cache_options = fragment
        .cache_options
        .as_ref()
        .expect("cache options should be preserved");
    assert_eq!(cache_options.ttl, Duration::from_secs(30));

    let rebuilt = QueryBuilder::<QueryTestUser>::from_fragment(&fragment);

    assert_eq!(rebuilt.build_sql_preview(), original.build_sql_preview());
}

#[test]
fn test_window_function_sql_uses_postgres_identifier_quoting() {
    let (sql, params) = QueryBuilder::<QueryTestUser>::new()
        .first_value("first_name", "na\"me", "id", "na\"me", Order::Asc)
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);

    assert!(params.is_empty());
    assert!(sql.contains("FIRST_VALUE(\"na\"\"me\") OVER (PARTITION BY \"id\" ORDER BY \"na\"\"me\" ASC) AS \"first_name\""));
}

#[test]
fn test_window_function_sql_uses_mysql_identifier_quoting() {
    let (sql, params) = QueryBuilder::<QueryTestUser>::new()
        .first_value("first_name", "na`me", "id", "na`me", Order::Asc)
        .build_select_sql_with_params_for_db(DatabaseType::MySQL);

    assert!(params.is_empty());
    assert!(sql.contains(
        "FIRST_VALUE(`na``me`) OVER (PARTITION BY `id` ORDER BY `na``me` ASC) AS `first_name`"
    ));
}

#[cfg(feature = "fulltext")]
#[test]
fn test_fulltext_build_postgres_sql_parameterizes_query_and_escapes_identifiers() {
    let builder = FullTextSearchBuilder::<QueryTestUser>::new(&["na\"me", "bio"], "o'hai")
        .language("en'g\"lish");

    let (sql, params) = builder.build_sql(DatabaseType::Postgres).unwrap();

    assert!(sql.contains("SELECT * FROM \"query_test_users\""));
    assert!(sql.contains("COALESCE(\"na\"\"me\", '')"));
    assert!(sql.contains("plainto_tsquery(CAST($1 AS regconfig), $2)"));
    assert!(
        matches!(params.first(), Some(Value::String(Some(language))) if language == "en'g\"lish")
    );
    assert!(matches!(params.get(1), Some(Value::String(Some(query))) if query == "o'hai"));
}

#[cfg(feature = "fulltext")]
#[test]
fn test_fulltext_build_postgres_ranked_sql_binds_prefix_query_and_min_rank() {
    let builder = FullTextSearchBuilder::<QueryTestUser>::new(&["name"], "quick fox")
        .mode(SearchMode::Prefix)
        .with_ranking()
        .min_rank(0.75);

    let (sql, params) = builder.build_ranked_sql(DatabaseType::Postgres).unwrap();

    assert!(sql.contains("to_tsquery(CAST($1 AS regconfig), $2)"));
    assert!(sql.contains(" >= $4"));
    assert!(matches!(params.first(), Some(Value::String(Some(language))) if language == "english"));
    assert!(
        matches!(params.get(1), Some(Value::String(Some(query))) if query == "'quick':* & 'fox':*")
    );
    assert!(
        matches!(params.get(3), Some(Value::Double(Some(rank))) if (*rank - 0.75).abs() < f64::EPSILON)
    );
}

#[cfg(feature = "fulltext")]
#[test]
fn test_fulltext_build_postgres_boolean_sql_sanitizes_tsquery_operators() {
    let builder = FullTextSearchBuilder::<QueryTestUser>::new(&["name"], "test* OR 1")
        .mode(SearchMode::Boolean);

    let (sql, params) = builder.build_sql(DatabaseType::Postgres).unwrap();

    assert!(sql.contains("to_tsquery(CAST($1 AS regconfig), $2)"));
    assert!(matches!(params.first(), Some(Value::String(Some(language))) if language == "english"));
    assert!(
        matches!(params.get(1), Some(Value::String(Some(query))) if query == "'test' & 'OR' & '1'")
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
    assert!(sql.contains("LIMIT ? OFFSET ?"));
    assert!(
        matches!(params.first(), Some(Value::String(Some(query))) if query == "\"say\" \"hello\" \"to\" \"it's\"")
    );
    assert!(matches!(params.get(1), Some(Value::BigInt(Some(limit))) if *limit == 5));
    assert!(matches!(params.get(2), Some(Value::BigInt(Some(offset))) if *offset == 2));
}

#[cfg(feature = "fulltext")]
#[test]
fn test_fulltext_build_sqlite_sql_neutralizes_fts_operators() {
    let builder = FullTextSearchBuilder::<QueryTestUser>::new(&["name"], "test* OR 1");

    let (_, params) = builder.build_sql(DatabaseType::SQLite).unwrap();

    assert!(
        matches!(params.first(), Some(Value::String(Some(query))) if query == "\"test*\" \"OR\" \"1\"")
    );
}
