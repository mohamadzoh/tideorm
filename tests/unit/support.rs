//! Helpers the `#[path]`-included unit tests share, each gated on the
//! backend it needs.

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

/// Forget the globals, then install an in-memory SQLite database as the
/// global one, with each statement of `schema` run on it.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
pub(crate) async fn install_sqlite_global(schema: &[&str]) -> crate::database::Database {
    reset_globals();
    let db = crate::database::Database::connect("sqlite::memory:")
        .await
        .expect("an in-memory SQLite database");
    crate::database::Database::set_global(db.clone()).expect("installing the global database");
    for statement in schema {
        db.__execute_with_params(statement, vec![])
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
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
    crate::database::__in_db_scope(db.as_ref(), async {
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
