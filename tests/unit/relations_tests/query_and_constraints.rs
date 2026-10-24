use super::*;

#[test]
fn self_ref_tree_sql_uses_recursive_cte_and_local_key() {
    let (sql, params) = build_self_ref_tree_sql::<RelationTestNode>(
        "parent_slug",
        "slug",
        &serde_json::json!("root"),
        3,
        DatabaseType::Postgres,
    )
    .unwrap();

    assert!(sql.starts_with("WITH RECURSIVE \"tide_tree\""));
    assert!(sql.contains("\"node\".\"slug\" AS \"tree_key\""));
    assert!(sql.contains("\"child\".\"parent_slug\" = \"tree\".\"tree_key\""));
    assert!(sql.contains("\"tree\".\"depth\" < $2"));
    assert!(sql.contains("SELECT \"result_node\".\"id\""), "{sql}");
    assert!(
        !sql.contains(".*"),
        "the tree query must name its columns: {sql}"
    );
    assert!(matches!(params.first(), Some(Value::String(Some(root))) if root == "root"));
    assert!(matches!(params.get(1), Some(Value::BigInt(Some(depth))) if *depth == 3));
}

#[test]
fn self_ref_tree_sql_uses_backend_specific_placeholders() {
    let (sql, params) = build_self_ref_tree_sql::<RelationTestNode>(
        "parent_slug",
        "slug",
        &serde_json::json!("root"),
        2,
        DatabaseType::MySQL,
    )
    .unwrap();

    assert!(sql.starts_with("WITH RECURSIVE `tide_tree`"));
    assert!(sql.contains("`node`.`parent_slug` = ?"));
    assert!(sql.contains("`tree`.`depth` < ?"));
    assert!(matches!(params.first(), Some(Value::String(Some(root))) if root == "root"));
    assert!(matches!(params.get(1), Some(Value::BigInt(Some(depth))) if *depth == 2));
}

#[test]
fn self_ref_tree_sql_rejects_unknown_columns() {
    let err = build_self_ref_tree_sql::<RelationTestNode>(
        "parent_slug",
        "missing_column",
        &serde_json::json!("root"),
        2,
        DatabaseType::MySQL,
    )
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("Unknown self-reference column 'missing_column'")
    );
}

#[tokio::test]
async fn relation_loads_reject_composite_keys() {
    let composite_key = || json!([1, 2]);
    let refusals = [
        (
            "HasMany::load",
            HasMany::<RelationTestNode>::new("parent_slug", "slug")
                .with_parent_pk(composite_key())
                .load()
                .await
                .map(drop),
        ),
        (
            "BelongsTo::load",
            BelongsTo::<RelationTestNode>::new("parent_slug", "slug")
                .with_fk_value(composite_key())
                .load()
                .await
                .map(drop),
        ),
        (
            "HasManyThrough::load",
            HasManyThrough::<RelationTestNode, RelationTestPivot>::new(
                "left_id",
                "right_id",
                "id",
                "id",
                "relation_test_pivots",
            )
            .with_parent_pk(composite_key())
            .load()
            .await
            .map(drop),
        ),
        (
            "MorphOne::load",
            MorphOne::<RelationTestNode>::new("nodeable", "id")
                .with_parent(composite_key(), "relation_test_nodes".to_string())
                .load()
                .await
                .map(drop),
        ),
        (
            "SelfRefMany::load_tree",
            SelfRefMany::<RelationTestNode>::new("parent_slug", "slug")
                .with_parent_pk(composite_key())
                .load_tree(2)
                .await
                .map(drop),
        ),
    ];

    for (call, outcome) in refusals {
        let error = outcome.expect_err(call).to_string();
        assert!(
            error.contains(&format!("{call} only supports scalar relation keys")),
            "{error}"
        );
    }
}

#[test]
fn self_ref_tree_sql_collapses_cyclic_duplicates_to_one_row_per_node() {
    let (sql, _params) = build_self_ref_tree_sql::<RelationTestNode>(
        "parent_slug",
        "slug",
        &json!("root"),
        5,
        DatabaseType::Postgres,
    )
    .unwrap();

    assert!(
        sql.contains("MIN(\"tide_tree\".\"depth\") AS \"depth\""),
        "a cycle in the foreign key must not repeat a node once per level: {sql}"
    );
    assert!(sql.contains("GROUP BY \"tide_tree\".\"pk\""), "{sql}");
    assert!(
        sql.contains(") \"result_tree\" ON \"result_node\".\"id\" = \"result_tree\".\"pk\""),
        "{sql}"
    );
}

#[test]
fn self_ref_tree_sql_matches_every_column_of_a_composite_key() {
    let (sql, _params) = build_self_ref_tree_sql::<CompositeTreeNode>(
        "parent_code",
        "code",
        &json!("root"),
        3,
        DatabaseType::Postgres,
    )
    .unwrap();

    assert!(
        sql.starts_with("WITH RECURSIVE \"tide_tree\" (\"pk\", \"pk_2\", \"tree_key\""),
        "{sql}"
    );
    assert!(
        sql.contains("GROUP BY \"tide_tree\".\"pk\", \"tide_tree\".\"pk_2\""),
        "{sql}"
    );
    assert!(
        sql.contains(
            "ON \"result_node\".\"id\" = \"result_tree\".\"pk\" \
             AND \"result_node\".\"tenant\" = \"result_tree\".\"pk_2\""
        ),
        "{sql}"
    );
}

#[test]
fn self_ref_tree_sql_renders_a_field_name_as_its_column() {
    let (sql, _params) = build_self_ref_tree_sql::<AliasedKeyModel>(
        "account_id",
        "id",
        &json!(1),
        2,
        DatabaseType::Postgres,
    )
    .unwrap();

    assert!(sql.contains("\"node\".\"owner_id\" = $1"), "{sql}");
    assert!(!sql.contains("account_id"), "{sql}");
}
