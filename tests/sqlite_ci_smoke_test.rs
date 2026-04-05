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

#[path = "sqlite_ci_smoke_test/crud_and_key_tests.rs"]
mod crud_and_key_tests;

#[path = "sqlite_ci_smoke_test/eager_soft_delete_tests.rs"]
mod eager_soft_delete_tests;

#[path = "sqlite_ci_smoke_test/transaction_and_helpers_tests.rs"]
mod transaction_and_helpers_tests;

#[path = "sqlite_ci_smoke_test/query_context_tests.rs"]
mod query_context_tests;
