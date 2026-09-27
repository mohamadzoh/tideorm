use super::*;
use crate::query::db_sql::JsonExistence;

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
fn test_json_contains_bound_mysql_uses_parameterized_json() {
    let bound = db_sql::json_contains_bound(
        DatabaseType::MySQL,
        "`data`",
        &serde_json::json!({"role": "admin'"}),
    );

    // PostgreSQL's reading: the scalar under `role` must not match an array
    // holding it, and the document must be an object.
    assert_eq!(
        bound.sql,
        "JSON_CONTAINS(`data`, ?) \
         AND JSON_TYPE(JSON_EXTRACT(`data`, ?)) <> 'ARRAY' \
         AND JSON_TYPE(JSON_EXTRACT(`data`, ?)) = 'OBJECT'"
    );
    assert_eq!(
        bound.values,
        vec![
            Value::String(Some("{\"role\":\"admin'\"}".to_string())),
            Value::String(Some("$.\"role\"".to_string())),
            Value::String(Some("$".to_string())),
        ]
    );
}

#[test]
fn test_json_contained_by_bound_mysql_requires_arrays_where_the_target_has_them() {
    let bound = db_sql::json_contained_by_bound(
        DatabaseType::MySQL,
        "`data`",
        &serde_json::json!({"tags": ["a", "b"]}),
    );

    assert_eq!(
        bound.sql,
        "JSON_CONTAINS(?, `data`) \
         AND COALESCE(JSON_TYPE(JSON_EXTRACT(`data`, ?)), 'ARRAY') = 'ARRAY'"
    );
    assert_eq!(
        bound.values[1],
        Value::String(Some("$.\"tags\"".to_string()))
    );
}

#[test]
fn test_json_contains_bound_postgres_uses_postgres_placeholder() {
    let bound = db_sql::json_contains_bound(
        DatabaseType::Postgres,
        "\"data\"",
        &serde_json::json!({"role": "admin'"}),
    );

    assert_eq!(bound.sql, "(\"data\")::jsonb @> $1");
    assert!(matches!(bound.values.as_slice(), [Value::Json(Some(_))]));
}

fn json_exists(
    db_type: DatabaseType,
    existence: JsonExistence,
    target: &str,
    negated: bool,
) -> Option<(String, String)> {
    db_sql::json_exists_bound(db_type, "\"data\"", existence, target, negated).map(|bound| {
        let [Value::String(Some(path))] = bound.values.as_slice() else {
            panic!("expected one bound string, got {:?}", bound.values);
        };
        (bound.sql, path.clone())
    })
}

#[test]
fn test_json_key_exists_renders_each_backend_operator() {
    assert_eq!(
        json_exists(DatabaseType::Postgres, JsonExistence::Key, "email", false),
        Some(("(\"data\")::jsonb ? $1".to_string(), "email".to_string()))
    );

    for db_type in [DatabaseType::MySQL, DatabaseType::MariaDB] {
        assert_eq!(
            json_exists(db_type, JsonExistence::Key, "email", false),
            Some((
                "JSON_CONTAINS_PATH(\"data\", 'one', ?)".to_string(),
                "$.\"email\"".to_string()
            ))
        );
    }

    assert_eq!(
        json_exists(DatabaseType::SQLite, JsonExistence::Key, "email", false),
        Some((
            "CASE WHEN \"data\" IS NOT NULL THEN json_type(\"data\", ?) IS NOT NULL END"
                .to_string(),
            "$.\"email\"".to_string()
        ))
    );
}

#[test]
fn test_json_existence_negation_wraps_the_positive_test() {
    for db_type in [
        DatabaseType::Postgres,
        DatabaseType::MySQL,
        DatabaseType::MariaDB,
        DatabaseType::SQLite,
    ] {
        for existence in [JsonExistence::Key, JsonExistence::Path] {
            let (positive, path) = json_exists(db_type, existence, "$.user", false).unwrap();
            let (negative, negated_path) = json_exists(db_type, existence, "$.user", true).unwrap();

            assert_eq!(negative, format!("NOT ({})", positive), "{db_type:?}");
            assert_eq!(path, negated_path, "{db_type:?}");
        }
    }
}

