//! Integration scenarios every backend target runs.
//!
//! The including target supplies `mod backend` with `DATABASE_TYPE` and
//! `async fn connect() -> bool`, which installs the global database and returns
//! `false` when the backend is switched off for this run. Tables are built with
//! the migration `Schema`, so the same DDL renders for all three backends.

use std::sync::{LazyLock, Mutex};

use tideorm::internal::ConnectionTrait;
use tideorm::migration::Schema;
use tideorm::prelude::*;
use tideorm::{Database, Error};

use super::backend;

#[derive(Model, PartialEq)]
#[tideorm(table = "test_users")]
pub struct TestUser {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub age: i32,
    pub active: bool,
}

static CALLBACK_EVENTS: LazyLock<Mutex<Vec<&'static str>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

#[derive(Model, PartialEq)]
#[tideorm(table = "callback_users")]
pub struct CallbackUser {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
}

impl Callbacks for CallbackUser {
    fn before_validation(&mut self) -> tideorm::Result<()> {
        CALLBACK_EVENTS.lock().unwrap().push("before_validation");
        Ok(())
    }

    fn after_validation(&self) -> tideorm::Result<()> {
        CALLBACK_EVENTS.lock().unwrap().push("after_validation");
        Ok(())
    }

    fn before_save(&mut self) -> tideorm::Result<()> {
        CALLBACK_EVENTS.lock().unwrap().push("before_save");
        self.email = self.email.to_lowercase();
        Ok(())
    }

    fn after_save(&self) -> tideorm::Result<()> {
        CALLBACK_EVENTS.lock().unwrap().push("after_save");
        Ok(())
    }

    fn before_create(&mut self) -> tideorm::Result<()> {
        CALLBACK_EVENTS.lock().unwrap().push("before_create");
        Ok(())
    }

    fn after_create(&self) -> tideorm::Result<()> {
        CALLBACK_EVENTS.lock().unwrap().push("after_create");
        Ok(())
    }

    fn before_update(&mut self) -> tideorm::Result<()> {
        CALLBACK_EVENTS.lock().unwrap().push("before_update");
        Ok(())
    }

    fn after_update(&self) -> tideorm::Result<()> {
        CALLBACK_EVENTS.lock().unwrap().push("after_update");
        Ok(())
    }

    fn before_delete(&self) -> tideorm::Result<()> {
        CALLBACK_EVENTS.lock().unwrap().push("before_delete");
        Ok(())
    }

    fn after_delete(&self) -> tideorm::Result<()> {
        CALLBACK_EVENTS.lock().unwrap().push("after_delete");
        Ok(())
    }
}

