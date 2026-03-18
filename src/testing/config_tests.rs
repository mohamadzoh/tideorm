use super::*;

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
fn test_database_type_supports_json() {
    assert!(DatabaseType::Postgres.supports_json());
    assert!(DatabaseType::MySQL.supports_json());
    assert!(DatabaseType::MariaDB.supports_json());
    assert!(DatabaseType::SQLite.supports_json());
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
    assert!(DatabaseType::MariaDB.supports_returning());
    assert!(DatabaseType::SQLite.supports_returning());
}

#[test]
fn test_database_type_supports_upsert() {
    assert!(DatabaseType::Postgres.supports_upsert());
    assert!(DatabaseType::MySQL.supports_upsert());
    assert!(DatabaseType::MariaDB.supports_upsert());
    assert!(DatabaseType::SQLite.supports_upsert());
}

#[test]
fn test_database_type_supports_fulltext_search() {
    assert!(DatabaseType::Postgres.supports_fulltext_search());
    assert!(DatabaseType::MySQL.supports_fulltext_search());
    assert!(DatabaseType::MariaDB.supports_fulltext_search());
    assert!(DatabaseType::SQLite.supports_fulltext_search());
}

#[test]
fn test_database_type_supports_window_functions() {
    assert!(DatabaseType::Postgres.supports_window_functions());
    assert!(DatabaseType::MySQL.supports_window_functions());
    assert!(DatabaseType::MariaDB.supports_window_functions());
    assert!(DatabaseType::SQLite.supports_window_functions());
}

#[test]
fn test_database_type_supports_cte() {
    assert!(DatabaseType::Postgres.supports_cte());
    assert!(DatabaseType::MySQL.supports_cte());
    assert!(DatabaseType::MariaDB.supports_cte());
    assert!(DatabaseType::SQLite.supports_cte());
}

#[test]
fn test_database_type_supports_schemas() {
    assert!(DatabaseType::Postgres.supports_schemas());
    assert!(!DatabaseType::MySQL.supports_schemas());
    assert!(!DatabaseType::MariaDB.supports_schemas());
    assert!(!DatabaseType::SQLite.supports_schemas());
}

#[test]
fn test_database_type_optimal_batch_size() {
    assert_eq!(DatabaseType::Postgres.optimal_batch_size(), 1000);
    assert_eq!(DatabaseType::MySQL.optimal_batch_size(), 500);
    assert_eq!(DatabaseType::MariaDB.optimal_batch_size(), 500);
    assert_eq!(DatabaseType::SQLite.optimal_batch_size(), 100);
}

#[test]
fn test_database_type_param_style() {
    assert_eq!(DatabaseType::Postgres.param_style(), "$");
    assert_eq!(DatabaseType::MySQL.param_style(), "?");
    assert_eq!(DatabaseType::MariaDB.param_style(), "?");
    assert_eq!(DatabaseType::SQLite.param_style(), "?");
}

#[test]
fn test_database_type_quote_char() {
    assert_eq!(DatabaseType::Postgres.quote_char(), '"');
    assert_eq!(DatabaseType::MySQL.quote_char(), '`');
    assert_eq!(DatabaseType::MariaDB.quote_char(), '`');
    assert_eq!(DatabaseType::SQLite.quote_char(), '"');
}

#[test]
fn test_database_type_default_port() {
    assert_eq!(DatabaseType::Postgres.default_port(), 5432);
    assert_eq!(DatabaseType::MySQL.default_port(), 3306);
    assert_eq!(DatabaseType::MariaDB.default_port(), 3306);
    assert_eq!(DatabaseType::SQLite.default_port(), 0);
}

#[test]
fn test_database_type_url_scheme() {
    assert_eq!(DatabaseType::Postgres.url_scheme(), "postgres");
    assert_eq!(DatabaseType::MySQL.url_scheme(), "mysql");
    assert_eq!(DatabaseType::MariaDB.url_scheme(), "mariadb");
    assert_eq!(DatabaseType::SQLite.url_scheme(), "sqlite");
}

#[test]
fn test_database_type_display() {
    assert_eq!(format!("{}", DatabaseType::Postgres), "PostgreSQL");
    assert_eq!(format!("{}", DatabaseType::MySQL), "MySQL");
    assert_eq!(format!("{}", DatabaseType::MariaDB), "MariaDB");
    assert_eq!(format!("{}", DatabaseType::SQLite), "SQLite");
}

#[test]
fn test_database_type_is_mysql_compatible() {
    assert!(!DatabaseType::Postgres.is_mysql_compatible());
    assert!(DatabaseType::MySQL.is_mysql_compatible());
    assert!(DatabaseType::MariaDB.is_mysql_compatible());
    assert!(!DatabaseType::SQLite.is_mysql_compatible());
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
    let mut config = Config::default();
    config.file_base_url = Some("https://cdn.example.com/uploads".to_string());
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
        .max_connections(3)
        .fallback_language("fr")
        .apply();

    TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .max_connections(9)
        .fallback_language("ar")
        .apply();

    assert_eq!(TideConfig::get_database_type(), Some(DatabaseType::SQLite));
    assert_eq!(TideConfig::pool_config().max_connections, 9);
    assert_eq!(Config::global().fallback_language, "ar");
}

#[test]
fn test_tide_config_reset_restores_defaults() {
    TideConfig::reset();

    TideConfig::init()
        .database_type(DatabaseType::MariaDB)
        .max_connections(17)
        .fallback_language("fr")
        .apply();

    TideConfig::reset();

    assert_eq!(TideConfig::get_database_type(), None);
    assert_eq!(TideConfig::pool_config().max_connections, PoolConfig::default().max_connections);
    assert_eq!(Config::global().fallback_language, "en");
}

#[test]
fn test_tide_config_apply_clears_database_type_when_omitted() {
    TideConfig::reset();

    TideConfig::init()
        .database_type(DatabaseType::Postgres)
        .apply();

    TideConfig::init()
        .fallback_language("ar")
        .apply();

    assert_eq!(TideConfig::get_database_type(), None);
    assert_eq!(Config::global().fallback_language, "ar");
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
