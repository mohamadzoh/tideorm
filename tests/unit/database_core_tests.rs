use super::Database;
use crate::config::DatabaseType;
use crate::internal::Backend;

#[test]
fn resolved_backend_prefers_the_live_handle_over_configuration() {
    assert_eq!(
        Database::resolve_backend(Some(DatabaseType::Postgres), Backend::Sqlite, false),
        DatabaseType::SQLite
    );
    assert_eq!(
        Database::resolve_backend(Some(DatabaseType::SQLite), Backend::Postgres, false),
        DatabaseType::Postgres
    );
    assert_eq!(
        Database::resolve_backend(Some(DatabaseType::Postgres), Backend::MySql, false),
        DatabaseType::MySQL
    );
}

#[test]
fn resolved_backend_keeps_configured_mariadb_on_a_mysql_handle() {
    assert_eq!(
        Database::resolve_backend(Some(DatabaseType::MariaDB), Backend::MySql, false),
        DatabaseType::MariaDB
    );
    assert_eq!(
        Database::resolve_backend(Some(DatabaseType::MariaDB), Backend::Sqlite, false),
        DatabaseType::SQLite
    );
}

/// A MySQL-protocol pool that found MariaDB when it opened is MariaDB
/// whatever was configured, as `Database::connect` sets up no configuration.
#[test]
fn resolved_backend_follows_a_handle_that_detected_mariadb() {
    assert_eq!(
        Database::resolve_backend(None, Backend::MySql, true),
        DatabaseType::MariaDB
    );
    assert_eq!(
        Database::resolve_backend(Some(DatabaseType::MySQL), Backend::MySql, true),
        DatabaseType::MariaDB
    );
}

#[test]
fn resolved_backend_falls_back_to_the_handle_without_configuration() {
    assert_eq!(
        Database::resolve_backend(None, Backend::MySql, false),
        DatabaseType::MySQL
    );
    assert_eq!(
        Database::resolve_backend(None, Backend::Sqlite, false),
        DatabaseType::SQLite
    );
    assert_eq!(
        Database::resolve_backend(None, Backend::Postgres, false),
        DatabaseType::Postgres
    );
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn metadata_answers_for_the_handle_it_was_called_on() {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed");
    // Stands in for a second handle whose connection is not the ambient
    // one: whatever it reports has to come from itself, not from the
    // transaction running around it.
    let other = Database::disconnected();

    db.transaction(move |_| {
        Box::pin(async move {
            assert!(
                other.__internal_backend().is_err(),
                "a handle must report its own backend, not the ambient transaction's"
            );
            assert!(
                other.ping().await.is_err(),
                "a handle must be pinged through its own connection"
            );

            // Execution still joins the ambient transaction: that is what
            // makes a nested `transaction` a SAVEPOINT instead of a second,
            // independent top-level one.
            let nested: crate::error::Result<()> =
                other.transaction(|_| Box::pin(async { Ok(()) })).await;
            nested.expect("a nested transaction should join the ambient one");

            Ok(())
        })
    })
    .await
    .expect("the outer transaction should commit");
}
