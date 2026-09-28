use super::helpers::build_self_ref_tree_sql;
use super::{
    BelongsTo, HasMany, HasManyThrough, HasOne, MorphMany, MorphOne, MorphTo, SelfRef, SelfRefMany,
};
use crate::config::DatabaseType;
use crate::internal::Value;
use serde_json::json;

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use crate::Database;

#[tideorm::model(table = "relation_test_nodes")]
struct RelationTestNode {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    slug: String,
    parent_slug: Option<String>,
}

#[tideorm::model(table = "relation_test_pivots")]
struct RelationTestPivot {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    left_id: i64,
    right_id: i64,
}

#[tideorm::model(table = "relation_test_images")]
struct RelationTestImage {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    imageable_type: String,
    imageable_id: i64,

    #[tideorm(morph_name = "imageable")]
    owner: MorphTo<RelationTestNode>,
}

#[tideorm::model(table = "relation_test_employees")]
struct RelationTestEmployee {
    #[tideorm(primary_key)]
    id: i64,
    manager_id: Option<i64>,

    #[tideorm(foreign_key = "manager_id")]
    manager: SelfRef<RelationTestEmployee>,

    #[tideorm(foreign_key = "manager_id")]
    reports: SelfRefMany<RelationTestEmployee>,

    #[tideorm(morph_name = "imageable")]
    avatar: MorphOne<RelationTestImage>,
}

#[tideorm::model(table = "aliased_key_models")]
struct AliasedKeyModel {
    #[tideorm(primary_key)]
    id: i64,
    #[tideorm(column = "owner_id")]
    account_id: i64,
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
async fn setup_direct_relation_test_db() -> Database {
    crate::test_support::install_sqlite_global(&[]).await
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tideorm::model(table = "relation_test_users")]
struct DirectRelationUser {
    #[tideorm(primary_key)]
    id: i64,
    name: String,

    #[tideorm(has_one = "DirectRelationProfile", foreign_key = "user_id")]
    profile: HasOne<DirectRelationProfile>,

    #[tideorm(has_many = "DirectRelationPost", foreign_key = "user_id")]
    posts: HasMany<DirectRelationPost>,
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tideorm::model(table = "relation_test_profiles")]
struct DirectRelationProfile {
    #[tideorm(primary_key)]
    id: i64,
    user_id: i64,
    name: String,
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tideorm::model(table = "relation_test_posts")]
struct DirectRelationPost {
    #[tideorm(primary_key)]
    id: i64,
    user_id: i64,
    title: String,
}

#[path = "relations_tests/query_and_constraints.rs"]
mod query_and_constraints;

#[path = "relations_tests/macro_runtime.rs"]
mod macro_runtime;

#[path = "relations_tests/cached_payloads.rs"]
mod cached_payloads;

#[path = "relations_tests/sqlite_runtime.rs"]
mod sqlite_runtime;

/// A comment whose owner may be absent: both morph columns are nullable.
#[tideorm::model(table = "relation_optional_comments")]
struct RelationOptionalComment {
    #[tideorm(primary_key)]
    id: i64,
    commentable_type: Option<String>,
    commentable_id: Option<i64>,
    #[tideorm(morph_name = "commentable")]
    commentable: MorphTo<RelationTestNode>,
}

/// A `MorphTo` over a nullable type column compiles, and a row holding NULL
/// in both columns has no owner.
#[tokio::test]
async fn a_morph_to_with_null_columns_has_no_owner() {
    let comment = RelationOptionalComment::default();
    assert_eq!(comment.commentable.type_value(), None);
    assert!(
        comment
            .commentable
            .load()
            .await
            .expect("load failed")
            .is_none()
    );
}

/// Keys the database may match to one stored key are paired by it: ASCII
/// case and trailing spaces are folded here, anything beyond plain ASCII is
/// left to the collation.
#[test]
fn keys_a_collation_could_fold_together_are_left_to_the_database() {
    use super::eager::fold_together;

    assert!(fold_together(&[json!("abc"), json!("ABC ")]));
    assert!(fold_together(&[json!("cafe"), json!("café")]));
    assert!(fold_together(&[json!("strasse"), json!("straße")]));
    assert!(!fold_together(&[json!("abc"), json!("abd")]));
    assert!(!fold_together(&[json!("café")]));
    assert!(!fold_together(&[json!(1), json!(2)]));
}

/// A relation reads, deletes and loads its pivot rows through the pivot
/// model, so a pivot table other than that model's is refused rather than
/// written to beside it.
#[tokio::test]
async fn a_pivot_table_other_than_the_pivot_models_is_refused() {
    let relation = HasManyThrough::<RelationTestNode, RelationTestPivot>::new(
        "left_id",
        "right_id",
        "id",
        "id",
        "some_other_table",
    )
    .with_parent_pk(json!(1));

    let error = relation.count().await.expect_err("the pivot is refused");
    assert!(
        error.to_string().contains(
            "'some_other_table' is not the table of its pivot model, 'relation_test_pivots'"
        ),
        "{error}"
    );
    let error = relation.attach(2).await.expect_err("the pivot is refused");
    assert!(error.to_string().contains("some_other_table"), "{error}");
}
