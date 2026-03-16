use tideorm::{HasMany, HasOne, Model};

#[derive(Model)]
#[tide(table = "profiles")]
struct Profile {
    #[tide(primary_key)]
    pub id: i64,
    pub user_id: i64,
    pub bio: String,
}

#[derive(Model)]
#[tide(table = "posts")]
struct Post {
    #[tide(primary_key)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
}

#[derive(Model)]
#[tide(table = "users")]
struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,

    #[tide(has_one = "Profile", foreign_key = "user_id")]
    pub profile: HasOne<Profile>,

    #[tide(has_many = "Post", foreign_key = "user_id")]
    pub posts: HasMany<Post>,
}

#[test]
fn generated_serde_skips_relation_fields() {
    let user = User {
        id: 1,
        email: "alice@example.com".to_string(),
        name: "Alice".to_string(),
        profile: Default::default(),
        posts: Default::default(),
    };

    let value = serde_json::to_value(&user).unwrap();

    assert_eq!(value, serde_json::json!({
        "id": 1,
        "email": "alice@example.com",
        "name": "Alice"
    }));
    assert!(value.get("profile").is_none());
    assert!(value.get("posts").is_none());
}

#[test]
fn generated_serde_restores_relation_fields_with_defaults() {
    let user: User = serde_json::from_value(serde_json::json!({
        "id": 1,
        "email": "alice@example.com",
        "name": "Alice"
    }))
    .unwrap();

    assert_eq!(user.id, 1);
    assert_eq!(user.email, "alice@example.com");
    assert_eq!(user.name, "Alice");

    let reserialized = serde_json::to_value(&user).unwrap();
    assert_eq!(reserialized, serde_json::json!({
        "id": 1,
        "email": "alice@example.com",
        "name": "Alice"
    }));
}