use super::*;

/// The derive wires each wrapper's columns into `Default` and
/// `with_relations()`, and `with_relations()` hands each its key, so a load
/// reaches the query instead of failing on a missing column or key.
#[tokio::test]
async fn macro_generated_morph_and_self_ref_relations_are_configured() {
    let defaulted = RelationTestEmployee::default();
    let wired = RelationTestEmployee {
        id: 42,
        manager_id: Some(7),
        manager: Default::default(),
        reports: Default::default(),
        avatar: Default::default(),
    }
    .with_relations();

    for employee in [&defaulted, &wired] {
        assert_eq!(employee.manager.foreign_key, "manager_id");
        assert_eq!(employee.manager.local_key, "id");
        assert_eq!(employee.reports.foreign_key, "manager_id");
        assert_eq!(employee.reports.local_key, "id");
        assert_eq!(employee.avatar.morph_name, "imageable");
        assert_eq!(employee.avatar.local_key, "id");
    }
    let image = RelationTestImage::default();
    assert_eq!(image.owner.type_column, "imageable_type");
    assert_eq!(image.owner.id_column, "imageable_id");

    // Each load reaches its query, which fails with no table (or connection)
    // to read; a `SelfRef` without a key would answer `Ok(None)` instead.
    wired
        .manager
        .load()
        .await
        .expect_err("the manager's key should be set");
    let reports_err = wired.reports.load().await.unwrap_err().to_string();
    assert!(
        !reports_err.contains("Parent primary key not set"),
        "{reports_err}"
    );
    let avatar_err = wired.avatar.load().await.unwrap_err().to_string();
    assert!(!avatar_err.contains("not configured"), "{avatar_err}");
    assert!(
        !avatar_err.contains("Parent primary key not set"),
        "{avatar_err}"
    );
}

#[test]
fn morph_to_is_wired_to_its_columns_and_their_values() {
    let image = RelationTestImage {
        id: 9,
        imageable_type: "relation_test_nodes".to_string(),
        imageable_id: 4,
        owner: Default::default(),
    }
    .with_relations();

    assert_eq!(image.owner.type_column, "imageable_type");
    assert_eq!(image.owner.id_column, "imageable_id");
    assert_eq!(image.owner.type_value(), Some("relation_test_nodes"));
    assert_eq!(image.owner.id_value(), Some(&Value::BigInt(Some(4))));
    assert!(image.owner.is_type::<RelationTestNode>());
    assert!(!image.owner.is_type::<RelationTestPivot>());
}

#[tokio::test]
async fn morph_to_load_rejects_a_mismatched_target_type() {
    let error = MorphTo::<RelationTestNode>::new("imageable_type", "imageable_id")
        .with_values("relation_test_pivots".to_string(), 4_i64)
        .load()
        .await
        .expect_err("a discriminator naming another table must not silently return None");

    assert!(
        error.to_string().contains("relation_test_pivots"),
        "{error}"
    );
    assert!(error.to_string().contains("load_as"), "{error}");
}

#[tokio::test]
async fn morph_to_load_as_returns_none_for_another_target_type() {
    let owner = MorphTo::<RelationTestNode>::new("imageable_type", "imageable_id")
        .with_values("relation_test_pivots".to_string(), 4_i64);

    assert!(
        owner
            .load_as::<RelationTestNode>()
            .await
            .expect("a type mismatch is not an error for load_as")
            .is_none()
    );
}

#[test]
fn morph_to_rebuild_keeps_the_values_its_columns_now_hold() {
    let mut image = RelationTestImage {
        id: 9,
        imageable_type: "relation_test_nodes".to_string(),
        imageable_id: 4,
        owner: Default::default(),
    }
    .with_relations();

    image.imageable_type = "relation_test_pivots".to_string();
    image.imageable_id = 5;
    let image = image.with_relations();

    // Rebuilding used to copy the previous wrapper's values back over the ones
    // just read from the columns, so the relation kept pointing at the old owner.
    assert_eq!(image.owner.type_value(), Some("relation_test_pivots"));
    assert_eq!(image.owner.id_value(), Some(&Value::BigInt(Some(5))));
}

/// A folder tree keyed by a path the folder holds, not by its primary key.
#[tideorm::model(table = "relation_test_folders")]
struct RelationTestFolder {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    path: String,
    parent_path: Option<String>,
    #[tideorm(foreign_key = "parent_path", local_key = "path")]
    children: SelfRefMany<RelationTestFolder>,
}

/// The owner's own `is_new()`, not a guess from its key's value, says whether
/// the rows cached in its relation are the caller's: they carry over to a
/// wrapper rebuilt under another key only while the owner was new. A new
/// folder's path is not a placeholder, yet its children are still unsaved.
#[test]
fn the_owner_says_whether_a_rebuilt_relation_keeps_its_cached_rows() {
    let folder = |id, path: &str| RelationTestFolder {
        id,
        path: path.to_string(),
        ..Default::default()
    };
    let rebuilt_after_a_rename = |id| {
        let mut owner = folder(id, "draft").with_relations();
        owner.children.clear();
        let children = owner.children.as_mut().expect("cleared is loaded");
        children.push(folder(0, "draft/a"));
        // A `before_create` hook may rename it; a stored row may be renamed.
        owner.path = "docs".to_string();
        owner.with_relations()
    };

    let created = rebuilt_after_a_rename(0);
    assert_eq!(created.children.get_cached().map(<[_]>::len), Some(1));

    let renamed = rebuilt_after_a_rename(9);
    assert!(!renamed.children.is_loaded());
}
