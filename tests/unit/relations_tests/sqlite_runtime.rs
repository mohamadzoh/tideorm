#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use super::*;

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn direct_relation_load_refreshes_stale_cached_values() {
    let _guard = direct_relation_db_guard().lock().await;

    let db = setup_direct_relation_test_db().await;

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
        vec![
            Value::BigInt(Some(1)),
            Value::String(Some("Fresh User".to_string())),
        ],
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

    let mut has_one =
        HasOne::<DirectRelationProfile>::new("user_id", "id").with_parent_pk(serde_json::json!(1));
    has_one.set_cached(Some(DirectRelationProfile {
        id: 10,
        user_id: 1,
        name: "Stale Profile".to_string(),
    }));

    let mut belongs_to =
        BelongsTo::<DirectRelationUser>::new("user_id", "id").with_fk_value(serde_json::json!(1));
    belongs_to.set_cached(Some(DirectRelationUser {
        id: 1,
        name: "Stale User".to_string(),
        profile: Default::default(),
        posts: Default::default(),
    }));

    let mut has_many =
        HasMany::<DirectRelationPost>::new("user_id", "id").with_parent_pk(serde_json::json!(1));
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

    cleanup_direct_relation_test_db();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn query_builder_with_batch_loads_direct_relations() {
    let _guard = direct_relation_db_guard().lock().await;

    let db = setup_direct_relation_test_db().await;

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
        "INSERT INTO relation_test_users (id, name) VALUES (?, ?), (?, ?)",
        vec![
            Value::BigInt(Some(1)),
            Value::String(Some("Eager User".to_string())),
            Value::BigInt(Some(2)),
            Value::String(Some("Filtered Out".to_string())),
        ],
    )
    .await
    .expect("inserting users should succeed");
    db.__execute_with_params(
        "INSERT INTO relation_test_profiles (id, user_id, name) VALUES (?, ?, ?), (?, ?, ?)",
        vec![
            Value::BigInt(Some(10)),
            Value::BigInt(Some(1)),
            Value::String(Some("Eager Profile".to_string())),
            Value::BigInt(Some(20)),
            Value::BigInt(Some(2)),
            Value::String(Some("Other Profile".to_string())),
        ],
    )
    .await
    .expect("inserting profiles should succeed");
    db.__execute_with_params(
        "INSERT INTO relation_test_posts (id, user_id, title) VALUES (?, ?, ?), (?, ?, ?), (?, ?, ?)",
        vec![
            Value::BigInt(Some(100)),
            Value::BigInt(Some(1)),
            Value::String(Some("First Post".to_string())),
            Value::BigInt(Some(101)),
            Value::BigInt(Some(1)),
            Value::String(Some("Second Post".to_string())),
            Value::BigInt(Some(200)),
            Value::BigInt(Some(2)),
            Value::String(Some("Filtered Post".to_string())),
        ],
    )
    .await
    .expect("inserting posts should succeed");

    let users = DirectRelationUser::query()
        .where_eq("id", 1)
        .with("profile")
        .with("posts")
        .get()
        .await
        .expect("query builder eager loading should succeed");

    assert_eq!(users.len(), 1);
    assert_eq!(users[0].name, "Eager User");
    assert_eq!(
        users[0]
            .profile
            .get_cached()
            .map(|profile| profile.name.as_str()),
        Some("Eager Profile")
    );

    let posts = users[0]
        .posts
        .get_cached()
        .expect("posts should be cached by eager loading");
    assert_eq!(posts.len(), 2);
    assert!(posts.iter().any(|post| post.title == "First Post"));
    assert!(posts.iter().any(|post| post.title == "Second Post"));
    assert!(!posts.iter().any(|post| post.title == "Filtered Post"));

    cleanup_direct_relation_test_db();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn has_many_through_attach_uses_backend_specific_placeholders() {
    let _guard = direct_relation_db_guard().lock().await;

    let db = setup_direct_relation_test_db().await;

    db.__execute_with_params(
        "CREATE TABLE relation_test_pivots (id INTEGER PRIMARY KEY, left_id INTEGER NOT NULL, right_id INTEGER NOT NULL)",
        vec![],
    )
    .await
    .expect("creating relation_test_pivots should succeed");

    HasManyThrough::<RelationTestNode, RelationTestPivot>::new(
        "left_id",
        "right_id",
        "id",
        "id",
        "relation_test_pivots",
    )
    .with_parent_pk(json!(1))
    .attach(json!(2))
    .await
    .expect("attaching pivot row should succeed on sqlite");

    let pivots = RelationTestPivot::query()
        .get()
        .await
        .expect("loading pivot rows should succeed");

    assert_eq!(pivots.len(), 1);
    assert_eq!(pivots[0].left_id, 1);
    assert_eq!(pivots[0].right_id, 2);

    cleanup_direct_relation_test_db();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn direct_relation_load_preserves_cached_payloads_without_query_context() {
    let _guard = direct_relation_db_guard().lock().await;

    let _db = setup_direct_relation_test_db().await;

    let has_one: HasOne<DirectRelationProfile> = serde_json::from_value(json!({
        "id": 10,
        "user_id": 1,
        "name": "Cached Profile"
    }))
    .expect("has_one should deserialize cached payload");

    let has_many: HasMany<DirectRelationPost> = serde_json::from_value(json!([
        { "id": 100, "user_id": 1, "title": "Cached Post" }
    ]))
    .expect("has_many should deserialize cached payload");

    let belongs_to: BelongsTo<DirectRelationUser> = serde_json::from_value(json!({
        "id": 1,
        "name": "Cached User"
    }))
    .expect("belongs_to should deserialize cached payload");

    assert_eq!(
        has_one
            .load()
            .await
            .expect("cached has-one relation should load")
            .expect("cached profile should exist")
            .name,
        "Cached Profile"
    );
    assert_eq!(
        has_many
            .load()
            .await
            .expect("cached has-many relation should load")
            .len(),
        1
    );
    assert_eq!(
        belongs_to
            .load()
            .await
            .expect("cached belongs-to relation should load")
            .expect("cached user should exist")
            .name,
        "Cached User"
    );

    cleanup_direct_relation_test_db();
}
