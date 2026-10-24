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

/// A relation wrapper left at `Default`, with no keys from a `#[tideorm(..)]`
/// field, fails its load naming the relation instead of returning nothing.
#[tokio::test]
async fn an_unconfigured_relation_fails_to_load() {
    #[track_caller]
    fn assert_unconfigured<T: std::fmt::Debug>(result: tideorm::Result<T>, relation: &str) {
        let error = result.expect_err(relation).to_string();
        assert!(
            error.contains(&format!("{relation} relation is not configured")),
            "{relation}: {error}"
        );
    }

    type Model = RelationFieldTestModel;
    assert_unconfigured(
        HasOne::<Model>::default()
            .with_parent_pk(json!(1))
            .load()
            .await,
        "HasOne",
    );
    assert_unconfigured(
        HasMany::<Model>::default()
            .with_parent_pk(json!(1))
            .load()
            .await,
        "HasMany",
    );
    assert_unconfigured(
        BelongsTo::<Model>::default()
            .with_fk_value(json!(1))
            .load()
            .await,
        "BelongsTo",
    );
    assert_unconfigured(
        HasManyThrough::<Model, RelationFieldPivotTestModel>::default()
            .with_parent_pk(json!(1))
            .load()
            .await,
        "HasManyThrough",
    );
    assert_unconfigured(MorphOne::<Model>::default().load().await, "MorphOne");
    assert_unconfigured(MorphMany::<Model>::default().load().await, "MorphMany");
}
