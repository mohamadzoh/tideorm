use super::{CatalogObject, Views, classify, query_outputs};
use crate::internal::sql_lexer::sqlite_significant;

fn json_columns(sql: &str, names: &[&str], views: &[CatalogObject]) -> Vec<bool> {
    classify(sql, names, views)
        .into_iter()
        .map(|(document, _)| document)
        .collect()
}

fn unresolved_columns(sql: &str, names: &[&str]) -> Vec<bool> {
    classify(sql, names, &[])
        .into_iter()
        .map(|(_, unresolved)| unresolved)
        .collect()
}

fn json_outputs(sql: &str) -> Vec<String> {
    let tokens: Vec<_> = sqlite_significant(sql).collect();
    query_outputs(
        sql,
        &tokens,
        &[],
        Views {
            sources: &[],
            schema: None,
        },
    )
    .into_iter()
    .filter_map(|output| output.document.then_some(output.name))
    .collect()
}

#[test]
fn json_outputs_name_the_select_items_that_are_json_expressions() {
    let sql = "SELECT '[1,2]' AS literal, json_object('a', 1) AS object, json_array(1, 2), \
               doc -> '$.a' AS extracted, doc ->> '$.a' AS text_value, t.doc -> 0 nested, \
               (SELECT json_group_array(id) FROM t) AS grouped, upper(json('[1]')) AS wrapped, \
               CAST(json('{}') AS TEXT) AS cast_text, json(doc) || '' AS joined, \
               json_set(doc, '$.b', 2) \"Quoted \"\"name\"\"\" FROM t";
    assert_eq!(
        json_outputs(sql),
        [
            "object",
            "json_array(1, 2)",
            "extracted",
            "nested",
            "grouped",
            "Quoted \"name\"",
        ]
    );
    assert!(json_outputs("SELECT '{\"a\":1}' AS doc, label FROM t").is_empty());
}

#[test]
fn json_aliases_belong_to_the_query_that_selects_them() {
    assert!(json_outputs("SELECT '[2]' AS doc FROM (SELECT json_array(1) AS doc)").is_empty());
    assert!(
        json_outputs("SELECT '[2]' AS doc WHERE EXISTS (SELECT json_array(1) AS doc)").is_empty()
    );
    assert_eq!(
        json_outputs("SELECT doc FROM (SELECT json_array(2) AS doc)"),
        ["doc"]
    );
}

#[test]
fn derived_json_outputs_follow_references_and_aliases() {
    for sql in [
        "SELECT doc FROM (SELECT json_array(2) AS doc)",
        "SELECT DOC FROM (SELECT json_array(2) AS doc)",
        "SELECT * FROM (SELECT json_array(2) AS doc)",
        "SELECT nested.* FROM (SELECT json_array(2) AS doc) AS nested",
        "SELECT nested.original AS doc FROM (SELECT json_array(2) AS original) nested",
        "WITH nested AS (SELECT json_array(2) AS doc) SELECT doc FROM nested",
        "WITH nested(doc) AS (SELECT json_array(2)) SELECT doc FROM nested",
        "WITH nested(doc) AS NOT MATERIALIZED (SELECT json_array(2)) SELECT doc FROM nested",
        "WITH nested AS (SELECT json_array(2) AS original), next AS (SELECT original AS doc FROM nested) SELECT doc FROM next",
        "WITH next AS (SELECT original AS doc FROM nested), nested AS (SELECT json_array(2) AS original) SELECT doc FROM next",
        "SELECT (SELECT json_array(2) AS inner_doc) AS doc",
        "SELECT (WITH nested AS (SELECT json_array(2) AS doc) SELECT doc FROM nested) AS doc",
        "SELECT json_array(2) AS doc UNION ALL SELECT json_array(3)",
        "SELECT json_array(2) AS doc INTERSECT SELECT '[2]'",
        "SELECT json_array(2) AS doc EXCEPT SELECT '[3]' UNION ALL SELECT json_array(4)",
    ] {
        assert_eq!(json_outputs(sql), ["doc"], "{sql}");
    }
}

