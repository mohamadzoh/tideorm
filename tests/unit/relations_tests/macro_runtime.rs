use super::*;

#[tokio::test]
async fn advanced_relation_helpers_reject_composite_keys() {
    let composite_key = serde_json::json!([1, 2]);

    let err = HasManyThrough::<RelationTestNode, RelationTestPivot>::new(
        "left_id",
        "right_id",
        "id",
        "id",
        "relation_test_pivots",
    )
    .with_parent_pk(composite_key.clone())
    .load()
    .await
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("HasManyThrough::load only supports scalar relation keys")
    );

    let err = MorphOne::<RelationTestNode>::new("nodeable", "id")
        .with_parent(composite_key.clone(), String::from("relation_test_nodes"))
        .load()
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("MorphOne::load only supports scalar relation keys")
    );

    let err = SelfRefMany::<RelationTestNode>::new("parent_slug", "slug")
        .with_parent_pk(composite_key)
        .load_tree(2)
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("SelfRefMany::load_tree only supports scalar relation keys")
    );
}

#[tokio::test]
async fn macro_generated_morph_and_self_ref_relations_are_configured() {
    let employee = RelationTestEmployee {
        id: 42,
        manager_id: Some(7),
        manager: Default::default(),
        reports: Default::default(),
        avatar: Default::default(),
    }
    .with_relations();

    assert_eq!(employee.manager.foreign_key, "manager_id");
    assert_eq!(employee.manager.local_key, "id");
    assert_eq!(employee.reports.foreign_key, "manager_id");
    assert_eq!(employee.reports.local_key, "id");
    assert_eq!(employee.avatar.morph_name, "imageable");
    assert_eq!(employee.avatar.local_key, "id");

    let manager_err = employee.manager.load().await.unwrap_err();
    assert!(
        !manager_err
            .to_string()
            .contains("Foreign key value not set")
    );

    let reports_err = employee.reports.load().await.unwrap_err();
    assert!(
        !reports_err
            .to_string()
            .contains("Parent primary key not set")
    );

    let avatar_err = employee.avatar.load().await.unwrap_err();
    assert!(
        !avatar_err
            .to_string()
            .contains("MorphOne relation is not configured")
    );
    assert!(
        !avatar_err
            .to_string()
            .contains("Parent primary key not set")
    );
}

#[test]
fn macro_generated_morph_to_relation_is_configured() {
    let image = RelationTestImage {
        id: 9,
        imageable_type: "relation_test_nodes".to_string(),
        imageable_id: 4,
        owner: Default::default(),
    }
    .with_relations();

    assert_eq!(image.owner.type_column, "imageable_type");
    assert_eq!(image.owner.id_column, "imageable_id");
}

#[test]
fn morph_to_exposes_the_stored_type_and_id() {
    let image = RelationTestImage {
        id: 9,
        imageable_type: "relation_test_nodes".to_string(),
        imageable_id: 4,
        owner: Default::default(),
    }
    .with_relations();

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

#[tokio::test]
async fn macro_generated_default_initializes_runtime_relations() {
    let employee = RelationTestEmployee::default();

    assert_eq!(employee.manager.foreign_key, "manager_id");
    assert_eq!(employee.manager.local_key, "id");
    assert_eq!(employee.reports.foreign_key, "manager_id");
    assert_eq!(employee.reports.local_key, "id");
    assert_eq!(employee.avatar.morph_name, "imageable");
    assert_eq!(employee.avatar.local_key, "id");

    if let Err(err) = employee.manager.load().await {
        assert!(!err.to_string().contains("not configured"));
    }

    if let Err(err) = employee.reports.load().await {
        assert!(!err.to_string().contains("not configured"));
    }

    if let Err(err) = employee.avatar.load().await {
        assert!(!err.to_string().contains("not configured"));
    }

    let image = RelationTestImage::default();
    assert_eq!(image.owner.type_column, "imageable_type");
    assert_eq!(image.owner.id_column, "imageable_id");
}
