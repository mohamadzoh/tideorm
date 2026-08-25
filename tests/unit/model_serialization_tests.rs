use super::{hash_map_output_key, to_json};
use serde_json::json;

#[test]
fn hash_map_output_key_hides_structured_presenter_params() {
    assert_eq!(
        hash_map_output_key("params", &json!({"view": "minimal"})),
        None
    );
    assert_eq!(hash_map_output_key("params", &json!(["minimal"])), None);
    assert_eq!(hash_map_output_key("title", &json!("title")), Some("title"));
}

#[test]
fn hash_map_output_key_preserves_scalar_params_values() {
    assert_eq!(
        hash_map_output_key("params", &json!("keep me")),
        Some("params")
    );
}

// Three models wired into a two-hop chain (post -> author -> profile) so the
// eager-loaded payloads `to_json` has to filter are reachable in-process.
//
// The hidden lists deliberately disagree: `internal_notes` is hidden on the post
// and *not* on the author, which is what tells the two failure modes apart —
// filtering a nested payload with the parent's list both leaks the target's
// secrets and eats columns the target never hid.

#[tideorm::model(table = "serialization_test_profiles", hidden = "secret_token")]
struct SerializationProfile {
    #[tideorm(primary_key)]
    id: i64,
    author_id: i64,
    bio: String,
    secret_token: String,
}

#[tideorm::model(table = "serialization_test_authors", hidden = "password_hash")]
struct SerializationAuthor {
    #[tideorm(primary_key)]
    id: i64,
    name: String,
    password_hash: String,
    internal_notes: String,

    #[tideorm(has_one = "SerializationProfile", foreign_key = "author_id")]
    profile: crate::relations::HasOne<SerializationProfile>,

    #[tideorm(has_many = "SerializationPost", foreign_key = "author_id")]
    posts: crate::relations::HasMany<SerializationPost>,
}

#[tideorm::model(table = "serialization_test_posts", hidden = "internal_notes")]
struct SerializationPost {
    #[tideorm(primary_key)]
    id: i64,
    author_id: i64,
    title: String,
    internal_notes: String,

    #[tideorm(belongs_to = "SerializationAuthor", foreign_key = "author_id")]
    author: crate::relations::BelongsTo<SerializationAuthor>,
}

fn serialization_test_author() -> SerializationAuthor {
    SerializationAuthor {
        id: 7,
        name: "Ada".to_string(),
        password_hash: "super-secret".to_string(),
        internal_notes: "author notes".to_string(),
        profile: Default::default(),
        posts: Default::default(),
    }
    .with_relations()
}

fn serialization_test_post() -> SerializationPost {
    SerializationPost {
        id: 1,
        author_id: 7,
        title: "Hello".to_string(),
        internal_notes: "post notes".to_string(),
        author: Default::default(),
    }
    .with_relations()
}

#[test]
fn to_json_filters_a_relation_payload_with_the_target_models_hidden_list() {
    let mut post = serialization_test_post();
    post.author.set_cached(Some(serialization_test_author()));

    let value = to_json(&post, None);

    assert_eq!(value.get("internal_notes"), None);

    let author = value
        .get("author")
        .expect("the cached author should be serialized");
    assert_eq!(
        author.get("password_hash"),
        None,
        "the author payload must be filtered by SerializationAuthor::hidden_attributes()"
    );
    assert_eq!(author.get("name"), Some(&json!("Ada")));
    assert_eq!(
        author.get("internal_notes"),
        Some(&json!("author notes")),
        "the post's hidden list must not be applied to the author payload"
    );
}

#[test]
fn to_json_filters_relation_payloads_at_every_depth() {
    let profile = SerializationProfile {
        id: 3,
        author_id: 7,
        bio: "Writes things".to_string(),
        secret_token: "tok_live".to_string(),
    }
    .with_relations();

    let mut author = serialization_test_author();
    author.profile.set_cached(Some(profile));

    let mut post = serialization_test_post();
    post.author.set_cached(Some(author));

    let value = to_json(&post, None);
    let profile = value
        .get("author")
        .and_then(|author| author.get("profile"))
        .expect("the nested profile should be serialized");

    assert_eq!(
        profile.get("secret_token"),
        None,
        "a payload two relations deep is still filtered by its own model's list"
    );
    assert_eq!(profile.get("bio"), Some(&json!("Writes things")));
}

#[test]
fn to_json_filters_every_element_of_a_has_many_payload() {
    let mut author = serialization_test_author();
    author.posts.set_cached(vec![serialization_test_post()]);

    let value = to_json(&author, None);

    assert_eq!(value.get("password_hash"), None);

    let posts = value
        .get("posts")
        .and_then(serde_json::Value::as_array)
        .expect("the cached posts should be serialized as an array");
    assert_eq!(posts.len(), 1);
    assert_eq!(
        posts[0].get("internal_notes"),
        None,
        "each element must be filtered by SerializationPost::hidden_attributes()"
    );
    assert_eq!(posts[0].get("title"), Some(&json!("Hello")));
}
