//! SQLite integration tests: the shared scenarios in
//! `support/integration_parity.rs` plus global-profiler coverage, on an
//! in-memory database. Opt out with `SKIP_SQLITE_TESTS`.
//!
//! Run with:
//! cargo test --test sqlite_integration_tests --no-default-features --features sqlite,runtime-tokio

use std::future::Future;

use tideorm::Database;
use tideorm::prelude::*;
use tideorm::profiling::GlobalProfiler;

use parity::TestUser;

#[path = "support/sqlite_test_config.rs"]
mod test_config;

mod backend {
    use tideorm::TideConfig;
    use tideorm::config::DatabaseType;

    use super::test_config::should_run_sqlite_tests;

    pub const DATABASE_TYPE: DatabaseType = DatabaseType::SQLite;

    /// A file database for a scenario that connects more than once and needs
    /// the same database each time, which an in-memory one is not.
    pub fn database_url() -> &'static str {
        static URL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        URL.get_or_init(|| {
            let path =
                std::env::temp_dir().join(format!("tideorm_parity_{}.db", std::process::id()));
            let _ = std::fs::remove_file(&path);
            format!(
                "sqlite://{}?mode=rwc",
                path.display().to_string().replace('\\', "/")
            )
        })
    }

    pub async fn connect() -> bool {
        if !should_run_sqlite_tests() {
            println!("Skipping SQLite test (SKIP_SQLITE_TESTS is set)");
            return false;
        }
        TideConfig::init()
            .database_type(DatabaseType::SQLite)
            .database("sqlite::memory:")
            .max_connections(1)
            .connect()
            .await
            .expect("failed to connect to SQLite");
        true
    }
}

#[path = "support/integration_parity.rs"]
mod parity;

async fn assert_profiled_operation<T, Fut>(label: &str, future: Fut) -> T
where
    Fut: Future<Output = tideorm::Result<T>>,
{
    GlobalProfiler::enable();
    GlobalProfiler::reset();
    GlobalProfiler::set_slow_threshold(0);

    let result = future
        .await
        .unwrap_or_else(|err| panic!("{} failed during profiler test: {}", label, err));

    let profiler_stats = GlobalProfiler::stats();
    assert!(
        profiler_stats.total_queries >= 1,
        "expected {} to increment total_queries, got {:?}",
        label,
        profiler_stats
    );
    assert!(
        profiler_stats.slow_queries >= 1,
        "expected {} to increment slow_queries when threshold is zero, got {:?}",
        label,
        profiler_stats
    );

    GlobalProfiler::disable();
    GlobalProfiler::reset();
    GlobalProfiler::set_slow_threshold(100);

    result
}

#[tokio::test]
async fn global_profiler_records_every_execution_path() {
    if !parity::setup().await {
        return;
    }
    parity::seed_users(10).await;

    let first_user = TestUser::query()
        .order_by("id", Order::Asc)
        .first()
        .await
        .expect("Query failed")
        .expect("Expected at least one seeded user");

    assert_profiled_operation("TestUser::query().get()", TestUser::query().get()).await;
    assert_profiled_operation("TestUser::query().count()", TestUser::query().count()).await;
    assert_profiled_operation(
        "TestUser::query().count_distinct(\"active\")",
        TestUser::query().count_distinct("active"),
    )
    .await;
    assert_profiled_operation(
        "TestUser::query().sum(\"age\")",
        TestUser::query().sum::<i64>("age"),
    )
    .await;
    assert_profiled_operation(
        "Database::raw::<TestUser>()",
        Database::raw::<TestUser>(
            "SELECT id, email, name, age, active FROM test_users ORDER BY id LIMIT 2",
        ),
    )
    .await;
    assert_profiled_operation(
        "Database::raw_with_params::<TestUser>()",
        Database::raw_with_params::<TestUser>(
            "SELECT id, email, name, age, active FROM test_users WHERE age > ?",
            vec![25.into()],
        ),
    )
    .await;
    assert_profiled_operation(
        "Database::execute()",
        Database::execute("UPDATE test_users SET active = active"),
    )
    .await;
    assert_profiled_operation(
        "Database::execute_with_params()",
        Database::execute_with_params(
            "UPDATE test_users SET name = ? WHERE id = ?",
            vec![first_user.name.clone().into(), first_user.id.into()],
        ),
    )
    .await;

    let updated_user = assert_profiled_operation("TestUser::update()", async move {
        let mut user = first_user;
        user.name = format!("{} (profiled)", user.name);
        user.update().await
    })
    .await;
    assert!(updated_user.name.ends_with("(profiled)"));

    let temp_user = assert_profiled_operation(
        "TestUser::save()",
        TestUser {
            id: 0,
            email: "profile-save@example.com".to_string(),
            name: "Profile Save".to_string(),
            age: 33,
            active: true,
        }
        .save(),
    )
    .await;
    assert_profiled_operation("TestUser::delete()", temp_user.delete()).await;

    let destroy_target = TestUser {
        id: 0,
        email: "profile-destroy@example.com".to_string(),
        name: "Profile Destroy".to_string(),
        age: 44,
        active: false,
    }
    .save()
    .await
    .expect("Failed to create destroy target");
    assert_profiled_operation("TestUser::destroy()", TestUser::destroy(destroy_target.id)).await;
}