#[test]
fn test_json_key_is_bound_not_spliced() {
    let (sql, path) = json_exists(
        DatabaseType::MySQL,
        JsonExistence::Key,
        "key'; DROP TABLE--",
        false,
    )
    .unwrap();

    assert!(!sql.contains("DROP"), "{sql}");
    assert_eq!(path, "$.\"key'; DROP TABLE--\"");
}

#[test]
fn test_json_path_exists_binds_the_path() {
    assert_eq!(
        json_exists(
            DatabaseType::Postgres,
            JsonExistence::Path,
            "$.user.name",
            false
        ),
        Some((
            "(\"data\")::jsonb @? ($1::jsonpath)".to_string(),
            "$.user.name".to_string()
        ))
    );

    for db_type in [DatabaseType::MySQL, DatabaseType::SQLite] {
        let (_, path) = json_exists(db_type, JsonExistence::Path, "$.user.name", false).unwrap();
        assert_eq!(path, "$.\"user\".\"name\"", "{db_type:?}");
    }
}

#[test]
fn test_json_path_injection_is_rejected_for_mysql_and_sqlite() {
    let path = "$.user') OR 1=1 --";

    for db_type in [DatabaseType::MySQL, DatabaseType::SQLite] {
        for negated in [false, true] {
            assert_eq!(
                json_exists(db_type, JsonExistence::Path, path, negated),
                None,
                "{db_type:?}"
            );
        }
    }
}

#[test]
fn test_json_path_special_keys_are_quoted_safely() {
    let (_, path) = json_exists(
        DatabaseType::MySQL,
        JsonExistence::Path,
        "$['weird.key'][0].name",
        false,
    )
    .unwrap();

    assert_eq!(path, "$.\"weird.key\"[0].\"name\"");
}

#[test]
fn test_unexpressible_json_path_matches_nothing_instead_of_being_dropped() {
    let path = "$.user') OR 1=1 --";
    for query in [
        QueryBuilder::<QueryTestUser>::new().where_json_path_exists("data", path),
        QueryBuilder::<QueryTestUser>::new().where_json_path_not_exists("data", path),
    ] {
        let (sql, params) = query.build_select_sql_with_params_for_db(DatabaseType::MySQL);

        assert!(sql.ends_with("WHERE 0 = 1"), "{sql}");
        assert!(params.is_empty());
    }
}

#[test]
fn test_json_path_not_exists_negates_the_path_test_and_binds_the_path() {
    let (sql, params) = QueryBuilder::<QueryTestUser>::new()
        .where_json_path_not_exists("data", "$.user.name")
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);

    assert!(
        sql.ends_with("WHERE NOT ((\"data\")::jsonb @? ($1::jsonpath))"),
        "{sql}"
    );
    assert!(matches!(params.as_slice(), [Value::String(Some(path))] if path == "$.user.name"));
}

#[test]
fn test_empty_array_predicates_are_valid_and_consistent() {
    let empty: Vec<&str> = Vec::new();

    for db_type in [
        DatabaseType::Postgres,
        DatabaseType::MySQL,
        DatabaseType::MariaDB,
        DatabaseType::SQLite,
    ] {
        let render = |query: QueryBuilder<QueryTestUser>| {
            let (sql, _) = query.build_select_sql_with_params_for_db(db_type);
            sql.split_once(" WHERE ")
                .map(|(_, where_sql)| where_sql.to_string())
                .unwrap_or_default()
        };

        // An empty "contains all" is vacuously satisfied.
        let contains = render(QueryBuilder::new().where_array_contains("tags", empty.clone()));
        assert!(!contains.contains("ARRAY[]"), "{db_type:?}: {contains}");
        assert!(!contains.contains("()"), "{db_type:?}: {contains}");

        // An empty "contains any" can never match.
        let overlaps = render(QueryBuilder::new().where_array_overlaps("tags", empty.clone()));
        assert_eq!(overlaps, "0 = 1", "{db_type:?}");

        // An empty "contained by" only holds for an empty column.
        let contained_by =
            render(QueryBuilder::new().where_array_contained_by("tags", empty.clone()));
        assert!(
            !contained_by.contains("ARRAY[]"),
            "{db_type:?}: {contained_by}"
        );
    }
}

