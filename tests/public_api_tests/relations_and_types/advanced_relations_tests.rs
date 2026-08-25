use tideorm::relations::{
    MorphResult, MorphResult3, MorphResult4, RelationInfo, RelationPath, RelationTree,
    RelationType, WithPivot,
};

// =========================================================================
// RELATION TYPE TESTS
// =========================================================================

#[test]
fn test_relation_type_display_has_many_through() {
    assert_eq!(
        format!("{}", RelationType::HasManyThrough),
        "has_many_through"
    );
}

#[test]
fn test_relation_type_display_morph_to() {
    assert_eq!(format!("{}", RelationType::MorphTo), "morph_to");
}

#[test]
fn test_relation_type_display_morph_one() {
    assert_eq!(format!("{}", RelationType::MorphOne), "morph_one");
}

#[test]
fn test_relation_type_display_morph_many() {
    assert_eq!(format!("{}", RelationType::MorphMany), "morph_many");
}

#[test]
fn test_relation_type_equality() {
    assert_eq!(RelationType::HasManyThrough, RelationType::HasManyThrough);
    assert_ne!(RelationType::HasManyThrough, RelationType::HasMany);
    assert_ne!(RelationType::MorphOne, RelationType::MorphMany);
}

// =========================================================================
// RELATION INFO BUILDER TESTS
// =========================================================================

#[test]
fn test_relation_info_belongs_to_builder() {
    let info = RelationInfo::belongs_to("author", "users", "user_id", "id");

    assert_eq!(info.name, "author");
    assert_eq!(info.relation_type, RelationType::BelongsTo);
    assert_eq!(info.related_table, "users");
    assert_eq!(info.foreign_key, "user_id");
    assert_eq!(info.local_key, "id");
    assert!(info.pivot_table.is_none());
}

#[test]
fn test_relation_info_has_one_builder() {
    let info = RelationInfo::has_one("profile", "profiles", "user_id", "id");

    assert_eq!(info.name, "profile");
    assert_eq!(info.relation_type, RelationType::HasOne);
}

#[test]
fn test_relation_info_has_many_builder() {
    let info = RelationInfo::has_many("posts", "posts", "user_id", "id");

    assert_eq!(info.name, "posts");
    assert_eq!(info.relation_type, RelationType::HasMany);
}

#[test]
fn test_relation_info_has_many_through_builder() {
    let info =
        RelationInfo::has_many_through("roles", "roles", "user_roles", "user_id", "role_id", "id");

    assert_eq!(info.name, "roles");
    assert_eq!(info.relation_type, RelationType::HasManyThrough);
    assert_eq!(info.pivot_table, Some("user_roles".to_string()));
}

#[test]
fn test_relation_info_morph_one_builder() {
    let info = RelationInfo::morph_one("image", "images", "imageable_type", "imageable_id", "id");

    assert_eq!(info.name, "image");
    assert_eq!(info.relation_type, RelationType::MorphOne);
    assert_eq!(info.morph_type_column, Some("imageable_type".to_string()));
    assert_eq!(info.morph_id_column, Some("imageable_id".to_string()));
}

#[test]
fn test_relation_info_morph_many_builder() {
    let info = RelationInfo::morph_many(
        "comments",
        "comments",
        "commentable_type",
        "commentable_id",
        "id",
    );

    assert_eq!(info.name, "comments");
    assert_eq!(info.relation_type, RelationType::MorphMany);
}

// =========================================================================
// RELATION PATH TESTS (for nested eager loading)
// =========================================================================

#[test]
fn test_relation_path_simple() {
    let path = RelationPath::parse("posts");

    assert_eq!(path.full_path, "posts");
    assert_eq!(path.segments.len(), 1);
    assert_eq!(path.root(), "posts");
    assert!(!path.is_nested());
    assert_eq!(path.depth(), 1);
    assert!(path.nested().is_none());
}

