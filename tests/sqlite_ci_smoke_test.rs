use std::sync::{Mutex, OnceLock};

use tideorm::prelude::*;
use tideorm::{Database, TideConfig};

fn leaked_transaction_db_slot() -> &'static Mutex<Option<Database>> {
    static LEAKED_TRANSACTION_DB: OnceLock<Mutex<Option<Database>>> = OnceLock::new();
    LEAKED_TRANSACTION_DB.get_or_init(|| Mutex::new(None))
}

#[derive(Model, PartialEq)]
#[tideorm(table = "ci_users")]
struct CiUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    email: String,
    name: String,
    active: bool,
}

#[derive(Model, PartialEq)]
#[tideorm(table = "ci_user_roles")]
struct CiUserRole {
    #[tideorm(primary_key)]
    user_id: i64,
    #[tideorm(primary_key)]
    role_id: i64,
    label: String,
    active: bool,
}

#[tokio::test]
async fn sqlite_ci_smoke_test() {
    TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .database("sqlite::memory:")
        .max_connections(1)
        .connect()
        .await
        .expect("failed to connect to SQLite");

    let _ = Database::execute("DROP TABLE IF EXISTS ci_users").await;

    Database::execute(
        r#"
        CREATE TABLE ci_users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            email TEXT NOT NULL,
            name TEXT NOT NULL,
            active INTEGER NOT NULL DEFAULT 1
        )
    "#,
    )
    .await
    .expect("failed to create ci_users table");

    let created = CiUser {
        id: 0,
        email: "ci@example.com".to_string(),
        name: "CI User".to_string(),
        active: true,
    }
    .save()
    .await
    .expect("failed to insert user");

    assert!(created.id > 0, "auto-increment id should be assigned");

    let fetched = CiUser::find(created.id)
        .await
        .expect("failed to fetch inserted user")
        .expect("inserted user should exist");
    assert_eq!(fetched.email, "ci@example.com");
    assert_eq!(fetched.name, "CI User");
    assert!(fetched.active);

    let updated = CiUser {
        name: "Updated CI User".to_string(),
        ..fetched
    }
    .update()
    .await
    .expect("failed to update user");
    assert_eq!(updated.name, "Updated CI User");

    let deleted_rows = updated.delete().await.expect("failed to delete user");
    assert_eq!(deleted_rows, 1);

    let missing = CiUser::find(created.id)
        .await
        .expect("failed to verify deletion");
    assert!(missing.is_none(), "deleted user should not exist");
}

#[tokio::test]
async fn sqlite_composite_primary_key_crud_smoke_test() {
    TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .database("sqlite::memory:")
        .max_connections(1)
        .connect()
        .await
        .expect("failed to connect to SQLite");

    let _ = Database::execute("DROP TABLE IF EXISTS ci_user_roles").await;

    Database::execute(
        r#"
        CREATE TABLE ci_user_roles (
            user_id INTEGER NOT NULL,
            role_id INTEGER NOT NULL,
            label TEXT NOT NULL,
            active INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY (user_id, role_id)
        )
    "#,
    )
    .await
    .expect("failed to create ci_user_roles table");

    let created = CiUserRole {
        user_id: 1,
        role_id: 7,
        label: "owner".to_string(),
        active: true,
    }
    .save()
    .await
    .expect("failed to insert composite-key row");

    assert_eq!(created.user_id, 1);
    assert_eq!(created.role_id, 7);

    let fetched = CiUserRole::find((1_i64, 7_i64))
        .await
        .expect("failed to fetch inserted composite-key row")
        .expect("inserted composite-key row should exist");
    assert_eq!(fetched.label, "owner");
    assert!(fetched.active);

    let updated = CiUserRole {
        label: "admin".to_string(),
        active: false,
        ..fetched
    }
    .update()
    .await
    .expect("failed to update composite-key row");
    assert_eq!(updated.label, "admin");
    assert!(!updated.active);

    let deleted_rows = CiUserRole::destroy((1_i64, 7_i64))
        .await
        .expect("failed to delete composite-key row");
    assert_eq!(deleted_rows, 1);

    let missing = CiUserRole::find((1_i64, 7_i64))
        .await
        .expect("failed to verify composite-key deletion");
    assert!(
        missing.is_none(),
        "deleted composite-key row should not exist"
    );
}

