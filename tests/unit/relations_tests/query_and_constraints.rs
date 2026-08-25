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
    assert!(sql.contains("SELECT \"result_node\".*"));
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
async fn direct_relation_helpers_reject_composite_keys() {
    let composite_key = serde_json::json!([1, 2]);

    let err = HasMany::<RelationTestNode>::new("parent_slug", "slug")
        .with_parent_pk(composite_key.clone())
        .load()
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("HasMany::load only supports scalar relation keys")
    );

    let err = BelongsTo::<RelationTestNode>::new("parent_slug", "slug")
        .with_fk_value(composite_key)
        .load()
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("BelongsTo::load only supports scalar relation keys")
    );
}

#[test]
fn eager_query_builder_accepts_typed_columns() {
    let builder = RelationTestNode::eager()
        .with("owner")
        .where_eq(RelationTestNode::columns.slug, "root")
        .where_in(RelationTestNode::columns.id, vec![1, 2])
        .order_by(RelationTestNode::columns.slug, crate::query::Order::Asc);

    assert_eq!(
        builder.get_relation_tree().roots(),
        vec!["owner".to_string()]
    );
}

#[test]
fn relation_constraints_accept_typed_columns() {
    let constraints = RelationConstraints::new()
        .where_eq(RelationTestNode::columns.slug, "root")
        .order_by(RelationTestNode::columns.id, crate::query::Order::Desc);

    let query = constraints.apply(RelationTestNode::query());

    assert_eq!(query.conditions.len(), 1);
    assert_eq!(query.conditions[0].column, "slug");
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
fn relation_info_has_many_through_records_the_related_key_in_its_own_field() {
    let info = crate::relations::RelationInfo::has_many_through(
        "tags",
        "tags",
        "post_tags",
        "post_id",
        "tag_id",
        "id",
    );

    assert_eq!(info.pivot_table.as_deref(), Some("post_tags"));
    assert_eq!(info.related_key.as_deref(), Some("tag_id"));
    assert_eq!(
        info.morph_type_column, None,
        "a through relation has no polymorphic type column"
    );
}
