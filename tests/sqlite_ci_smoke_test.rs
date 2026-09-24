use tideorm::internal::ConnectionTrait;
use tideorm::prelude::*;
use tideorm::{Database, TideConfig};

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

#[derive(Model, PartialEq)]
#[tideorm(table = "ci_validated_users")]
struct CiValidatedUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[validate(email)]
    email: String,
    #[validate(min_length = 3)]
    name: String,
    active: bool,
}

#[derive(Model, PartialEq)]
#[tideorm(table = "ci_api_keys")]
struct CiApiKey {
    #[tideorm(primary_key)]
    key: String,
    label: String,
    active: bool,
}

#[derive(Model, PartialEq)]
#[tideorm(table = "ci_soft_delete_users", soft_delete)]
struct CiSoftDeleteUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

const CI_USERS_DDL: &str = "CREATE TABLE ci_users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    email TEXT NOT NULL,
    name TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1
)";

const CI_SOFT_DELETE_USERS_DDL: &str = "CREATE TABLE ci_soft_delete_users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    deleted_at TEXT NULL
)";

/// Install a new global in-memory database. Every call starts from an empty
/// schema, so no test sees another's tables.
async fn connect_global() {
    TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .database("sqlite::memory:")
        .max_connections(1)
        .connect()
        .await
        .expect("failed to connect to SQLite");
}

/// A fresh global database holding just the table `ddl` creates.
async fn fresh_table(ddl: &str) {
    connect_global().await;
    Database::execute(ddl)
        .await
        .expect("failed to create test table");
}

/// A private in-memory database, never installed as the global one.
async fn local_db_with(ddl: &str) -> Database {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("failed to connect to local SQLite database");
    db.__internal_connection()
        .expect("local SQLite connection should be available")
        .execute_unprepared(ddl)
        .await
        .expect("failed to create table for local db");
    db
}

#[path = "sqlite_ci_smoke_test/crud_and_key_tests.rs"]
mod crud_and_key_tests;

#[path = "sqlite_ci_smoke_test/eager_soft_delete_tests.rs"]
mod eager_soft_delete_tests;

#[path = "sqlite_ci_smoke_test/transaction_and_helpers_tests.rs"]
mod transaction_and_helpers_tests;

#[path = "sqlite_ci_smoke_test/query_context_tests.rs"]
mod query_context_tests;
