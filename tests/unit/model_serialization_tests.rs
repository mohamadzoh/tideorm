use super::{hash_map_output_key, to_json};
use serde_json::json;

#[test]
fn hash_map_output_key_hides_structured_params() {
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

#[cfg(feature = "translations")]
#[tideorm::model(
    table = "serialization_test_hidden_translations",
    translatable = "title",
    hidden = "title"
)]
struct HiddenTranslatableModel {
    #[tideorm(primary_key)]
    id: i64,
    title: String,
    translations: Option<serde_json::Value>,
}

#[cfg(feature = "translations")]
#[test]
fn to_json_keeps_a_hidden_translatable_field_hidden() {
    let model = HiddenTranslatableModel {
        id: 1,
        title: "internal".to_string(),
        translations: Some(json!({"title": {"en": "internal, translated"}})),
    };

    let payload = to_json(&model, None);

    assert!(payload.get("title").is_none(), "{payload}");
    assert!(payload.get("translations").is_none(), "{payload}");
    assert_eq!(payload["id"], json!(1));
}

#[cfg(feature = "attachments")]
#[tideorm::model(
    table = "serialization_test_hidden_attachments",
    has_one_files = "avatar,cover",
    hidden = "avatar"
)]
struct HiddenAttachmentModel {
    #[tideorm(primary_key)]
    id: i64,
    files: Option<serde_json::Value>,
}

#[cfg(feature = "attachments")]
#[test]
fn to_json_keeps_a_hidden_attachment_hidden() {
    let attachment =
        |key: &str| json!({"key": key, "filename": key, "created_at": "2026-01-01T00:00:00Z"});
    let model = HiddenAttachmentModel {
        id: 1,
        files: Some(json!({"avatar": attachment("a.png"), "cover": attachment("c.png")})),
    };

    let payload = to_json(&model, None);

    assert!(payload.get("avatar").is_none(), "{payload}");
    assert!(payload.get("cover").is_some(), "{payload}");
    assert!(payload.get("files").is_none(), "{payload}");
}

#[tideorm::model(
    table = "serialization_test_removed_rows",
    soft_delete,
    deleted_at_column = "removed_at"
)]
struct RemovedAtModel {
    #[tideorm(primary_key)]
    id: i64,
    removed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[tideorm::model(table = "serialization_test_secrets", hidden = "secret_column")]
struct HiddenByColumnModel {
    #[tideorm(primary_key)]
    id: i64,
    #[tideorm(column = "secret_column")]
    secret: String,
}

#[test]
fn hidden_fields_default_to_the_soft_delete_field_and_resolve_column_names() {
    use crate::model::ModelMeta;

    assert_eq!(RemovedAtModel::hidden_attributes(), ["removed_at"]);
    let payload = to_json(
        &RemovedAtModel {
            id: 1,
            removed_at: Some(chrono::Utc::now()),
        },
        None,
    );
    assert!(payload.get("removed_at").is_none(), "{payload}");

    // `hidden` names the column; the payload key is the field's.
    assert_eq!(HiddenByColumnModel::hidden_attributes(), ["secret"]);
    let payload = to_json(
        &HiddenByColumnModel {
            id: 1,
            secret: "s".to_string(),
        },
        None,
    );
    assert!(payload.get("secret").is_none(), "{payload}");
}

#[tideorm::model(table = "serialization_test_indexed")]
#[index("display_name")]
struct IndexedByFieldModel {
    #[tideorm(primary_key)]
    id: i64,
    #[tideorm(column = "display")]
    display_name: String,
}

#[test]
fn an_index_naming_a_field_indexes_its_column() {
    use crate::model::ModelMeta;

    let indexes = IndexedByFieldModel::indexes();
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].columns, ["display"]);
}

#[tideorm::model(table = "serialization_test_field_indexed")]
struct FieldIndexedModel {
    #[tideorm(primary_key)]
    id: i64,
    #[index]
    name: String,
    #[unique_index]
    #[tideorm(column = "mail")]
    email: String,
}

/// `#[index]` and `#[unique_index]` on a field index its column; they were
/// accepted there and ignored.
#[test]
fn a_field_index_attribute_indexes_the_fields_column() {
    use crate::model::ModelMeta;

    let indexes = FieldIndexedModel::indexes();
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].columns, ["name"]);
    let unique = FieldIndexedModel::unique_indexes();
    assert_eq!(unique.len(), 1);
    assert_eq!(unique[0].columns, ["mail"]);
    assert!(unique[0].unique);
}

#[tideorm::model(table = "serialization_test_sequence_owners")]
struct SequenceOwner {
    #[tideorm(primary_key)]
    id: i64,
    #[tideorm(has_many = "SequenceChild", foreign_key = "owner_id")]
    children: crate::relations::HasMany<SequenceChild>,
    email: String,
}

#[tideorm::model(table = "serialization_test_sequence_children")]
struct SequenceChild {
    #[tideorm(primary_key)]
    id: i64,
    owner_id: i64,
}

/// A sequence holds the columns first, the relations after, as `Serialize`
/// writes them for a format that reads fields by position; the relation
/// declared between two columns was read in its place.
#[test]
fn a_model_reads_a_sequence_in_the_order_serialize_writes_one() {
    let owner: SequenceOwner = serde_json::from_value(serde_json::json!([7, "a@example.com"]))
        .expect("the sequence reads");
    assert_eq!((owner.id, owner.email.as_str()), (7, "a@example.com"));
    assert!(owner.children.get_cached().is_none());

    let owner: SequenceOwner = serde_json::from_value(
        serde_json::json!([7, "a@example.com", [{ "id": 1, "owner_id": 7 }]]),
    )
    .expect("the sequence with a relation reads");
    assert_eq!(
        owner.children.get_cached().map(|children| children.len()),
        Some(1)
    );
}
