//! PostgreSQL Integration Tests for TideORM
//!
//! These tests require a running PostgreSQL instance with:
//! - Host: localhost
//! - Port: 5432
//! - User: postgres
//! - Password: postgres
//! - Database: test_tide_orm
//!
//! Run with: cargo test --test postgres_integration_tests

use std::sync::{LazyLock, Mutex};
use tideorm::prelude::*;
use tideorm::{Database, TideConfig};

#[path = "postgres_integration_tests/batch_operations.rs"]
mod batch_operations;
#[path = "postgres_integration_tests/batch_update.rs"]
mod batch_update;
#[path = "postgres_integration_tests/callbacks_scopes_cleanup.rs"]
mod callbacks_scopes_cleanup;
#[path = "postgres_integration_tests/connection.rs"]
mod connection;
#[path = "postgres_integration_tests/crud.rs"]
mod crud;
#[path = "postgres_integration_tests/query_builder.rs"]
mod query_builder;
#[path = "postgres_integration_tests/raw_json.rs"]
mod raw_json;
#[path = "postgres_integration_tests/raw_sql.rs"]
mod raw_sql;
#[path = "postgres_integration_tests/setup.rs"]
mod setup;
#[path = "postgres_integration_tests/soft_delete.rs"]
mod soft_delete;
#[path = "support/postgres_test_config.rs"]
mod test_config;
#[path = "postgres_integration_tests/transaction.rs"]
mod transaction;
#[path = "postgres_integration_tests/upsert.rs"]
mod upsert;

use test_config::test_database_url;

// =============================================================================
// TEST MODELS
// =============================================================================

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

#[tideorm::model(table = "test_posts")]
pub struct TestPost {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub content: String,
    pub published: bool,
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

// =============================================================================
// SINGLE INTEGRATION TEST - Runs all scenarios sequentially
// =============================================================================

#[tokio::test]
async fn postgres_integration_tests() {
    setup::run().await;
    connection::run().await;
    raw_json::run().await;
    crud::run().await;
    query_builder::run().await;
    soft_delete::run().await;
    transaction::run().await;
    raw_sql::run().await;
    batch_operations::run().await;
    upsert::run().await;
    batch_update::run().await;
    callbacks_scopes_cleanup::run().await;
}
