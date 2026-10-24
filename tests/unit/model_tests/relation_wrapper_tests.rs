use crate::internal::InternalModel;
use serde_json::json;

#[tideorm::model(table = "model_test_relation_profiles")]
struct RelationProfile {
    #[tideorm(primary_key)]
    id: i64,
    user_id: i64,
    bio: String,
}

#[tideorm::model(table = "model_test_relation_posts")]
struct RelationPost {
    #[tideorm(primary_key)]
    id: i64,
    user_id: i64,
    title: String,

    #[tideorm(belongs_to = "RelationUser", foreign_key = "user_id")]
    author: crate::relations::BelongsTo<RelationUser>,
}

#[tideorm::model(table = "model_test_relation_roles")]
struct RelationRole {
    #[tideorm(primary_key)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "model_test_relation_user_roles")]
struct RelationUserRole {
    #[tideorm(primary_key)]
    id: i64,
    user_id: i64,
    role_id: i64,
}

#[tideorm::model(table = "model_test_relation_users")]
struct RelationUser {
    #[tideorm(primary_key)]
    id: i64,
    title: String,

    #[tideorm(has_one = "RelationProfile", foreign_key = "user_id")]
    profile: crate::relations::HasOne<RelationProfile>,

    #[tideorm(has_many = "RelationPost", foreign_key = "user_id")]
    posts: crate::relations::HasMany<RelationPost>,

    #[tideorm(
        has_many_through = "RelationRole",
        pivot = "model_test_relation_user_roles",
        foreign_key = "user_id",
        related_key = "role_id"
    )]
    roles: crate::relations::HasManyThrough<RelationRole, RelationUserRole>,
}

fn user() -> RelationUser {
    RelationUser {
        id: 1,
        title: "Title".to_string(),
        profile: Default::default(),
        posts: Default::default(),
        roles: Default::default(),
    }
    .with_relations()
}

/// Post 20 by user 1, holding its author.
fn post_with_author() -> RelationPost {
    let mut post = RelationPost {
        id: 20,
        user_id: 1,
        title: "Cached post".to_string(),
        author: Default::default(),
    }
    .with_relations();
    post.author.set_cached(Some(user()));
    post
}

/// User 1 holding profile 10, post 20 and role 30.
fn user_with_cached_relations() -> RelationUser {
    let mut user = user();
    user.profile.set_cached(Some(
        RelationProfile {
            id: 10,
            user_id: 1,
            bio: "Cached profile".to_string(),
        }
        .with_relations(),
    ));
    user.posts.set_cached(vec![post_with_author()]);
    user.roles.set_cached(vec![
        RelationRole {
            id: 30,
            name: "Cached role".to_string(),
        }
        .with_relations(),
    ]);
    user
}

fn post_ids(posts: Option<&[RelationPost]>) -> Option<Vec<i64>> {
    posts.map(|posts| posts.iter().map(|post| post.id).collect())
}

fn role_ids(roles: Option<&[RelationRole]>) -> Option<Vec<i64>> {
    roles.map(|roles| roles.iter().map(|role| role.id).collect())
}

/// `set_field_json` writes one field in place and leaves every relation
/// wrapper, with the rows it holds, as it was.
#[test]
fn set_field_json_preserves_loaded_relations() {
    let mut user = user_with_cached_relations();
    assert!(user.set_field_json("title", json!("Renamed")).unwrap());

    assert_eq!(user.title, "Renamed");
    assert_eq!(
        user.profile.get_cached().map(|profile| profile.id),
        Some(10)
    );
    assert_eq!(post_ids(user.posts.get_cached()), Some(vec![20]));
    assert_eq!(role_ids(user.roles.get_cached()), Some(vec![30]));

    let mut post = post_with_author();
    assert!(post.set_field_json("title", json!("Renamed")).unwrap());

    assert_eq!(post.title, "Renamed");
    assert_eq!(post.author.get_cached().map(|author| author.id), Some(1));
}

/// Serde carries each relation's cached rows, and the model it reads back
/// wires its wrappers from its own columns with those rows in place.
#[test]
fn a_serialization_round_trip_keeps_cached_relations() {
    let value = serde_json::to_value(user_with_cached_relations())
        .expect("model should serialize with cached relations");
    assert_eq!(value["profile"]["id"], 10, "{value}");
    assert_eq!(value["posts"][0]["author"]["id"], 1, "{value}");
    assert_eq!(value["roles"][0]["id"], 30, "{value}");

    let user: RelationUser = serde_json::from_value(value)
        .expect("model should deserialize after serialization round trip");

    assert_eq!(user.profile.foreign_key, "user_id");
    assert_eq!(user.profile.local_key, "id");
    assert_eq!(
        user.profile.get_cached().map(|profile| profile.id),
        Some(10)
    );

    assert_eq!(user.posts.foreign_key, "user_id");
    assert_eq!(user.posts.local_key, "id");
    assert_eq!(post_ids(user.posts.get_cached()), Some(vec![20]));
    let post = &user.posts.get_cached().expect("the posts should be cached")[0];
    assert_eq!(post.author.foreign_key, "user_id");
    assert_eq!(post.author.owner_key, "id");
    assert_eq!(post.author.get_cached().map(|author| author.id), Some(1));

    assert_eq!(user.roles.foreign_key, "user_id");
    assert_eq!(user.roles.related_key, "role_id");
    assert_eq!(role_ids(user.roles.get_cached()), Some(vec![30]));
}
