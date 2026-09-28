#[cfg(feature = "attachments")]
use super::*;

#[cfg(feature = "attachments")]
#[test]
fn test_set_field_json_preserves_loaded_belongs_to_relation() {
    let cached_author = AttachmentRelationUser {
        id: 7,
        name: "Cached author".to_string(),
    }
    .with_relations();
    let mut model = AttachmentRelationPost {
        id: 3,
        user_id: 7,
        files: None,
        author: Default::default(),
    }
    .with_relations();
    model.author.set_cached(Some(cached_author));

    crate::internal::InternalModel::set_field_json(
        &mut model,
        "files",
        serde_json::json!({ "thumbnail": { "key": "uploads/example.png" } }),
    )
    .unwrap();

    assert_eq!(model.author.get_cached().map(|author| author.id), Some(7));
    assert_eq!(
        model.files,
        Some(serde_json::json!({
            "thumbnail": {
                "key": "uploads/example.png"
            }
        }))
    );
}

#[cfg(feature = "attachments")]
#[test]
fn test_with_relations_preserves_deserialized_belongs_to_cache() {
    let model: AttachmentRelationPost = serde_json::from_value(serde_json::json!({
        "id": 3,
        "user_id": 7,
        "files": null,
        "author": {
            "id": 7,
            "name": "Cached author"
        }
    }))
    .expect("attachment relation model should deserialize with cached author");

    assert_eq!(model.author.foreign_key, "user_id");
    assert_eq!(model.author.owner_key, "id");
    assert_eq!(model.author.get_cached().map(|author| author.id), Some(7));
}
