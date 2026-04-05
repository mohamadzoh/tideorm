use super::*;

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
async fn sqlite_save_and_update_run_model_validation() {
    TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .database("sqlite::memory:")
        .max_connections(1)
        .connect()
        .await
        .expect("failed to connect to SQLite");

    let _ = Database::execute("DROP TABLE IF EXISTS ci_validated_users").await;

    Database::execute(
        r#"
        CREATE TABLE ci_validated_users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            email TEXT NOT NULL,
            name TEXT NOT NULL,
            active INTEGER NOT NULL DEFAULT 1
        )
    "#,
    )
    .await
    .expect("failed to create ci_validated_users table");

    let create_err = CiValidatedUser {
        id: 0,
        email: "not-an-email".to_string(),
        name: "ok".to_string(),
        active: true,
    }
    .save()
    .await
    .expect_err("invalid model save should fail validation");
    assert!(create_err.is_validation_error());

    let count_after_failed_create = CiValidatedUser::count()
        .await
        .expect("failed to count validated users after rejected create");
    assert_eq!(count_after_failed_create, 0);

    let created = CiValidatedUser {
        id: 0,
        email: "valid@example.com".to_string(),
        name: "Valid User".to_string(),
        active: true,
    }
    .save()
    .await
    .expect("valid model save should succeed");

    let update_err = CiValidatedUser {
        email: "still-valid@example.com".to_string(),
        name: "no".to_string(),
        ..created
    }
    .update()
    .await
    .expect_err("invalid model update should fail validation");
    assert!(update_err.is_validation_error());

    let reloaded = CiValidatedUser::find(1_i64)
        .await
        .expect("failed to reload validated user")
        .expect("validated user should still exist");
    assert_eq!(reloaded.email, "valid@example.com");
    assert_eq!(reloaded.name, "Valid User");
    assert!(reloaded.active);
}

#[tokio::test]
async fn sqlite_direct_crud_helpers_respect_soft_delete_scope() {
    TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .database("sqlite::memory:")
        .max_connections(1)
        .connect()
        .await
        .expect("failed to connect to SQLite");

    let _ = Database::execute("DROP TABLE IF EXISTS ci_soft_delete_users").await;

    Database::execute(
        r#"
        CREATE TABLE ci_soft_delete_users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            deleted_at TEXT NULL
        )
    "#,
    )
    .await
    .expect("failed to create ci_soft_delete_users table");

    let first = CiSoftDeleteUser {
        id: 0,
        name: "First".to_string(),
        deleted_at: None,
    }
    .save()
    .await
    .expect("failed to insert first soft-delete user");

    let middle = CiSoftDeleteUser {
        id: 0,
        name: "Middle".to_string(),
        deleted_at: None,
    }
    .save()
    .await
    .expect("failed to insert middle soft-delete user");

    let last = CiSoftDeleteUser {
        id: 0,
        name: "Last".to_string(),
        deleted_at: None,
    }
    .save()
    .await
    .expect("failed to insert last soft-delete user");

    first
        .soft_delete()
        .await
        .expect("failed to soft delete first user");
    last.soft_delete()
        .await
        .expect("failed to soft delete last user");

    let all_users = CiSoftDeleteUser::all()
        .await
        .expect("failed to fetch all soft-delete users");
    assert_eq!(all_users.len(), 1);
    assert_eq!(all_users[0].name, "Middle");

    let first_user = CiSoftDeleteUser::first()
        .await
        .expect("failed to fetch first soft-delete user")
        .expect("middle user should remain visible");
    assert_eq!(first_user.name, "Middle");

    let last_user = CiSoftDeleteUser::last()
        .await
        .expect("failed to fetch last soft-delete user")
        .expect("middle user should remain visible");
    assert_eq!(last_user.name, "Middle");

    let count = CiSoftDeleteUser::count()
        .await
        .expect("failed to count visible soft-delete users");
    assert_eq!(count, 1);
    let exists_any = CiSoftDeleteUser::exists_any()
        .await
        .expect("failed to check visible soft-delete users");
    assert!(exists_any);

    let page = CiSoftDeleteUser::paginate(1, 10)
        .await
        .expect("failed to paginate soft-delete users");
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].name, "Middle");

    let with_trashed = CiSoftDeleteUser::query()
        .with_trashed()
        .get()
        .await
        .expect("failed to query soft-delete users with trashed rows");
    assert_eq!(with_trashed.len(), 3);

    let only_trashed = CiSoftDeleteUser::query()
        .only_trashed()
        .get()
        .await
        .expect("failed to query only trashed rows");
    assert_eq!(only_trashed.len(), 2);
    assert!(only_trashed.iter().any(|user| user.name == "First"));
    assert!(only_trashed.iter().any(|user| user.name == "Last"));

    let middle_found = CiSoftDeleteUser::find(middle.id)
        .await
        .expect("failed to find middle user")
        .expect("middle user should still be findable");
    assert_eq!(middle_found.name, "Middle");
}

#[tokio::test]
async fn sqlite_reload_returns_not_found_after_delete() {
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

    let user = CiUser {
        id: 0,
        email: "reload-delete@example.com".to_string(),
        name: "Reload Delete".to_string(),
        active: true,
    }
    .save()
    .await
    .expect("failed to insert user");

    user.clone().delete().await.expect("failed to delete user");

    let err = user
        .reload()
        .await
        .expect_err("reload should fail after delete");
    assert!(err.is_not_found());
    assert!(err.to_string().contains("ci_users"));
    assert!(err.to_string().contains("no longer exists"));
}

#[tokio::test]
async fn sqlite_reload_still_finds_soft_deleted_records() {
    TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .database("sqlite::memory:")
        .max_connections(1)
        .connect()
        .await
        .expect("failed to connect to SQLite");

    let _ = Database::execute("DROP TABLE IF EXISTS ci_soft_delete_users").await;

    Database::execute(
        r#"
        CREATE TABLE ci_soft_delete_users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            deleted_at TEXT NULL
        )
    "#,
    )
    .await
    .expect("failed to create ci_soft_delete_users table");

    let user = CiSoftDeleteUser {
        id: 0,
        name: "Soft Deleted".to_string(),
        deleted_at: None,
    }
    .save()
    .await
    .expect("failed to insert soft-delete user");

    let deleted = user
        .soft_delete()
        .await
        .expect("failed to soft delete user");

    let reloaded = deleted
        .reload()
        .await
        .expect("reload should include soft-deleted records");
    assert_eq!(reloaded.id, deleted.id);
    assert!(reloaded.deleted_at.is_some());
}