#[tokio::test]
async fn sqlite_eager_find_preserves_existing_filters() {
    TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .database("sqlite::memory:")
        .max_connections(1)
        .connect()
        .await
        .expect("failed to connect to SQLite");

    let _ = Database::execute("DROP TABLE IF EXISTS ci_users").await;

    Database::execute(
        r#"
        CREATE TABLE ci_users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            email TEXT NOT NULL,
            name TEXT NOT NULL,
            active INTEGER NOT NULL DEFAULT 1
        )
    "#,
    )
    .await
    .expect("failed to create ci_users table");

    let created = CiUser {
        id: 0,
        email: "eager@example.com".to_string(),
        name: "Eager User".to_string(),
        active: true,
    }
    .save()
    .await
    .expect("failed to insert eager test user");

    let filtered_out = CiUser::eager()
        .where_eq("active", false)
        .find(created.id)
        .await
        .expect("eager find with filter should succeed");
    assert!(
        filtered_out.is_none(),
        "eager find should respect prior query filters"
    );

    let matched = CiUser::eager()
        .where_eq("active", true)
        .find(created.id)
        .await
        .expect("eager find with matching filter should succeed");
    assert!(
        matched.is_some(),
        "eager find should still find matching rows"
    );
}

#[tokio::test]
async fn sqlite_transaction_model_methods_use_transaction_connection() {
    TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .database("sqlite::memory:")
        .max_connections(1)
        .connect()
        .await
        .expect("failed to connect to SQLite");

    let _ = Database::execute("DROP TABLE IF EXISTS ci_users").await;

    Database::execute(
        r#"
        CREATE TABLE ci_users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            email TEXT NOT NULL,
            name TEXT NOT NULL,
            active INTEGER NOT NULL DEFAULT 1
        )
    "#,
    )
    .await
    .expect("failed to create ci_users table");

    let baseline = CiUser {
        id: 0,
        email: "baseline@example.com".to_string(),
        name: "Baseline".to_string(),
        active: true,
    }
    .save()
    .await
    .expect("failed to insert baseline user");

    let save_result: tideorm::Result<()> = CiUser::transaction(|_tx| {
        Box::pin(async move {
            CiUser {
                id: 0,
                email: "tx-save@example.com".to_string(),
                name: "Transaction Save".to_string(),
                active: true,
            }
            .save()
            .await?;

            Err(tideorm::Error::query("rollback save transaction"))
        })
    })
    .await;
    assert!(save_result.is_err(), "save transaction should roll back");

    let rolled_back_save = CiUser::query()
        .where_eq("email", "tx-save@example.com")
        .first()
        .await
        .expect("failed to query rolled back save");
    assert!(
        rolled_back_save.is_none(),
        "saved row should not persist after rollback"
    );

    let update_result: tideorm::Result<()> = CiUser::transaction(|_tx| {
        let baseline = baseline.clone();
        Box::pin(async move {
            CiUser {
                name: "Updated In Transaction".to_string(),
                ..baseline
            }
            .update()
            .await?;

            Err(tideorm::Error::query("rollback update transaction"))
        })
    })
    .await;
    assert!(
        update_result.is_err(),
        "update transaction should roll back"
    );

    let unchanged = CiUser::find(baseline.id)
        .await
        .expect("failed to reload baseline user")
        .expect("baseline user should still exist");
    assert_eq!(unchanged.name, "Baseline");

    let delete_result: tideorm::Result<()> = CiUser::transaction(|_tx| {
        let baseline = unchanged.clone();
        Box::pin(async move {
            baseline.delete().await?;
            Err(tideorm::Error::query("rollback delete transaction"))
        })
    })
    .await;
    assert!(
        delete_result.is_err(),
        "delete transaction should roll back"
    );

    let still_present = CiUser::find(baseline.id)
        .await
        .expect("failed to check rolled back delete")
        .expect("baseline user should remain after rollback");
    assert_eq!(still_present.email, "baseline@example.com");
}

#[tokio::test]
async fn sqlite_transaction_leak_on_error_returns_transaction_error() {
    TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .database("sqlite::memory:")
        .max_connections(1)
        .connect()
        .await
        .expect("failed to connect to SQLite");

    leaked_transaction_db_slot()
        .lock()
        .expect("leaked transaction slot lock poisoned")
        .take();

    let err = CiUser::transaction(|_tx| {
        Box::pin(async move {
            let leaked_db = tideorm::database::__current_db()
                .expect("transaction-scoped database should be available inside transaction");
            *leaked_transaction_db_slot()
                .lock()
                .expect("leaked transaction slot lock poisoned") = Some(leaked_db);

            Err::<(), _>(tideorm::Error::query(
                "rollback with leaked transaction handle",
            ))
        })
    })
    .await
    .expect_err("leaked transaction handle should surface as a transaction error");

    assert!(
        err.to_string()
            .contains("transaction handle leaked outside the transaction scope"),
        "unexpected error: {err}"
    );

    leaked_transaction_db_slot()
        .lock()
        .expect("leaked transaction slot lock poisoned")
        .take();
}

