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
    let bound = db_sql::json_containment_bound(
        DatabaseType::MySQL,
        "`data`",
        &serde_json::json!({"role": "admin'"}),
        db_sql::JsonContainment::Contains,
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
    let bound = db_sql::json_containment_bound(
        DatabaseType::MySQL,
        "`data`",
        &serde_json::json!({"tags": ["a", "b"]}),
        db_sql::JsonContainment::ContainedBy,
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
    let bound = db_sql::json_containment_bound(
        DatabaseType::Postgres,
        "\"data\"",
        &serde_json::json!({"role": "admin'"}),
        db_sql::JsonContainment::Contains,
    );

    assert_eq!(bound.sql, "(\"data\")::jsonb @> $1");
    assert!(matches!(bound.values.as_slice(), [Value::Json(Some(_))]));
}

/// The SQL of an existence test and the path it binds first.
fn json_exists(
    db_type: DatabaseType,
    existence: JsonExistence,
    target: &str,
    negated: bool,
) -> Option<(String, String)> {
    json_exists_values(db_type, existence, target, negated)
        .map(|(sql, mut paths)| (sql, paths.remove(0)))
}

/// The SQL of an existence test and every path it binds.
fn json_exists_values(
    db_type: DatabaseType,
    existence: JsonExistence,
    target: &str,
    negated: bool,
) -> Option<(String, Vec<String>)> {
    db_sql::json_exists_bound(db_type, "\"data\"", existence, target, negated).map(|bound| {
        let paths = bound
            .values
            .iter()
            .map(|value| match value {
                Value::String(Some(path)) => path.clone(),
                other => panic!("expected bound strings, got {other:?}"),
            })
            .collect();
        (bound.sql, paths)
    })
}

#[test]
fn test_json_key_exists_renders_each_backend_operator() {
    assert_eq!(
        json_exists(DatabaseType::Postgres, JsonExistence::Key, "email", false),
        Some((
            "CASE WHEN \"data\" IS NOT NULL THEN (jsonb_typeof((\"data\")::jsonb) = 'object' \
             AND (\"data\")::jsonb ? $1) END"
                .to_string(),
            "email".to_string()
        ))
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
            "CASE WHEN \"data\" IS NOT NULL THEN COALESCE((\"data\")::jsonb @? ($1::jsonpath), FALSE) END"
                .to_string(),
            "strict $.\"user\".\"name\"".to_string()
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
    let (sql, paths) = json_exists_values(
        DatabaseType::MySQL,
        JsonExistence::Path,
        "$['weird.key'][0].name",
        false,
    )
    .unwrap();

    // The indexed step checks its parent is an array.
    assert_eq!(
        sql,
        "JSON_CONTAINS_PATH(\"data\", 'one', ?) \
         AND JSON_TYPE(JSON_EXTRACT(\"data\", ?)) = 'ARRAY'"
    );
    assert_eq!(paths, ["$.\"weird.key\"[0].\"name\"", "$.\"weird.key\""]);
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
        sql.ends_with(
            "WHERE NOT (CASE WHEN \"data\" IS NOT NULL THEN COALESCE((\"data\")::jsonb @? ($1::jsonpath), FALSE) END)"
        ),
        "{sql}"
    );
    assert!(matches!(
        params.as_slice(),
        [Value::String(Some(path))] if path == "strict $.\"user\".\"name\""
    ));
}

/// A list of no elements leaves each predicate what it means for no
/// elements: containment holds for every array, an overlap never holds, and
/// only an empty array is contained; a NULL column matches neither. Nothing
/// renders `ARRAY[]`, which PostgreSQL rejects for want of an element type.
#[test]
fn test_empty_array_predicates_are_valid_and_consistent() {
    let empty: Vec<&str> = Vec::new();
    let text = |value: &str| Value::String(Some(value.to_string()));
    let mysql_family = (
        "(JSON_TYPE(`tags`) = 'ARRAY' AND (JSON_CONTAINS(`tags`, ?)))",
        vec![text("[]")],
        "(JSON_TYPE(`tags`) = 'ARRAY' AND (JSON_CONTAINS(?, `tags`) AND JSON_TYPE(JSON_EXTRACT(`tags`, ?)) <> 'OBJECT'))",
        vec![text("[]"), text("$")],
    );

    for (db_type, (contains, contains_params, contained_by, contained_by_params)) in [
        (
            DatabaseType::Postgres,
            (
                "\"tags\" IS NOT NULL",
                vec![],
                "(\"tags\" IS NOT NULL AND NOT EXISTS (SELECT 1 FROM unnest(\"tags\") AS tideorm_array_element(element)))",
                vec![],
            ),
        ),
        (DatabaseType::MySQL, mysql_family.clone()),
        (DatabaseType::MariaDB, mysql_family.clone()),
        (
            DatabaseType::SQLite,
            (
                "(json_type(\"tags\") = 'array' AND (\"tags\" IS NOT NULL))",
                vec![],
                "(json_type(\"tags\") = 'array' AND ((\"tags\" IS NOT NULL AND NOT EXISTS (SELECT 1 FROM json_each(\"tags\") AS tideorm_element))))",
                vec![],
            ),
        ),
    ] {
        let render = |query: QueryBuilder<QueryTestUser>| {
            let (sql, params) = query.build_select_sql_with_params_for_db(db_type);
            let where_sql = sql
                .split_once(" WHERE ")
                .map(|(_, where_sql)| where_sql.to_string())
                .unwrap_or_default();
            (where_sql, params)
        };

        assert_eq!(
            render(QueryBuilder::new().where_array_contains("tags", empty.clone())),
            (contains.to_string(), contains_params),
            "{db_type:?} contains"
        );
        assert_eq!(
            render(QueryBuilder::new().where_array_overlaps("tags", empty.clone())),
            ("0 = 1".to_string(), vec![]),
            "{db_type:?} overlaps"
        );
        assert_eq!(
            render(QueryBuilder::new().where_array_contained_by("tags", empty.clone())),
            (contained_by.to_string(), contained_by_params),
            "{db_type:?} contained by"
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
fn test_inline_parameters() {
    let two = vec![
        Value::String(Some("it's".to_string())),
        Value::BigInt(Some(7)),
    ];
    let one = vec![Value::BigInt(Some(1))];

    for (db_type, sql, params, expected) in [
        // Values are written the way the backend spells them.
        (
            DatabaseType::SQLite,
            "SELECT 1 WHERE name = ? AND id = ?",
            &two,
            "SELECT 1 WHERE name = 'it''s' AND id = 7",
        ),
        (
            DatabaseType::Postgres,
            "SELECT 1 WHERE id = $2 AND name = $1",
            &two,
            "SELECT 1 WHERE id = 7 AND name = E'it\\'s'",
        ),
        // A `?` inside a literal is no placeholder.
        (
            DatabaseType::MySQL,
            "SELECT '?' WHERE a = ?",
            &one,
            "SELECT '?' WHERE a = 1",
        ),
        // A raw fragment can carry a placeholder with no value behind it, and
        // sea-query's inliner would index past the values and panic: a
        // statement whose placeholders and values do not pair up, either way,
        // is shown as it is.
        (
            DatabaseType::MySQL,
            "SELECT 1 WHERE a = ? AND b = ?",
            &one,
            "SELECT 1 WHERE a = ? AND b = ?",
        ),
        (
            DatabaseType::SQLite,
            "SELECT 1 WHERE a = ? AND b = ?",
            &one,
            "SELECT 1 WHERE a = ? AND b = ?",
        ),
        (
            DatabaseType::Postgres,
            "SELECT 1 WHERE a = $1 AND b = $2",
            &one,
            "SELECT 1 WHERE a = $1 AND b = $2",
        ),
        (
            DatabaseType::Postgres,
            "SELECT 1 WHERE a = $0",
            &one,
            "SELECT 1 WHERE a = $0",
        ),
        (DatabaseType::Postgres, "SELECT 1", &one, "SELECT 1"),
        // The backends disagree on whether `\'` ends a literal, so a statement
        // that contains a backslash is shown with its placeholders.
        (
            DatabaseType::Postgres,
            r"SELECT 1 WHERE note = 'C:\' AND a = $1",
            &one,
            r"SELECT 1 WHERE note = 'C:\' AND a = $1",
        ),
    ] {
        assert_eq!(
            db_sql::inline_parameters(db_type, sql, params),
            expected,
            "{db_type:?}: {sql}"
        );
    }
}

/// Only real `$n` markers move: one inside a single-quoted, escape-string or
/// dollar-quoted literal, or inside a comment, is text.
#[test]
fn test_rebase_placeholders_skips_quoted_text_and_comments() {
    for (sql, offset, expected) in [
        (
            "name = 'price is $5' AND id = $1 AND note = 'it''s still $2'",
            3,
            "name = 'price is $5' AND id = $4 AND note = 'it''s still $2'",
        ),
        (
            "note = E'price isn\\'t $5' AND id = $1 AND raw = e'keep \\$2 here'",
            4,
            "note = E'price isn\\'t $5' AND id = $5 AND raw = e'keep \\$2 here'",
        ),
        (
            concat!(
                "note = $$literal $1$$ AND id = $2 ",
                "/* keep $3 */ ",
                "-- keep $4\n",
                "AND body = $tag$still $5$tag$"
            ),
            2,
            concat!(
                "note = $$literal $1$$ AND id = $4 ",
                "/* keep $3 */ ",
                "-- keep $4\n",
                "AND body = $tag$still $5$tag$"
            ),
        ),
        // A `$1` inside an identifier is no parameter, and PostgreSQL comments
        // nest.
        (
            "a$1 + $1 /* outer /* $2 */ $3 */ + $2",
            1,
            "a$1 + $2 /* outer /* $2 */ $3 */ + $3",
        ),
    ] {
        assert_eq!(
            db_sql::rebase_placeholders(DatabaseType::Postgres, sql, offset),
            expected,
            "{sql}"
        );
    }
}

/// `format_column` quotes anything that is not a plain `column` or
/// `table.column` as one identifier; the trusted-expression variant passes
/// such text through, for slots validated before they render.
#[test]
fn test_format_column() {
    for (db_type, input, column, trusted) in [
        (DatabaseType::Postgres, "name", "\"name\"", "\"name\""),
        (DatabaseType::MySQL, "name", "`name`", "`name`"),
        (DatabaseType::MariaDB, "name", "`name`", "`name`"),
        (
            DatabaseType::Postgres,
            "users.name",
            "\"users\".\"name\"",
            "\"users\".\"name\"",
        ),
        (
            DatabaseType::MySQL,
            "users.name",
            "`users`.`name`",
            "`users`.`name`",
        ),
        (
            DatabaseType::MariaDB,
            "users.name",
            "`users`.`name`",
            "`users`.`name`",
        ),
        (
            DatabaseType::Postgres,
            "COUNT(*)",
            "\"COUNT(*)\"",
            "COUNT(*)",
        ),
        (
            DatabaseType::Postgres,
            "name\" OR 1=1 --",
            "\"name\"\" OR 1=1 --\"",
            "name\" OR 1=1 --",
        ),
    ] {
        assert_eq!(
            db_sql::format_column(db_type, input),
            column,
            "{db_type:?}: {input}"
        );
        assert_eq!(
            db_sql::format_column_or_trusted_expression(db_type, input),
            trusted,
            "{db_type:?}: {input}"
        );
    }
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
fn test_having_validation_allows_aggregate_predicates_and_expression_syntax() {
    for sql in [
        "COUNT(*) > 1",
        "SUM(\"amount\") >= 10 AND AVG(\"amount\") < 20",
        "STDDEV(\"amount\") > 0",
        "EXTRACT(YEAR FROM \"created_at\") >= 2024",
        "\"status\" IS DISTINCT FROM 'archived'",
    ] {
        db_sql::validate_having_sql_fragment("HAVING raw SQL", sql)
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }
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

/// A template's `?` is a placeholder only outside quoted text and comments: a
/// string literal, a quoted identifier, a dollar-quoted string, a comment, or
/// the brackets the engine reads as quoted and never binds a `?` inside.
#[test]
fn test_template_placeholders_skip_quoted_text() {
    const NESTED_TEMPLATE: &str = "? /* ? /* ? */ ? */ + '?' + $$?$$ + \"?\" -- ?\n + ?";

    for (template, count) in [
        ("MAX(note) LIKE '%?%'", 0),
        ("SUM(\"a?\") > ? AND b = '?'", 1),
        ("tags && ARRAY[?] AND id = ?", 1),
        ("name = $$?$$ AND id = ?", 1),
        ("name = $tag$ it's ? $tag$ AND id = ?", 1),
        // A tag closes only on the same tag.
        ("a = $x$ ? $y$ ? $x$", 0),
        // No closing tag, a positional parameter, and a `$` inside a name are
        // no dollar quotes.
        ("a = $$ ? AND b = ?", 2),
        ("a = $1 AND b = ?", 1),
        ("price$$ = ? AND b$$ = ?", 2),
        // Nested comments, quoted identifiers and line comments hide a `?` too.
        (NESTED_TEMPLATE, 2),
    ] {
        assert_eq!(
            db_sql::count_template_placeholders(template),
            count,
            "{template}"
        );
    }

    for (template, expected) in [
        (
            "x = ? AND y LIKE 'it''s ?' AND z = ?",
            "x = $ AND y LIKE 'it''s ?' AND z = $",
        ),
        ("name = $$?$$ AND id = ?", "name = $$?$$ AND id = $"),
    ] {
        assert_eq!(
            db_sql::map_template_placeholders(template, || "$".to_string()),
            expected,
            "{template}"
        );
    }
    assert_eq!(
        db_sql::render_template(DatabaseType::Postgres, NESTED_TEMPLATE, 1),
        "$1 /* ? /* ? */ ? */ + '?' + $$?$$ + \"?\" -- ?\n + $2"
    );
}