#[tideorm::model(table = "test_soft_deletes", soft_delete)]
pub struct TestSoftDelete {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Model, PartialEq)]
#[tideorm(table = "timestamp_users")]
pub struct TimestampUser {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub login_count: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[tideorm::model(table = "test_products")]
pub struct TestProduct {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub category: String,
    pub price: i64,
    #[tideorm(nullable)]
    pub metadata: Option<serde_json::Value>,
}

const TABLES: [&str; 5] = [
    "test_users",
    "callback_users",
    "test_soft_deletes",
    "timestamp_users",
    "test_products",
];

/// Connect the backend and recreate every table from scratch, so each test
/// starts empty and auto-increment ids start at 1. `false` means the backend is
/// switched off and the test should return without asserting anything.
pub async fn setup() -> bool {
    if !backend::connect().await {
        return false;
    }

    let mut schema = Schema::new(backend::DATABASE_TYPE);
    for table in TABLES {
        schema
            .drop_table_if_exists(table)
            .await
            .expect("failed to drop test table");
    }

    schema
        .create_table("test_users", |t| {
            t.id();
            t.string("email").not_null();
            t.string("name").not_null();
            t.integer("age").not_null();
            t.boolean("active").not_null();
        })
        .await
        .expect("failed to create test_users");
    schema
        .create_table("callback_users", |t| {
            t.id();
            t.string("email").not_null();
            t.string("name").not_null();
        })
        .await
        .expect("failed to create callback_users");
    schema
        .create_table("test_soft_deletes", |t| {
            t.id();
            t.string("name").not_null();
            t.soft_deletes();
        })
        .await
        .expect("failed to create test_soft_deletes");
    schema
        .create_table("timestamp_users", |t| {
            t.id();
            t.string("email").not_null().unique();
            t.string("name").not_null();
            t.integer("login_count").not_null();
            t.timestamptz("created_at").not_null();
            t.timestamptz("updated_at").not_null();
        })
        .await
        .expect("failed to create timestamp_users");
    schema
        .create_table("test_products", |t| {
            t.id();
            t.string("name").not_null();
            t.string("category").not_null();
            t.big_integer("price").not_null();
            t.jsonb("metadata");
        })
        .await
        .expect("failed to create test_products");

    true
}

/// The `n`th positional placeholder in raw SQL for the backend under test.
fn param(n: usize) -> String {
    match backend::DATABASE_TYPE {
        DatabaseType::Postgres => format!("${n}"),
        _ => "?".to_string(),
    }
}

/// Insert users `1..=count` with ages `21..` and every even one active.
pub async fn seed_users(count: i32) {
    for i in 1..=count {
        TestUser {
            id: 0,
            email: format!("user{i}@example.com"),
            name: format!("User {i:02}"),
            age: 20 + i,
            active: i % 2 == 0,
        }
        .save()
        .await
        .expect("failed to seed user");
    }
}

/// `pluck`, `value`, `get_as` and `paginate` read one column, one value, a
/// caller's own row type, and a page with its total.
#[tokio::test]
async fn rows_read_as_columns_values_structs_and_pages() {
    if !setup().await {
        return;
    }
    seed_users(5).await;

    let emails: Vec<String> = TestUser::query()
        .where_eq("active", true)
        .order_asc("id")
        .pluck("email")
        .await
        .expect("pluck failed");
    assert_eq!(emails, ["user2@example.com", "user4@example.com"]);
    let flags: Vec<bool> = TestUser::query()
        .distinct()
        .order_asc("active")
        .pluck(TestUser::columns.active)
        .await
        .expect("distinct pluck failed");
    assert_eq!(flags, [false, true]);

    let oldest: Option<i32> = TestUser::query()
        .order_desc("age")
        .value("age")
        .await
        .expect("value failed");
    assert_eq!(oldest, Some(25));
    let nobody: Option<String> = TestUser::query()
        .where_eq("age", 99)
        .value("name")
        .await
        .expect("value of nothing failed");
    assert_eq!(nobody, None);

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct ActiveCount {
        active: bool,
        users: i64,
    }
    let counts: Vec<ActiveCount> = TestUser::query()
        .select_raw("active, COUNT(*) AS users")
        .group_by("active")
        .order_asc("active")
        .get_as()
        .await
        .expect("get_as failed");
    assert_eq!(
        counts,
        [
            ActiveCount {
                active: false,
                users: 3
            },
            ActiveCount {
                active: true,
                users: 2
            },
        ]
    );

    let page = TestUser::query()
        .order_asc("id")
        .paginate(2, 2)
        .await
        .expect("paginate failed");
    assert_eq!(page.total, 5);
    assert_eq!(page.last_page(), 3);
    assert!(page.has_next_page());
    let names: Vec<&str> = page.items.iter().map(|user| user.name.as_str()).collect();
    assert_eq!(names, ["User 03", "User 04"]);
    let json = serde_json::to_value(&page).expect("a page serializes");
    assert_eq!(json["last_page"], 3);
    assert_eq!(json["items"].as_array().map(Vec::len), Some(2));
    assert!(TestUser::query().paginate(0, 2).await.is_err());
}

/// Lists from any iterable, raw SQL with bound values, typed `having` and
/// `reorder` run the same on every backend.
#[tokio::test]
async fn builders_take_iterables_bound_raw_sql_typed_having_and_reorder() {
    if !setup().await {
        return;
    }
    seed_users(6).await;

    let ids: Vec<i64> = TestUser::query()
        .order_asc("id")
        .pluck("id")
        .await
        .expect("pluck ids failed");
    let first_two = [ids[0], ids[1]];
    assert_eq!(
        TestUser::query()
            .where_in("id", first_two)
            .count()
            .await
            .expect("array where_in failed"),
        2
    );
    assert_eq!(
        TestUser::query()
            .where_not_in("id", &ids[..3])
            .count()
            .await
            .expect("slice where_not_in failed"),
        3
    );
    assert_eq!(
        TestUser::query()
            .where_col(TestUser::columns.age.is_in([21, 22, 99]))
            .count()
            .await
            .expect("typed is_in failed"),
        2
    );

    let named = TestUser::query()
        .where_raw_with(
            "LOWER(name) = LOWER(?) OR (age > ? AND name <> '?')",
            vec!["user 01".into(), 25.into()],
        )
        .order_asc("id")
        .pluck::<String>("name")
        .await
        .expect("where_raw_with failed");
    assert_eq!(named, ["User 01", "User 06"]);
    assert!(
        TestUser::query()
            .where_raw_with("age > ?", vec![])
            .get()
            .await
            .is_err(),
        "a placeholder without a value fails"
    );

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Group {
        active: bool,
        users: i64,
    }
    let groups: Vec<Group> = TestUser::query()
        .select_raw("active, COUNT(*) AS users")
        .group_by("active")
        .having(Aggregate::count().gte(3))
        .having(Aggregate::max("age").gt(25))
        .order_asc("active")
        .get_as()
        .await
        .expect("typed having failed");
    assert_eq!(
        groups,
        [Group {
            active: true,
            users: 3
        }]
    );

    let names: Vec<String> = TestUser::query()
        .order_asc("age")
        .reorder("age", Order::Desc)
        .limit(2)
        .pluck("name")
        .await
        .expect("reorder failed");
    assert_eq!(names, ["User 06", "User 05"]);

    // Ages run 21..=26.
    assert_eq!(
        TestUser::query()
            .where_not_between("age", 22, 25)
            .count()
            .await
            .expect("where_not_between failed"),
        2
    );
    assert!(
        TestUser::query()
            .where_not_between("age", 22, None::<i32>)
            .count()
            .await
            .is_err(),
        "a NULL bound is refused"
    );
}

#[tideorm::model(table = "joined_owners")]
pub struct JoinedOwner {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub token: uuid::Uuid,
    pub joined_at: chrono::DateTime<chrono::Utc>,
}

#[tideorm::model(table = "joined_pets")]
pub struct JoinedPet {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub owner_id: i64,
    pub name: String,
}

/// A filter on a joined table's UUID or timestamp column binds the value as
/// that column's type: bound as text, PostgreSQL refused `uuid = text` and
/// MySQL and SQLite, which store a UUID as bytes, matched nothing.
#[tokio::test]
async fn filters_on_a_joined_table_bind_its_column_types() {
    if !setup().await {
        return;
    }
    fresh_table("joined_owners", |t| {
        t.id();
        t.uuid("token").not_null();
        t.timestamptz("joined_at").not_null();
    })
    .await;
    fresh_table("joined_pets", |t| {
        t.id();
        t.big_integer("owner_id").not_null();
        t.string("name").not_null();
    })
    .await;

    let joined_at = chrono::Utc::now() - chrono::Duration::days(1);
    let mut tokens = Vec::new();
    for name in ["Rex", "Tom"] {
        let owner = JoinedOwner {
            id: 0,
            token: uuid::Uuid::new_v4(),
            joined_at,
        }
        .save()
        .await
        .expect("save owner failed");
        JoinedPet {
            id: 0,
            owner_id: owner.id,
            name: name.into(),
        }
        .save()
        .await
        .expect("save pet failed");
        tokens.push(owner.token);
    }

    let by_token = JoinedPet::query()
        .inner_join("joined_owners", "joined_pets.owner_id", "joined_owners.id")
        .where_eq("joined_owners.token", tokens[1])
        .where_gt(
            "joined_owners.joined_at",
            joined_at - chrono::Duration::hours(1),
        )
        .pluck::<String>("joined_pets.name")
        .await
        .expect("filtering on the joined table failed");
    assert_eq!(by_token, ["Tom"]);

    let by_alias = JoinedPet::query()
        .inner_join_as("joined_owners", "o", "joined_pets.owner_id", "o.id")
        .where_in("o.token", [tokens[0]])
        .pluck::<String>("joined_pets.name")
        .await
        .expect("filtering on an aliased join failed");
    assert_eq!(by_alias, ["Rex"]);

    // Another model's typed column reads that model's column in an aggregate.
    let latest: Option<chrono::DateTime<chrono::Utc>> = JoinedPet::query()
        .inner_join("joined_owners", "joined_pets.owner_id", "joined_owners.id")
        .max(JoinedOwner::columns.joined_at)
        .await
        .expect("max over a joined model's column failed");
    assert_eq!(
        latest.map(|at| at.timestamp_micros()),
        Some(joined_at.timestamp_micros())
    );

    // A typed HAVING compares as the column: a timestamp is not text.
    let recent = JoinedOwner::query()
        .select_raw("token")
        .group_by("token")
        .having(Aggregate::max("joined_at").gt(joined_at - chrono::Duration::hours(1)))
        .get_json()
        .await
        .expect("a typed having on a timestamp failed");
    assert_eq!(recent.len(), 2);

    // A typed HAVING written before the join names its own table.
    let grouped = JoinedPet::query()
        .select_raw("joined_pets.owner_id")
        .group_by("joined_pets.owner_id")
        .having(Aggregate::count_distinct("id").gte(1))
        .inner_join("joined_owners", "joined_pets.owner_id", "joined_owners.id")
        .get_json()
        .await
        .expect("a having before the join was ambiguous");
    assert_eq!(grouped.len(), 2);

    // `pluck` finds its column beside other projections.
    let names: Vec<String> = JoinedPet::query()
        .select_raw("1 AS a")
        .order_asc("joined_pets.id")
        .pluck("name")
        .await
        .expect("pluck beside select_raw failed");
    assert_eq!(names, ["Rex", "Tom"]);

    // `has_related` binds its value as the related column's type too.
    let by_related: Vec<String> = JoinedPet::query()
        .has_related("joined_owners", "id", "owner_id", "token", tokens[0])
        .pluck("name")
        .await
        .expect("has_related on a UUID column failed");
    assert_eq!(by_related, ["Rex"]);

    // A linked partial select reads into a struct of the caller's.
    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct PetToken {
        name: String,
        token: uuid::Uuid,
    }
    let linked: Vec<PetToken> = JoinedPet::query()
        .select_with_linked(
            vec!["name"],
            "joined_owners",
            "owner_id",
            "id",
            vec!["token"],
        )
        .order_asc("joined_pets.id")
        .get_as()
        .await
        .expect("linked select failed");
    assert_eq!(
        linked,
        [
            PetToken {
                name: "Rex".into(),
                token: tokens[0],
            },
            PetToken {
                name: "Tom".into(),
                token: tokens[1],
            },
        ]
    );
}

/// An update started from a query keeps its filters and its soft-delete
/// scope, where `Model::update_all()` reaches trashed rows too.
#[tokio::test]
async fn updating_from_a_query_keeps_its_filters_and_scope() {
    if !setup().await {
        return;
    }
    for name in ["a", "b", "c"] {
        TestSoftDelete {
            id: 0,
            name: name.into(),
            deleted_at: None,
        }
        .save()
        .await
        .expect("save failed");
    }
    TestSoftDelete::query()
        .where_eq("name", "c")
        .soft_delete()
        .await
        .expect("soft delete failed");

    let live = TestSoftDelete::query()
        .where_in("name", ["a", "c"])
        .update_all()
        .set("name", "z")
        .execute()
        .await
        .expect("update from a query failed");
    assert_eq!(
        live, 1,
        "the trashed row stays out, as the query leaves it out"
    );
    let trashed = TestSoftDelete::query()
        .only_trashed()
        .where_eq("name", "c")
        .update_all()
        .set("name", "y")
        .execute()
        .await
        .expect("update of the trash failed");
    assert_eq!(trashed, 1);
    let live = TestSoftDelete::update_all()
        .where_in("name", ["z", "y"])
        .set("name", "x")
        .execute()
        .await
        .expect("Model::update_all failed");
    assert_eq!(live, 1, "Model::update_all() leaves trashed rows out");
    let everything = TestSoftDelete::update_all()
        .with_trashed()
        .where_in("name", ["x", "y"])
        .set("name", "w")
        .execute()
        .await
        .expect("Model::update_all().with_trashed() failed");
    assert_eq!(everything, 2, "with_trashed() reaches them");

    assert!(
        TestSoftDelete::query()
            .update_all()
            .set("name", "w")
            .execute()
            .await
            .is_err(),
        "an unfiltered update is still refused"
    );
    assert!(
        TestSoftDelete::query()
            .where_eq("name", "b")
            .limit(1)
            .update_all()
            .set("name", "w")
            .execute()
            .await
            .is_err(),
        "a paged query is not an UPDATE"
    );
}

#[tideorm::model(table = "has_posts", soft_delete)]
pub struct HasPost {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub published: bool,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// `where_has` and `where_doesnt_have` read the related model's own query, so
/// its soft-delete scope applies, where the table-level `has_related` counts
/// trashed rows too.
#[tokio::test]
async fn where_has_filters_by_the_related_models_own_query() {
    if !setup().await {
        return;
    }
    fresh_table("has_posts", |t| {
        t.id();
        t.big_integer("user_id").not_null();
        t.boolean("published").not_null();
        t.soft_deletes();
    })
    .await;
    seed_users(3).await;
    let ids: Vec<i64> = TestUser::query()
        .order_asc("id")
        .pluck("id")
        .await
        .expect("pluck ids failed");
    for (user_id, published) in [(ids[0], true), (ids[1], false), (ids[2], true)] {
        HasPost {
            id: 0,
            user_id,
            published,
            deleted_at: None,
        }
        .save()
        .await
        .expect("save post failed");
    }
    HasPost::query()
        .where_eq("user_id", ids[2])
        .soft_delete()
        .await
        .expect("soft delete failed");

    let names = |query: QueryBuilder<TestUser>| async move {
        query
            .order_asc("id")
            .pluck::<String>("name")
            .await
            .expect("where_has failed")
    };
    let published = |posts: QueryBuilder<HasPost>| posts.where_eq(HasPost::columns.published, true);
    assert_eq!(
        names(TestUser::query().where_has::<HasPost>(
            HasPost::columns.user_id,
            TestUser::columns.id,
            published
        ))
        .await,
        ["User 01"]
    );
    assert_eq!(
        names(TestUser::query().where_doesnt_have::<HasPost>(
            HasPost::columns.user_id,
            TestUser::columns.id,
            published
        ))
        .await,
        ["User 02", "User 03"]
    );
    assert_eq!(
        names(TestUser::query().where_has::<HasPost>("user_id", "id", |posts| posts)).await,
        ["User 01", "User 02"]
    );
    // The table-level form counts the trashed post.
    assert_eq!(
        names(TestUser::query().has_related("has_posts", "user_id", "id", "published", true)).await,
        ["User 01", "User 03"]
    );

    // With the key on this side: posts of active users (User 02).
    let posts: Vec<i64> = HasPost::query()
        .where_has::<TestUser>(TestUser::columns.id, HasPost::columns.user_id, |users| {
            users.where_eq(TestUser::columns.active, true)
        })
        .pluck("user_id")
        .await
        .expect("inverse where_has failed");
    assert_eq!(posts, [ids[1]]);

    assert!(
        TestUser::query()
            .where_has::<HasPost>("nope", "id", |posts| posts)
            .get()
            .await
            .is_err(),
        "an unknown key column is refused"
    );

    // Two columns of the same row, alone and across a join.
    let count = |query: QueryBuilder<HasPost>| async move {
        query.count().await.expect("column comparison failed")
    };
    assert_eq!(
        count(HasPost::query().where_column_eq("user_id", "user_id")).await,
        2
    );
    assert_eq!(
        count(HasPost::query().where_column_ne("user_id", "user_id")).await,
        0
    );
    let joined = || HasPost::query().inner_join("test_users", "has_posts.user_id", "test_users.id");
    assert_eq!(
        count(joined().where_column_eq(HasPost::columns.user_id, TestUser::columns.id)).await,
        2
    );
    assert_eq!(
        count(joined().where_column_lt("test_users.age", "has_posts.user_id")).await,
        0
    );
    assert_eq!(
        count(joined().or_where_column_gt("test_users.age", "has_posts.user_id")).await,
        2
    );

    // `when` works on a batch update as it does on a query.
    let changed = TestUser::update_all()
        .where_eq("name", "User 01")
        .when(false, |update| update.set("age", 99))
        .when_some(Some(50), |update, age| update.set("age", age))
        .execute()
        .await
        .expect("conditional update failed");
    assert_eq!(changed, 1);
    let age: Option<i32> = TestUser::query()
        .where_eq("name", "User 01")
        .value("age")
        .await
        .expect("value failed");
    assert_eq!(age, Some(50));
}

fn user(email: &str, name: &str, age: i32, active: bool) -> TestUser {
    TestUser {
        id: 0,
        email: email.to_string(),
        name: name.to_string(),
        age,
        active,
    }
}

#[tokio::test]
async fn connection_reports_backend_and_runs_raw_sql() {
    if !setup().await {
        return;
    }

    let db = tideorm::require_db().expect("global database should be installed");
    db.ping().await.expect("database ping failed");
    assert_eq!(db.backend(), backend::DATABASE_TYPE);
    Database::execute("SELECT 1")
        .await
        .expect("raw SQL execution failed");
}

#[tokio::test]
async fn crud_create_find_update_delete_and_destroy() {
    if !setup().await {
        return;
    }

    let saved = user("test@example.com", "Test User", 25, true)
        .save()
        .await
        .expect("Failed to save user");
    assert!(saved.id > 0, "User should have an auto-generated ID");
    let found = TestUser::find(saved.id)
        .await
        .expect("Failed to find user")
        .expect("User should be found");
    assert_eq!(found.email, "test@example.com");
    assert_eq!(found.name, "Test User");

    let mut saved = user("update@example.com", "Original Name", 30, true)
        .save()
        .await
        .expect("Failed to save user");
    saved.name = "Updated Name".to_string();
    saved.age = 31;
    let updated = saved.update().await.expect("Failed to update user");
    assert_eq!(updated.name, "Updated Name");
    assert_eq!(updated.age, 31);
    let reloaded = TestUser::find(updated.id)
        .await
        .expect("Failed to reload")
        .expect("Updated user should exist");
    assert_eq!(reloaded.name, "Updated Name");

    let saved = user("delete@example.com", "To Delete", 25, true)
        .save()
        .await
        .expect("Failed to save user");
    let user_id = saved.id;
    assert_eq!(saved.delete().await.expect("Failed to delete"), 1);
    assert!(
        TestUser::find(user_id)
            .await
            .expect("Find failed")
            .is_none()
    );

    let saved = user("destroy@example.com", "To Destroy", 25, true)
        .save()
        .await
        .expect("Failed to save user");
    assert_eq!(
        TestUser::destroy(saved.id)
            .await
            .expect("Failed to destroy"),
        1
    );
    assert!(
        TestUser::find(saved.id)
            .await
            .expect("Find failed")
            .is_none()
    );
}

#[tokio::test]
async fn query_builder_comparisons() {
    if !setup().await {
        return;
    }

    seed_users(10).await;

    let active = TestUser::query()
        .where_eq("active", true)
        .get()
        .await
        .expect("Query failed");
    assert_eq!(active.len(), 5);
    assert!(active.iter().all(|user| user.active));

    let older = TestUser::query()
        .where_gt("age", 27)
        .get()
        .await
        .expect("Query failed");
    assert_eq!(older.len(), 3, "ages 28, 29, 30");

    let younger = TestUser::query()
        .where_lt("age", 25)
        .get()
        .await
        .expect("Query failed");
    assert_eq!(younger.len(), 4, "ages 21-24");

    let in_range = TestUser::query()
        .where_between("age", 23, 27)
        .get()
        .await
        .expect("Query failed");
    assert_eq!(in_range.len(), 5, "BETWEEN is inclusive");

    let listed = TestUser::query()
        .where_in("age", vec![21, 23, 25])
        .get()
        .await
        .expect("Query failed");
    assert_eq!(listed.len(), 3);
}

#[tokio::test]
async fn query_builder_like() {
    if !setup().await {
        return;
    }

    for (email, name, age) in [
        ("john@gmail.com", "John Doe", 25),
        ("jane@gmail.com", "Jane Doe", 30),
        ("bob@yahoo.com", "Bob Smith", 35),
    ] {
        user(email, name, age, true)
            .save()
            .await
            .expect("Failed to save");
    }
    let gmail = TestUser::query()
        .where_like("email", "%gmail%")
        .get()
        .await
        .expect("Query failed");
    assert_eq!(gmail.len(), 2, "Should have 2 gmail users");
    let does = TestUser::query()
        .where_like("name", "%Doe%")
        .get()
        .await
        .expect("Query failed");
    assert_eq!(does.len(), 2, "Should have 2 Doe users");
}

#[tokio::test]
async fn query_builder_ordering_paging_and_terminals() {
    if !setup().await {
        return;
    }

    seed_users(20).await;

    let oldest = TestUser::query()
        .order_by("age", Order::Desc)
        .limit(3)
        .get()
        .await
        .expect("Query failed");
    let ages: Vec<i32> = oldest.iter().map(|user| user.age).collect();
    assert_eq!(ages, [40, 39, 38]);

    let page2 = TestUser::query()
        .order_by("age", Order::Asc)
        .page(2, 5)
        .get()
        .await
        .expect("Query failed");
    assert_eq!(page2.len(), 5, "Should have 5 users on page 2");
    assert_eq!(page2[0].age, 26, "First user on page 2 should have age 26");

    assert_eq!(TestUser::count().await.expect("Count failed"), 20);
    let active_count = TestUser::query()
        .where_eq("active", true)
        .count()
        .await
        .expect("Count failed");
    assert_eq!(active_count, 10);

    let first = TestUser::query()
        .where_gt("age", 22)
        .order_by("age", Order::Asc)
        .first()
        .await
        .expect("Query failed")
        .expect("a user older than 22 exists");
    assert_eq!(first.age, 23);

    let found = TestUser::query()
        .where_eq("email", "user1@example.com")
        .first_or_fail()
        .await
        .expect("first_or_fail should find an existing row");
    assert_eq!(found.age, 21);
    let missing = TestUser::query()
        .where_eq("email", "nonexistent@example.com")
        .first_or_fail()
        .await
        .expect_err("first_or_fail should fail for a missing row");
    assert!(matches!(missing, Error::NotFound { .. }), "{missing:?}");

    assert!(
        TestUser::query()
            .where_eq("email", "user1@example.com")
            .exists()
            .await
            .expect("Exists failed")
    );
    assert!(
        !TestUser::query()
            .where_eq("email", "nonexistent@example.com")
            .exists()
            .await
            .expect("Exists failed")
    );

    let deleted = TestUser::query()
        .where_eq("active", false)
        .delete()
        .await
        .expect("Delete failed");
    assert_eq!(deleted, 10, "Should have deleted the inactive users");
    assert_eq!(TestUser::count().await.expect("Count failed"), 10);
}

#[tokio::test]
async fn aggregations() {
    if !setup().await {
        return;
    }

    seed_users(10).await;

    let sum: i64 = TestUser::query().sum("age").await.expect("Sum failed");
    assert_eq!(sum, 255, "21 + 22 + ... + 30");
    let avg: Option<f64> = TestUser::query().avg("age").await.expect("Avg failed");
    assert!(
        avg.is_some_and(|avg| (avg - 25.5).abs() < 0.01),
        "avg was {avg:?}"
    );
    let min: Option<i32> = TestUser::query().min("age").await.expect("Min failed");
    assert_eq!(min, Some(21));
    let max: Option<i32> = TestUser::query()
        .max(TestUser::columns.age)
        .await
        .expect("Max failed");
    assert_eq!(max, Some(30));

    // Any column type, read as the model reads it.
    let first: Option<String> = TestUser::query()
        .min("name")
        .await
        .expect("min(name) failed");
    let last: Option<String> = TestUser::query()
        .max("name")
        .await
        .expect("max(name) failed");
    assert_eq!(
        (first.as_deref(), last.as_deref()),
        (Some("User 01"), Some("User 10"))
    );

    // Over no rows a sum is zero and the others are None.
    let nobody = || TestUser::query().where_gt("age", 1000);
    assert_eq!(
        nobody().sum::<i64>("age").await.expect("empty sum failed"),
        0
    );
    assert_eq!(
        nobody().avg::<f64>("age").await.expect("empty avg failed"),
        None
    );
    assert_eq!(
        nobody().max::<i32>("age").await.expect("empty max failed"),
        None
    );
    let distinct = TestUser::query()
        .count_distinct("active")
        .await
        .expect("Count distinct failed");
    assert_eq!(distinct, 2);
}

#[tokio::test]
async fn soft_delete_restore_and_force_delete() {
    if !setup().await {
        return;
    }

    assert!(TestSoftDelete::soft_delete_enabled());

    let mut records = Vec::new();
    for name in ["Record 1", "Record 2", "Record 3"] {
        records.push(
            TestSoftDelete {
                id: 0,
                name: name.into(),
                deleted_at: None,
            }
            .save()
            .await
            .expect("Failed to save"),
        );
    }
    let record2 = records.remove(1);
    let record1 = records.remove(0);

    let deleted_record = record1.soft_delete().await.expect("Failed to soft delete");
    assert!(
        deleted_record.deleted_at.is_some(),
        "deleted_at should be set"
    );

    let active = TestSoftDelete::query().get().await.expect("Query failed");
    assert_eq!(active.len(), 2, "default query excludes soft-deleted rows");

    let all = TestSoftDelete::query()
        .with_trashed()
        .get()
        .await
        .expect("Query failed");
    assert_eq!(all.len(), 3);

    let trashed = TestSoftDelete::query()
        .only_trashed()
        .get()
        .await
        .expect("Query failed");
    assert_eq!(trashed.len(), 1);
    assert_eq!(trashed[0].name, "Record 1");

    let restored = deleted_record.restore().await.expect("Failed to restore");
    assert!(
        restored.deleted_at.is_none(),
        "deleted_at should be cleared"
    );
    let active = TestSoftDelete::query().get().await.expect("Query failed");
    assert_eq!(active.len(), 3, "restore brings the row back");

    record2
        .force_delete()
        .await
        .expect("Failed to force delete");
    let remaining = TestSoftDelete::query()
        .with_trashed()
        .count()
        .await
        .expect("Count failed");
    assert_eq!(remaining, 2, "force_delete removes the row outright");
}

#[tokio::test]
async fn transactions_commit_and_roll_back() {
    if !setup().await {
        return;
    }

    TestUser::transaction(|_tx| {
        Box::pin(async move {
            let saved = user("tx_commit@example.com", "Transaction User", 25, true)
                .save()
                .await?;
            Ok(saved.id)
        })
    })
    .await
    .expect("Transaction should succeed");
    let committed = TestUser::query()
        .where_eq("email", "tx_commit@example.com")
        .first()
        .await
        .expect("Query failed");
    assert!(committed.is_some(), "User should exist after commit");

    let result: tideorm::Result<i64> = TestUser::transaction(|_tx| {
        Box::pin(async move { Err(Error::query("Intentional rollback")) })
    })
    .await;
    assert!(result.is_err(), "the closure's error should be returned");

    let db = tideorm::require_db().expect("global database should be installed");
    let raw_insert: tideorm::Result<()> = db
        .transaction(|tx| {
            Box::pin(async move {
                tx.connection()
                    .execute_unprepared(
                        "INSERT INTO test_users (email, name, age, active) \
                         VALUES ('tx_test@example.com', 'TX User', 30, true)",
                    )
                    .await
                    .map_err(Error::from)?;
                Err(Error::query("Intentional rollback"))
            })
        })
        .await;
    assert!(raw_insert.is_err(), "Transaction should fail");
    let rolled_back = TestUser::query()
        .where_eq("email", "tx_test@example.com")
        .first()
        .await
        .expect("Query failed");
    assert!(rolled_back.is_none(), "raw insert should roll back");

    let baseline = user("tx_baseline@example.com", "Baseline User", 41, true)
        .save()
        .await
        .expect("Failed to save baseline transaction user");

    let save_result: tideorm::Result<()> = TestUser::transaction(|_tx| {
        Box::pin(async move {
            user("tx_model_save@example.com", "Transaction Save", 22, true)
                .save()
                .await?;
            Err(Error::query("Intentional rollback after save"))
        })
    })
    .await;
    assert!(save_result.is_err(), "save transaction should roll back");
    let rolled_back_save = TestUser::query()
        .where_eq("email", "tx_model_save@example.com")
        .first()
        .await
        .expect("Failed to query rolled back save");
    assert!(rolled_back_save.is_none(), "saved row should not persist");

    let update_result: tideorm::Result<()> = TestUser::transaction(|_tx| {
        let baseline = baseline.clone();
        Box::pin(async move {
            TestUser {
                name: "Updated In Transaction".to_string(),
                age: 99,
                ..baseline
            }
            .update()
            .await?;
            Err(Error::query("Intentional rollback after update"))
        })
    })
    .await;
    assert!(
        update_result.is_err(),
        "update transaction should roll back"
    );
    let unchanged = TestUser::find(baseline.id)
        .await
        .expect("Failed to reload baseline user")
        .expect("Baseline user should still exist");
    assert_eq!(unchanged.name, "Baseline User");
    assert_eq!(unchanged.age, 41);

    let delete_result: tideorm::Result<()> = TestUser::transaction(|_tx| {
        let baseline = unchanged.clone();
        Box::pin(async move {
            baseline.delete().await?;
            Err(Error::query("Intentional rollback after delete"))
        })
    })
    .await;
    assert!(
        delete_result.is_err(),
        "delete transaction should roll back"
    );
    let still_present = TestUser::find(baseline.id)
        .await
        .expect("Failed to reload baseline user after delete rollback")
        .expect("Baseline user should remain after delete rollback");
    assert_eq!(still_present.email, "tx_baseline@example.com");
}

#[tokio::test]
async fn raw_sql_with_params() {
    if !setup().await {
        return;
    }

    // All active: MySQL counts changed rows, not matched ones, so the UPDATE
    // below must change every row it matches for the count to agree.
    for i in 1..=5 {
        user(
            &format!("raw{i}@example.com"),
            &format!("Raw User {i}"),
            20 + i,
            true,
        )
        .save()
        .await
        .expect("Failed to save");
    }

    let users: Vec<TestUser> = Database::raw_with_params::<TestUser>(
        &format!(
            "SELECT * FROM test_users WHERE age > {} ORDER BY age",
            param(1)
        ),
        vec![22.into()],
    )
    .await
    .expect("Raw query failed");
    let ages: Vec<i32> = users.iter().map(|user| user.age).collect();
    assert_eq!(ages, [23, 24, 25]);

    let affected = Database::execute_with_params(
        &format!(
            "UPDATE test_users SET active = false WHERE age > {}",
            param(1)
        ),
        vec![23.into()],
    )
    .await
    .expect("Execute failed");
    assert_eq!(affected, 2, "ages 24 and 25");
    let inactive = TestUser::query()
        .where_eq("active", false)
        .count()
        .await
        .expect("Count failed");
    assert_eq!(inactive, 2);
}

#[tokio::test]
async fn insert_all_assigns_distinct_ids() {
    if !setup().await {
        return;
    }

    let inserted = TestUser::insert_all(vec![
        user("batch1@example.com", "Batch 1", 25, true),
        user("batch2@example.com", "Batch 2", 30, true),
        user("batch3@example.com", "Batch 3", 35, false),
    ])
    .await
    .expect("Insert all failed");

    let emails: Vec<&str> = inserted.iter().map(|user| user.email.as_str()).collect();
    assert_eq!(
        emails,
        [
            "batch1@example.com",
            "batch2@example.com",
            "batch3@example.com"
        ]
    );
    let mut ids: Vec<i64> = inserted.iter().map(|user| user.id).collect();
    assert!(
        ids.iter().all(|id| *id > 0),
        "every row needs an id: {ids:?}"
    );
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 3, "ids must be distinct: {ids:?}");
    assert_eq!(TestUser::count().await.expect("Count failed"), 3);
}

#[tokio::test]
async fn upsert_insert_or_update_and_on_conflict() {
    if !setup().await {
        return;
    }

    let inserted = TestUser::insert_or_update(
        TestUser {
            id: 1,
            ..user("upsert@example.com", "Initial Upsert", 28, true)
        },
        vec!["id"],
    )
    .await
    .expect("insert_or_update should insert when missing");
    assert_eq!(inserted.id, 1, "an explicit primary key is kept");
    assert_eq!(inserted.name, "Initial Upsert");

    let updated = TestUser::insert_or_update(
        TestUser {
            id: 1,
            ..user("upsert@example.com", "Updated Upsert", 29, false)
        },
        vec!["id"],
    )
    .await
    .expect("insert_or_update should update on conflict");
    assert_eq!(updated.id, 1);
    assert_eq!(updated.name, "Updated Upsert");
    assert_eq!(updated.age, 29);
    assert!(!updated.active, "every non-conflict column is overwritten");

    let selective = TestUser::on_conflict(vec!["id"])
        .update_columns(vec!["name", "age"])
        .insert(TestUser {
            id: 1,
            ..user("upsert@example.com", "Selective Update", 31, true)
        })
        .await
        .expect("on_conflict builder should update chosen columns");
    assert_eq!(selective.name, "Selective Update");
    assert_eq!(selective.age, 31);
    assert!(
        !selective.active,
        "columns outside update_columns keep their value"
    );

    let reloaded = TestUser::find(1)
        .await
        .expect("Reload failed")
        .expect("User should exist after upsert");
    assert_eq!(reloaded.name, "Selective Update");
    assert!(!reloaded.active);

    let quoted_payload = "Robert'); DROP TABLE test_users; --";
    let quoted = TestUser::insert_or_update(
        TestUser {
            id: 1,
            ..user("upsert@example.com", quoted_payload, 32, true)
        },
        vec!["id"],
    )
    .await
    .expect("upsert should treat quoted payload as data");
    assert_eq!(quoted.name, quoted_payload);

    let created_at = chrono::Utc::now();
    let updated_at = created_at + chrono::TimeDelta::minutes(15);
    let inserted = TimestampUser::insert_or_update(
        TimestampUser {
            id: 0,
            email: "typed-upsert@example.com".into(),
            name: "Initial Timestamp User".into(),
            login_count: 1,
            created_at,
            updated_at,
        },
        vec!["email"],
    )
    .await
    .expect("insert_or_update should bind timestamp parameters on insert");
    assert!(inserted.id > 0, "Upsert insert should assign a primary key");
    assert_eq!(inserted.login_count, 1);
    assert!(inserted.created_at <= inserted.updated_at);

    let updated = TimestampUser::insert_or_update(
        TimestampUser {
            id: inserted.id,
            email: "typed-upsert@example.com".into(),
            name: "Updated Timestamp User".into(),
            login_count: 2,
            created_at,
            updated_at: updated_at + chrono::TimeDelta::minutes(30),
        },
        vec!["email"],
    )
    .await
    .expect("insert_or_update should bind timestamp parameters on conflict");
    assert_eq!(updated.id, inserted.id);
    assert_eq!(updated.name, "Updated Timestamp User");
    assert_eq!(updated.login_count, 2);
    // `>=`, not `>`: updated_at is managed by TideORM, and MySQL's TIMESTAMP
    // keeps whole seconds, so two upserts in one second store equal values.
    assert!(updated.created_at >= inserted.created_at);
    assert!(updated.updated_at >= inserted.updated_at);
}

#[tideorm::model(table = "window_readings")]
pub struct WindowReading {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub sensor: String,
    pub value: Option<i64>,
}

/// A `lag`/`lead` default fills in only past the edge of the partition: a NULL
/// one row away stays NULL. MariaDB's `LAG` takes no default, so the MySQL
/// family renders it differently.
#[tokio::test]
async fn lag_and_lead_defaults_apply_only_past_the_partition_edge() {
    use serde_json::{Value, json};

    if !setup().await {
        return;
    }
    fresh_table("window_readings", |t| {
        t.id();
        t.string("sensor").not_null();
        t.big_integer("value").nullable();
    })
    .await;
    for (sensor, value) in [("a", Some(1)), ("a", None), ("a", Some(3)), ("b", Some(7))] {
        WindowReading {
            id: 0,
            sensor: sensor.to_string(),
            value,
        }
        .save()
        .await
        .expect("save failed");
    }

    let rows = WindowReading::query()
        .lag("prev", "value", 1, Some("0"), "sensor", "id", Order::Asc)
        .lead("next", "value", 1, Some("0"), "sensor", "id", Order::Asc)
        .order_asc("id")
        .get_json()
        .await
        .expect("window query failed");
    let neighbours: Vec<(Value, Value)> = rows
        .iter()
        .map(|row| (row["prev"].clone(), row["next"].clone()))
        .collect();
    assert_eq!(
        neighbours,
        [
            (json!(0), json!(null)),
            (json!(1), json!(3)),
            (json!(null), json!(0)),
            (json!(0), json!(0)),
        ]
    );
}

/// `execute_returning` needs an `UPDATE .. RETURNING`, which MySQL lacks and
/// MariaDB has only from 13.0: on both it is refused before anything is written.
#[tokio::test]
async fn execute_returning_returns_the_rows_or_writes_nothing() {
    if !setup().await {
        return;
    }
    seed_users(4).await;

    let returned = TestUser::update_all()
        .set("name", "Renamed")
        .where_eq("active", true)
        .execute_returning()
        .await;
    let renamed = TestUser::query()
        .where_eq("name", "Renamed")
        .count()
        .await
        .expect("count failed");
    if matches!(
        backend::DATABASE_TYPE,
        DatabaseType::MySQL | DatabaseType::MariaDB
    ) {
        assert!(
            matches!(returned, Err(Error::BackendNotSupported { .. })),
            "{returned:?}"
        );
        assert_eq!(renamed, 0, "nothing may be written");
    } else {
        let mut ids: Vec<i64> = returned
            .expect("execute_returning failed")
            .into_iter()
            .map(|user| {
                assert_eq!(user.name, "Renamed");
                user.id
            })
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, [2, 4]);
        assert_eq!(renamed, 2);
    }
}