#[tokio::test]
async fn sqlite_query_with_and_find_with_work_without_global_db() {
    use tideorm::internal::{ActiveModelTrait, ConnectionTrait, InternalModel};

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("failed to connect to local SQLite database");
    let conn = db
        .__internal_connection()
        .expect("local SQLite connection should be available");

    conn.execute_unprepared(
        r#"
            CREATE TABLE ci_users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                email TEXT NOT NULL,
                name TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1
            )
        "#,
    )
    .await
    .expect("failed to create ci_users table for local db");

    let inserted = CiUser {
        id: 0,
        email: "local@example.com".to_string(),
        name: "Local User".to_string(),
        active: true,
    }
    .into_active_model()
    .insert(&conn)
    .await
    .expect("failed to seed local user");

    let fetched = CiUser::find_with(inserted.id, &db)
        .await
        .expect("find_with failed")
        .expect("local user should exist");
    assert_eq!(fetched.email, "local@example.com");

    let queried = CiUser::query_with(&db)
        .where_eq("email", "local@example.com")
        .get()
        .await
        .expect("query_with failed");
    assert_eq!(queried.len(), 1);
    assert_eq!(queried[0].name, "Local User");
}

#[tokio::test]
async fn sqlite_query_with_supports_aggregate_queries_without_global_db() {
    use tideorm::internal::{ActiveModelTrait, ConnectionTrait, InternalModel};

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("failed to connect to local SQLite database");
    let conn = db
        .__internal_connection()
        .expect("local SQLite connection should be available");

    conn.execute_unprepared(
        r#"
            CREATE TABLE ci_users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                email TEXT NOT NULL,
                name TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1
            )
        "#,
    )
    .await
    .expect("failed to create ci_users table for local db");

    let first = CiUser {
        id: 0,
        email: "aggregate1@example.com".to_string(),
        name: "Aggregate One".to_string(),
        active: true,
    }
    .into_active_model()
    .insert(&conn)
    .await
    .expect("failed to seed first local user");

    let second = CiUser {
        id: 0,
        email: "aggregate2@example.com".to_string(),
        name: "Aggregate Two".to_string(),
        active: true,
    }
    .into_active_model()
    .insert(&conn)
    .await
    .expect("failed to seed second local user");

    let third = CiUser {
        id: 0,
        email: "aggregate3@example.com".to_string(),
        name: "Aggregate Three".to_string(),
        active: false,
    }
    .into_active_model()
    .insert(&conn)
    .await
    .expect("failed to seed third local user");

    let active_sum = CiUser::query_with(&db)
        .where_eq("active", true)
        .sum("id")
        .await
        .expect("sum with explicit db should succeed");
    assert_eq!(active_sum, (first.id + second.id) as f64);

    let inactive_distinct = CiUser::query_with(&db)
        .where_eq("active", false)
        .count_distinct("email")
        .await
        .expect("count_distinct with explicit db should succeed");
    assert_eq!(inactive_distinct, 1);

    let total_avg = CiUser::query_with(&db)
        .avg("id")
        .await
        .expect("avg with explicit db should succeed");
    assert_eq!(total_avg, (first.id + second.id + third.id) as f64 / 3.0);
}

#[tokio::test]
async fn sqlite_query_errors_include_query_builder_context() {
    use tideorm::Database;
    use tideorm::internal::ConnectionTrait;

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("failed to connect to local SQLite database");
    let conn = db
        .__internal_connection()
        .expect("local SQLite connection should be available");

    conn.execute_unprepared(
        r#"
            CREATE TABLE ci_users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                email TEXT NOT NULL,
                name TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1
            )
        "#,
    )
    .await
    .expect("failed to create ci_users table for local db");

    let err = CiUser::query_with(&db)
        .where_eq("active", true)
        .begin_or()
        .or_where_raw("not_a_real_sql_function() = 1")
        .or_where_eq("email", "broken@example.com")
        .end_or()
        .get()
        .await
        .expect_err("query should fail due to invalid SQL function");

    let ctx = err.context().expect("query errors should include context");
    assert_eq!(ctx.table.as_deref(), Some("ci_users"));
    assert!(
        ctx.conditions
            .iter()
            .any(|condition| condition == "active = true"),
        "expected rendered top-level condition in context: {:?}",
        ctx.conditions
    );
    assert!(
        ctx.conditions.iter().any(
            |condition| condition.contains("not_a_real_sql_function() = 1")
                && condition.contains("OR")
        ),
        "expected rendered OR-group condition in context: {:?}",
        ctx.conditions
    );
    assert!(
        ctx.operator_chain
            .as_deref()
            .is_some_and(|chain| chain.contains("active = true")
                && chain.contains("not_a_real_sql_function() = 1")
                && chain.contains("OR")),
        "expected logical operator chain in context: {:?}",
        ctx.operator_chain
    );
    assert!(
        ctx.query
            .as_deref()
            .is_some_and(|query| query.contains("not_a_real_sql_function() = 1")),
        "expected SQL query preview in context: {:?}",
        ctx.query
    );
}

