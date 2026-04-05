use super::*;

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
    let sql =
        db_sql::preview_json_contains(DatabaseType::Postgres, "metadata", r#"{"key": "value"}"#);
    assert!(sql.contains("@>"));
    assert!(sql.contains("\"metadata\""));
}

#[test]
fn test_json_contains_mysql() {
    let sql = db_sql::preview_json_contains(DatabaseType::MySQL, "metadata", r#"{"key": "value"}"#);
    assert!(sql.contains("JSON_CONTAINS"));
    assert!(sql.contains("`metadata`"));

    let sql =
        db_sql::preview_json_contains(DatabaseType::MariaDB, "metadata", r#"{"key": "value"}"#);
    assert!(sql.contains("JSON_CONTAINS"));
    assert!(sql.contains("`metadata`"));
}

#[test]
fn test_json_contains_sqlite() {
    let sql = db_sql::preview_json_contains(DatabaseType::SQLite, "metadata", "test_value");
    assert!(sql.contains("json_each"));
    assert!(sql.contains("\"metadata\""));
}

#[test]
fn test_json_key_exists_postgres() {
    let sql = db_sql::preview_json_key_exists(DatabaseType::Postgres, "data", "email");
    assert_eq!(sql, "\"data\" ? 'email'");
}

#[test]
fn test_json_key_exists_mysql() {
    let sql = db_sql::preview_json_key_exists(DatabaseType::MySQL, "data", "email");
    assert!(sql.contains("JSON_CONTAINS_PATH"));
    assert!(sql.contains("$.\"email\""));

    let sql = db_sql::preview_json_key_exists(DatabaseType::MariaDB, "data", "email");
    assert!(sql.contains("JSON_CONTAINS_PATH"));
    assert!(sql.contains("$.\"email\""));
}

#[test]
fn test_json_key_exists_sqlite() {
    let sql = db_sql::preview_json_key_exists(DatabaseType::SQLite, "data", "email");
    assert!(sql.contains("json_extract"));
    assert!(sql.contains("$.\"email\""));
    assert!(sql.contains("IS NOT NULL"));
}

#[test]
fn test_json_path_exists_postgres() {
    let sql = db_sql::preview_json_path_exists(DatabaseType::Postgres, "data", "$.user.name");
    assert!(sql.contains("@?"));
}

#[test]
fn test_json_path_exists_mysql() {
    let sql = db_sql::preview_json_path_exists(DatabaseType::MySQL, "data", "$.user.name");
    assert!(sql.contains("JSON_CONTAINS_PATH"));
    assert!(sql.contains("$.\"user\".\"name\""));

    let sql = db_sql::preview_json_path_exists(DatabaseType::MariaDB, "data", "$.user.name");
    assert!(sql.contains("JSON_CONTAINS_PATH"));
    assert!(sql.contains("$.\"user\".\"name\""));
}

#[test]
fn test_json_path_exists_sqlite() {
    let sql = db_sql::preview_json_path_exists(DatabaseType::SQLite, "data", "$.user.name");
    assert!(sql.contains("json_extract"));
    assert!(sql.contains("$.\"user\".\"name\""));
}

#[test]
fn test_json_contains_bound_mysql_uses_parameterized_json() {
    let bound = db_sql::json_contains_bound(
        DatabaseType::MySQL,
        "`data`",
        &serde_json::json!({"role": "admin'"}),
    );

    assert_eq!(bound.sql, "JSON_CONTAINS(`data`, CAST(? AS JSON))");
    assert!(matches!(
        bound.values.as_slice(),
        [Value::String(Some(json))] if json == "{\"role\":\"admin'\"}"
    ));
}

#[test]
fn test_json_contains_bound_postgres_uses_postgres_placeholder() {
    let bound = db_sql::json_contains_bound(
        DatabaseType::Postgres,
        "\"data\"",
        &serde_json::json!({"role": "admin'"}),
    );

    assert_eq!(bound.sql, "\"data\" @> $1");
    assert!(matches!(bound.values.as_slice(), [Value::Json(Some(_))]));
}

#[test]
fn test_json_key_exists_bound_mysql_uses_parameterized_path() {
    let bound = db_sql::json_key_exists_bound(DatabaseType::MySQL, "`data`", "unsafe'key");

    assert_eq!(bound.sql, "JSON_CONTAINS_PATH(`data`, 'one', ?)");
    assert!(matches!(
        bound.values.as_slice(),
        [Value::String(Some(path))] if path == "$.\"unsafe'key\""
    ));
}

#[test]
fn test_json_path_exists_bound_postgres_uses_jsonpath_placeholder() {
    let bound = db_sql::json_path_exists_bound(DatabaseType::Postgres, "\"data\"", "$.user.name")
        .expect("postgres jsonpath helper should always bind valid paths");

    assert_eq!(bound.sql, "\"data\" @? ($1::jsonpath)");
    assert!(matches!(
        bound.values.as_slice(),
        [Value::String(Some(path))] if path == "$.user.name"
    ));
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
        db_sql::format_column_or_trusted_expression(DatabaseType::Postgres, "COUNT(*)"),
        "COUNT(*)"
    );
}

#[test]
fn test_format_column_quotes_non_identifier_input() {
    assert_eq!(
        db_sql::format_column(DatabaseType::Postgres, "COUNT(*)"),
        "\"COUNT(*)\""
    );
    assert_eq!(
        db_sql::format_column(DatabaseType::Postgres, "name\" OR 1=1 --"),
        "\"name\"\" OR 1=1 --\""
    );
}

#[test]
fn test_json_contains_quotes_unsafe_column_input() {
    let sql = db_sql::preview_json_contains(DatabaseType::Postgres, "data\" OR 1=1 --", "value");
    assert_eq!(sql, "\"data\"\" OR 1=1 --\" @> 'value'");
}

#[test]
fn test_format_identifier_reference_quotes_reserved_words() {
    assert_eq!(
        db_sql::format_identifier_reference(DatabaseType::Postgres, "order"),
        Some("\"order\"".to_string())
    );
    assert_eq!(
        db_sql::format_identifier_reference(DatabaseType::MySQL, "users.group"),
        Some("`users`.`group`".to_string())
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
    let sql = db_sql::preview_json_contains(DatabaseType::Postgres, "data", "O'Brien");
    assert!(sql.contains("O''Brien"));

    let sql = db_sql::preview_json_key_exists(DatabaseType::MySQL, "data", "key'; DROP TABLE--");
    assert_eq!(
        sql,
        "JSON_CONTAINS_PATH(`data`, 'one', '$.\"key''; DROP TABLE--\"')"
    );

    let sql = db_sql::preview_json_key_exists(DatabaseType::MariaDB, "data", "key'; DROP TABLE--");
    assert_eq!(
        sql,
        "JSON_CONTAINS_PATH(`data`, 'one', '$.\"key''; DROP TABLE--\"')"
    );
}

#[test]
fn test_mysql_json_literals_escape_backslash_quote_pairs() {
    let payload = r#"\' OR 1=1 --"#;

    let sql = db_sql::preview_json_contains(DatabaseType::MySQL, "data", payload);
    assert_eq!(sql, "JSON_CONTAINS(`data`, '\\\\'' OR 1=1 --')");

    let sql = db_sql::preview_json_contains(DatabaseType::MariaDB, "data", payload);
    assert_eq!(sql, "JSON_CONTAINS(`data`, '\\\\'' OR 1=1 --')");
}

#[test]
fn test_postgres_json_literals_preserve_literal_backslashes() {
    let sql = db_sql::preview_json_contains(DatabaseType::Postgres, "data", r#"C:\temp"#);
    assert_eq!(sql, "\"data\" @> 'C:\\temp'");
}

#[test]
fn test_json_path_injection_is_rejected_for_mysql_and_sqlite() {
    let path = "$.user') OR 1=1 --";

    assert_eq!(
        db_sql::preview_json_path_exists(DatabaseType::MySQL, "data", path),
        "0 = 1"
    );
    assert_eq!(
        db_sql::preview_json_path_not_exists(DatabaseType::MySQL, "data", path),
        "0 = 1"
    );
    assert_eq!(
        db_sql::preview_json_path_exists(DatabaseType::SQLite, "data", path),
        "0 = 1"
    );
    assert_eq!(
        db_sql::preview_json_path_not_exists(DatabaseType::SQLite, "data", path),
        "0 = 1"
    );
}

#[test]
fn test_json_path_special_keys_are_quoted_safely() {
    let sql =
        db_sql::preview_json_path_exists(DatabaseType::MySQL, "data", "$['weird.key'][0].name");
    assert_eq!(
        sql,
        "JSON_CONTAINS_PATH(`data`, 'one', '$.\"weird.key\"[0].\"name\"')"
    );
}

#[test]
fn test_mysql_array_literals_are_json_encoded() {
    let values = vec!["'ad\"min'".to_string(), "'slash\\user'".to_string()];

    let contains_sql = db_sql::array_contains(DatabaseType::MySQL, "roles", &values);
    assert_eq!(
        contains_sql,
        "JSON_CONTAINS(`roles`, '[\"ad\\\\\"min\",\"slash\\\\\\\\user\"]')"
    );

    let overlaps_sql = db_sql::array_overlaps(DatabaseType::MySQL, "roles", &values);
    assert_eq!(
        overlaps_sql,
        "(JSON_CONTAINS(`roles`, '\"ad\\\\\"min\"') OR JSON_CONTAINS(`roles`, '\"slash\\\\\\\\user\"'))"
    );
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
fn test_raw_sql_fragment_validation_rejects_injection_tokens() {
    let err =
        db_sql::validate_raw_sql_fragment("WHERE raw SQL", "1 = 1; DROP TABLE users").unwrap_err();
    assert!(err.contains("unsafe WHERE raw SQL"));

    let comment_err =
        db_sql::validate_raw_sql_fragment("WHERE raw SQL", "1 = 1 -- comment").unwrap_err();
    assert!(comment_err.contains("unsafe WHERE raw SQL"));
}

#[test]
fn test_having_validation_rejects_subquery_like_payload() {
    let err = db_sql::validate_having_sql_fragment(
        "HAVING raw SQL",
        "1 = 1 OR (SELECT password FROM users LIMIT 1)::text = 'x'",
    )
    .unwrap_err();

    assert!(err.contains("unsafe HAVING raw SQL"));
}

#[test]
fn test_having_validation_allows_basic_aggregate_predicates() {
    db_sql::validate_having_sql_fragment("HAVING raw SQL", "COUNT(*) > 1")
        .expect("COUNT(*) predicate should be allowed");
    db_sql::validate_having_sql_fragment(
        "HAVING raw SQL",
        "SUM(\"amount\") >= 10 AND AVG(\"amount\") < 20",
    )
    .expect("aggregate predicates over quoted identifiers should be allowed");
}

#[test]
fn test_having_validation_allows_custom_functions_and_from_based_expressions() {
    db_sql::validate_having_sql_fragment("HAVING raw SQL", "STDDEV(\"amount\") > 0")
        .expect("custom aggregate functions should be allowed");
    db_sql::validate_having_sql_fragment(
        "HAVING raw SQL",
        "EXTRACT(YEAR FROM \"created_at\") >= 2024",
    )
    .expect("FROM-based expression syntax should be allowed");
    db_sql::validate_having_sql_fragment(
        "HAVING raw SQL",
        "\"status\" IS DISTINCT FROM 'archived'",
    )
    .expect("IS DISTINCT FROM predicates should be allowed");
}

#[test]
fn test_subquery_validation_rejects_non_select_sql() {
    let err = db_sql::validate_subquery_sql("DELETE FROM users").unwrap_err();
    assert!(err.contains("unsafe subquery"));
}

#[test]
fn test_subquery_validation_rejects_top_level_compound_queries() {
    let err =
        db_sql::validate_subquery_sql("SELECT id FROM users UNION SELECT password FROM users")
            .unwrap_err();
    assert!(err.contains("top-level 'union' queries are not allowed here"));
}

#[test]
fn test_compound_subquery_validation_allows_recursive_cte_shape() {
    db_sql::validate_compound_subquery_sql("SELECT 1 UNION ALL SELECT 2")
        .expect("recursive CTE bodies should allow top-level UNION ALL");
}
