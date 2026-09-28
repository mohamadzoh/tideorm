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
fn test_default_config() {
    let config = Config::default();
    assert_eq!(config.languages, vec!["en".to_string()]);
    assert_eq!(config.fallback_language, "en");
}

#[test]
fn test_config_builder() {
    TideConfig::reset();
    TideConfig::init()
        .languages(&["en", "fr"])
        .fallback_language("fr")
        .apply();

    let config = Config::global();
    assert!(config.languages.contains(&"fr".to_string()));
    assert_eq!(config.fallback_language, "fr");
}

#[test]
fn test_tide_config_models_matching_registers_models_by_file_folder_and_recursive_globs() {
    for pattern in [
        "**/config_tests/model_pattern_fixture.rs",
        "**/config_tests/*",
        "**/config_tests/**/*.rs",
    ] {
        SyncRegistry::clear();
        TideConfig::reset();

        let _ = TideConfig::init().models_matching(pattern);

        assert_eq!(
            registered_sync_table_names(),
            vec![
                "config_path_match_posts".to_string(),
                "config_path_match_users".to_string(),
            ],
            "{pattern}"
        );
    }

    SyncRegistry::clear();
    TideConfig::reset();
}

#[test]
fn test_tide_config_models_matching_ignores_non_matching_patterns() {
    SyncRegistry::clear();
    TideConfig::reset();

    let _ = TideConfig::init().models_matching("**/does_not_exist/*.rs");

    assert!(registered_sync_table_names().is_empty());

    SyncRegistry::clear();
    TideConfig::reset();
}

#[test]
fn test_database_type_from_url() {
    assert_eq!(
        DatabaseType::from_url("postgres://localhost/test"),
        Some(DatabaseType::Postgres)
    );
    assert_eq!(
        DatabaseType::from_url("postgresql://localhost/test"),
        Some(DatabaseType::Postgres)
    );
    assert_eq!(
        DatabaseType::from_url("mysql://localhost/test"),
        Some(DatabaseType::MySQL)
    );
    assert_eq!(
        DatabaseType::from_url("mariadb://localhost/test"),
        Some(DatabaseType::MariaDB)
    );
    assert_eq!(
        DatabaseType::from_url("sqlite:./test.db"),
        Some(DatabaseType::SQLite)
    );
    assert_eq!(
        DatabaseType::from_url("sqlite::memory:"),
        Some(DatabaseType::SQLite)
    );
    assert_eq!(DatabaseType::from_url("invalid://localhost"), None);
}

#[test]
fn test_rewrite_driver_url_for_mariadb() {
    assert_eq!(
        rewrite_driver_url("mariadb://localhost/test"),
        "mysql://localhost/test"
    );
}

#[test]
fn test_rewrite_driver_url_leaves_malformed_mariadb_urls_unchanged() {
    assert_eq!(
        rewrite_driver_url("mariadb:/localhost/test"),
        "mariadb:/localhost/test"
    );
    assert_eq!(
        rewrite_driver_url("mariadb:localhost/test"),
        "mariadb:localhost/test"
    );
}

#[test]
fn test_database_type_supports_arrays() {
    assert!(DatabaseType::Postgres.supports_arrays());
    assert!(!DatabaseType::MySQL.supports_arrays());
    assert!(!DatabaseType::MariaDB.supports_arrays());
    assert!(!DatabaseType::SQLite.supports_arrays());
}

#[test]
fn test_database_type_supports_returning() {
    assert!(DatabaseType::Postgres.supports_returning());
    assert!(!DatabaseType::MySQL.supports_returning());
    assert!(!DatabaseType::MariaDB.supports_returning());
    assert!(DatabaseType::SQLite.supports_returning());
}

#[test]
fn test_database_type_optimal_batch_size() {
    assert_eq!(DatabaseType::Postgres.optimal_batch_size(), 1000);
    assert_eq!(DatabaseType::MySQL.optimal_batch_size(), 500);
    assert_eq!(DatabaseType::MariaDB.optimal_batch_size(), 500);
    assert_eq!(DatabaseType::SQLite.optimal_batch_size(), 100);
}

#[test]
fn test_database_type_quote_char() {
    assert_eq!(DatabaseType::Postgres.quote_char(), '"');
    assert_eq!(DatabaseType::MySQL.quote_char(), '`');
    assert_eq!(DatabaseType::MariaDB.quote_char(), '`');
    assert_eq!(DatabaseType::SQLite.quote_char(), '"');
}

#[test]
fn test_database_type_display() {
    assert_eq!(format!("{}", DatabaseType::Postgres), "PostgreSQL");
    assert_eq!(format!("{}", DatabaseType::MySQL), "MySQL");
    assert_eq!(format!("{}", DatabaseType::MariaDB), "MariaDB");
    assert_eq!(format!("{}", DatabaseType::SQLite), "SQLite");
}