#[test]
fn unrelated_sources_and_compound_text_do_not_inherit_json_types() {
    for sql in [
        "WITH nested AS (SELECT json_array(1) AS doc) SELECT '[2]' AS doc FROM nested",
        "SELECT upper(doc) AS doc FROM (SELECT json_array(2) AS doc)",
        "SELECT text.doc FROM (SELECT '[2]' AS doc) text CROSS JOIN (SELECT json_array(1) AS doc) document",
        "SELECT '[2]' AS doc UNION ALL SELECT json_array(3) AS doc",
        "SELECT json_array(3) AS doc UNION ALL SELECT '[2]' AS doc",
        "SELECT json_array(2) AS doc EXCEPT SELECT '[3]' UNION ALL SELECT '[4]'",
        "SELECT (SELECT '[2]' AS doc FROM (SELECT json_array(1) AS doc)) AS doc",
        "SELECT (SELECT json_array(3) UNION ALL SELECT '[2]') AS doc",
        "SELECT '[2]' AS \"json_array(1)\", coalesce(NULL, json_array(1)) AS doc",
    ] {
        assert!(json_outputs(sql).is_empty(), "{sql}");
    }
}

#[test]
fn an_arrow_path_is_not_an_implicit_alias() {
    assert_eq!(json_outputs("SELECT doc -> '$.a' FROM t"), ["doc -> '$.a'"]);
    assert_eq!(json_outputs("SELECT json_array(2) AS 'doc'"), ["doc"]);
}

#[test]
fn only_document_returning_functions_mark_json_outputs() {
    for sql in [
        "SELECT json_type(doc) AS value FROM t",
        "SELECT json_extract(doc, '$.a') AS value FROM t",
        "SELECT json_custom(doc) AS value FROM t",
        "SELECT json_extract(json_object('a', 1, 'b', 2), '$.a') AS value",
        "SELECT json_extract(doc, '$.a,b') AS value FROM t",
    ] {
        assert!(json_outputs(sql).is_empty(), "{sql}");
    }
    for sql in [
        "SELECT json_extract(doc, '$.a', '$.b') AS value FROM t",
        "SELECT json_quote(doc) AS value FROM t",
        "SELECT json_extract(json_object('a', 1), '$.a', '$.b') AS value",
    ] {
        assert_eq!(json_outputs(sql), ["value"], "{sql}");
    }
}

#[test]
fn json_column_occurrences_keep_their_own_decoding() {
    for (sql, names, expected) in [
        (
            "SELECT json_array(1) AS v, '[2]' AS v",
            vec!["v", "v"],
            vec![true, false],
        ),
        (
            "SELECT '[2]' AS v, json_array(1) AS v",
            vec!["v", "v"],
            vec![false, true],
        ),
        (
            "SELECT t.*, '[2]' AS v, json_array(1) AS v FROM t",
            vec!["id", "v", "v"],
            vec![false, false, true],
        ),
        // A wildcard can repeat the explicit expression's alias.
        (
            "SELECT t.*, json_array(1) AS v FROM t",
            vec!["v", "v"],
            vec![false, true],
        ),
    ] {
        assert_eq!(json_columns(sql, &names, &[]), expected, "{sql}");
    }
}

#[test]
fn unknown_wildcard_widths_do_not_shift_json_types() {
    for sql in [
        "WITH q(a, b, c) AS (SELECT t.*, json_array(1) FROM t) SELECT b FROM q",
        "SELECT t.*, json_array(1) AS doc FROM t UNION ALL SELECT 1, json_array(2), '[3]'",
    ] {
        assert!(json_outputs(sql).is_empty(), "{sql}");
    }
}

