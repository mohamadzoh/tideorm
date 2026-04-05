use super::*;

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

    let saved_again = CiUser {
        name: "Saved Again".to_string(),
        active: false,
        ..fetched
    }
    .save()
    .await
    .expect("failed to save existing user");

    assert_eq!(saved_again.id, created.id);
    assert_eq!(saved_again.name, "Saved Again");
    assert!(!saved_again.active);

    let reloaded_after_save = CiUser::find(created.id)
        .await
        .expect("failed to reload saved user")
        .expect("saved user should still exist");
    assert_eq!(reloaded_after_save.name, "Saved Again");
    assert!(!reloaded_after_save.active);

    let updated = CiUser {
        name: "Updated CI User".to_string(),
        ..saved_again
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

    let saved_again = CiUserRole {
        label: "editor".to_string(),
        active: false,
        ..fetched
    }
    .save()
    .await
    .expect("failed to save existing composite-key row");
    assert_eq!(saved_again.user_id, 1);
    assert_eq!(saved_again.role_id, 7);
    assert_eq!(saved_again.label, "editor");
    assert!(!saved_again.active);

    let reloaded_after_save = CiUserRole::find((1_i64, 7_i64))
        .await
        .expect("failed to reload saved composite-key row")
        .expect("saved composite-key row should still exist");
    assert_eq!(reloaded_after_save.label, "editor");
    assert!(!reloaded_after_save.active);

    let updated = CiUserRole {
        label: "admin".to_string(),
        active: false,
        ..saved_again
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
async fn sqlite_natural_primary_key_save_smoke_test() {
    TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .database("sqlite::memory:")
        .max_connections(1)
        .connect()
        .await
        .expect("failed to connect to SQLite");

    let _ = Database::execute("DROP TABLE IF EXISTS ci_api_keys").await;

    Database::execute(
        r#"
        CREATE TABLE ci_api_keys (
            key TEXT PRIMARY KEY,
            label TEXT NOT NULL,
            active INTEGER NOT NULL DEFAULT 1
        )
    "#,
    )
    .await
    .expect("failed to create ci_api_keys table");

    let created = CiApiKey {
        key: "ci-key-1".to_string(),
        label: "First key".to_string(),
        active: true,
    }
    .save()
    .await
    .expect("failed to insert natural-key row");

    assert_eq!(created.key, "ci-key-1");
    assert_eq!(created.label, "First key");
    assert!(created.active);

    let saved_again = CiApiKey {
        label: "Updated key".to_string(),
        active: false,
        ..created
    }
    .save()
    .await
    .expect("failed to update natural-key row");

    assert_eq!(saved_again.key, "ci-key-1");
    assert_eq!(saved_again.label, "Updated key");
    assert!(!saved_again.active);
}
