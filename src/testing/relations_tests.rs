use super::{
    BelongsTo, EagerLoadExt, HasMany, HasManyThrough, MorphOne, MorphTo, RelationConstraints,
    SelfRef, SelfRefMany, Value, build_self_ref_tree_sql,
};
use crate::config::DatabaseType;
use crate::model::Model as _;
use serde_json::json;

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use std::sync::{Mutex, OnceLock};
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use super::HasOne;

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

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn direct_relation_db_guard() -> &'static Mutex<()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD.get_or_init(|| Mutex::new(()))
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tideorm::model(table = "relation_test_users")]
struct DirectRelationUser {
    #[tideorm(primary_key)]
    id: i64,
    name: String,
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tideorm::model(table = "relation_test_profiles")]
struct DirectRelationProfile {
    #[tideorm(primary_key)]
    id: i64,
    user_id: i64,
    name: String,
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tideorm::model(table = "relation_test_posts")]
struct DirectRelationPost {
    #[tideorm(primary_key)]
    id: i64,
    user_id: i64,
    title: String,
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

#[test]
fn eager_query_builder_accepts_typed_columns() {
    let builder = RelationTestNode::eager()
        .with("owner")
        .where_eq(RelationTestNode::columns.slug, "root")
        .where_in(RelationTestNode::columns.id, vec![1, 2])
        .order_by(RelationTestNode::columns.slug, crate::query::Order::Asc);

    assert_eq!(builder.get_relation_tree().roots(), vec!["owner".to_string()]);
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
    assert!(
        !manager_err
            .to_string()
            .contains("Foreign key value not set")
    );

    let reports_err = employee.reports.load().await.unwrap_err();
    assert!(
        !reports_err
            .to_string()
            .contains("Parent primary key not set")
    );

    let avatar_err = employee.avatar.load().await.unwrap_err();
    assert!(
        !avatar_err
            .to_string()
            .contains("MorphOne relation is not configured")
    );
    assert!(
        !avatar_err
            .to_string()
            .contains("Parent primary key not set")
    );
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

#[test]
fn direct_relations_deserialize_cached_payloads() {
    let has_one: super::HasOne<RelationTestNode> = serde_json::from_value(json!({
        "id": 1,
        "slug": "root",
        "parent_slug": null
    }))
    .expect("has_one should deserialize cached payload");
    assert_eq!(has_one.get_cached().map(|node| node.id), Some(1));

    let has_many: HasMany<RelationTestNode> = serde_json::from_value(json!([
        { "id": 2, "slug": "child-a", "parent_slug": "root" },
        { "id": 3, "slug": "child-b", "parent_slug": "root" }
    ]))
    .expect("has_many should deserialize cached payload");
    assert_eq!(has_many.get_cached().map(|nodes| nodes.len()), Some(2));

    let belongs_to: BelongsTo<RelationTestNode> = serde_json::from_value(json!({
        "id": 4,
        "slug": "parent",
        "parent_slug": null
    }))
    .expect("belongs_to should deserialize cached payload");
    assert_eq!(belongs_to.get_cached().map(|node| node.slug.as_str()), Some("parent"));
}

#[test]
fn advanced_relations_deserialize_cached_payloads() {
    let self_ref: SelfRef<RelationTestEmployee> = serde_json::from_value(json!({
        "id": 1,
        "manager_id": null,
        "manager": null,
        "reports": [],
        "avatar": null
    }))
    .expect("self_ref should deserialize cached payload");
    assert_eq!(self_ref.get_cached().map(|employee| employee.id), Some(1));

    let self_ref_many: SelfRefMany<RelationTestEmployee> = serde_json::from_value(json!([
        {
            "id": 2,
            "manager_id": 1,
            "manager": null,
            "reports": [],
            "avatar": null
        }
    ]))
    .expect("self_ref_many should deserialize cached payload");
    assert_eq!(self_ref_many.get_cached().map(|employees| employees.len()), Some(1));

    let morph_one: MorphOne<RelationTestImage> = serde_json::from_value(json!({
        "id": 9,
        "imageable_type": "relation_test_nodes",
        "imageable_id": 4,
        "owner": null
    }))
    .expect("morph_one should deserialize cached payload");
    assert_eq!(morph_one.get_cached().map(|image| image.id), Some(9));

    let morph_many: super::MorphMany<RelationTestImage> = serde_json::from_value(json!([
        {
            "id": 10,
            "imageable_type": "relation_test_nodes",
            "imageable_id": 4,
            "owner": null
        }
    ]))
    .expect("morph_many should deserialize cached payload");
    assert_eq!(morph_many.get_cached().map(|images| images.len()), Some(1));

    let has_many_through: HasManyThrough<RelationTestNode, RelationTestPivot> =
        serde_json::from_value(json!([
            { "id": 5, "slug": "related", "parent_slug": null }
        ]))
        .expect("has_many_through should deserialize cached payload");
    assert_eq!(has_many_through.get_cached().map(|nodes| nodes.len()), Some(1));
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn direct_relation_load_refreshes_stale_cached_values() {
    let _guard = direct_relation_db_guard()
        .lock()
        .expect("relation test database guard should not be poisoned");

    crate::Database::reset_global();
    let db = crate::Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed");
    crate::Database::set_global(db.clone()).expect("setting global database should succeed");

    db.__execute_with_params(
        "CREATE TABLE relation_test_users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
        vec![],
    )
    .await
    .expect("creating relation_test_users should succeed");
    db.__execute_with_params(
        "CREATE TABLE relation_test_profiles (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, name TEXT NOT NULL)",
        vec![],
    )
    .await
    .expect("creating relation_test_profiles should succeed");
    db.__execute_with_params(
        "CREATE TABLE relation_test_posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL)",
        vec![],
    )
    .await
    .expect("creating relation_test_posts should succeed");

    db.__execute_with_params(
        "INSERT INTO relation_test_users (id, name) VALUES (?, ?)",
        vec![Value::BigInt(Some(1)), Value::String(Some("Fresh User".to_string()))],
    )
    .await
    .expect("inserting user should succeed");
    db.__execute_with_params(
        "INSERT INTO relation_test_profiles (id, user_id, name) VALUES (?, ?, ?)",
        vec![
            Value::BigInt(Some(10)),
            Value::BigInt(Some(1)),
            Value::String(Some("Fresh Profile".to_string())),
        ],
    )
    .await
    .expect("inserting profile should succeed");
    db.__execute_with_params(
        "INSERT INTO relation_test_posts (id, user_id, title) VALUES (?, ?, ?), (?, ?, ?)",
        vec![
            Value::BigInt(Some(100)),
            Value::BigInt(Some(1)),
            Value::String(Some("Fresh Post 1".to_string())),
            Value::BigInt(Some(101)),
            Value::BigInt(Some(1)),
            Value::String(Some("Fresh Post 2".to_string())),
        ],
    )
    .await
    .expect("inserting posts should succeed");

    let mut has_one = HasOne::<DirectRelationProfile>::new("user_id", "id").with_parent_pk(serde_json::json!(1));
    has_one.set_cached(Some(DirectRelationProfile {
        id: 10,
        user_id: 1,
        name: "Stale Profile".to_string(),
    }));

    let mut belongs_to = BelongsTo::<DirectRelationUser>::new("user_id", "id").with_fk_value(serde_json::json!(1));
    belongs_to.set_cached(Some(DirectRelationUser {
        id: 1,
        name: "Stale User".to_string(),
    }));

    let mut has_many = HasMany::<DirectRelationPost>::new("user_id", "id").with_parent_pk(serde_json::json!(1));
    has_many.set_cached(vec![DirectRelationPost {
        id: 999,
        user_id: 1,
        title: "Stale Post".to_string(),
    }]);

    let loaded_profile = has_one
        .load()
        .await
        .expect("loading has-one relation should succeed")
        .expect("fresh profile should exist");
    assert_eq!(loaded_profile.name, "Fresh Profile");
    assert_eq!(
        has_one
            .get_cached()
            .expect("cached profile should remain inspectable")
            .name,
        "Stale Profile"
    );

    let loaded_user = belongs_to
        .load()
        .await
        .expect("loading belongs-to relation should succeed")
        .expect("fresh user should exist");
    assert_eq!(loaded_user.name, "Fresh User");
    assert_eq!(
        belongs_to
            .get_cached()
            .expect("cached user should remain inspectable")
            .name,
        "Stale User"
    );

    let loaded_posts = has_many
        .load()
        .await
        .expect("loading has-many relation should succeed");
    assert_eq!(loaded_posts.len(), 2);
    assert!(loaded_posts.iter().any(|post| post.title == "Fresh Post 1"));
    assert!(loaded_posts.iter().any(|post| post.title == "Fresh Post 2"));
    assert_eq!(
        has_many
            .get_cached()
            .expect("cached posts should remain inspectable")
            .len(),
        1
    );

    crate::Database::reset_global();
}