#[test]
fn every_arrow_and_wrapped_json_call_marks_a_json_output() {
    let sql = "SELECT doc -> 'a' -> 'b' AS chained, doc -> ?1 AS numbered, doc -> ?12 AS long, \
               doc -> :path AS named, doc -> @path AS at, doc -> $path AS dollar, \
               json_extract(doc, '$.a') -> '$.b' AS called, (SELECT doc FROM t) -> 0 AS nested, \
               '[1]' -> 0 AS literal, (json_object('a', 1)) AS wrapped, \
               json_group_array(id) FILTER (WHERE id > 1) AS filtered, \
               json_group_array(id) OVER (PARTITION BY kind) AS windowed, \
               json_group_object(k, v) OVER w AS named_window, \
               doc -> 'a' ->> 'b' AS text_value, (doc) AS parenthesized FROM t";
    assert_eq!(
        json_outputs(sql),
        [
            "chained",
            "numbered",
            "long",
            "named",
            "at",
            "dollar",
            "called",
            "nested",
            "literal",
            "wrapped",
            "filtered",
            "windowed",
            "named_window",
        ]
    );
}

/// The statement is read as SQLite reads it: what a nested comment opener
/// leaves inside a string is no statement. Two CTEs of one name in different
/// cases made the reader panic.
#[test]
fn a_statement_inside_a_string_or_two_ctes_of_one_name_do_not_panic() {
    assert_eq!(
        json_columns(
            "/* /* */ SELECT '*/ WITH a AS (SELECT 1), A AS (SELECT 2) SELECT 1' AS x -- '",
            &["x"],
            &[]
        ),
        [false]
    );
    assert!(
        json_outputs("WITH a AS (SELECT json_array(1) AS d), A AS (SELECT 2 AS d) SELECT d FROM a")
            .is_empty()
    );
    assert_eq!(
        json_outputs("WITH Docs AS (SELECT json_array(1) AS d) SELECT d FROM docs"),
        ["d"]
    );
}

/// A view's definition answers for the columns read through it, as a CTE's
/// does, a view reading another included; a column read from a source the
/// statement does not describe is unresolved until then.
#[test]
fn views_resolve_like_ctes_and_unknown_sources_stay_unresolved() {
    let views = [
        CatalogObject {
            schema: "temp".into(),
            name: "w".into(),
            sql: Some(
                "CREATE TEMP VIEW IF NOT EXISTS temp.w(n, made) AS SELECT id, built FROM v".into(),
            ),
        },
        CatalogObject {
            schema: "main".into(),
            name: "v".into(),
            sql: Some("CREATE VIEW v AS SELECT id, json_object('k', id) AS built FROM t".into()),
        },
    ];
    for (sql, names, expected) in [
        ("SELECT built FROM v", vec!["built"], vec![true]),
        (
            "SELECT x.built, id FROM v AS x",
            vec!["built", "id"],
            vec![true, false],
        ),
        ("SELECT * FROM v", vec!["id", "built"], vec![false, true]),
        ("SELECT made FROM temp.w", vec!["made"], vec![true]),
        (
            "SELECT made, n FROM w",
            vec!["made", "n"],
            vec![true, false],
        ),
        // A CTE of the name shadows the view; a qualified name never reads a CTE.
        (
            "WITH v AS (SELECT 1 AS built) SELECT built FROM v",
            vec!["built"],
            vec![false],
        ),
        (
            "WITH v AS (SELECT 1 AS built) SELECT built FROM main.v",
            vec!["built"],
            vec![true],
        ),
    ] {
        assert_eq!(json_columns(sql, &names, &views), expected, "{sql}");
    }
    assert_eq!(
        unresolved_columns("SELECT built, count(*) AS n FROM v", &["built", "n"]),
        [true, false]
    );
    assert_eq!(
        unresolved_columns("SELECT json_array(1) AS a FROM t", &["a"]),
        [false]
    );
    assert_eq!(
        unresolved_columns("SELECT * FROM t", &["a", "b"]),
        [true, true]
    );
    assert_eq!(
        unresolved_columns("WITH c AS (SELECT 1 AS x) SELECT x FROM c", &["x"]),
        [false]
    );
}