#[tokio::test]
async fn sqlite_uncached_queries_do_not_touch_global_query_cache() {
    use tideorm::QueryCache;
    use tideorm::internal::{ActiveModelTrait, ConnectionTrait, InternalModel};

    let cache = QueryCache::global();
    cache.clear();
    cache.reset_stats();
    cache.enable();

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("failed to connect to local SQLite database");
    let conn = db
        .__internal_connection()
        .expect("local SQLite connection should be available");

    conn.execute_unprepared(
        r#"
            CREATE TABLE ci_users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                email TEXT NOT NULL,
                name TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1
            )
        "#,
    )
    .await
    .expect("failed to create ci_users table for local db");

    CiUser {
        id: 0,
        email: "uncached@example.com".to_string(),
        name: "Uncached User".to_string(),
        active: true,
    }
    .into_active_model()
    .insert(&conn)
    .await
    .expect("failed to seed uncached user");

    let results = CiUser::query_with(&db)
        .where_eq("email", "uncached@example.com")
        .get()
        .await
        .expect("uncached query should succeed");

    assert_eq!(results.len(), 1);

    let stats = cache.stats();
    assert_eq!(stats.hits, 0, "uncached queries should not read the cache");
    assert_eq!(
        stats.misses, 0,
        "uncached queries should not probe the cache"
    );
    assert_eq!(
        stats.entries, 0,
        "uncached queries should not populate the cache"
    );
    assert!(
        cache.is_empty(),
        "uncached queries should leave the cache empty"
    );

    cache.disable();
    cache.clear();
    cache.reset_stats();
}

#[tokio::test]
async fn sqlite_model_crud_errors_include_model_context() {
    if !tideorm::has_global_db() {
        TideConfig::init()
            .database_type(DatabaseType::SQLite)
            .database("sqlite::memory:")
            .max_connections(1)
            .connect()
            .await
            .expect("failed to initialize global SQLite database");
    }

    let _ = Database::execute("DROP TABLE IF EXISTS ci_users").await;

    let find_err = CiUser::find(1)
        .await
        .expect_err("find should fail when the table is missing");
    let find_ctx = find_err
        .context()
        .expect("find errors should include model context");
    assert_eq!(find_ctx.table.as_deref(), Some("ci_users"));
    assert_eq!(find_ctx.conditions, vec!["id = 1".to_string()]);
    assert_eq!(find_ctx.operator_chain.as_deref(), Some("id = 1"));
    assert!(
        find_ctx
            .query
            .as_deref()
            .is_some_and(|query| query.contains("find(id = 1)")),
        "expected model operation in context: {:?}",
        find_ctx.query
    );

    let update_err = CiUser {
        id: 7,
        email: "update@example.com".to_string(),
        name: "Update Fail".to_string(),
        active: true,
    }
    .update()
    .await
    .expect_err("update should fail when the table is missing");
    let update_ctx = update_err
        .context()
        .expect("update errors should include model context");
    assert_eq!(update_ctx.table.as_deref(), Some("ci_users"));
    assert_eq!(update_ctx.conditions, vec!["id = 7".to_string()]);
    assert_eq!(update_ctx.operator_chain.as_deref(), Some("id = 7"));
    assert!(
        update_ctx
            .query
            .as_deref()
            .is_some_and(|query| query.contains("update where id = 7")),
        "expected update operation in context: {:?}",
        update_ctx.query
    );
}

#[tokio::test]
async fn sqlite_model_helper_errors_include_context() {
    if !tideorm::has_global_db() {
        TideConfig::init()
            .database_type(DatabaseType::SQLite)
            .database("sqlite::memory:")
            .max_connections(1)
            .connect()
            .await
            .expect("failed to initialize global SQLite database");
    }

    let _ = Database::execute("DROP TABLE IF EXISTS ci_users").await;

    let all_err = CiUser::all()
        .await
        .expect_err("all should fail when the table is missing");
    let all_ctx = all_err
        .context()
        .expect("all errors should include model context");
    assert_eq!(all_ctx.table.as_deref(), Some("ci_users"));
    assert!(
        all_ctx
            .query
            .as_deref()
            .is_some_and(|query| query.contains("find_all()")),
        "expected helper operation in context: {:?}",
        all_ctx.query
    );

    let count_err = CiUser::count()
        .await
        .expect_err("count should fail when the table is missing");
    let count_ctx = count_err
        .context()
        .expect("count errors should include model context");
    assert_eq!(count_ctx.table.as_deref(), Some("ci_users"));
    assert!(
        count_ctx
            .query
            .as_deref()
            .is_some_and(|query| query.contains("count(*)")),
        "expected helper operation in context: {:?}",
        count_ctx.query
    );
}
