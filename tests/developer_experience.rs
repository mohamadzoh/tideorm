use tideorm::prelude::*;

#[tideorm::model(table = "dx_users")]
struct User {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[tideorm(column = "display_name")]
    name: String,
    active: bool,
    age: Option<i32>,
    #[tideorm(has_many = "Post", foreign_key = "user_id")]
    posts: HasMany<Post>,
}

#[tideorm::model(table = "dx_posts")]
struct Post {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    title: String,
}

#[test]
fn constants_and_typed_values_preserve_sql() {
    assert_eq!(User::NAME.name(), "display_name");
    assert_eq!(User::POSTS, "posts");
    let typed = User::query()
        .where_eq(User::ACTIVE, true)
        .where_eq(User::NAME, "Ada")
        .where_in(User::ID, [1, 2])
        .where_between(User::ID, 1, 10)
        .where_eq(User::AGE, Some(30))
        .begin_or()
        .or_where_eq(User::ACTIVE, false)
        .and_where_gt(User::ID, 2)
        .end_or();
    let dynamic = User::query()
        .where_eq("active", true)
        .where_eq("display_name", "Ada")
        .where_in("id", [1, 2])
        .where_between("id", 1, 10)
        .where_eq("age", Some(30))
        .begin_or()
        .or_where_eq("active", false)
        .and_where_gt("id", 2)
        .end_or();
    typed.validate().unwrap();
    assert_eq!(typed.build_sql_preview(), dynamic.build_sql_preview());
}

#[tideorm::model(table = "dx_profiles")]
struct Profile {
    #[tideorm(primary_key)]
    id: i64,
    nickname: Option<String>,
    rank: i16,
    views: u64,
    score: Option<f64>,
}

#[test]
fn typed_values_accept_borrowed_nullable_and_widened_forms() {
    let ids: Vec<i64> = vec![1, 2];
    let narrow: i32 = 30;
    let typed = Profile::query()
        .where_eq(Profile::NICKNAME, "ada")
        .where_in(Profile::ID, &ids)
        .where_eq(Profile::RANK, 5)
        .where_eq(Profile::VIEWS, 7)
        .where_gt(Profile::SCORE, narrow)
        .where_between(Profile::ID, 1, &ids[1]);
    let dynamic = Profile::query()
        .where_eq("nickname", "ada")
        .where_in("id", [1, 2])
        .where_eq("rank", 5)
        .where_eq("views", 7)
        .where_gt("score", 30.0)
        .where_between("id", 1, 2);
    typed.validate().unwrap();
    assert_eq!(typed.build_sql_preview(), dynamic.build_sql_preview());
}

#[test]
fn unknown_column_lists_choices() {
    let error = User::query()
        .where_eq("actvie", true)
        .validate()
        .unwrap_err()
        .to_string();
    assert!(error.contains("actvie"), "{error}");
    assert!(error.contains("active"), "{error}");
    assert!(error.contains("display_name"), "{error}");
}

#[tokio::test]
async fn unknown_relations_fail_before_connecting() {
    for path in ["poosts", "posts.typo", "", "posts."] {
        let error = User::query()
            .with(path)
            .get()
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("Unknown relation"), "{error}");
        assert!(error.contains("valid relations:"), "{error}");
        assert!(
            error.contains(if path == "posts.typo" {
                "(none)"
            } else {
                "posts"
            }),
            "{error}"
        );
    }
}

#[test]
fn pathless_discovery_is_idempotent() {
    use tideorm::sync::SyncRegistry;
    SyncRegistry::clear();
    let _ = TideConfig::init().auto_models();
    let tables: Vec<_> = SyncRegistry::get_all_schemas()
        .into_iter()
        .map(|s| s.table_name)
        .collect();
    assert!(tables.contains(&"dx_users".to_owned()));
    assert!(tables.contains(&"dx_posts".to_owned()));
    let count = SyncRegistry::schema_count();
    let _ = TideConfig::init().auto_models();
    assert_eq!(count, SyncRegistry::schema_count());
    SyncRegistry::clear();
}

#[test]
fn one_model_registers_without_a_tuple() {
    use tideorm::sync::SyncRegistry;
    let registered = || {
        let mut tables: Vec<_> = SyncRegistry::get_all_schemas()
            .into_iter()
            .map(|s| s.table_name)
            .collect();
        tables.sort();
        tables
    };
    SyncRegistry::clear();
    let _ = TideConfig::init().models::<User>();
    assert_eq!(registered(), ["dx_users"]);
    let _ = TideConfig::init().models::<(User, Post)>();
    assert_eq!(registered(), ["dx_posts", "dx_users"]);
    SyncRegistry::clear();
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn missing_sync_registration_names_available_models() {
    use tideorm::sync::SyncRegistry;
    SyncRegistry::clear();
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let error = tideorm::sync::sync_database(&db)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("No models registered"), "{error}");
    assert!(error.contains("auto_models()"), "{error}");
    assert!(error.contains("dx_users"), "{error}");
    assert!(error.contains("dx_posts"), "{error}");
}