#[tokio::test]
async fn batch_update_filters_and_trusted_raw() {
    if !setup().await {
        return;
    }

    for i in 0..5 {
        user(
            &format!("batch-update-{i}@example.com"),
            &format!("Batch Update {i}"),
            24 + (i * 3),
            true,
        )
        .save()
        .await
        .expect("Failed to seed user for batch update");
    }

    let affected = TestUser::update_all()
        .set("active", false)
        .where_gt("age", 30)
        .execute()
        .await
        .expect("Batch update should succeed");
    assert_eq!(affected, 2, "ages 33 and 36");
    let inactive = TestUser::query()
        .where_eq("active", false)
        .count()
        .await
        .expect("Count inactive failed");
    assert_eq!(inactive, 2);
    let active = TestUser::query()
        .where_eq("active", true)
        .count()
        .await
        .expect("Count active failed");
    assert_eq!(active, 3);

    let trusted_raw_affected = TestUser::update_all()
        .set_trusted_raw("name", "'trusted-batch-update'")
        .where_eq("id", 1)
        .execute()
        .await
        .expect("set_trusted_raw should execute trusted SQL");
    assert_eq!(trusted_raw_affected, 1);
    let renamed = TestUser::find_or_fail(1)
        .await
        .expect("Reload trusted raw update failed");
    assert_eq!(renamed.name, "trusted-batch-update");
}

