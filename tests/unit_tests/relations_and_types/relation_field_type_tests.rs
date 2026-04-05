use serde_json::json;
use tideorm::relations::{
    BelongsTo, HasMany, HasManyThrough, HasOne, MorphMany, MorphOne, RelationConstraints,
};

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

// =========================================================================
// HasOne TESTS
// =========================================================================

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

// =========================================================================
// HasMany TESTS
// =========================================================================

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

// =========================================================================
// BelongsTo TESTS
// =========================================================================

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

// =========================================================================
// RelationConstraints TESTS
// =========================================================================

#[test]
fn test_relation_constraints_default() {
    let constraints = RelationConstraints::default();
    assert!(constraints.conditions.is_empty());
    assert!(constraints.order_by.is_none());
    assert!(constraints.limit.is_none());
    assert!(constraints.offset.is_none());
}

#[test]
fn test_relation_constraints_with_where() {
    let constraints = RelationConstraints::default().where_eq("status", json!("active"));

    assert_eq!(constraints.conditions.len(), 1);
    assert_eq!(constraints.conditions[0].0, "status");
    assert_eq!(constraints.conditions[0].1, json!("active"));
}

#[test]
fn test_relation_constraints_chained() {
    use tideorm::query::Order;
    let constraints = RelationConstraints::default()
        .where_eq("active", json!(true))
        .where_eq("published", json!(true))
        .order_by("created_at", Order::Desc)
        .limit(10)
        .offset(5);

    assert_eq!(constraints.conditions.len(), 2);
    assert_eq!(
        constraints.order_by,
        Some(("created_at".to_string(), Order::Desc))
    );
    assert_eq!(constraints.limit, Some(10));
    assert_eq!(constraints.offset, Some(5));
}

#[test]
fn test_relation_constraints_order_asc() {
    use tideorm::query::Order;
    let constraints = RelationConstraints::default().order_by("name", Order::Asc);

    assert_eq!(constraints.order_by, Some(("name".to_string(), Order::Asc)));
}

#[test]
fn test_relation_constraints_clone() {
    let constraints = RelationConstraints::default()
        .where_eq("status", json!("active"))
        .limit(5);

    let cloned = constraints.clone();
    assert_eq!(cloned.conditions.len(), 1);
    assert_eq!(cloned.limit, Some(5));
}
