//! Helpers the `#[path]`-included unit tests share, each gated on the
//! features it needs.

/// One lock for every unit test that installs a global database, so none
/// swaps the connection or the query cache under another.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
pub(crate) fn global_db_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// Forget the global database and configuration.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
pub(crate) fn reset_globals() {
    crate::database::Database::reset_global();
    crate::config::TideConfig::reset();
}

/// Empty the global query cache and turn it on or off.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
pub(crate) fn set_query_cache(enabled: bool) {
    let cache = crate::cache::QueryCache::global();
    cache.clear();
    if enabled {
        cache.enable();
    } else {
        cache.disable();
    }
}

/// Turn the query cache off and forget the globals: the teardown of a test
/// that enabled the cache.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
pub(crate) fn reset_globals_and_cache() {
    set_query_cache(false);
    reset_globals();
}

/// Run each of `statements` on `db`, panicking at the first that fails.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
pub(crate) async fn sqlite_execute(db: &crate::database::Database, statements: &[impl AsRef<str>]) {
    for statement in statements {
        let statement = statement.as_ref();
        db.__execute_with_params(statement, vec![])
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

/// An in-memory SQLite database, not installed globally, with each
/// statement of `schema` run on it.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
pub(crate) async fn sqlite_memory(schema: &[impl AsRef<str>]) -> crate::database::Database {
    let db = crate::database::Database::connect("sqlite::memory:")
        .await
        .expect("an in-memory SQLite database");
    sqlite_execute(&db, schema).await;
    db
}

/// Forget the globals, then install an in-memory SQLite database as the
/// global one, with each statement of `schema` run on it.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
pub(crate) async fn install_sqlite_global(schema: &[&str]) -> crate::database::Database {
    reset_globals();
    let db = sqlite_memory(schema).await;
    crate::database::Database::set_global(db.clone()).expect("installing the global database");
    db
}

/// Every row of `M` by id, read through the query cache.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
pub(crate) async fn cached_rows<M: crate::model::Model>() -> Vec<M> {
    M::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(std::time::Duration::from_secs(60))
        .get()
        .await
        .expect("a cached read")
}

/// How many entries the global query cache holds.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
pub(crate) fn cached_entries() -> usize {
    crate::cache::QueryCache::global().stats().entries
}

/// Connect to the PostgreSQL test server with each of `tables`, `(name,
/// columns)`, created afresh, dropped last-first; `None` when the PostgreSQL
/// tests are off.
#[cfg(feature = "entity-manager")]
pub(crate) async fn fresh_postgres_tables(
    tables: &[(&str, &str)],
) -> crate::error::Result<Option<std::sync::Arc<crate::database::Database>>> {
    if !crate::postgres_test_config::should_run_postgres_tests() {
        println!("{}", crate::postgres_test_config::SKIPPED);
        return Ok(None);
    }
    let db = std::sync::Arc::new(
        crate::database::Database::connect(crate::postgres_test_config::test_database_url())
            .await?,
    );
    db.scope(async {
        for (table, _) in tables.iter().rev() {
            crate::database::Database::execute(&format!("DROP TABLE IF EXISTS {table} CASCADE"))
                .await?;
        }
        for (table, columns) in tables {
            crate::database::Database::execute(&format!("CREATE TABLE {table} ({columns})"))
                .await?;
        }
        Ok(())
    })
    .await?;
    Ok(Some(db))
}

/// A deterministic xorshift generator for property tests: a fixed seed gives
/// the same cases on every run, so a failure reproduces.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub(crate) fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % bound as u64) as usize
    }

    /// Up to `max` pieces of `alphabet`, joined.
    pub(crate) fn text(&mut self, alphabet: &[&str], max: usize) -> String {
        let len = self.below(max + 1);
        (0..len)
            .map(|_| alphabet[self.below(alphabet.len())])
            .collect()
    }
}