#[test]
fn test_postgres_array_predicates_take_their_placeholders_outside_brackets() {
    let operands = db_sql::placeholders(DatabaseType::Postgres, 2);

    assert_eq!(
        db_sql::postgres_array_contains("\"roles\"", &operands),
        "($1 = ANY(\"roles\") AND $2 = ANY(\"roles\"))"
    );
    assert_eq!(
        db_sql::postgres_array_overlaps("\"roles\"", &operands),
        "($1 = ANY(\"roles\") OR $2 = ANY(\"roles\"))"
    );
    // Both NULL guards are part of the contract, because `NOT EXISTS` over
    // `unnest` inverts what `<@` does with unknowns: it is TRUE for a NULL
    // column (zero rows to find) and for a NULL element (`NULL NOT IN (..)`
    // is unknown, so the offending row goes uncounted), where `<@` matches
    // neither.
    assert_eq!(
        db_sql::postgres_array_contained_by("\"roles\"", &operands, false),
        "(\"roles\" IS NOT NULL AND NOT EXISTS (SELECT 1 FROM unnest(\"roles\") AS \
         tideorm_array_element(element) WHERE tideorm_array_element.element IS NULL OR \
         tideorm_array_element.element NOT IN ($1, $2)))"
    );
    // A NULL in the list lets a NULL element through and stays out of NOT IN.
    assert_eq!(
        db_sql::postgres_array_contained_by("\"roles\"", &operands, true),
        "(\"roles\" IS NOT NULL AND NOT EXISTS (SELECT 1 FROM unnest(\"roles\") AS \
         tideorm_array_element(element) WHERE tideorm_array_element.element NOT IN ($1, $2)))"
    );
}

#[test]
fn test_placeholders_follow_the_backend_marker() {
    assert_eq!(
        db_sql::placeholders(DatabaseType::Postgres, 3),
        vec!["$1", "$2", "$3"]
    );
    assert_eq!(
        db_sql::placeholders(DatabaseType::SQLite, 2),
        vec!["?", "?"]
    );
    assert!(db_sql::placeholders(DatabaseType::MySQL, 0).is_empty());
}

#[test]
fn test_inline_parameters_writes_values_the_way_the_backend_spells_them() {
    let params = vec![
        Value::String(Some("it's".to_string())),
        Value::BigInt(Some(7)),
    ];

    assert_eq!(
        db_sql::inline_parameters(
            DatabaseType::SQLite,
            "SELECT 1 WHERE name = ? AND id = ?",
            &params
        ),
        "SELECT 1 WHERE name = 'it''s' AND id = 7"
    );
    assert_eq!(
        db_sql::inline_parameters(
            DatabaseType::Postgres,
            "SELECT 1 WHERE id = $2 AND name = $1",
            &params
        ),
        "SELECT 1 WHERE id = 7 AND name = E'it\\'s'"
    );
}

#[test]
fn test_inline_parameters_leaves_unbound_placeholders_alone() {
    // A raw fragment can carry a placeholder with no value behind it;
    // sea-query's inliner would index past the values and panic.
    let one = vec![Value::BigInt(Some(1))];

    for (db_type, sql) in [
        (DatabaseType::MySQL, "SELECT 1 WHERE a = ? AND b = ?"),
        (DatabaseType::SQLite, "SELECT 1 WHERE a = ? AND b = ?"),
        (DatabaseType::Postgres, "SELECT 1 WHERE a = $1 AND b = $2"),
        (DatabaseType::Postgres, "SELECT 1 WHERE a = $0"),
    ] {
        assert_eq!(db_sql::inline_parameters(db_type, sql, &one), sql);
    }

    // Every value has to be placed, too.
    assert_eq!(
        db_sql::inline_parameters(DatabaseType::Postgres, "SELECT 1", &one),
        "SELECT 1"
    );
}

#[test]
fn test_inline_parameters_keeps_placeholders_inside_quotes() {
    let one = vec![Value::BigInt(Some(1))];

    assert_eq!(
        db_sql::inline_parameters(DatabaseType::MySQL, "SELECT '?' WHERE a = ?", &one),
        "SELECT '?' WHERE a = 1"
    );
}

#[test]
fn test_inline_parameters_does_not_guess_around_backslashes() {
    // The backends disagree on whether `\'` ends a literal, so a statement
    // that contains a backslash is shown with its placeholders instead.
    let one = vec![Value::BigInt(Some(1))];
    let sql = r"SELECT 1 WHERE note = 'C:\' AND a = $1";

    assert_eq!(
        db_sql::inline_parameters(DatabaseType::Postgres, sql, &one),
        sql
    );
}

