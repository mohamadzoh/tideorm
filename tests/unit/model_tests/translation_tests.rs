#[cfg(feature = "translations")]
use super::*;

#[cfg(feature = "translations")]
#[test]
fn test_load_language_translations_updates_model_fields() {
    let mut model = TranslationSerializationModel {
        id: 1,
        title: "Default Title".to_string(),
        translations: Some(serde_json::json!({
            "title": {
                "en": "English Title",
                "fr": "French Title"
            }
        })),
    };

    model.load_language_translations("fr").unwrap();

    assert_eq!(model.title, "French Title");
}

#[cfg(feature = "translations")]
#[test]
fn test_load_language_translations_preserves_loaded_relations() {
    let cached_profile = TranslationRelationProfile {
        id: 10,
        user_id: 1,
        bio: "Cached profile".to_string(),
    }
    .with_relations();
    let cached_post = TranslationRelationPost {
        id: 20,
        user_id: 1,
        title: "Cached post".to_string(),
        author: Default::default(),
    }
    .with_relations();

    let mut model = TranslationRelationUser {
        id: 1,
        title: "Default Title".to_string(),
        translations: Some(serde_json::json!({
            "title": {
                "en": "English Title",
                "fr": "French Title"
            }
        })),
        profile: Default::default(),
        posts: Default::default(),
        roles: Default::default(),
    }
    .with_relations();
    model.profile.set_cached(Some(cached_profile));
    model.posts.set_cached(vec![cached_post]);

    model.load_language_translations("fr").unwrap();

    assert_eq!(model.title, "French Title");
    assert_eq!(
        model.profile.get_cached().map(|profile| profile.id),
        Some(10)
    );
    assert_eq!(model.posts.get_cached().map(|posts| posts.len()), Some(1));
    assert_eq!(model.posts.get_cached().unwrap()[0].id, 20);
}

#[cfg(feature = "translations")]
#[test]
fn test_load_language_translations_preserves_loaded_has_many_through_relations() {
    let cached_role = TranslationRelationRole {
        id: 30,
        name: "Cached role".to_string(),
    }
    .with_relations();

    let mut model = TranslationRelationUser {
        id: 1,
        title: "Default Title".to_string(),
        translations: Some(serde_json::json!({
            "title": {
                "en": "English Title",
                "fr": "French Title"
            }
        })),
        profile: Default::default(),
        posts: Default::default(),
        roles: Default::default(),
    }
    .with_relations();
    model.roles.set_cached(vec![cached_role]);

    model.load_language_translations("fr").unwrap();

    assert_eq!(model.title, "French Title");
    assert_eq!(model.roles.get_cached().map(|roles| roles.len()), Some(1));
    assert_eq!(model.roles.get_cached().unwrap()[0].id, 30);
}

#[cfg(feature = "translations")]
#[test]
fn test_with_relations_preserves_deserialized_cached_relations() {
    let model: TranslationRelationUser = serde_json::from_value(serde_json::json!({
        "id": 1,
        "title": "Default Title",
        "translations": null,
        "profile": {
            "id": 10,
            "user_id": 1,
            "bio": "Cached profile"
        },
        "posts": [
            {
                "id": 20,
                "user_id": 1,
                "title": "Cached post",
                "author": null
            }
        ],
        "roles": [
            {
                "id": 30,
                "name": "Cached role"
            }
        ]
    }))
    .expect("model should deserialize with cached relations");

    assert_eq!(model.profile.foreign_key, "user_id");
    assert_eq!(model.profile.local_key, "id");
    assert_eq!(
        model.profile.get_cached().map(|profile| profile.id),
        Some(10)
    );

    assert_eq!(model.posts.foreign_key, "user_id");
    assert_eq!(model.posts.local_key, "id");
    assert_eq!(model.posts.get_cached().map(|posts| posts.len()), Some(1));
    assert_eq!(model.posts.get_cached().unwrap()[0].id, 20);

    assert_eq!(model.roles.foreign_key, "user_id");
    assert_eq!(model.roles.related_key, "role_id");
    assert_eq!(model.roles.get_cached().map(|roles| roles.len()), Some(1));
    assert_eq!(model.roles.get_cached().unwrap()[0].id, 30);
}

#[cfg(feature = "translations")]
#[test]
fn test_model_relation_serialization_round_trip_preserves_cached_relations() {
    let cached_profile = TranslationRelationProfile {
        id: 10,
        user_id: 1,
        bio: "Cached profile".to_string(),
    }
    .with_relations();
    let cached_post = TranslationRelationPost {
        id: 20,
        user_id: 1,
        title: "Cached post".to_string(),
        author: Default::default(),
    }
    .with_relations();
    let cached_role = TranslationRelationRole {
        id: 30,
        name: "Cached role".to_string(),
    }
    .with_relations();

    let mut model = TranslationRelationUser {
        id: 1,
        title: "Default Title".to_string(),
        translations: None,
        profile: Default::default(),
        posts: Default::default(),
        roles: Default::default(),
    }
    .with_relations();
    model.profile.set_cached(Some(cached_profile));
    model.posts.set_cached(vec![cached_post]);
    model.roles.set_cached(vec![cached_role]);

    let value = serde_json::to_value(&model).expect("model should serialize with cached relations");
    let round_trip: TranslationRelationUser = serde_json::from_value(value)
        .expect("model should deserialize after serialization round trip");

    assert_eq!(
        round_trip.profile.get_cached().map(|profile| profile.id),
        Some(10)
    );
    assert_eq!(
        round_trip.posts.get_cached().map(|posts| posts.len()),
        Some(1)
    );
    assert_eq!(round_trip.posts.get_cached().unwrap()[0].id, 20);
    assert_eq!(
        round_trip.roles.get_cached().map(|roles| roles.len()),
        Some(1)
    );
    assert_eq!(round_trip.roles.get_cached().unwrap()[0].id, 30);
}