#[tokio::test]
async fn callbacks_run_in_order() {
    if !setup().await {
        return;
    }

    CALLBACK_EVENTS.lock().unwrap().clear();
    let created = CallbackUser {
        id: 0,
        email: "UPPER@EXAMPLE.COM".into(),
        name: "Callback User".into(),
    }
    .save()
    .await
    .expect("Callback save should succeed");
    assert_eq!(created.email, "upper@example.com");
    assert_eq!(
        CALLBACK_EVENTS.lock().unwrap().clone(),
        [
            "before_validation",
            "after_validation",
            "before_save",
            "before_create",
            "after_create",
            "after_save"
        ]
    );

    CALLBACK_EVENTS.lock().unwrap().clear();
    let updated = CallbackUser {
        id: created.id,
        email: "SECOND@EXAMPLE.COM".into(),
        name: "Callback User Updated".into(),
    }
    .update()
    .await
    .expect("Callback update should succeed");
    assert_eq!(updated.email, "second@example.com");
    assert_eq!(
        CALLBACK_EVENTS.lock().unwrap().clone(),
        [
            "before_validation",
            "after_validation",
            "before_save",
            "before_update",
            "after_update",
            "after_save"
        ]
    );

    CALLBACK_EVENTS.lock().unwrap().clear();
    assert_eq!(
        updated
            .delete()
            .await
            .expect("Callback delete should succeed"),
        1
    );
    assert_eq!(
        CALLBACK_EVENTS.lock().unwrap().clone(),
        ["before_delete", "after_delete"]
    );
}

#[tokio::test]
async fn scopes_and_conditional_filters() {
    if !setup().await {
        return;
    }

    seed_users(10).await;

    fn active_scope(q: QueryBuilder<TestUser>) -> QueryBuilder<TestUser> {
        q.where_eq("active", true)
    }

    fn adult_scope(q: QueryBuilder<TestUser>) -> QueryBuilder<TestUser> {
        q.where_gte("age", 27)
    }

    let users = TestUser::query()
        .scope(active_scope)
        .scope(adult_scope)
        .order_by("age", Order::Asc)
        .get()
        .await
        .expect("Query failed");
    let ages: Vec<i32> = users.iter().map(|user| user.age).collect();
    assert_eq!(ages, [28, 30], "scopes chain with AND");

    let filtered = TestUser::query()
        .when(true, |q| q.where_eq("active", true))
        .get()
        .await
        .expect("Query failed");
    assert_eq!(filtered.len(), 5);
    let unfiltered = TestUser::query()
        .when(false, |q| q.where_eq("active", true))
        .get()
        .await
        .expect("Query failed");
    assert_eq!(unfiltered.len(), 10);

    let min_age: Option<i32> = Some(28);
    let users = TestUser::query()
        .when_some(min_age, |q, age| q.where_gte("age", age))
        .get()
        .await
        .expect("Query failed");
    assert_eq!(users.len(), 3);
}

#[tokio::test]
async fn json_column_round_trips() {
    if !setup().await {
        return;
    }

    let metadata = serde_json::json!({
        "brand": "TechCorp",
        "features": ["fast", "lightweight"],
        "specs": {"ram": 16, "storage": 512}
    });
    TestProduct {
        id: 0,
        name: "Laptop".to_string(),
        category: "Electronics".to_string(),
        price: 999,
        metadata: Some(metadata.clone()),
    }
    .save()
    .await
    .expect("Failed to save product");
    TestProduct {
        id: 0,
        name: "Cable".to_string(),
        category: "Electronics".to_string(),
        price: 9,
        metadata: None,
    }
    .save()
    .await
    .expect("Failed to save product");

    let products = TestProduct::query()
        .where_eq("category", "Electronics")
        .order_by("price", Order::Desc)
        .get()
        .await
        .expect("Query failed");
    assert_eq!(products.len(), 2);
    assert_eq!(products[0].metadata, Some(metadata));
    assert_eq!(products[1].metadata, None);
}

/// Drop and recreate a table for one scenario, outside the shared `TABLES`.
async fn fresh_table<F>(name: &str, build: F)
where
    F: FnOnce(&mut tideorm::migration::TableBuilder),
{
    let mut schema = Schema::new(backend::DATABASE_TYPE);
    schema
        .drop_table_if_exists(name)
        .await
        .expect("failed to drop scenario table");
    schema
        .create_table(name, build)
        .await
        .expect("failed to create scenario table");
}

#[tideorm::model(table = "typed_value_rows")]
pub struct TypedValueRow {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub uid: Uuid,
    pub day: chrono::NaiveDate,
    pub at: chrono::DateTime<chrono::Utc>,
    pub amount: Decimal,
    pub note: Option<String>,
    pub data: Vec<u8>,
    pub body: String,
}

async fn typed_value_rows() -> Vec<TypedValueRow> {
    fresh_table("typed_value_rows", |t| {
        t.id();
        t.uuid("uid").not_null();
        t.date("day").not_null();
        t.timestamptz("at").not_null();
        t.decimal_with("amount", 12, 2).not_null();
        t.string("note").nullable();
        t.binary("data").not_null();
        t.text("body").not_null();
    })
    .await;
    let mut rows = Vec::new();
    for (i, (year, note)) in [(1969, None), (2001, Some("b")), (2100, Some("c"))]
        .into_iter()
        .enumerate()
    {
        let at = chrono::NaiveDate::from_ymd_opt(year, 7, 20)
            .unwrap()
            .and_hms_micro_opt(20, 17, 40, 123_456)
            .unwrap()
            .and_utc();
        rows.push(
            TypedValueRow {
                id: 0,
                uid: Uuid::from_u128(0x7d44_4840_9dc0_11d1_b245_5ffd_ce74_fa00 + i as u128),
                day: at.date_naive(),
                at,
                amount: Decimal::new(1_000 + i as i64, 2),
                note: note.map(str::to_string),
                data: vec![0, 1, 2, 255, i as u8],
                body: format!("{:0>5}", i),
            }
            .save()
            .await
            .expect("failed to save a typed row"),
        );
    }
    rows
}

#[tokio::test]
async fn typed_values_round_trip_and_filter() {
    if !setup().await {
        return;
    }

    let rows = typed_value_rows().await;
    let first = TypedValueRow::find(rows[0].id)
        .await
        .expect("find failed")
        .expect("row should exist");
    // Microseconds, a pre-1970 instant and bytes all survive the round trip.
    assert_eq!(first.at, rows[0].at);
    assert_eq!(first.data, rows[0].data);

    let by = |query: QueryBuilder<TypedValueRow>| async move {
        let mut ids: Vec<i64> = query
            .get()
            .await
            .expect("typed filter failed")
            .into_iter()
            .map(|row| row.id)
            .collect();
        ids.sort_unstable();
        ids
    };
    // Native values bind as the column's type.
    assert_eq!(
        by(TypedValueRow::query().where_eq("uid", rows[1].uid)).await,
        [rows[1].id]
    );
    assert_eq!(
        by(TypedValueRow::query().where_in("uid", vec![rows[0].uid, rows[2].uid])).await,
        [rows[0].id, rows[2].id]
    );
    assert_eq!(
        by(TypedValueRow::query().where_gt("day", rows[0].day)).await,
        [rows[1].id, rows[2].id]
    );
    assert_eq!(
        by(TypedValueRow::query().where_between("at", rows[0].at, rows[1].at)).await,
        [rows[0].id, rows[1].id]
    );
    assert_eq!(
        by(TypedValueRow::query().where_eq("amount", Decimal::new(1_001, 2))).await,
        [rows[1].id]
    );
    // A NULL member of an IN list matches NULL rows; of a NOT IN list, it
    // keeps the non-NULL rows outside the list.
    assert_eq!(
        by(TypedValueRow::query().where_in("note", vec![Some("b"), None])).await,
        [rows[0].id, rows[1].id]
    );
    assert_eq!(
        by(TypedValueRow::query().where_not_in("note", vec![Some("b"), None])).await,
        [rows[2].id]
    );

    let moved = TypedValueRow::update_all()
        .set("uid", Uuid::nil())
        .set("amount", Decimal::new(99, 1))
        .where_eq("uid", rows[2].uid)
        .execute()
        .await
        .expect("typed batch update failed");
    assert_eq!(moved, 1);
    let updated = TypedValueRow::find(rows[2].id)
        .await
        .expect("find failed")
        .expect("row should exist");
    assert_eq!(updated.uid, Uuid::nil());
    assert_eq!(updated.amount, Decimal::new(99, 1));
}

#[tokio::test]
async fn get_json_reports_model_columns_as_the_model_does() {
    if !setup().await {
        return;
    }

    let rows = typed_value_rows().await;
    let json = TypedValueRow::query()
        .order_by("id", Order::Asc)
        .get_json()
        .await
        .expect("get_json failed");
    for (row, got) in rows.iter().zip(&json) {
        assert_eq!(got, &serde_json::to_value(row).unwrap());
    }

    // Raw SQL has no model; a text column still comes back verbatim.
    let raw = Database::raw_json("SELECT body FROM typed_value_rows ORDER BY id")
        .await
        .expect("raw_json failed");
    assert_eq!(raw[0]["body"], serde_json::json!("00000"));
}

#[tokio::test]
async fn model_queries_survive_a_column_added_under_them() {
    if !setup().await {
        return;
    }

    seed_users(3).await;
    assert_eq!(TestUser::query().get().await.expect("read failed").len(), 3);
    // A deploy adds a column the running code does not know yet.
    Database::execute("ALTER TABLE test_users ADD COLUMN nickname VARCHAR(20)")
        .await
        .expect("ALTER TABLE failed");
    assert_eq!(
        TestUser::query()
            .get()
            .await
            .expect("read after ALTER failed")
            .len(),
        3
    );
    let rows = TestUser::query().get_json().await.expect("get_json failed");
    assert!(rows[0].get("nickname").is_none(), "{:?}", rows[0]);
}

#[tokio::test]
async fn statements_past_the_bind_parameter_limit() {
    if !setup().await {
        return;
    }

    seed_users(3).await;
    // Past 65,535 placeholders, the most any backend binds in one statement.
    let ids: Vec<i64> = (1..=70_000).collect();
    let found = TestUser::query()
        .where_in("id", ids.clone())
        .count()
        .await
        .expect("a 70,000-id IN list failed");
    assert_eq!(found, 3);
    let none = TestUser::query()
        .where_not_in("id", ids)
        .count()
        .await
        .expect("a 70,000-id NOT IN list failed");
    assert_eq!(none, 0);

    // 40,000 text values: past SQLite's 32,766 parameters, which reads them
    // from one JSON value, as PostgreSQL does from one array.
    let mut emails: Vec<String> = (0..40_000)
        .map(|i| format!("nobody{i}@example.com"))
        .collect();
    emails.push("user2@example.com".to_string());
    let found = TestUser::query()
        .where_in("email", emails.clone())
        .get()
        .await
        .expect("a 40,000-value text IN list failed");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].email, "user2@example.com");
    let others = TestUser::query()
        .where_not_in("email", emails)
        .count()
        .await
        .expect("a 40,000-value text NOT IN list failed");
    assert_eq!(others, 2);

    // Four bound columns each (the database assigns the key): 8,500 rows bind
    // 34,000 parameters, past SQLite's 32,766, so the batch is split.
    let batch: Vec<TestUser> = (0..8_500)
        .map(|i| user(&format!("bulk{i}@example.com"), "Bulk", 30, i % 2 == 0))
        .collect();
    let inserted = TestUser::insert_all(batch)
        .await
        .expect("a batch past the parameter limit failed");
    assert_eq!(inserted.len(), 8_500);
    assert_eq!(TestUser::count().await.expect("count failed"), 8_503);
    // Returned in input order, each with the key its own row got.
    for (i, model) in inserted.iter().enumerate() {
        assert_eq!(model.email, format!("bulk{i}@example.com"));
    }
    assert!(inserted.windows(2).all(|pair| pair[0].id < pair[1].id));
    for model in [&inserted[0], &inserted[4_321], &inserted[8_499]] {
        let stored = TestUser::find(model.id)
            .await
            .expect("find failed")
            .expect("row should exist");
        assert_eq!(stored.email, model.email);
    }
}

