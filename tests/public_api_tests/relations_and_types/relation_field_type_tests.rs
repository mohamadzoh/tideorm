use serde_json::json;
use tideorm::relations::{BelongsTo, HasMany, HasManyThrough, HasOne, MorphMany, MorphOne};

#[tideorm::model(table = "relation_field_test_models")]
struct RelationFieldTestModel {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "relation_field_test_pivots")]
struct RelationFieldPivotTestModel {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
}

macro_rules! assert_unconfigured_relation_load_fails {
    ($test_name:ident, $relation:expr, $message:literal) => {
        #[tokio::test]
        async fn $test_name() {
            let relation = $relation;

            let err = relation.load().await.unwrap_err();
            assert!(err.to_string().contains($message));
        }
    };
}

#[test]
fn test_has_one_default_has_none_cached() {
    let relation = HasOne::<RelationFieldTestModel>::default();

    assert_eq!(relation.foreign_key, "");
    assert_eq!(relation.local_key, "");
    assert!(relation.get_cached().is_none());
}

assert_unconfigured_relation_load_fails!(
    test_has_one_default_fails_loudly_when_unconfigured,
    HasOne::<RelationFieldTestModel>::default().with_parent_pk(json!(1)),
    "HasOne relation is not configured"
);

#[test]
fn test_has_many_default_has_none_cached() {
    let relation = HasMany::<RelationFieldTestModel>::default();

    assert_eq!(relation.foreign_key, "");
    assert_eq!(relation.local_key, "");
    assert!(relation.get_cached().is_none());
}

assert_unconfigured_relation_load_fails!(
    test_has_many_default_fails_loudly_when_unconfigured,
    HasMany::<RelationFieldTestModel>::default().with_parent_pk(json!(1)),
    "HasMany relation is not configured"
);

#[test]
fn test_belongs_to_default_has_none_cached() {
    let relation = BelongsTo::<RelationFieldTestModel>::default();

    assert_eq!(relation.foreign_key, "");
    assert_eq!(relation.owner_key, "");
    assert!(relation.get_cached().is_none());
}

assert_unconfigured_relation_load_fails!(
    test_belongs_to_default_fails_loudly_when_unconfigured,
    BelongsTo::<RelationFieldTestModel>::default().with_fk_value(json!(1)),
    "BelongsTo relation is not configured"
);

#[test]
fn test_has_many_through_default_has_none_cached() {
    let relation = HasManyThrough::<RelationFieldTestModel, RelationFieldPivotTestModel>::default();

    assert_eq!(relation.foreign_key, "");
    assert_eq!(relation.related_key, "");
    assert_eq!(relation.local_key, "");
    assert_eq!(relation.related_local_key, "");
    assert_eq!(relation.pivot_table, "");
    assert!(relation.get_cached().is_none());
}

assert_unconfigured_relation_load_fails!(
    test_has_many_through_default_fails_loudly_when_unconfigured,
    HasManyThrough::<RelationFieldTestModel, RelationFieldPivotTestModel>::default()
        .with_parent_pk(json!(1)),
    "HasManyThrough relation is not configured"
);

#[test]
fn test_morph_one_default_has_none_cached() {
    let relation = MorphOne::<RelationFieldTestModel>::default();

    assert_eq!(relation.morph_name, "");
    assert_eq!(relation.local_key, "");
    assert!(relation.get_cached().is_none());
}

assert_unconfigured_relation_load_fails!(
    test_morph_one_default_fails_loudly_when_unconfigured,
    MorphOne::<RelationFieldTestModel>::default(),
    "MorphOne relation is not configured"
);

#[test]
fn test_morph_many_default_has_none_cached() {
    let relation = MorphMany::<RelationFieldTestModel>::default();

    assert_eq!(relation.morph_name, "");
    assert_eq!(relation.local_key, "");
    assert!(relation.get_cached().is_none());
}

assert_unconfigured_relation_load_fails!(
    test_morph_many_default_fails_loudly_when_unconfigured,
    MorphMany::<RelationFieldTestModel>::default(),
    "MorphMany relation is not configured"
);
