use super::*;

/// Every relation wrapper reads a payload as its cached rows.
#[test]
fn relation_wrappers_deserialize_cached_payloads() {
    fn read<T: serde::de::DeserializeOwned>(payload: serde_json::Value) -> T {
        serde_json::from_value(payload).expect("the cached payload should deserialize")
    }
    let node = |id: i64| json!({ "id": id, "slug": format!("node-{id}"), "parent_slug": null });
    let employee = |id: i64| {
        json!({
            "id": id,
            "manager_id": null,
            "manager": null,
            "reports": [],
            "avatar": null
        })
    };
    let image = |id: i64| {
        json!({
            "id": id,
            "imageable_type": "relation_test_nodes",
            "imageable_id": 4,
            "owner": null
        })
    };

    let has_one: HasOne<RelationTestNode> = read(node(1));
    assert_eq!(has_one.get_cached().map(|node| node.id), Some(1));
    let belongs_to: BelongsTo<RelationTestNode> = read(node(4));
    assert_eq!(
        belongs_to.get_cached().map(|node| node.slug.as_str()),
        Some("node-4")
    );
    let has_many: HasMany<RelationTestNode> = read(json!([node(2), node(3)]));
    assert_eq!(has_many.get_cached().map(<[_]>::len), Some(2));
    let has_many_through: HasManyThrough<RelationTestNode, RelationTestPivot> =
        read(json!([node(5)]));
    assert_eq!(has_many_through.get_cached().map(<[_]>::len), Some(1));

    let self_ref: SelfRef<RelationTestEmployee> = read(employee(1));
    assert_eq!(self_ref.get_cached().map(|employee| employee.id), Some(1));
    let self_ref_many: SelfRefMany<RelationTestEmployee> = read(json!([employee(2)]));
    assert_eq!(self_ref_many.get_cached().map(<[_]>::len), Some(1));

    let morph_one: MorphOne<RelationTestImage> = read(image(9));
    assert_eq!(morph_one.get_cached().map(|image| image.id), Some(9));
    let morph_many: MorphMany<RelationTestImage> = read(json!([image(10)]));
    assert_eq!(morph_many.get_cached().map(<[_]>::len), Some(1));
}
