use super::{
    BelongsTo, EagerLoadExt, HasMany, HasManyThrough, HasOne, MorphMany, MorphOne, MorphTo,
    RelationConstraints, RelationExt, SelfRef, SelfRefMany, Value, build_self_ref_tree_sql,
};
use crate::config::DatabaseType;
use crate::model::Model as _;
use serde_json::json;

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use crate::Database;
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use std::sync::OnceLock;
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use tokio::sync::Mutex;

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

#[tideorm::model(table = "relation_ext_lookup_models")]
struct RelationExtLookupModel {
    #[tideorm(primary_key)]
    id: i64,
    #[tideorm(column = "owner_id")]
    account_id: i64,
}

#[tideorm::model(table = "relation_ext_parent_models")]
struct RelationExtParentModel {
    #[tideorm(primary_key)]
    id: i64,
    name: String,

    #[tideorm(foreign_key = "parent_id")]
    child: HasOne<RelationExtChildModel>,
}

#[tideorm::model(table = "relation_ext_child_models")]
struct RelationExtChildModel {
    #[tideorm(primary_key)]
    id: i64,
    parent_id: i64,
    label: String,
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn direct_relation_db_guard() -> &'static Mutex<()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD.get_or_init(|| Mutex::new(()))
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn cleanup_direct_relation_test_db() {
    Database::reset_global();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
async fn setup_direct_relation_test_db() -> Database {
    cleanup_direct_relation_test_db();

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed");
    Database::set_global(db.clone()).expect("setting global database should succeed");

    db
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

#[path = "relations_tests/relation_ext.rs"]
mod relation_ext;

#[path = "relations_tests/macro_runtime.rs"]
mod macro_runtime;

#[path = "relations_tests/cached_payloads.rs"]
mod cached_payloads;

#[path = "relations_tests/sqlite_runtime.rs"]
mod sqlite_runtime;