#[tideorm::model(table = "validated_rows")]
pub struct ValidatedRow {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    #[validate(email)]
    pub email: String,
}

#[tokio::test]
async fn bulk_and_upsert_writes_are_validated() {
    if !setup().await {
        return;
    }

    fresh_table("validated_rows", |t| {
        t.id();
        t.string("email").not_null();
    })
    .await;
    let row = |email: &str| ValidatedRow {
        id: 0,
        email: email.to_string(),
    };
    assert!(
        ValidatedRow::insert_all(vec![row("a@example.com"), row("not an email")])
            .await
            .is_err()
    );
    assert!(
        ValidatedRow::on_conflict(vec!["id"])
            .insert(row("not an email"))
            .await
            .is_err()
    );
    assert_eq!(ValidatedRow::count().await.expect("count failed"), 0);
}

#[tideorm::model(table = "naive_timestamp_rows")]
pub struct NaiveTimestampRow {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[tokio::test]
async fn naive_timestamps_are_managed_like_utc_ones() {
    if !setup().await {
        return;
    }

    fresh_table("naive_timestamp_rows", |t| {
        t.id();
        t.string("name").not_null();
        t.timestamps_naive();
    })
    .await;
    let before = chrono::Utc::now().naive_utc() - chrono::TimeDelta::seconds(5);
    let saved = NaiveTimestampRow {
        id: 0,
        name: "a".into(),
        created_at: Default::default(),
        updated_at: Default::default(),
    }
    .save()
    .await
    .expect("save failed");
    assert!(saved.created_at > before, "{}", saved.created_at);
    assert!(saved.updated_at > before, "{}", saved.updated_at);
}

#[tokio::test]
async fn model_paginate_orders_by_primary_key() {
    if !setup().await {
        return;
    }

    seed_users(10).await;
    let first = TestUser::paginate(1, 5).await.expect("page 1 failed");
    // PostgreSQL moves a rewritten row to the end of its heap.
    TestUser::update_all()
        .set("name", "moved")
        .where_eq("id", first[2].id)
        .execute()
        .await
        .expect("update failed");
    let second = TestUser::paginate(2, 5).await.expect("page 2 failed");
    let ids: Vec<i64> = first.iter().chain(&second).map(|user| user.id).collect();
    assert_eq!(ids, (1..=10).collect::<Vec<_>>());
}

#[tokio::test]
async fn constraint_failures_keep_their_kind() {
    if !setup().await {
        return;
    }

    Database::execute("DROP TABLE IF EXISTS constraint_children")
        .await
        .expect("drop failed");
    Database::execute(
        "CREATE TABLE constraint_children (id INTEGER PRIMARY KEY, user_id BIGINT NOT NULL, \
         qty INTEGER NOT NULL CHECK (qty > 0), FOREIGN KEY (user_id) REFERENCES test_users(id))",
    )
    .await
    .expect("create failed");
    seed_users(1).await;
    let insert = |values: &str| {
        let sql = format!("INSERT INTO constraint_children (id, user_id, qty) VALUES ({values})");
        async move { Database::execute(&sql).await }
    };
    let orphan = insert("1, 999, 1")
        .await
        .expect_err("an orphan row inserted");
    assert_eq!(
        orphan.failure_kind(),
        tideorm::error::DbFailureKind::ForeignKeyViolation
    );
    let zero = insert("2, 1, 0").await.expect_err("qty 0 passed its CHECK");
    assert_eq!(
        zero.failure_kind(),
        tideorm::error::DbFailureKind::CheckViolation
    );
    Database::execute("DROP TABLE constraint_children")
        .await
        .expect("drop failed");
}

#[tokio::test]
async fn json_containment_agrees_across_backends() {
    if !setup().await {
        return;
    }

    for (name, metadata) in [
        (
            "laptop",
            serde_json::json!({"brand": "Tech", "tags": ["fast", "light"], "specs": {"ram": 16, "ssd": true}}),
        ),
        (
            "cable",
            serde_json::json!({"brand": "Wire", "tags": ["long"], "specs": {"ram": 0}}),
        ),
        ("list", serde_json::json!([1, 2, 3])),
        ("scalar_tag", serde_json::json!({"tags": "fast"})),
        ("wrapped", serde_json::json!([{"k": 1}])),
        ("keyed", serde_json::json!({"k": 1})),
        ("word", serde_json::json!("a")),
    ] {
        TestProduct {
            id: 0,
            name: name.to_string(),
            category: "json".to_string(),
            price: 1,
            metadata: Some(metadata),
        }
        .save()
        .await
        .expect("failed to save product");
    }
    let names = |candidate: serde_json::Value| async move {
        let mut names: Vec<String> = TestProduct::query()
            .where_json_contains("metadata", candidate)
            .get()
            .await
            .expect("json containment failed")
            .into_iter()
            .map(|product| product.name)
            .collect();
        names.sort();
        names
    };
    assert_eq!(
        names(serde_json::json!({"brand": "Tech"})).await,
        ["laptop"]
    );
    assert_eq!(
        names(serde_json::json!({"tags": ["light"]})).await,
        ["laptop"]
    );
    assert_eq!(
        names(serde_json::json!({"specs": {"ssd": true}})).await,
        ["laptop"]
    );
    assert_eq!(
        names(serde_json::json!({"specs": {"ram": 16}})).await,
        ["laptop"]
    );
    assert_eq!(names(serde_json::json!([3, 1])).await, ["list"]);
    assert!(names(serde_json::json!({"brand": "Nope"})).await.is_empty());
    assert!(
        names(serde_json::json!({"tags": ["fast", "long"]}))
            .await
            .is_empty()
    );

    // Where MySQL's JSON_CONTAINS used to disagree: a scalar or an object only
    // matches its own kind below the top level, and only a scalar matches a
    // top-level array holding it.
    assert_eq!(
        names(serde_json::json!({"tags": "fast"})).await,
        ["scalar_tag"]
    );
    assert_eq!(names(serde_json::json!({"k": 1})).await, ["keyed"]);
    assert_eq!(names(serde_json::json!("a")).await, ["word"]);

    let contained_in = |target: serde_json::Value| async move {
        let mut names: Vec<String> = TestProduct::query()
            .where_json_contained_by("metadata", target)
            .get()
            .await
            .expect("json containment failed")
            .into_iter()
            .map(|product| product.name)
            .collect();
        names.sort();
        names
    };
    assert!(
        contained_in(serde_json::json!({"tags": ["fast", "slow"]}))
            .await
            .is_empty()
    );
    assert_eq!(
        contained_in(serde_json::json!(["a", "b", {"k": 1}])).await,
        ["word", "wrapped"]
    );
}

#[tokio::test]
async fn scaling_an_integer_column_keeps_it_readable() {
    if !setup().await {
        return;
    }

    seed_users(3).await;
    // SQLite stored `age * 1.1` as a REAL, after which no row decoded.
    TestUser::update_all()
        .multiply("age", 1.1)
        .where_gt("age", 0)
        .execute()
        .await
        .expect("multiply failed");
    let ages: Vec<i32> = TestUser::query()
        .order_by("id", Order::Asc)
        .get()
        .await
        .expect("rows no longer decode after multiply")
        .into_iter()
        .map(|user| user.age)
        .collect();
    assert_eq!(ages, [23, 24, 25]);
}

#[tokio::test]
async fn a_locked_read_check_write_does_not_oversell() {
    if !setup().await {
        return;
    }

    let id = user("stock@example.com", "Stock", 4, true)
        .save()
        .await
        .expect("save failed")
        .id;
    // Two requests each take 3 of the 4 at once. Unlocked, both read 4 and
    // both succeed; locked, the second waits for the first to commit.
    let take = move || {
        TestUser::transaction(move |_tx| {
            Box::pin(async move {
                let mut row = TestUser::query()
                    .where_eq("id", id)
                    .lock_for_update()
                    .first_or_fail()
                    .await?;
                if row.age < 3 {
                    return Ok(false);
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                row.age -= 3;
                row.update().await?;
                Ok(true)
            })
        })
    };
    let (a, b) = tokio::join!(tokio::spawn(take()), tokio::spawn(take()));
    let shipped = [a.unwrap(), b.unwrap()]
        .into_iter()
        .map(|result| result.expect("a locked transaction failed"))
        .filter(|&shipped| shipped)
        .count();
    let left = TestUser::find(id)
        .await
        .expect("find failed")
        .expect("row should exist")
        .age;
    assert_eq!((shipped, left), (1, 1));
}

#[tideorm::model(table = "stamped_accounts")]
pub struct StampedAccount {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub plan: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[tideorm::model(table = "stamped_settings")]
pub struct StampedSetting {
    #[tideorm(primary_key)]
    pub key: String,
    pub value: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[tokio::test]
async fn managed_timestamps_survive_upserts_and_request_bodies() {
    if !setup().await {
        return;
    }

    fresh_table("stamped_accounts", |t| {
        t.id();
        t.string("email").not_null().unique();
        t.string("plan").not_null();
        t.timestamps();
    })
    .await;
    fresh_table("stamped_settings", |t| {
        t.string("key").primary_key();
        t.string("value").not_null();
        t.timestamps();
    })
    .await;
    let pause = || tokio::time::sleep(std::time::Duration::from_millis(20));

    // A request body may leave the managed timestamps out.
    let body: StampedAccount =
        serde_json::from_str(r#"{"email": "a@example.com", "plan": "free"}"#).unwrap();
    let original = body.save().await.expect("save failed");
    let created_at = original.created_at;
    pause().await;

    // An update from a body that names created_at keeps the stored one.
    let body = format!(
        r#"{{"id": {}, "email": "a@example.com", "plan": "pro", "created_at": "2001-01-01T00:00:00Z"}}"#,
        original.id
    );
    let updated = serde_json::from_str::<StampedAccount>(&body)
        .unwrap()
        .update()
        .await
        .expect("update failed");
    assert_eq!(updated.created_at, created_at);
    assert!(updated.updated_at > original.updated_at);

    // Both upsert forms keep an existing row's creation time.
    let incoming = || StampedAccount {
        email: "a@example.com".into(),
        plan: "team".into(),
        ..Default::default()
    };
    StampedAccount::insert_or_update(incoming(), vec!["email"])
        .await
        .expect("upsert failed");
    StampedAccount::on_conflict(vec!["email"])
        .update_all_except(vec!["email"])
        .insert(incoming())
        .await
        .expect("upsert failed");
    let stored = StampedAccount::find(original.id)
        .await
        .expect("find failed")
        .expect("row should exist");
    assert_eq!(
        (stored.plan.as_str(), stored.created_at),
        ("team", created_at)
    );

    // A natural-key upsert stamps its insert instead of writing the epoch.
    let setting = |value: &str| StampedSetting {
        key: "theme".into(),
        value: value.into(),
        ..Default::default()
    };
    let first = StampedSetting::insert_or_update(setting("dark"), vec!["key"])
        .await
        .expect("upsert failed");
    assert_ne!(first.created_at, chrono::DateTime::<chrono::Utc>::default());
    pause().await;
    let second = StampedSetting::insert_or_update(setting("light"), vec!["key"])
        .await
        .expect("upsert failed");
    assert_eq!(second.value, "light");
    assert_eq!(second.created_at, first.created_at);
    assert!(second.updated_at > first.updated_at);
}

#[tokio::test]
async fn a_cached_read_does_not_outlive_a_commit() {
    use std::time::Duration;
    use tideorm::cache::{CacheConfig, QueryCache};

    if !setup().await {
        return;
    }

    let id = user("cached@example.com", "Before", 30, true)
        .save()
        .await
        .expect("save failed")
        .id;
    let previous = QueryCache::global().config();
    QueryCache::init_global(CacheConfig {
        enabled: true,
        ..CacheConfig::default()
    });
    let cached_name = move || async move {
        TestUser::query()
            .where_eq("id", id)
            .cache(Duration::from_secs(60))
            .first()
            .await
            .expect("read failed")
            .expect("row should exist")
            .name
    };
    assert_eq!(cached_name().await, "Before");

    let writer = tokio::spawn(TestUser::transaction(move |_tx| {
        Box::pin(async move {
            let mut row = TestUser::find(id).await?.expect("row should exist");
            row.name = "After".into();
            row.update().await?;
            // Other requests read, and cache, the committed row meanwhile.
            tokio::time::sleep(Duration::from_millis(300)).await;
            Ok(())
        })
    }));
    tokio::time::sleep(Duration::from_millis(100)).await;
    let during = cached_name().await;
    writer
        .await
        .expect("writer panicked")
        .expect("transaction failed");
    let after = cached_name().await;
    QueryCache::init_global(previous);
    assert_eq!(after, "After", "read {during:?} while the write was open");
}

#[tideorm::model(table = "plain_json_docs")]
pub struct PlainJsonDoc {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub body: serde_json::Value,
}

/// `where_eq`, `where_not`, `where_in` and `where_not_in` on a JSON column
/// compare documents: PostgreSQL has no `=` for `json` and failed, and MySQL
/// compared the document with a string and matched nothing.
#[tokio::test]
async fn equality_filters_compare_json_documents() {
    use serde_json::json;

    if !setup().await {
        return;
    }
    fresh_table("plain_json_docs", |t| {
        t.id();
        t.json("body").not_null();
    })
    .await;
    for body in [json!({"a": 1, "b": [1, 2]}), json!({"a": 2}), json!([1, 2])] {
        PlainJsonDoc { id: 0, body }
            .save()
            .await
            .expect("save failed");
    }
    let count = |query: QueryBuilder<PlainJsonDoc>| async move {
        query.count().await.expect("json equality filter failed")
    };

    assert_eq!(
        count(PlainJsonDoc::query().where_eq("body", json!({"a": 1, "b": [1, 2]}))).await,
        1
    );
    assert_eq!(
        count(PlainJsonDoc::query().where_not("body", json!({"a": 2}))).await,
        2
    );
    assert_eq!(
        count(PlainJsonDoc::query().where_in("body", [json!({"a": 2}), json!([1, 2])])).await,
        2
    );
    assert_eq!(
        count(PlainJsonDoc::query().where_not_in("body", [json!({"a": 2}), json!([1, 2])])).await,
        1
    );
    // An array's order is part of the document.
    assert_eq!(
        count(PlainJsonDoc::query().where_eq("body", json!([2, 1]))).await,
        0
    );

    // The JSON filters join an OR group like any other.
    assert_eq!(
        count(
            PlainJsonDoc::query()
                .or_where_json_contains("body", json!({"a": 2}))
                .or_where_json_key_exists("body", "b")
        )
        .await,
        2
    );
}

/// A `json` column (`t.json(..)`), which PostgreSQL gives none of the `jsonb`
/// operators, in both directions of containment.
#[tokio::test]
async fn json_operators_work_on_a_plain_json_column() {
    use serde_json::json;

    if !setup().await {
        return;
    }

    fresh_table("plain_json_docs", |t| {
        t.id();
        t.json("body").not_null();
    })
    .await;
    for body in [
        json!({"a": 1}),
        json!({"a": 1, "b": 2}),
        json!([1, 2]),
        json!({"a": "x%y"}),
        json!(1),
    ] {
        PlainJsonDoc { id: 0, body }
            .save()
            .await
            .expect("save failed");
    }
    let ids = |query: QueryBuilder<PlainJsonDoc>| async move {
        let mut ids: Vec<i64> = query
            .get()
            .await
            .expect("json filter failed")
            .into_iter()
            .map(|doc| doc.id)
            .collect();
        ids.sort_unstable();
        ids
    };
    let contains = |value| ids(PlainJsonDoc::query().where_json_contains("body", value));
    let within = |value| ids(PlainJsonDoc::query().where_json_contained_by("body", value));

    assert_eq!(contains(json!({"a": 1})).await, [1, 2]);
    assert_eq!(contains(json!([1])).await, [3]);
    assert_eq!(within(json!({"a": 1, "b": 2, "c": 3})).await, [1, 2]);
    // An array holds a bare scalar only at the top level.
    assert_eq!(within(json!([2, 1, 3])).await, [3, 5]);
    // Text is compared as a value, never as a LIKE pattern.
    assert!(within(json!({"a": "xzy"})).await.is_empty());
    assert_eq!(within(json!({"a": "x%y", "z": 0})).await, [4]);
    assert_eq!(
        ids(PlainJsonDoc::query().where_json_key_exists("body", "b")).await,
        [2]
    );
    assert_eq!(
        ids(PlainJsonDoc::query().where_json_path_exists("body", "$.a")).await,
        [1, 2, 4]
    );

    // An object or a string is set as that JSON value, not as its text.
    for (id, value, body) in [
        (1, json!(9), json!({"a": 9})),
        (
            2,
            json!({"c": [1, "x"]}),
            json!({"a": {"c": [1, "x"]}, "b": 2}),
        ),
        (4, json!("text"), json!({"a": "text"})),
    ] {
        let moved = PlainJsonDoc::update_all()
            .json_set("body", "$.a", value)
            .where_eq("id", id)
            .execute()
            .await
            .expect("json_set failed");
        assert_eq!(moved, 1);
        let doc = PlainJsonDoc::find(id)
            .await
            .expect("find failed")
            .expect("row should exist");
        assert_eq!(doc.body, body);
    }
}

#[tideorm::model(table = "uuid_keyed_codes")]
pub struct UuidKeyedCode {
    #[tideorm(primary_key)]
    pub id: Uuid,
    pub code: String,
    pub hits: i64,
}

#[tokio::test]
async fn an_unset_uuid_key_is_generated_on_insert() {
    if !setup().await {
        return;
    }

    fresh_table("uuid_keyed_codes", |t| {
        t.uuid("id").primary_key();
        t.string("code").not_null().unique();
        t.big_integer("hits").not_null();
    })
    .await;
    let code = |code: &str, hits: i64| UuidKeyedCode {
        id: Uuid::nil(),
        code: code.to_string(),
        hits,
    };

    // A nil key used to be stored as-is, so the second such row collided.
    let first = code("a", 1).save().await.expect("first save failed");
    let second = code("b", 1).save().await.expect("second save failed");
    assert!(!first.id.is_nil() && first.id != second.id);

    let batch = UuidKeyedCode::insert_all(vec![code("c", 1), code("d", 1)])
        .await
        .expect("insert_all failed");
    assert!(batch.iter().all(|row| !row.id.is_nil()));
    assert_ne!(batch[0].id, batch[1].id);

    let upserted = UuidKeyedCode::insert_or_update(code("a", 2), vec!["code"])
        .await
        .expect("upsert failed");
    assert_eq!((upserted.id, upserted.hits), (first.id, 2));

    let kept = Uuid::new_v4();
    let given = UuidKeyedCode {
        id: kept,
        ..code("e", 1)
    }
    .save()
    .await
    .expect("save failed");
    assert_eq!(given.id, kept);
}

#[tideorm::model(table = "camel_owners", hidden = "password_hash")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CamelOwner {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub display_name: String,
    pub password_hash: String,
    #[tideorm(has_many = "CamelPet", foreign_key = "owner_id")]
    #[serde(default)]
    pub pets: HasMany<CamelPet>,
}

#[tideorm::model(table = "camel_pets")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CamelPet {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub owner_id: i64,
    pub pet_name: String,
}

/// A model that derives `Serialize` itself with renamed keys: TideORM has to
/// find its fields under the keys serde writes, not under their names.
#[tokio::test]
async fn renamed_serde_keys_keep_hidden_fields_hidden_and_children_attached() {
    use tideorm::model::NestedSave;

    if !setup().await {
        return;
    }

    fresh_table("camel_owners", |t| {
        t.id();
        t.string("display_name").not_null();
        t.string("password_hash").not_null();
    })
    .await;
    fresh_table("camel_pets", |t| {
        t.id();
        t.big_integer("owner_id").not_null();
        t.string("pet_name").not_null();
    })
    .await;

    let owner = CamelOwner {
        id: 0,
        display_name: "Ada".into(),
        password_hash: "secret-hash".into(),
        pets: Default::default(),
    };
    let pets = vec![
        CamelPet {
            id: 0,
            owner_id: 0,
            pet_name: "Rex".into(),
        },
        CamelPet {
            id: 0,
            owner_id: 0,
            pet_name: "Tom".into(),
        },
    ];
    let (owner, pets) = owner
        .save_with_many(pets, "owner_id")
        .await
        .expect("nested save failed");
    assert!(pets.iter().all(|pet| pet.owner_id == owner.id), "{pets:?}");

    let json = owner.to_json(None);
    assert_eq!(json["displayName"], "Ada");
    assert!(json.get("passwordHash").is_none(), "{json}");

    let loaded = CamelOwner::query()
        .with("pets")
        .first()
        .await
        .expect("eager load failed")
        .expect("row should exist");
    assert_eq!(loaded.pets.get_cached().map(|pets| pets.len()), Some(2));

    // Any query method shapes the root query of an eager load.
    let shaped = CamelOwner::query()
        .with("pets")
        .query(|query| {
            query
                .where_starts_with("display_name", "Ad")
                .order_desc("id")
        })
        .get()
        .await
        .expect("shaped eager load failed");
    assert_eq!(shaped.len(), 1);
    assert_eq!(shaped[0].pets.get_cached().map(|pets| pets.len()), Some(2));
}

#[tideorm::model(table = "unsigned_keyed_rows")]
pub struct UnsignedKeyedRow {
    #[tideorm(primary_key)]
    pub id: u64,
    pub label: String,
}

/// A `u64` past `i64::MAX`, such as a key or filter read from a request,
/// matches nothing instead of panicking the SQLite and PostgreSQL drivers.
#[tokio::test]
async fn unsigned_values_past_i64_max_match_nothing() {
    if !setup().await {
        return;
    }

    seed_users(3).await;
    // Empty: only MySQL's driver reads a `u64` column back, but every backend
    // must look one up.
    fresh_table("unsigned_keyed_rows", |t| {
        t.big_integer("id").primary_key();
        t.string("label").not_null();
    })
    .await;

    let huge = u64::MAX;
    assert!(
        UnsignedKeyedRow::find(huge)
            .await
            .expect("find failed")
            .is_none()
    );
    assert!(!UnsignedKeyedRow::exists(huge).await.expect("exists failed"));

    let users = TestUser::query().where_eq("id", huge).get().await;
    assert!(users.expect("where_eq failed").is_empty());
    let users = TestUser::query().where_in("id", vec![1, huge]).get().await;
    assert_eq!(users.expect("where_in failed").len(), 1);
    let below = TestUser::query().where_lt("id", huge).count().await;
    assert_eq!(below.expect("where_lt failed"), 3);

    let error = TestUser::paginate(1, huge)
        .await
        .expect_err("a page size past i64::MAX must be rejected");
    assert!(matches!(error, Error::Validation { .. }), "{error:?}");
    let page = TestUser::paginate(2, 2).await.expect("paginate failed");
    assert_eq!(page.len(), 1);
}

#[tideorm::model(table = "unsigned_counters")]
pub struct UnsignedCounter {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub hits: u64,
}

/// Where the driver cannot read a `u64` back, the write is refused before it
/// starts, instead of being stored and then reported as a failure.
#[tokio::test]
async fn an_integer_type_the_driver_cannot_read_back_is_refused_before_the_write() {
    if !setup().await {
        return;
    }

    fresh_table("unsigned_counters", |t| {
        t.id();
        t.column("hits", tideorm::migration::ColumnType::BigUnsigned)
            .not_null();
    })
    .await;
    let counter = || UnsignedCounter {
        id: 0,
        hits: u64::MAX,
    };

    let saved = counter().save().await;
    let batch = UnsignedCounter::insert_all(vec![counter(), counter()]).await;
    let stored = UnsignedCounter::query()
        .count()
        .await
        .expect("count failed");
    if matches!(
        backend::DATABASE_TYPE,
        DatabaseType::MySQL | DatabaseType::MariaDB
    ) {
        assert_eq!(saved.expect("MySQL stores a u64").hits, u64::MAX);
        assert_eq!(batch.expect("MySQL stores u64s").len(), 2);
        assert_eq!(stored, 3);
    } else {
        assert!(matches!(saved, Err(Error::Conversion { .. })), "{saved:?}");
        assert!(matches!(batch, Err(Error::Conversion { .. })), "{batch:?}");
        assert_eq!(stored, 0, "nothing may be written");
    }
}

#[tideorm::model(table = "linked_articles")]
pub struct LinkedArticle {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    #[tideorm(
        has_many_through = "LinkedLabel",
        pivot = "linked_article_labels",
        foreign_key = "article_id",
        related_key = "label_id"
    )]
    pub labels: HasManyThrough<LinkedLabel, LinkedArticleLabel>,
}

#[tideorm::model(table = "linked_labels")]
pub struct LinkedLabel {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
}

#[tideorm::model(table = "linked_article_labels")]
pub struct LinkedArticleLabel {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub article_id: i64,
    pub label_id: i64,
}

/// Attaching a missing pair from many tasks at once against a unique pivot
/// key, attaching it again, and attaching other pairs at the same time each
/// store one row and report no error; MySQL used to deadlock on the first.
#[tokio::test]
async fn attach_stores_one_pivot_row_even_when_calls_race() {
    if !setup().await {
        return;
    }

    fresh_table("linked_articles", |t| {
        t.id();
        t.string("title").not_null();
    })
    .await;
    fresh_table("linked_labels", |t| {
        t.id();
        t.string("name").not_null();
    })
    .await;
    fresh_table("linked_article_labels", |t| {
        t.id();
        t.big_integer("article_id").not_null();
        t.big_integer("label_id").not_null();
        t.unique_index(&["article_id", "label_id"]);
    })
    .await;
    let article = LinkedArticle {
        id: 0,
        title: "Release notes".into(),
        labels: Default::default(),
    }
    .save()
    .await
    .expect("save article failed");
    let label = LinkedLabel {
        id: 0,
        name: "news".into(),
    }
    .save()
    .await
    .expect("save label failed");

    let mut racing = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let labels = article.labels.clone();
        racing.spawn(async move { labels.attach(label.id).await });
    }
    while let Some(result) = racing.join_next().await {
        result
            .expect("attach task panicked")
            .expect("racing attach of a missing pair failed");
    }
    article
        .labels
        .attach(label.id)
        .await
        .expect("attaching an attached pair failed");
    assert_eq!(
        LinkedArticleLabel::query()
            .count()
            .await
            .expect("count failed"),
        1
    );

