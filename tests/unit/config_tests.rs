use super::database::rewrite_driver_url;
use super::*;
use crate::sync::SyncRegistry;
use std::time::Duration;

#[path = "config_tests/model_pattern_fixture.rs"]
mod model_pattern_fixture;

fn registered_sync_table_names() -> Vec<String> {
    let mut table_names: Vec<_> = SyncRegistry::get_all_schemas()
        .into_iter()
        .map(|schema| schema.table_name)
        .collect();
    table_names.sort();
    table_names
}

#[test]
fn test_tide_config_models_matching_registers_models_by_file_folder_and_recursive_globs() {
    let fixture_tables = vec![
        "config_path_match_posts".to_string(),
        "config_path_match_users".to_string(),
    ];
    for (pattern, expected) in [
        (
            "**/config_tests/model_pattern_fixture.rs",
            fixture_tables.clone(),
        ),
        ("**/config_tests/*", fixture_tables.clone()),
        ("**/config_tests/**/*.rs", fixture_tables),
        ("**/does_not_exist/*.rs", Vec::new()),
    ] {
        SyncRegistry::clear();
        TideConfig::reset();

        let _ = TideConfig::init().models_matching(pattern);

        assert_eq!(registered_sync_table_names(), expected, "{pattern}");
    }

    SyncRegistry::clear();
    TideConfig::reset();
}

#[test]
fn test_database_type_from_url() {
    for (url, expected) in [
        ("postgres://localhost/test", Some(DatabaseType::Postgres)),
        ("postgresql://localhost/test", Some(DatabaseType::Postgres)),
        ("mysql://localhost/test", Some(DatabaseType::MySQL)),
        ("mariadb://localhost/test", Some(DatabaseType::MariaDB)),
        ("sqlite:./test.db", Some(DatabaseType::SQLite)),
        ("sqlite::memory:", Some(DatabaseType::SQLite)),
        ("invalid://localhost", None),
    ] {
        assert_eq!(DatabaseType::from_url(url), expected, "{url}");
    }
}

#[test]
fn test_rewrite_driver_url_points_only_a_mariadb_scheme_at_the_mysql_driver() {
    for (url, expected) in [
        ("mariadb://localhost/test", "mysql://localhost/test"),
        ("MariaDB://localhost/test", "mysql://localhost/test"),
        // Malformed, so left for the driver to report.
        ("mariadb:/localhost/test", "mariadb:/localhost/test"),
        ("mariadb:localhost/test", "mariadb:localhost/test"),
        // Only the scheme is read.
        (
            "postgres://localhost/mariadb://",
            "postgres://localhost/mariadb://",
        ),
    ] {
        assert_eq!(rewrite_driver_url(url), expected, "{url}");
    }
}

#[test]
fn test_database_type_capabilities() {
    for (db_type, arrays, returning, batch_size, quote, name) in [
        (DatabaseType::Postgres, true, true, 1000, '"', "PostgreSQL"),
        (DatabaseType::MySQL, false, false, 500, '`', "MySQL"),
        (DatabaseType::MariaDB, false, false, 500, '`', "MariaDB"),
        (DatabaseType::SQLite, false, true, 100, '"', "SQLite"),
    ] {
        assert_eq!(db_type.supports_arrays(), arrays, "{db_type:?}");
        assert_eq!(db_type.supports_returning(), returning, "{db_type:?}");
        assert_eq!(db_type.optimal_batch_size(), batch_size, "{db_type:?}");
        assert_eq!(db_type.quote_char(), quote, "{db_type:?}");
        assert_eq!(db_type.to_string(), name, "{db_type:?}");
    }
}

#[test]
fn test_config_resolve_file_base_url_prefers_field_match() {
    let config = TideConfig::init()
        .file_base_url("https://cdn.example.com/uploads")
        .file_base_url_for("thumbnail", "https://thumbs.example.com/uploads")
        .config;

    assert_eq!(
        config.resolve_file_base_url("thumbnail"),
        Some("https://thumbs.example.com/uploads")
    );
    assert_eq!(
        config.resolve_file_base_url("gallery"),
        Some("https://cdn.example.com/uploads")
    );
    assert_eq!(Config::default().resolve_file_base_url("gallery"), None);
}

/// `apply` installs the whole configuration it was given: a setting the next
/// call leaves out goes back to its default rather than surviving.
#[test]
fn test_tide_config_apply_replaces_the_global_configuration() {
    TideConfig::reset();

    TideConfig::init()
        .database_type(DatabaseType::Postgres)
        .schema_file("first_schema.sql")
        .languages(&["en", "fr"])
        .fallback_language("fr")
        .apply();
    assert_eq!(
        TideConfig::get_database_type(),
        Some(DatabaseType::Postgres)
    );
    assert_eq!(
        TideConfig::schema_file_path().as_deref(),
        Some("first_schema.sql")
    );
    assert_eq!(Config::global().languages, ["en", "fr"]);
    assert_eq!(Config::global().fallback_language, "fr");

    TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .schema_file("second_schema.sql")
        .fallback_language("ar")
        .apply();
    assert_eq!(TideConfig::get_database_type(), Some(DatabaseType::SQLite));
    assert_eq!(
        TideConfig::schema_file_path().as_deref(),
        Some("second_schema.sql")
    );
    assert_eq!(Config::global().fallback_language, "ar");

    TideConfig::init().fallback_language("ar").apply();
    assert_eq!(TideConfig::get_database_type(), None);
    assert_eq!(TideConfig::schema_file_path(), None);
    assert_eq!(Config::global().languages, ["en"]);

    TideConfig::reset();
}

