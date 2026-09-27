use super::*;

#[test]
fn a_mysql_backslash_escaped_quote_cannot_smuggle_a_trailing_comment() {
    // MySQL and MariaDB read `\'` as an escaped quote under the default
    // `sql_mode`, so the literal closes at the *doubled* quote and the
    // trailing `--` comments out the soft-delete scoping, later `AND`
    // predicates, `ORDER BY`, and `LIMIT` that the builder appends.
    let error = validate_raw_sql_fragment("WHERE raw SQL", r"name = 'a\'' -- '").unwrap_err();

    assert!(error.contains("unsafe WHERE raw SQL"), "{error}");
    assert!(error.contains("backslash-escaped quotes"), "{error}");
}

#[test]
fn a_postgres_backslash_before_a_closing_quote_cannot_smuggle_a_trailing_comment() {
    // The mirror image: with `standard_conforming_strings=on` PostgreSQL
    // (and SQLite always) reads the backslash as data and closes the
    // literal at the very next quote, leaving ` AND 1=1 --` as live SQL.
    let error = validate_raw_sql_fragment("WHERE raw SQL", r"name = 'a\' AND 1=1 --'").unwrap_err();

    assert!(error.contains("unsafe WHERE raw SQL"), "{error}");
    assert!(error.contains("backslash-escaped quotes"), "{error}");
}

#[test]
fn a_backslash_before_a_closing_quoted_identifier_is_rejected() {
    let error = validate_raw_sql_fragment("WHERE raw SQL", r#""na\" -- " = 1"#).unwrap_err();

    assert!(error.contains("unsafe WHERE raw SQL"), "{error}");
    assert!(
        error.contains("backslash-escaped quotes inside quoted identifiers"),
        "{error}"
    );

    let backtick_error = validate_raw_sql_fragment("WHERE raw SQL", "`na\\` -- ` = 1").unwrap_err();
    assert!(
        backtick_error.contains("backslash-escaped quotes inside quoted identifiers"),
        "{backtick_error}"
    );
}

#[test]
fn backslash_ambiguity_is_rejected_in_having_and_subquery_fragments_too() {
    let having_error =
        validate_having_sql_fragment("HAVING raw SQL", r"COUNT(*) > 1 AND x = 'a\'' -- '")
            .unwrap_err();
    assert!(
        having_error.contains("backslash-escaped quotes"),
        "{having_error}"
    );

    let subquery_error =
        validate_subquery_sql(r"SELECT id FROM users WHERE name = 'a\'' -- '").unwrap_err();
    assert!(
        subquery_error.contains("backslash-escaped quotes"),
        "{subquery_error}"
    );
}

#[test]
fn a_trailing_backslash_does_not_swallow_the_rest_of_the_fragment() {
    // `'oops\` is unterminated on every backend; the scan must not walk off
    // the end silently and report the fragment as clean.
    let error = validate_raw_sql_fragment("WHERE raw SQL", r"note = 'oops\").unwrap_err();

    assert!(error.contains("unsafe WHERE raw SQL"), "{error}");

    let unterminated = validate_raw_sql_fragment("WHERE raw SQL", "note = 'oops").unwrap_err();
    assert!(
        unterminated.contains("unterminated string literals"),
        "{unterminated}"
    );
}

#[test]
fn comment_introducers_inside_a_literal_are_still_accepted() {
    // The whole point of the literal-aware scan: a value that merely looks
    // like a comment must not be rejected, and a bound value never reaches
    // the scanner at all.
    validate_raw_sql_fragment("WHERE raw SQL", "\"note\" = 'buy 2 -- get 1 free'")
        .expect("a comment introducer inside a literal is just data");
    validate_raw_sql_fragment("WHERE raw SQL", "\"note\" = $1")
        .expect("a bound placeholder carries no literal at all");
    validate_raw_sql_fragment("WHERE raw SQL", "\"note\" = ?")
        .expect("a bound placeholder carries no literal at all");
}

#[test]
fn backslashes_that_are_not_adjacent_to_a_quote_stay_accepted() {
    validate_raw_sql_fragment("WHERE raw SQL", r"path = 'C:\temp'")
        .expect("a backslash in the middle of a literal ends it in no dialect");
    validate_raw_sql_fragment("WHERE raw SQL", r"path = 'C:\\'")
        .expect("an escaped backslash ends the literal at the same quote everywhere");
    validate_raw_sql_fragment("WHERE raw SQL", r"note = 'a\nb'")
        .expect("a newline escape does not move the closing quote");
}

#[test]
fn an_escaped_backslash_does_not_hide_a_following_comment() {
    // `'C:\\'` closes on both readings, so the trailing `--` is live SQL.
    let error = validate_raw_sql_fragment("WHERE raw SQL", r"path = 'C:\\' -- ").unwrap_err();

    assert!(error.contains("SQL comments"), "{error}");
}