#[test]
fn test_offset_postgres_placeholders_skips_single_quoted_literals() {
    let sql = "name = 'price is $5' AND id = $1 AND note = 'it''s still $2'";

    assert_eq!(
        db_sql::offset_postgres_placeholders(sql, 3),
        "name = 'price is $5' AND id = $4 AND note = 'it''s still $2'"
    );
}

#[test]
fn test_offset_postgres_placeholders_skips_dollar_quotes_and_comments() {
    let sql = concat!(
        "note = $$literal $1$$ AND id = $2 ",
        "/* keep $3 */ ",
        "-- keep $4\n",
        "AND body = $tag$still $5$tag$"
    );

    assert_eq!(
        db_sql::offset_postgres_placeholders(sql, 2),
        concat!(
            "note = $$literal $1$$ AND id = $4 ",
            "/* keep $3 */ ",
            "-- keep $4\n",
            "AND body = $tag$still $5$tag$"
        )
    );
}

#[test]
fn test_offset_postgres_placeholders_skips_escape_string_literals() {
    let sql = "note = E'price isn\\'t $5' AND id = $1 AND raw = e'keep \\$2 here'";

    assert_eq!(
        db_sql::offset_postgres_placeholders(sql, 4),
        "note = E'price isn\\'t $5' AND id = $5 AND raw = e'keep \\$2 here'"
    );
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
fn test_raw_sql_fragment_validation_rejects_mysql_hash_comment() {
    let where_err =
        db_sql::validate_raw_sql_fragment("WHERE raw SQL", "\"name\" = 'x' #").unwrap_err();
    assert!(where_err.contains("unsafe WHERE raw SQL"), "{where_err}");

    let having_err =
        db_sql::validate_having_sql_fragment("HAVING raw SQL", "COUNT(*) > 1 #").unwrap_err();
    assert!(having_err.contains("unsafe HAVING raw SQL"), "{having_err}");
}

#[test]
fn test_raw_sql_fragment_validation_is_string_literal_aware() {
    db_sql::validate_raw_sql_fragment("WHERE raw SQL", "\"note\" = 'issue #42 -- urgent'")
        .expect("comment introducers inside a string literal are just characters");

    let unterminated =
        db_sql::validate_raw_sql_fragment("WHERE raw SQL", "\"note\" = 'oops").unwrap_err();
    assert!(
        unterminated.contains("unsafe WHERE raw SQL"),
        "{unterminated}"
    );
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
fn test_subquery_validation_tells_a_function_from_a_statement() {
    for sql in [
        "SELECT REPLACE(name, 'a', 'b') FROM users",
        "SELECT id FROM users WHERE LOWER(REPLACE (email, ' ', '')) = 'x'",
    ] {
        db_sql::validate_subquery_sql(sql).unwrap_or_else(|error| panic!("{sql}: {error}"));
    }
    for sql in [
        "SELECT 1; REPLACE INTO users VALUES (1)",
        "SELECT id FROM users UPDATE users SET name = 'x'",
    ] {
        assert!(db_sql::validate_subquery_sql(sql).is_err(), "{sql}");
    }
    db_sql::validate_having_sql_fragment("HAVING", "LEFT(MAX(name), 1) = 'a'")
        .expect("LEFT() is a string function in HAVING");
    assert!(db_sql::validate_having_sql_fragment("HAVING", "COUNT(*) > 1 LEFT JOIN x").is_err());
}

#[test]
fn test_compound_subquery_validation_allows_recursive_cte_shape() {
    db_sql::validate_compound_subquery_sql("SELECT 1 UNION ALL SELECT 2")
        .expect("recursive CTE bodies should allow top-level UNION ALL");
}

#[test]
fn test_a_question_mark_in_quotes_is_no_placeholder() {
    assert_eq!(
        db_sql::count_template_placeholders("MAX(note) LIKE '%?%'"),
        0
    );
    assert_eq!(
        db_sql::count_template_placeholders("SUM(\"a?\") > ? AND b = '?'"),
        1
    );
    assert_eq!(
        db_sql::map_template_placeholders("x = ? AND y LIKE 'it''s ?' AND z = ?", || "$"
            .to_string()),
        "x = $ AND y LIKE 'it''s ?' AND z = $"
    );
}
