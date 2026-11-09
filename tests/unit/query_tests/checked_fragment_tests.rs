use super::*;

#[test]
fn test_invalid_bound_fragments_are_deferred_to_validation() {
    for (sql, count) in [("id = ? OR id = ?", 1), ("id = ?", 0), ("id = ?", 2)] {
        let mut query = QueryBuilder::<QueryTestUser>::new();
        query.clauses.conditions.push(crate::query::WhereCondition {
            column: String::new(),
            operator: crate::query::Operator::Raw,
            value: crate::query::ConditionValue::RawTemplate {
                sql: sql.into(),
                values: vec![1.into(); count],
            },
        });
        assert!(query.validate().is_err(), "{sql}");
        let _ = query.build_sql_preview();
    }
}

#[test]
fn test_value_less_raw_fragments_render_verbatim() {
    let query =
        QueryBuilder::<QueryTestUser>::new().where_raw("name = $$x$$ OR name = $tag$y$tag$");
    let (sql, params) = query
        .build_select_sql_with_params_for_db(DatabaseType::Postgres)
        .unwrap();
    assert!(
        sql.contains("name = $$x$$ OR name = $tag$y$tag$"),
        "sql: {sql}"
    );
    assert!(params.is_empty(), "params: {params:?}");
}
