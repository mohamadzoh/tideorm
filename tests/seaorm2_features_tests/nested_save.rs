#[test]
fn test_nested_save_builder_serialization() {
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    struct MockUser {
        id: i64,
        name: String,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    struct MockProfile {
        id: i64,
        user_id: i64,
        bio: String,
    }

    let user = MockUser {
        id: 0,
        name: "Test".into(),
    };
    let profile = MockProfile {
        id: 0,
        user_id: 0,
        bio: "Hello".into(),
    };

    let user_json = serde_json::to_value(&user).unwrap();
    let profile_json = serde_json::to_value(&profile).unwrap();

    assert!(user_json.is_object());
    assert!(profile_json.is_object());
}

#[test]
fn test_foreign_key_update_logic() {
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
    struct Child {
        id: i64,
        parent_id: i64,
        name: String,
    }

    let child = Child {
        id: 0,
        parent_id: 0,
        name: "Test".into(),
    };
    let mut json = serde_json::to_value(&child).unwrap();

    if let serde_json::Value::Object(ref mut map) = json {
        map.insert("parent_id".to_string(), serde_json::json!(42));
    }

    let updated: Child = serde_json::from_value(json).unwrap();
    assert_eq!(updated.parent_id, 42);
    assert_eq!(updated.name, "Test");
}

#[test]
fn test_multiple_children_update() {
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
    struct Post {
        id: i64,
        user_id: i64,
        title: String,
    }

    let posts = vec![
        Post {
            id: 0,
            user_id: 0,
            title: "First".into(),
        },
        Post {
            id: 0,
            user_id: 0,
            title: "Second".into(),
        },
        Post {
            id: 0,
            user_id: 0,
            title: "Third".into(),
        },
    ];

    let parent_id = 99i64;

    let updated: Vec<Post> = posts
        .into_iter()
        .map(|post| {
            let mut json = serde_json::to_value(&post).unwrap();
            if let serde_json::Value::Object(ref mut map) = json {
                map.insert("user_id".to_string(), serde_json::json!(parent_id));
            }
            serde_json::from_value(json).unwrap()
        })
        .collect();

    assert_eq!(updated.len(), 3);
    assert!(updated.iter().all(|p| p.user_id == 99));
    assert_eq!(updated[0].title, "First");
    assert_eq!(updated[1].title, "Second");
    assert_eq!(updated[2].title, "Third");
}
