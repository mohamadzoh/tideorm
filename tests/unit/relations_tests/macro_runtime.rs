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
    assert_eq!(image.owner.id_value(), Some(&serde_json::json!(4)));
    assert!(image.owner.is_type::<RelationTestNode>());
    assert!(!image.owner.is_type::<RelationTestPivot>());
}

#[tokio::test]
async fn morph_to_load_rejects_a_mismatched_target_type() {
    let error = MorphTo::<RelationTestNode>::new("imageable_type", "imageable_id")
        .with_values("relation_test_pivots".to_string(), serde_json::json!(4))
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
        .with_values("relation_test_pivots".to_string(), serde_json::json!(4));

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
    assert_eq!(image.owner.id_value(), Some(&serde_json::json!(5)));
}

#[test]
fn with_relations_round_trips_its_own_serialization() {
    use crate::relations::WithRelations;

    let mut wrapped = WithRelations::new(RelationTestNode {
        id: 1,
        slug: "root".to_string(),
        parent_slug: None,
    });
    wrapped
        .set_relation("children", &Vec::<RelationTestNode>::new())
        .expect("recording a relation payload should succeed");

    let json = serde_json::to_value(&wrapped).expect("WithRelations should serialize");
    let restored: WithRelations<RelationTestNode> =
        serde_json::from_value(json).expect("WithRelations should deserialize its own output");
    assert!(restored.has_relation("children"));

    let bare = WithRelations::new(RelationTestNode {
        id: 2,
        slug: "leaf".to_string(),
        parent_slug: None,
    });
    let json = serde_json::to_value(&bare).expect("WithRelations should serialize");
    let restored: WithRelations<RelationTestNode> = serde_json::from_value(json)
        .expect("an empty relation map is omitted and must deserialize back");
    assert!(!restored.has_relation("children"));
}
