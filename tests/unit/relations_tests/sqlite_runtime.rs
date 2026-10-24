use super::*;
use crate::model::Model as _;

const USERS: &str = "CREATE TABLE relation_test_users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)";
const PROFILES: &str = "CREATE TABLE relation_test_profiles (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, name TEXT NOT NULL)";
const POSTS: &str = "CREATE TABLE relation_test_posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL)";
const EMPLOYEES: &str =
    "CREATE TABLE relation_test_employees (id INTEGER PRIMARY KEY, manager_id INTEGER)";
const IMAGES: &str = "CREATE TABLE relation_test_images (id INTEGER PRIMARY KEY, imageable_type TEXT NOT NULL, imageable_id INTEGER NOT NULL)";

#[tokio::test]
async fn direct_relation_load_refreshes_stale_cached_values() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    crate::test_support::install_sqlite_global(&[
        USERS,
        PROFILES,
        POSTS,
        "INSERT INTO relation_test_users (id, name) VALUES (1, 'Fresh User')",
        "INSERT INTO relation_test_profiles (id, user_id, name) VALUES (10, 1, 'Fresh Profile')",
        "INSERT INTO relation_test_posts (id, user_id, title) VALUES (100, 1, 'Fresh Post 1'), (101, 1, 'Fresh Post 2')",
    ])
    .await;

    let mut has_one =
        HasOne::<DirectRelationProfile>::new("user_id", "id").with_parent_pk(json!(1));
    has_one.set_cached(Some(DirectRelationProfile {
        id: 10,
        user_id: 1,
        name: "Stale Profile".to_string(),
    }));

    let mut belongs_to =
        BelongsTo::<DirectRelationUser>::new("user_id", "id").with_fk_value(json!(1));
    belongs_to.set_cached(Some(DirectRelationUser {
        id: 1,
        name: "Stale User".to_string(),
        profile: Default::default(),
        posts: Default::default(),
    }));

    let mut has_many = HasMany::<DirectRelationPost>::new("user_id", "id").with_parent_pk(json!(1));
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

    crate::test_support::reset_globals();
}

/// `with(..)` caches each requested relation of each row, its own rows only,
/// and records the payload it fetched; a relation not asked for is neither.
#[tokio::test]
async fn eager_loading_caches_and_records_direct_relations() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    crate::test_support::install_sqlite_global(&[
        USERS,
        PROFILES,
        POSTS,
        "INSERT INTO relation_test_users (id, name) VALUES (1, 'Eager User'), (2, 'Filtered Out')",
        "INSERT INTO relation_test_profiles (id, user_id, name) VALUES (10, 1, 'Eager Profile'), (20, 2, 'Other Profile')",
        "INSERT INTO relation_test_posts (id, user_id, title) VALUES (100, 1, 'First Post'), (101, 1, 'Second Post'), (200, 2, 'Filtered Post')",
    ])
    .await;
    let titles = |posts: &[DirectRelationPost]| {
        let mut titles: Vec<String> = posts.iter().map(|post| post.title.clone()).collect();
        titles.sort();
        titles
    };

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
    assert_eq!(titles(posts), ["First Post", "Second Post"]);

    assert!(
        users[0].has_relation("posts"),
        "the eager loader must record what it fetched"
    );
    let recorded: Vec<DirectRelationPost> = users[0]
        .get_relation("posts")
        .expect("the recorded payload should deserialize back");
    assert_eq!(titles(&recorded), ["First Post", "Second Post"]);

    let users = DirectRelationUser::query()
        .where_eq("id", 1)
        .with("posts")
        .get()
        .await
        .expect("eager loading should succeed");
    assert!(!users[0].has_relation("profile"));
    assert!(users[0].profile.get_cached().is_none());

    crate::test_support::reset_globals();
}

