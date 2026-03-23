use super::{
    BelongsTo, HasMany, HasManyThrough, MorphOne, MorphTo, SelfRef, SelfRefMany, Value,
    build_self_ref_tree_sql,
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

#[tideorm::model(table = "relation_test_images")]
struct RelationTestImage {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    imageable_type: String,
    imageable_id: i64,

    #[tideorm(morph_name = "imageable")]
    owner: MorphTo<RelationTestNode>,
}

#[tideorm::model(table = "relation_test_employees")]
struct RelationTestEmployee {
    #[tideorm(primary_key)]
    id: i64,
    manager_id: Option<i64>,

    #[tideorm(foreign_key = "manager_id")]
    manager: SelfRef<RelationTestEmployee>,

    #[tideorm(foreign_key = "manager_id")]
    reports: SelfRefMany<RelationTestEmployee>,

    #[tideorm(morph_name = "imageable")]
    avatar: MorphOne<RelationTestImage>,
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

#[tokio::test]
async fn macro_generated_morph_and_self_ref_relations_are_configured() {
    let employee = RelationTestEmployee {
        id: 42,
        manager_id: Some(7),
        manager: Default::default(),
        reports: Default::default(),
        avatar: Default::default(),
    }
    .with_relations();

    assert_eq!(employee.manager.foreign_key, "manager_id");
    assert_eq!(employee.manager.local_key, "id");
    assert_eq!(employee.reports.foreign_key, "manager_id");
    assert_eq!(employee.reports.local_key, "id");
    assert_eq!(employee.avatar.morph_name, "imageable");
    assert_eq!(employee.avatar.local_key, "id");

    let manager_err = employee.manager.load().await.unwrap_err();
    assert!(!manager_err.to_string().contains("Foreign key value not set"));

    let reports_err = employee.reports.load().await.unwrap_err();
    assert!(!reports_err.to_string().contains("Parent primary key not set"));

    let avatar_err = employee.avatar.load().await.unwrap_err();
    assert!(!avatar_err.to_string().contains("MorphOne relation is not configured"));
    assert!(!avatar_err.to_string().contains("Parent primary key not set"));
}

#[test]
fn macro_generated_morph_to_relation_is_configured() {
    let image = RelationTestImage {
        id: 9,
        imageable_type: "relation_test_nodes".to_string(),
        imageable_id: 4,
        owner: Default::default(),
    }
    .with_relations();

    assert_eq!(image.owner.type_column, "imageable_type");
    assert_eq!(image.owner.id_column, "imageable_id");
}