    // Different pairs landing in the same stretch of the unique index.
    let mut others = Vec::new();
    for name in ["a", "b", "c", "d"] {
        let other = LinkedLabel {
            id: 0,
            name: name.into(),
        }
        .save()
        .await
        .expect("save label failed");
        others.push(other.id);
    }
    let mut racing = tokio::task::JoinSet::new();
    for id in others {
        let labels = article.labels.clone();
        racing.spawn(async move { labels.attach(id).await });
    }
    while let Some(result) = racing.join_next().await {
        result
            .expect("attach task panicked")
            .expect("racing attach of another pair failed");
    }

    let labels = article.labels.load().await.expect("load failed");
    assert_eq!(labels.len(), 5);
}

#[tideorm::model(table = "binned_articles")]
pub struct BinnedArticle {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    #[tideorm(
        has_many_through = "LinkedLabel",
        pivot = "binned_article_labels",
        foreign_key = "article_id",
        related_key = "label_id"
    )]
    pub labels: HasManyThrough<LinkedLabel, BinnedArticleLabel>,
}

#[tideorm::model(table = "binned_article_labels", soft_delete)]
pub struct BinnedArticleLabel {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub article_id: i64,
    pub label_id: i64,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// A soft-deleted pivot row unlinks its pair for every read, `attach()`
/// brings it back, and `sync()` clears it with the live rows.
#[tokio::test]
async fn a_soft_deleted_pivot_row_unlinks_its_pair() {
    if !setup().await {
        return;
    }

    fresh_table("binned_articles", |t| {
        t.id();
        t.string("title").not_null();
    })
    .await;
    fresh_table("linked_labels", |t| {
        t.id();
        t.string("name").not_null();
    })
    .await;
    fresh_table("binned_article_labels", |t| {
        t.id();
        t.big_integer("article_id").not_null();
        t.big_integer("label_id").not_null();
        t.soft_deletes();
        t.unique_index(&["article_id", "label_id"]);
    })
    .await;
    let article = BinnedArticle {
        id: 0,
        title: "Release notes".into(),
        labels: Default::default(),
    }
    .save()
    .await
    .expect("save article failed");
    let mut label_ids = Vec::new();
    for name in ["news", "draft"] {
        let label = LinkedLabel {
            id: 0,
            name: name.into(),
        }
        .save()
        .await
        .expect("save label failed");
        label_ids.push(label.id);
    }
    let (news, draft) = (label_ids[0], label_ids[1]);
    for id in [news, draft] {
        article.labels.attach(id).await.expect("attach failed");
    }

    let names = |labels: Vec<LinkedLabel>| {
        let mut names: Vec<String> = labels.into_iter().map(|label| label.name).collect();
        names.sort();
        names
    };
    let pivot_rows = || async {
        BinnedArticleLabel::query()
            .with_trashed()
            .count()
            .await
            .expect("pivot count failed")
    };

    BinnedArticleLabel::query()
        .where_eq("label_id", news)
        .soft_delete()
        .await
        .expect("soft delete failed");
    assert_eq!(
        names(article.labels.load().await.expect("load failed")),
        ["draft"]
    );
    assert_eq!(article.labels.count().await.expect("count failed"), 1);
    let eager = BinnedArticle::query()
        .with("labels")
        .first()
        .await
        .expect("eager load failed")
        .expect("row should exist");
    assert_eq!(
        eager
            .labels
            .get_cached()
            .map(|labels| names(labels.to_vec())),
        Some(vec!["draft".to_string()])
    );

    // Attaching the pair again restores its row instead of adding one.
    article.labels.attach(news).await.expect("re-attach failed");
    assert_eq!(
        names(article.labels.load().await.expect("load failed")),
        ["draft", "news"]
    );
    assert_eq!(pivot_rows().await, 2);
    assert_eq!(article.labels.count().await.expect("count failed"), 2);

    // A pivot row whose related row is gone counts no more than it loads.
    LinkedLabel::query()
        .where_eq("id", draft)
        .delete()
        .await
        .expect("delete label failed");
    assert_eq!(
        names(article.labels.load().await.expect("load failed")),
        ["news"]
    );
    assert_eq!(article.labels.count().await.expect("count failed"), 1);

    BinnedArticleLabel::query()
        .where_eq("label_id", news)
        .soft_delete()
        .await
        .expect("soft delete failed");
    article.labels.sync(vec![news]).await.expect("sync failed");
    assert_eq!(
        names(article.labels.load().await.expect("load failed")),
        ["news"]
    );
    assert_eq!(pivot_rows().await, 1);

    article
        .labels
        .sync(Vec::<i64>::new())
        .await
        .expect("sync to nothing failed");
    assert!(article.labels.load().await.expect("load failed").is_empty());
    assert_eq!(pivot_rows().await, 0);
}

/// On a soft-delete model every read leaves trashed rows out, `find` included,
/// and every delete marks the row; `force_delete` removes it.
#[tokio::test]
async fn soft_delete_models_hide_and_mark_rather_than_remove() {
    use tideorm::SoftDelete;

    if !setup().await {
        return;
    }
    let mut ids = Vec::new();
    for name in ["a", "b", "c"] {
        let row = TestSoftDelete {
            id: 0,
            name: name.into(),
            deleted_at: None,
        }
        .save()
        .await
        .expect("save failed");
        ids.push(row.id);
    }
    let (a, b) = (ids[0], ids[1]);

    let row = TestSoftDelete::find(a)
        .await
        .expect("find failed")
        .expect("row a exists");
    assert_eq!(row.delete().await.expect("delete failed"), 1);
    assert!(
        TestSoftDelete::find(a)
            .await
            .expect("find failed")
            .is_none()
    );
    assert!(!TestSoftDelete::exists(a).await.expect("exists failed"));
    let trashed = TestSoftDelete::query()
        .with_trashed()
        .find(a)
        .await
        .expect("with_trashed().find failed")
        .expect("with_trashed().find reads a trashed row");
    assert!(trashed.deleted_at.is_some(), "delete() marked the row");
    let trashed = trashed.reload().await.expect("reload reads a trashed row");

    assert_eq!(TestSoftDelete::destroy(b).await.expect("destroy failed"), 1);
    assert_eq!(
        TestSoftDelete::destroy(b).await.expect("destroy failed"),
        0,
        "a trashed row is not destroyed again"
    );
    let marked = TestSoftDelete::query().where_eq("name", "c").delete().await;
    assert_eq!(marked.expect("query delete failed"), 1);
    let total = || async {
        TestSoftDelete::query()
            .with_trashed()
            .count()
            .await
            .expect("count failed")
    };
    assert_eq!(total().await, 3, "every delete marked, none removed");
    assert!(
        TestSoftDelete::query()
            .only_trashed()
            .where_eq("name", "b")
            .delete()
            .await
            .is_err(),
        "deleting the trash asks for force_delete()"
    );

    assert_eq!(
        trashed.force_delete().await.expect("force_delete failed"),
        1
    );
    assert_eq!(total().await, 2);
}

/// `only_trashed()` is filter enough to restore or empty the whole trash, and
/// `force_delete()` under it never reaches a live row.
#[tokio::test]
async fn the_trash_can_be_restored_or_emptied_as_a_whole() {
    if !setup().await {
        return;
    }

    for name in ["kept", "binned", "binned"] {
        TestSoftDelete {
            id: 0,
            name: name.into(),
            deleted_at: None,
        }
        .save()
        .await
        .expect("save failed");
    }
    let binned = || TestSoftDelete::query().where_eq("name", "binned");
    binned().soft_delete().await.expect("soft delete failed");

    let unfiltered = TestSoftDelete::query().restore().await;
    assert!(
        unfiltered.is_err(),
        "restore without only_trashed() stays guarded"
    );
    let restored = TestSoftDelete::query().only_trashed().restore().await;
    assert_eq!(restored.expect("restoring the trash failed"), 2);
    assert_eq!(
        TestSoftDelete::query().count().await.expect("count failed"),
        3
    );

    binned().soft_delete().await.expect("soft delete failed");
    TestSoftDelete {
        id: 0,
        name: "binned".into(),
        deleted_at: None,
    }
    .save()
    .await
    .expect("save failed");
    let emptied = binned().only_trashed().force_delete().await;
    assert_eq!(emptied.expect("emptying the trash failed"), 2);
    let live = TestSoftDelete::query().get().await.expect("query failed");
    let mut names: Vec<_> = live.iter().map(|row| row.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["binned", "kept"], "live rows must survive");
    let trash = TestSoftDelete::query().only_trashed().count().await;
    assert_eq!(trash.expect("count failed"), 0);

    // On a soft-delete model, delete() and delete_all() mark live rows; the
    // trash is emptied with force_delete().
    binned().soft_delete().await.expect("soft delete failed");
    assert!(
        TestSoftDelete::query()
            .only_trashed()
            .delete_all()
            .await
            .is_err(),
        "only_trashed().delete_all() would mark rows already deleted"
    );
    let marked = TestSoftDelete::query().delete_all().await;
    assert_eq!(marked.expect("delete_all() failed"), 1);
    let total = || async {
        TestSoftDelete::query()
            .with_trashed()
            .count()
            .await
            .expect("count failed")
    };
    assert_eq!(
        TestSoftDelete::query().count().await.expect("count failed"),
        0
    );
    assert_eq!(total().await, 2, "delete_all() marks, it does not remove");
    let emptied = TestSoftDelete::query().only_trashed().force_delete().await;
    assert_eq!(emptied.expect("emptying the trash failed"), 2);
    assert_eq!(total().await, 0);
}

/// `aggregates()` answers several aggregates in one statement, with the values
/// the single terminals give, over plain, limited and empty row sets.
#[tokio::test]
async fn aggregates_match_the_single_terminals() {
    if !setup().await {
        return;
    }

    seed_users(6).await;
    let wanted = [
        Aggregate::count(),
        Aggregate::sum("age"),
        Aggregate::avg(TestUser::columns.age),
        Aggregate::min("age"),
        Aggregate::max("age"),
        Aggregate::count_distinct("active"),
    ];
    type Stats = (u64, i64, Option<f64>, Option<i32>, Option<i32>, u64);
    let adults = || TestUser::query().where_gt("age", 22);
    let limited = || adults().order_by("age", SortOrder::Desc).limit(3);
    for (label, query) in [("plain", adults()), ("limited", limited())] {
        let together: Stats = query
            .clone()
            .aggregates(&wanted)
            .await
            .expect("aggregates failed");
        // `count()` ignores the limit; the aggregates count the rows it leaves.
        let rows = query.clone().get().await.expect("get failed").len();
        let one_by_one: Stats = (
            rows as u64,
            query.clone().sum("age").await.expect("sum failed"),
            query.clone().avg("age").await.expect("avg failed"),
            query.clone().min("age").await.expect("min failed"),
            query.clone().max("age").await.expect("max failed"),
            query
                .clone()
                .count_distinct("active")
                .await
                .expect("count_distinct failed"),
        );
        assert_eq!(together, one_by_one, "{label}");
    }

    let none: Stats = TestUser::query()
        .where_gt("age", 1000)
        .aggregates(&wanted)
        .await
        .expect("empty aggregates failed");
    assert_eq!(none, (0, 0, None, None, None, 0));
    let nothing: Vec<f64> = TestUser::query()
        .aggregates(&[])
        .await
        .expect("no aggregates");
    assert!(nothing.is_empty());

    // Any column type, and the same column twice.
    let (first, last, oldest): (Option<String>, Option<String>, Option<i32>) = TestUser::query()
        .aggregates(&[
            Aggregate::min("name"),
            Aggregate::max("name"),
            Aggregate::max("age"),
        ])
        .await
        .expect("aggregates over text failed");
    assert_eq!(
        (first.as_deref(), last.as_deref(), oldest),
        (Some("User 01"), Some("User 06"), Some(26))
    );
}

/// The query logger sees the typed CRUD statements the engine renders, the
/// query builder's own statements exactly once, and raw SQL sent unprepared.
#[tokio::test]
async fn the_query_logger_sees_typed_crud_and_logs_each_statement_once() {
    use tideorm::logging::{LogLevel, QueryLogger};

    if !setup().await {
        return;
    }

    QueryLogger::clear_history();
    QueryLogger::global()
        .set_level(LogLevel::Error)
        .set_history_limit(100)
        .enable();
    let saved = user("logged@example.com", "Logged", 30, true)
        .save()
        .await
        .expect("save failed");
    TestUser::find(saved.id).await.expect("find failed");
    let crud = QueryLogger::history();

    QueryLogger::clear_history();
    TestUser::query()
        .where_eq("id", saved.id)
        .get()
        .await
        .expect("query failed");
    let builder = QueryLogger::history();

    // The engine reports no unprepared statement, so `execute` logs its own.
    QueryLogger::clear_history();
    Database::execute("UPDATE test_users SET age = age WHERE 1 = 0")
        .await
        .expect("execute failed");
    let raw = QueryLogger::history();
    QueryLogger::clear_history();
    QueryLogger::disable();

    assert!(
        crud.iter().any(|entry| entry.sql.starts_with("INSERT")),
        "{crud:#?}"
    );
    assert!(
        crud.iter()
            .any(|entry| entry.sql.starts_with("SELECT") && entry.sql.contains("test_users")),
        "{crud:#?}"
    );
    // Logged by the builder, which tags it with the table, and not again by
    // the engine.
    assert_eq!(builder.len(), 1, "{builder:#?}");
    assert!(
        builder[0].table.is_some() && builder[0].sql.starts_with("SELECT"),
        "{builder:#?}"
    );
    assert_eq!(raw.len(), 1, "{raw:#?}");
    assert!(raw[0].sql.starts_with("UPDATE"), "{raw:#?}");
}

#[tideorm::model(table = "coded_items")]
pub struct CodedItem {
    #[tideorm(primary_key)]
    pub code: String,
    pub label: String,
}

/// `insert_all` of models that set their own keys, a text code or a generated
/// UUID, returns every one in input order, batched where the backend can.
#[tokio::test]
async fn insert_all_of_models_keyed_by_the_client_keeps_their_order() {
    if !setup().await {
        return;
    }

    fresh_table("coded_items", |t| {
        t.string("code").primary_key();
        t.string("label").not_null();
    })
    .await;
    // Descending, so a batch read back in key order would come out reversed;
    // 20,000 rows take two statements even on SQLite.
    let wanted: Vec<usize> = (0..20_000).rev().collect();
    let items = wanted
        .iter()
        .map(|i| CodedItem {
            code: format!("code-{i:05}"),
            label: format!("item {i}"),
        })
        .collect();
    let inserted = CodedItem::insert_all(items)
        .await
        .expect("insert_all failed");
    assert_eq!(inserted.len(), wanted.len());
    for (item, i) in inserted.iter().zip(&wanted) {
        assert_eq!(item.code, format!("code-{i:05}"));
        assert_eq!(item.label, format!("item {i}"));
    }
    assert_eq!(CodedItem::count().await.expect("count failed"), 20_000);

    fresh_table("uuid_keyed_codes", |t| {
        t.uuid("id").primary_key();
        t.string("code").not_null().unique();
        t.big_integer("hits").not_null();
    })
    .await;
    let codes = (0..300)
        .map(|i| UuidKeyedCode {
            id: Uuid::nil(),
            code: format!("c{i}"),
            hits: i,
        })
        .collect();
    let inserted = UuidKeyedCode::insert_all(codes)
        .await
        .expect("insert_all failed");
    assert!(inserted.iter().all(|row| !row.id.is_nil()));
    assert!(
        inserted
            .iter()
            .enumerate()
            .all(|(i, row)| row.hits == i as i64 && row.code == format!("c{i}"))
    );
    let stored = UuidKeyedCode::find(inserted[123].id)
        .await
        .expect("find failed");
    assert_eq!(stored.expect("row should exist").code, "c123");
}

/// A union operand and a CTE body keep their own ordering and limit: the two
/// oldest users are two rows, not every user.
#[tokio::test]
async fn a_union_operand_and_a_cte_body_keep_their_limit() {
    if !setup().await {
        return;
    }

    // Ages 21 to 26.
    seed_users(6).await;
    let oldest_two = || TestUser::query().order_desc("age").limit(2);

    let ages: Vec<i32> = TestUser::query()
        .where_eq("age", 21)
        .union(oldest_two())
        .order_asc("age")
        .get()
        .await
        .expect("a union with a limited operand failed")
        .into_iter()
        .map(|user| user.age)
        .collect();
    assert_eq!(ages, vec![21, 25, 26]);

    let oldest = TestUser::query()
        .with_query("oldest", oldest_two())
        .where_raw("id IN (SELECT id FROM oldest)")
        .count()
        .await
        .expect("a query over a limited CTE failed");
    assert_eq!(oldest, 2);
}

#[tideorm::model(table = "join_authors")]
pub struct JoinAuthor {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub mentor_id: Option<i64>,
}

/// `right_join`, `right_join_as` and a self-join through `left_join_as` keep
/// the rows of their outer side that match nothing.
#[tokio::test]
async fn right_and_aliased_left_joins_keep_the_unmatched_rows() {
    if !setup().await {
        return;
    }

    fresh_table("join_authors", |t| {
        t.id();
        t.string("name").not_null();
        t.big_integer("mentor_id");
    })
    .await;
    fresh_table("join_books", |t| {
        t.id();
        t.string("title").not_null();
        t.big_integer("author_id").not_null();
    })
    .await;
    for (name, mentor_id) in [("Ada", None), ("Grace", Some(1))] {
        JoinAuthor {
            id: 0,
            name: name.to_string(),
            mentor_id,
        }
        .save()
        .await
        .expect("save failed");
    }
    for (title, author_id) in [("Notes", 1_i64), ("Compilers", 2), ("Orphan", 9)] {
        Database::execute_with_params(
            &format!(
                "INSERT INTO join_books (title, author_id) VALUES ({}, {})",
                param(1),
                param(2)
            ),
            vec![title.into(), author_id.into()],
        )
        .await
        .expect("insert failed");
    }

    let pairs = |query: QueryBuilder<JoinAuthor>, left: &'static str, right: &'static str| async move {
        let mut rows: Vec<(String, Option<String>)> = query
            .get_json()
            .await
            .expect("join failed")
            .into_iter()
            .map(|row| {
                (
                    row[left].as_str().expect("left column").to_string(),
                    row[right].as_str().map(str::to_string),
                )
            })
            .collect();
        rows.sort();
        rows
    };
    let books = vec![
        ("Compilers".to_string(), Some("Grace".to_string())),
        ("Notes".to_string(), Some("Ada".to_string())),
        ("Orphan".to_string(), None),
    ];

    let right = JoinAuthor::query()
        .right_join("join_books", "join_authors.id", "join_books.author_id")
        .select_raw("join_books.title AS title")
        .select_raw("join_authors.name AS author");
    assert_eq!(pairs(right, "title", "author").await, books);

    let right_aliased = JoinAuthor::query()
        .right_join_as("join_books", "b", "join_authors.id", "b.author_id")
        .select_raw("b.title AS title")
        .select_raw("join_authors.name AS author");
    assert_eq!(pairs(right_aliased, "title", "author").await, books);

    // The orphan's row has NULL in every author column, which is no model.
    assert!(
        JoinAuthor::query()
            .right_join("join_books", "join_authors.id", "join_books.author_id")
            .get()
            .await
            .is_err()
    );

    let mentors = JoinAuthor::query()
        .left_join_as(
            "join_authors",
            "mentor",
            "join_authors.mentor_id",
            "mentor.id",
        )
        .select_raw("join_authors.name AS name")
        .select_raw("mentor.name AS mentor");
    assert_eq!(
        pairs(mentors, "name", "mentor").await,
        [
            ("Ada".to_string(), None),
            ("Grace".to_string(), Some("Ada".to_string()))
        ]
    );
}

/// A JSON member holding `null` exists, a missing one does not, and a `NULL`
/// column matches neither the test nor its negation.
#[tokio::test]
async fn json_existence_counts_a_null_member_and_skips_a_null_column() {
    use serde_json::json;

    if !setup().await {
        return;
    }

    for metadata in [
        Some(json!({"a": {"b": 1}})),
        Some(json!({"a": {"b": null}})),
        Some(json!({"a": {}})),
        None,
    ] {
        TestProduct {
            id: 0,
            name: "Widget".to_string(),
            category: "tools".to_string(),
            price: 1,
            metadata,
        }
        .save()
        .await
        .expect("save failed");
    }
    let ids = |query: QueryBuilder<TestProduct>| async move {
        let mut ids: Vec<i64> = query
            .get()
            .await
            .expect("json filter failed")
            .into_iter()
            .map(|product| product.id)
            .collect();
        ids.sort_unstable();
        ids
    };

    assert_eq!(
        ids(TestProduct::query().where_json_path_exists("metadata", "$.a.b")).await,
        [1, 2]
    );
    assert_eq!(
        ids(TestProduct::query().where_json_path_not_exists("metadata", "$.a.b")).await,
        [3]
    );
    assert_eq!(
        ids(TestProduct::query().where_json_key_exists("metadata", "a")).await,
        [1, 2, 3]
    );
    assert_eq!(
        ids(TestProduct::query().where_json_key_not_exists("metadata", "z")).await,
        [1, 2, 3]
    );
}

/// `has_no_related` keeps a row whose related rows all fail the condition, and
/// a row with none at all.
#[tokio::test]
async fn has_no_related_keeps_rows_without_a_matching_related_row() {
    if !setup().await {
        return;
    }

    seed_users(3).await;
    fresh_table("user_posts", |t| {
        t.id();
        t.big_integer("user_id").not_null();
        t.string("status").not_null();
    })
    .await;
    for (user_id, status) in [(1_i64, "published"), (1, "draft"), (2, "draft")] {
        Database::execute_with_params(
            &format!(
                "INSERT INTO user_posts (user_id, status) VALUES ({}, {})",
                param(1),
                param(2)
            ),
            vec![user_id.into(), status.into()],
        )
        .await
        .expect("insert failed");
    }
    let ids = |query: QueryBuilder<TestUser>| async move {
        let mut ids: Vec<i64> = query
            .get()
            .await
            .expect("related filter failed")
            .into_iter()
            .map(|user| user.id)
            .collect();
        ids.sort_unstable();
        ids
    };

    assert_eq!(
        ids(TestUser::query().has_no_related("user_posts", "user_id", "id", "status", "published"))
            .await,
        [2, 3]
    );
    assert_eq!(
        ids(TestUser::query().has_related("user_posts", "user_id", "id", "status", "published"))
            .await,
        [1]
    );
}

/// `rename_table` keeps the rows, and a foreign key in another table follows
/// the rename and is still enforced.
#[tokio::test]
async fn rename_table_keeps_rows_and_incoming_foreign_keys() {
    if !setup().await {
        return;
    }

    let mut schema = Schema::new(backend::DATABASE_TYPE);
    for table in ["rename_children", "renamed_parents", "rename_parents"] {
        schema
            .drop_table_if_exists(table)
            .await
            .expect("drop failed");
    }
    schema
        .raw("CREATE TABLE rename_parents (id BIGINT PRIMARY KEY, name VARCHAR(50) NOT NULL)")
        .await
        .expect("create parents failed");
    schema
        .raw(
            "CREATE TABLE rename_children (id BIGINT PRIMARY KEY, parent_id BIGINT NOT NULL, \
             FOREIGN KEY (parent_id) REFERENCES rename_parents (id))",
        )
        .await
        .expect("create children failed");
    Database::execute("INSERT INTO rename_parents (id, name) VALUES (1, 'kept')")
        .await
        .expect("insert failed");

    schema
        .rename_table("rename_parents", "renamed_parents")
        .await
        .expect("rename failed");

    assert_eq!(
        Database::raw_json("SELECT name FROM renamed_parents")
            .await
            .expect("the renamed table should be readable"),
        [serde_json::json!({"name": "kept"})]
    );
    assert!(
        Database::raw_json("SELECT name FROM rename_parents")
            .await
            .is_err(),
        "the old name should be gone"
    );
    Database::execute("INSERT INTO rename_children (id, parent_id) VALUES (1, 1)")
        .await
        .expect("the foreign key should follow the rename");
    assert!(
        Database::execute("INSERT INTO rename_children (id, parent_id) VALUES (2, 99)")
            .await
            .is_err(),
        "the foreign key should still be enforced"
    );
}

/// `SchemaWriter::write_schema` reads a live table back well enough to
/// recreate it: run from the file, the table takes rows again and its unique
/// index still rejects a duplicate.
#[tokio::test]
async fn write_schema_exports_a_table_that_can_be_recreated_from_the_file() {
    if !setup().await {
        return;
    }

    fresh_table("exported_rows", |t| {
        t.id();
        t.string("code").not_null();
        t.integer("score");
        t.unique_index(&["code"]);
    })
    .await;

    let path = std::env::temp_dir().join(format!(
        "tideorm_schema_{:?}_{}.sql",
        backend::DATABASE_TYPE,
        std::process::id()
    ));
    SchemaWriter::write_schema(&path)
        .await
        .expect("write_schema failed");
    let file = std::fs::read_to_string(&path).expect("schema file missing");
    let _ = std::fs::remove_file(&path);

    let quoted = match backend::DATABASE_TYPE {
        DatabaseType::MySQL | DatabaseType::MariaDB => "`exported_rows`",
        _ => "\"exported_rows\"",
    };
    let statements: Vec<String> = file
        .split(";\n")
        .map(|chunk| {
            chunk
                .lines()
                .filter(|line| !line.starts_with("--"))
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string()
        })
        .filter(|statement| statement.contains(quoted))
        .collect();
    let create = statements
        .iter()
        .find(|statement| statement.starts_with("CREATE TABLE"))
        .unwrap_or_else(|| panic!("no CREATE TABLE for exported_rows in:\n{file}"));
    let index = statements
        .iter()
        .find(|statement| statement.starts_with("CREATE UNIQUE INDEX"))
        .unwrap_or_else(|| panic!("no unique index for exported_rows in:\n{file}"));

    Schema::new(backend::DATABASE_TYPE)
        .drop_table("exported_rows")
        .await
        .expect("drop failed");
    Database::execute(create)
        .await
        .unwrap_or_else(|error| panic!("the exported table did not run: {error}\n{create}"));
    Database::execute(index)
        .await
        .unwrap_or_else(|error| panic!("the exported index did not run: {error}\n{index}"));

    Database::execute("INSERT INTO exported_rows (code, score) VALUES ('a', 1)")
        .await
        .expect("the recreated table should take a row");
    Database::execute("INSERT INTO exported_rows (code) VALUES ('b')")
        .await
        .expect("score should stay nullable");
    assert!(
        Database::execute("INSERT INTO exported_rows (code, score) VALUES ('a', 2)")
            .await
            .is_err(),
        "the recreated unique index should reject a duplicate"
    );
}

#[tideorm::model(table = "tagged_rows")]
pub struct TaggedRow {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub tags: serde_json::Value,
}

/// On a JSON array, `array_remove` drops every element equal to the value and
/// keeps the others, types and order included, and leaves an array without
/// it as it was; `array_append` adds a JSON value, an object included.
/// PostgreSQL's arrays are native and take neither path.
#[tokio::test]
async fn array_append_and_remove_keep_the_rest_of_a_json_array() {
    use serde_json::json;

    if !setup().await || backend::DATABASE_TYPE == DatabaseType::Postgres {
        return;
    }

    fresh_table("tagged_rows", |t| {
        t.id();
        t.json("tags").not_null();
    })
    .await;
    for tags in [json!(["a", 1, true, null, "b", "a", "1"]), json!(["keep"])] {
        TaggedRow { id: 0, tags }.save().await.expect("save failed");
    }

    TaggedRow::update_all()
        .where_gt("id", 0)
        .array_remove("tags", "a")
        .execute()
        .await
        .expect("array_remove failed");
    TaggedRow::update_all()
        .where_gt("id", 0)
        .array_append("tags", json!({"x": 1}))
        .execute()
        .await
        .expect("array_append failed");

    let tags: Vec<serde_json::Value> = TaggedRow::query()
        .order_asc("id")
        .get()
        .await
        .expect("query failed")
        .into_iter()
        .map(|row| row.tags)
        .collect();
    assert_eq!(
        tags,
        [
            json!([1, true, null, "b", "1", {"x": 1}]),
            json!(["keep", {"x": 1}])
        ]
    );
}