#[tokio::test]
async fn eager_loading_resolves_morph_one_relations() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    crate::test_support::install_sqlite_global(&[
        EMPLOYEES,
        IMAGES,
        "INSERT INTO relation_test_employees (id, manager_id) VALUES (1, NULL), (2, 1)",
        "INSERT INTO relation_test_images (id, imageable_type, imageable_id) VALUES (1, 'relation_test_employees', 1), (2, 'relation_test_nodes', 1)",
    ])
    .await;

    let employees = RelationTestEmployee::query()
        .with("avatar")
        .get()
        .await
        .expect("eager loading a polymorphic relation should succeed");

    assert_eq!(employees.len(), 2);

    let owner = employees
        .iter()
        .find(|entry| entry.model.id == 1)
        .expect("employee 1 should be loaded");
    assert_eq!(
        owner.model.avatar.get_cached().map(|image| image.id),
        Some(1),
        "the row whose imageable_type names another table must not match"
    );
    assert!(owner.has_relation("avatar"));

    let without_avatar = employees
        .iter()
        .find(|entry| entry.model.id == 2)
        .expect("employee 2 should be loaded");
    assert!(without_avatar.model.avatar.get_cached().is_none());

    crate::test_support::reset_globals();
}

#[tokio::test]
async fn eager_loading_names_the_self_referencing_limitation() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    crate::test_support::install_sqlite_global(&[
        EMPLOYEES,
        "INSERT INTO relation_test_employees (id, manager_id) VALUES (1, NULL)",
    ])
    .await;

    let error = RelationTestEmployee::query()
        .with("manager")
        .get()
        .await
        .expect_err("self-referencing relations cannot be eager loaded");

    let message = error.to_string();
    assert!(message.contains("SelfRef"), "{message}");
    assert!(message.contains("manager"), "{message}");
    assert!(!message.contains("Unknown relation"), "{message}");

    crate::test_support::reset_globals();
}

/// A wrapper read from a payload has no key to look its rows up by, so even
/// with a database reachable `load` serves the payload.
#[tokio::test]
async fn direct_relation_load_preserves_cached_payloads_without_query_context() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    crate::test_support::install_sqlite_global(&[]).await;

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

    crate::test_support::reset_globals();
}

#[tokio::test]
async fn self_referencing_load_does_not_serve_a_deserialized_payload() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    crate::test_support::install_sqlite_global(&[
        EMPLOYEES,
        "INSERT INTO relation_test_employees (id, manager_id) VALUES (1, NULL), (2, 1), (3, 2)",
    ])
    .await;

    // A request body claiming employee 2 answers to 99 and manages 98. The
    // payload survives the model's own relation rebuild, so only `load`
    // preferring the database keeps it from being reported as stored rows.
    let employee: RelationTestEmployee = serde_json::from_value(json!({
        "id": 2,
        "manager_id": 1,
        "manager": { "id": 99, "manager_id": null, "reports": [], "avatar": null },
        "reports": [{ "id": 98, "manager_id": 2, "reports": [], "avatar": null }],
        "avatar": null
    }))
    .expect("employee should deserialize");
    assert_eq!(employee.manager.get_cached().map(|m| m.id), Some(99));

    let manager = employee
        .manager
        .load()
        .await
        .expect("loading the manager should succeed")
        .expect("employee 2 has a manager");
    assert_eq!(manager.id, 1);

    let reports = employee
        .reports
        .load()
        .await
        .expect("loading the reports should succeed");
    assert_eq!(
        reports.iter().map(|report| report.id).collect::<Vec<_>>(),
        vec![3]
    );

    crate::test_support::reset_globals();
}

#[tokio::test]
async fn morph_load_does_not_serve_a_deserialized_payload() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    crate::test_support::install_sqlite_global(&[
        IMAGES,
        "INSERT INTO relation_test_images (id, imageable_type, imageable_id) VALUES (7, 'relation_test_employees', 2)",
    ])
    .await;

    // A payload claiming employee 2 owns image 99 stands in for a request body.
    let forged = RelationTestImage {
        id: 99,
        imageable_type: "relation_test_employees".to_string(),
        imageable_id: 2,
        owner: Default::default(),
    };

    let mut one = MorphOne::<RelationTestImage>::new("imageable", "id")
        .with_parent(json!(2), "relation_test_employees".to_string());
    one.set_cached(Some(forged.clone()));
    let loaded = one
        .load()
        .await
        .expect("loading the morph-one should succeed");
    assert_eq!(loaded.map(|image| image.id), Some(7));

    let mut many = MorphMany::<RelationTestImage>::new("imageable", "id")
        .with_parent(json!(2), "relation_test_employees".to_string());
    many.set_cached(vec![forged]);
    let loaded = many
        .load()
        .await
        .expect("loading the morph-many should succeed");
    assert_eq!(
        loaded.iter().map(|image| image.id).collect::<Vec<_>>(),
        vec![7]
    );

    crate::test_support::reset_globals();
}