#[test]
fn test_tide_config_schema_file() {
    let config = TideConfig::init()
        .database_type(DatabaseType::Postgres)
        .database("postgres://localhost/test")
        .schema_file("test_schema.sql");

    assert_eq!(config.schema_file, Some("test_schema.sql".to_string()));
}

#[test]
fn test_tide_config_schema_file_with_path() {
    let config = TideConfig::init()
        .database("postgres://localhost/test")
        .schema_file("./database/schema.sql");

    assert_eq!(
        config.schema_file,
        Some("./database/schema.sql".to_string())
    );
}

#[test]
fn test_tide_config_no_schema_file() {
    let config = TideConfig::init().database("postgres://localhost/test");

    assert!(config.schema_file.is_none());
}

#[test]
fn test_tide_config_file_base_url_for_builder() {
    TideConfig::reset();
    let config = TideConfig::init()
        .file_base_url("https://cdn.example.com/uploads")
        .file_base_url_for("thumbnail", "https://thumbs.example.com/uploads")
        .file_base_url_for("avatar", "https://avatars.example.com/uploads");

    assert_eq!(
        config.config.file_field_base_urls.get("thumbnail"),
        Some(&"https://thumbs.example.com/uploads".to_string())
    );
    assert_eq!(
        config.config.file_field_base_urls.get("avatar"),
        Some(&"https://avatars.example.com/uploads".to_string())
    );
    assert_eq!(
        config.config.resolve_file_base_url("document"),
        Some("https://cdn.example.com/uploads")
    );
}

#[test]
fn test_config_resolve_file_base_url_prefers_field_match() {
    let mut config = Config {
        file_base_url: Some("https://cdn.example.com/uploads".to_string()),
        ..Default::default()
    };
    config.file_field_base_urls.insert(
        "thumbnail".to_string(),
        "https://thumbs.example.com/uploads".to_string(),
    );

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

#[test]
fn test_tide_config_apply_overwrites_existing_global_state() {
    TideConfig::reset();

    TideConfig::init()
        .database_type(DatabaseType::Postgres)
        .fallback_language("fr")
        .apply();

    TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .fallback_language("ar")
        .apply();

    assert_eq!(TideConfig::get_database_type(), Some(DatabaseType::SQLite));
    assert_eq!(Config::global().fallback_language, "ar");
}

#[test]
fn test_tide_config_reset_restores_defaults() {
    TideConfig::reset();

    TideConfig::init()
        .database_type(DatabaseType::MariaDB)
        .schema_file("reset_schema.sql")
        .fallback_language("fr")
        .apply();

    TideConfig::reset();

    assert_eq!(TideConfig::get_database_type(), None);
    assert_eq!(TideConfig::schema_file_path(), None);
    assert_eq!(Config::global().fallback_language, "en");
}

#[test]
fn test_tide_config_apply_clears_database_type_when_omitted() {
    TideConfig::reset();

    TideConfig::init()
        .database_type(DatabaseType::Postgres)
        .apply();

    TideConfig::init().fallback_language("ar").apply();

    assert_eq!(TideConfig::get_database_type(), None);
    assert_eq!(Config::global().fallback_language, "ar");
}

#[test]
fn test_tide_config_schema_file_path_replaced_without_leak_prone_static_refs() {
    TideConfig::reset();

    TideConfig::init().schema_file("first_schema.sql").apply();
    assert_eq!(
        TideConfig::schema_file_path().as_deref(),
        Some("first_schema.sql")
    );

    TideConfig::init().schema_file("second_schema.sql").apply();
    assert_eq!(
        TideConfig::schema_file_path().as_deref(),
        Some("second_schema.sql")
    );

    TideConfig::reset();
    assert_eq!(TideConfig::schema_file_path(), None);
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

#[test]
fn test_rewrite_driver_url_reads_the_scheme_in_any_case() {
    assert_eq!(
        rewrite_driver_url("MariaDB://localhost/test"),
        "mysql://localhost/test"
    );
    assert_eq!(
        rewrite_driver_url("postgres://localhost/mariadb://"),
        "postgres://localhost/mariadb://"
    );
}

#[test]
fn a_table_of_the_same_name_in_another_schema_registers_for_sync() {
    use crate::sync::ModelSchema;

    SyncRegistry::clear();
    SyncRegistry::register_schema(ModelSchema::new("users").schema("tenant_a"));
    SyncRegistry::register_schema(ModelSchema::new("users").schema("tenant_b"));
    SyncRegistry::register_schema(ModelSchema::new("users").schema("tenant_a"));

    let mut registered: Vec<(String, String)> = SyncRegistry::get_all_schemas()
        .into_iter()
        .map(|schema| (schema.schema_name, schema.table_name))
        .collect();
    registered.sort();
    SyncRegistry::clear();
    assert_eq!(
        registered,
        [
            ("tenant_a".to_string(), "users".to_string()),
            ("tenant_b".to_string(), "users".to_string()),
        ]
    );
}