#[test]
fn test_relation_path_nested() {
    let path = RelationPath::parse("posts.comments");

    assert_eq!(path.full_path, "posts.comments");
    assert_eq!(path.segments.len(), 2);
    assert_eq!(path.root(), "posts");
    assert!(path.is_nested());
    assert_eq!(path.depth(), 2);

    let nested = path.nested().unwrap();
    assert_eq!(nested.full_path, "comments");
    assert_eq!(nested.root(), "comments");
    assert!(!nested.is_nested());
}

#[test]
fn test_relation_path_deeply_nested() {
    let path = RelationPath::parse("posts.comments.author");

    assert_eq!(path.depth(), 3);
    assert!(path.is_nested());

    let nested1 = path.nested().unwrap();
    assert_eq!(nested1.root(), "comments");
    assert!(nested1.is_nested());

    let nested2 = nested1.nested().unwrap();
    assert_eq!(nested2.root(), "author");
    assert!(!nested2.is_nested());
}

#[test]
fn test_relation_path_empty() {
    let path = RelationPath::parse("");

    assert_eq!(path.depth(), 1);
    assert_eq!(path.root(), "");
}

// =========================================================================
// RELATION TREE TESTS
// =========================================================================

#[test]
fn test_relation_tree_new() {
    let tree = RelationTree::new();

    assert!(tree.is_empty());
    assert!(tree.roots().is_empty());
}

#[test]
fn test_relation_tree_add_simple_path() {
    let mut tree = RelationTree::new();
    tree.add_path(&RelationPath::parse("posts"));

    assert!(!tree.is_empty());
    let roots = tree.roots();
    assert_eq!(roots.len(), 1);
    assert!(roots.contains(&"posts".to_string()));
    assert!(!tree.has_nested("posts"));
}

#[test]
fn test_relation_tree_add_nested_path() {
    let mut tree = RelationTree::new();
    tree.add_path(&RelationPath::parse("posts.comments"));

    let roots = tree.roots();
    assert_eq!(roots.len(), 1);
    assert!(tree.has_nested("posts"));

    let nested = tree.get_nested("posts").unwrap();
    assert!(nested.roots().contains(&"comments".to_string()));
}

#[test]
fn test_relation_tree_multiple_paths() {
    let mut tree = RelationTree::new();
    tree.add_path(&RelationPath::parse("posts"));
    tree.add_path(&RelationPath::parse("profile"));
    tree.add_path(&RelationPath::parse("posts.comments"));
    tree.add_path(&RelationPath::parse("posts.comments.author"));

    let roots = tree.roots();
    assert_eq!(roots.len(), 2);
    assert!(roots.contains(&"posts".to_string()));
    assert!(roots.contains(&"profile".to_string()));

    // Profile has no nested
    assert!(!tree.has_nested("profile"));

    // Posts has nested comments
    assert!(tree.has_nested("posts"));
    let posts_nested = tree.get_nested("posts").unwrap();
    assert!(posts_nested.roots().contains(&"comments".to_string()));

    // Comments has nested author
    assert!(posts_nested.has_nested("comments"));
}

// =========================================================================
// MORPH RESULT TESTS
// =========================================================================

