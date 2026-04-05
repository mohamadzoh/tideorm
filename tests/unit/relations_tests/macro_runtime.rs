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
