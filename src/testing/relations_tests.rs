use super::{
    BelongsTo, HasMany, HasManyThrough, MorphOne, SelfRefMany, Value, build_self_ref_tree_sql,
};
use crate::config::DatabaseType;

#[tideorm::model(table = "relation_test_nodes")]
struct RelationTestNode {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    slug: String,
    parent_slug: Option<String>,
}

#[tideorm::model(table = "relation_test_pivots")]
struct RelationTestPivot {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    left_id: i64,
    right_id: i64,
}

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

#[tokio::test]
async fn advanced_relation_helpers_reject_composite_keys() {
    let composite_key = serde_json::json!([1, 2]);

    let err = HasManyThrough::<RelationTestNode, RelationTestPivot>::new(
        "left_id",
        "right_id",
        "id",
        "id",
        "relation_test_pivots",
    )
    .with_parent_pk(composite_key.clone())
    .load()
    .await
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("HasManyThrough::load only supports scalar relation keys")
    );

    let err = MorphOne::<RelationTestNode>::new("nodeable", "id")
        .with_parent(composite_key.clone(), String::from("relation_test_nodes"))
        .load()
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("MorphOne::load only supports scalar relation keys")
    );

    let err = SelfRefMany::<RelationTestNode>::new("parent_slug", "slug")
        .with_parent_pk(composite_key)
        .load_tree(2)
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("SelfRefMany::load_tree only supports scalar relation keys")
    );
}