#[test]
fn test_tide_config_reset_restores_defaults() {
    TideConfig::reset();

    TideConfig::init()
        .database_type(DatabaseType::MariaDB)
        .schema_file("reset_schema.sql")
        .languages(&["fr"])
        .fallback_language("fr")
        .apply();

    TideConfig::reset();

    assert_eq!(TideConfig::get_database_type(), None);
    assert_eq!(TideConfig::schema_file_path(), None);
    assert_eq!(Config::global().languages, ["en"]);
    assert_eq!(Config::global().fallback_language, "en");
}

#[tokio::test]
async fn test_mariadb_detection_failure_fails_instead_of_assuming_mysql() {
    let disconnected = crate::database::Database::disconnected();

    // Detection that cannot run must fail `connect()`, not quietly settle on
    // MySQL and turn MariaDB's RETURNING support off.
    let err = TideConfig::resolve_database_type(DatabaseType::MySQL, &disconnected)
        .await
        .expect_err("a failed MariaDB probe must not be read as MySQL");
    assert!(
        matches!(err, crate::error::Error::Connection { .. }),
        "{err:?}"
    );

    // Only a MySQL declaration is probed; other backends are taken as declared.
    assert_eq!(
        TideConfig::resolve_database_type(DatabaseType::Postgres, &disconnected)
            .await
            .expect("a Postgres declaration needs no probe"),
        DatabaseType::Postgres
    );
}

#[test]
fn test_pool_config_defaults() {
    let pool = PoolConfig::default();

    assert_eq!(pool.max_connections, 10);
    assert_eq!(pool.min_connections, 1);
    assert_eq!(pool.connect_timeout, Duration::from_secs(8));
    assert_eq!(pool.idle_timeout, Duration::from_secs(600));
    assert_eq!(pool.max_lifetime, Duration::from_secs(1800));
    assert_eq!(pool.acquire_timeout, Duration::from_secs(8));
}

/// Every builder setter writes its own field.
#[test]
fn test_tide_config_full_chain() {
    let config = TideConfig::init()
        .database_type(DatabaseType::Postgres)
        .database("postgres://localhost/test")
        .max_connections(20)
        .min_connections(5)
        .connect_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(300))
        .max_lifetime(Duration::from_secs(3600))
        .acquire_timeout(Duration::from_secs(5))
        .schema_file("schema.sql")
        .sync(false)
        .languages(&["en", "fr", "ar"])
        .fallback_language("en")
        .hidden_attributes(&["password", "secret"]);

    assert_eq!(config.database_type, Some(DatabaseType::Postgres));
    assert_eq!(
        config.database_url,
        Some("postgres://localhost/test".to_string())
    );
    assert_eq!(config.pool.max_connections, 20);
    assert_eq!(config.pool.min_connections, 5);
    assert_eq!(config.pool.connect_timeout, Duration::from_secs(10));
    assert_eq!(config.pool.idle_timeout, Duration::from_secs(300));
    assert_eq!(config.pool.max_lifetime, Duration::from_secs(3600));
    assert_eq!(config.pool.acquire_timeout, Duration::from_secs(5));
    assert_eq!(config.schema_file, Some("schema.sql".to_string()));
    assert!(!config.sync_enabled);
    assert_eq!(config.config.languages, vec!["en", "fr", "ar"]);
    assert_eq!(config.config.fallback_language, "en");
    assert_eq!(config.config.hidden_attributes, vec!["password", "secret"]);
}

fn apply_test_token_encoder(record_id: &str, model_name: &str) -> crate::error::Result<String> {
    Ok(format!("apply-encoder:{model_name}:{record_id}"))
}

fn apply_test_token_decoder(
    token: &str,
    _model_name: &str,
) -> crate::error::Result<Option<String>> {
    Ok(Some(format!("apply-decoder:{token}")))
}

#[test]
fn test_tide_config_apply_installs_tokenization_settings() {
    TideConfig::reset();
    crate::tokenization::TokenConfig::reset();

    TideConfig::init()
        .encryption_key("apply-installs-the-encryption-key-32ch")
        .token_encoder(apply_test_token_encoder)
        .token_decoder(apply_test_token_decoder)
        .apply();

    assert_eq!(
        crate::tokenization::TokenConfig::get_encryption_key()
            .expect("apply() must install the encryption key, not only connect()"),
        "apply-installs-the-encryption-key-32ch"
    );
    assert_eq!(
        (crate::tokenization::TokenConfig::get_encoder())("7", "Post")
            .expect("the configured encoder must run"),
        "apply-encoder:Post:7"
    );
    assert_eq!(
        (crate::tokenization::TokenConfig::get_decoder())("tok", "Post")
            .expect("the configured decoder must run"),
        Some("apply-decoder:tok".to_string())
    );

    crate::tokenization::TokenConfig::reset();
    TideConfig::reset();
}
