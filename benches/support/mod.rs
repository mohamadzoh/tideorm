use std::future::Future;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tideorm::{Database, TideConfig};
use tokio::runtime::Runtime;

#[path = "../../tests/support/postgres_test_config.rs"]
mod postgres_test_config;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Whether the PostgreSQL benchmarks should run: they are opt-in like the
/// PostgreSQL test suites, so a bare `cargo bench` skips them with a note
/// instead of panicking without a server.
pub fn postgres_benchmarks_enabled() -> bool {
    let enabled = postgres_test_config::should_run_postgres_tests();
    if !enabled {
        println!(
            "Skipping PostgreSQL benchmarks: set POSTGRESQL_DATABASE_URL, TEST_DATABASE_URL or RUN_POSTGRES_TESTS"
        );
    }
    enabled
}

/// `criterion_main!` for a PostgreSQL bench: run the `benches` group only when
/// a server is configured.
macro_rules! postgres_bench_main {
    () => {
        fn main() {
            if support::postgres_benchmarks_enabled() {
                benches();
                criterion::Criterion::default()
                    .configure_from_args()
                    .final_summary();
            }
        }
    };
}
pub(crate) use postgres_bench_main;

pub fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| Runtime::new().expect("Failed to build benchmark runtime"))
}

pub fn block_on<F>(future: F) -> F::Output
where
    F: Future,
{
    runtime().block_on(future)
}

pub fn init_postgres_database(initialized: &OnceLock<()>, setup_statements: &[&str]) {
    initialized.get_or_init(|| {
        block_on(async {
            TideConfig::init()
                .database(postgres_test_config::test_database_url())
                .max_connections(50)
                .min_connections(5)
                .acquire_timeout(Duration::from_secs(30))
                .connect()
                .await
                .expect("Failed to connect to benchmark database");

            for statement in setup_statements {
                Database::execute(statement)
                    .await
                    .expect("Failed to run benchmark setup SQL");
            }
        });
    });
}

pub fn execute_sql(statement: &str) {
    block_on(async {
        Database::execute(statement)
            .await
            .expect("Failed to execute benchmark SQL");
    });
}

pub fn truncate_table(table_name: &str) {
    execute_sql(&format!(
        "TRUNCATE TABLE {table_name} RESTART IDENTITY CASCADE"
    ));
}

#[allow(dead_code)]
pub fn for_each_batch<T, Build, Consume>(
    total: usize,
    batch_size: usize,
    mut build: Build,
    mut consume: Consume,
) where
    Build: FnMut(usize) -> T,
    Consume: FnMut(Vec<T>),
{
    assert!(batch_size > 0, "batch_size must be greater than zero");

    let mut start = 0;
    while start < total {
        let end = (start + batch_size).min(total);
        let batch = (start..end).map(&mut build).collect();
        consume(batch);
        start = end;
    }
}

#[allow(dead_code)]
pub struct IdCycler {
    ids: Vec<i64>,
    counter: AtomicU64,
}

impl IdCycler {
    #[allow(dead_code)]
    pub fn new(ids: Vec<i64>) -> Self {
        assert!(!ids.is_empty(), "IdCycler requires at least one id");

        Self {
            ids,
            counter: AtomicU64::new(0),
        }
    }

    #[allow(dead_code)]
    pub fn next(&self) -> i64 {
        let index = self.counter.fetch_add(1, Ordering::SeqCst) as usize % self.ids.len();
        self.ids[index]
    }
}
