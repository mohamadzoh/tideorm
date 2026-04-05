use super::*;

#[test]
fn direct_relations_deserialize_cached_payloads() {
    let has_one: super::HasOne<RelationTestNode> = serde_json::from_value(json!({
        "id": 1,
        "slug": "root",
        "parent_slug": null
    }))
    .expect("has_one should deserialize cached payload");
    assert_eq!(has_one.get_cached().map(|node| node.id), Some(1));

    let has_many: HasMany<RelationTestNode> = serde_json::from_value(json!([
        { "id": 2, "slug": "child-a", "parent_slug": "root" },
        { "id": 3, "slug": "child-b", "parent_slug": "root" }
    ]))
    .expect("has_many should deserialize cached payload");
    assert_eq!(has_many.get_cached().map(|nodes| nodes.len()), Some(2));

    let belongs_to: BelongsTo<RelationTestNode> = serde_json::from_value(json!({
        "id": 4,
        "slug": "parent",
        "parent_slug": null
    }))
    .expect("belongs_to should deserialize cached payload");
    assert_eq!(
        belongs_to.get_cached().map(|node| node.slug.as_str()),
        Some("parent")
    );
}

#[test]
fn advanced_relations_deserialize_cached_payloads() {
    let self_ref: SelfRef<RelationTestEmployee> = serde_json::from_value(json!({
        "id": 1,
        "manager_id": null,
        "manager": null,
        "reports": [],
        "avatar": null
    }))
    .expect("self_ref should deserialize cached payload");
    assert_eq!(self_ref.get_cached().map(|employee| employee.id), Some(1));

    let self_ref_many: SelfRefMany<RelationTestEmployee> = serde_json::from_value(json!([
        {
            "id": 2,
            "manager_id": 1,
            "manager": null,
            "reports": [],
            "avatar": null
        }
    ]))
    .expect("self_ref_many should deserialize cached payload");
    assert_eq!(
        self_ref_many.get_cached().map(|employees| employees.len()),
        Some(1)
    );

    let morph_one: MorphOne<RelationTestImage> = serde_json::from_value(json!({
        "id": 9,
        "imageable_type": "relation_test_nodes",
        "imageable_id": 4,
        "owner": null
    }))
    .expect("morph_one should deserialize cached payload");
    assert_eq!(morph_one.get_cached().map(|image| image.id), Some(9));

    let morph_many: super::MorphMany<RelationTestImage> = serde_json::from_value(json!([
        {
            "id": 10,
            "imageable_type": "relation_test_nodes",
            "imageable_id": 4,
            "owner": null
        }
    ]))
    .expect("morph_many should deserialize cached payload");
    assert_eq!(morph_many.get_cached().map(|images| images.len()), Some(1));

    let has_many_through: HasManyThrough<RelationTestNode, RelationTestPivot> =
        serde_json::from_value(json!([
            { "id": 5, "slug": "related", "parent_slug": null }
        ]))
        .expect("has_many_through should deserialize cached payload");
    assert_eq!(
        has_many_through.get_cached().map(|nodes| nodes.len()),
        Some(1)
    );
}