#[derive(Debug, Clone, PartialEq)]
struct Post {
    id: i32,
    title: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Video {
    id: i32,
    url: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Image {
    id: i32,
    path: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Audio {
    id: i32,
    file: String,
}

#[test]
fn test_morph_result_type_a() {
    let post = Post {
        id: 1,
        title: "Hello".to_string(),
    };
    let result: MorphResult<Post, Video> = MorphResult::TypeA(post.clone());

    assert!(result.is_type_a());
    assert!(!result.is_type_b());
    assert!(!result.is_unknown());
    assert_eq!(result.as_type_a(), Some(&post));
    assert_eq!(result.as_type_b(), None);
}

#[test]
fn test_morph_result_type_b() {
    let video = Video {
        id: 1,
        url: "http://example.com".to_string(),
    };
    let result: MorphResult<Post, Video> = MorphResult::TypeB(video.clone());

    assert!(!result.is_type_a());
    assert!(result.is_type_b());
    assert_eq!(result.as_type_b(), Some(&video));
}

#[test]
fn test_morph_result_unknown() {
    let result: MorphResult<Post, Video> =
        MorphResult::Unknown(serde_json::json!({"type": "document"}));

    assert!(!result.is_type_a());
    assert!(!result.is_type_b());
    assert!(result.is_unknown());
}

#[test]
fn test_morph_result_into_type_a() {
    let post = Post {
        id: 1,
        title: "Hello".to_string(),
    };
    let result: MorphResult<Post, Video> = MorphResult::TypeA(post.clone());

    assert_eq!(result.into_type_a(), Some(post));
}

#[test]
fn test_morph_result_into_type_b() {
    let video = Video {
        id: 1,
        url: "http://example.com".to_string(),
    };
    let result: MorphResult<Post, Video> = MorphResult::TypeB(video.clone());

    assert_eq!(result.into_type_b(), Some(video));
}

#[test]
fn test_morph_result3() {
    let _result: MorphResult3<Post, Video, Image> = MorphResult3::TypeA(Post {
        id: 1,
        title: "Test".to_string(),
    });
    let _result: MorphResult3<Post, Video, Image> = MorphResult3::TypeB(Video {
        id: 1,
        url: "url".to_string(),
    });
    let _result: MorphResult3<Post, Video, Image> = MorphResult3::TypeC(Image {
        id: 1,
        path: "path".to_string(),
    });
    let _result: MorphResult3<Post, Video, Image> = MorphResult3::Unknown(serde_json::json!({}));
}

#[test]
fn test_morph_result4() {
    let _result: MorphResult4<Post, Video, Image, Audio> = MorphResult4::TypeA(Post {
        id: 1,
        title: "Test".to_string(),
    });
    let _result: MorphResult4<Post, Video, Image, Audio> = MorphResult4::TypeB(Video {
        id: 1,
        url: "url".to_string(),
    });
    let _result: MorphResult4<Post, Video, Image, Audio> = MorphResult4::TypeC(Image {
        id: 1,
        path: "path".to_string(),
    });
    let _result: MorphResult4<Post, Video, Image, Audio> = MorphResult4::TypeD(Audio {
        id: 1,
        file: "file".to_string(),
    });
    let _result: MorphResult4<Post, Video, Image, Audio> =
        MorphResult4::Unknown(serde_json::json!({}));
}

// =========================================================================
// WITH PIVOT TESTS
// =========================================================================

#[derive(Debug, Clone)]
struct Role {
    id: i32,
    name: String,
}

#[derive(Debug, Clone)]
struct UserRolePivot {
    assigned_at: String,
    role_level: i32,
}

#[test]
fn test_with_pivot_creation() {
    let role = Role {
        id: 1,
        name: "Admin".to_string(),
    };
    let pivot = UserRolePivot {
        assigned_at: "2024-01-01".to_string(),
        role_level: 10,
    };

    let with_pivot = WithPivot::new(role.clone(), pivot.clone());

    assert_eq!(with_pivot.model.id, 1);
    assert_eq!(with_pivot.model.name, "Admin");
    assert_eq!(with_pivot.pivot.assigned_at, "2024-01-01");
    assert_eq!(with_pivot.pivot.role_level, 10);
}

#[test]
fn test_with_pivot_deref() {
    let role = Role {
        id: 1,
        name: "Admin".to_string(),
    };
    let pivot = UserRolePivot {
        assigned_at: "2024-01-01".to_string(),
        role_level: 10,
    };

    let with_pivot = WithPivot::new(role, pivot);

    // Test Deref - can access model fields directly
    assert_eq!(with_pivot.id, 1);
    assert_eq!(with_pivot.name, "Admin");
}

#[test]
fn test_with_pivot_into_parts() {
    let role = Role {
        id: 1,
        name: "Admin".to_string(),
    };
    let pivot = UserRolePivot {
        assigned_at: "2024-01-01".to_string(),
        role_level: 10,
    };

    let with_pivot = WithPivot::new(role, pivot);
    let (model, pivot) = with_pivot.into_parts();

    assert_eq!(model.id, 1);
    assert_eq!(pivot.role_level, 10);
}
