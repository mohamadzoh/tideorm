mod error_from_io {
    use tideorm::error::Error;

    #[test]
    fn test_io_error_converts_to_internal() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file gone");
        let tide_err: Error = io_err.into();
        match &tide_err {
            Error::Internal { message } => assert!(message.contains("file gone")),
            other => panic!("expected Internal, got {:?}", other),
        }
        assert_eq!(tide_err.code(), "TIDE_INTERNAL");
        assert_eq!(tide_err.http_status(), 500);
    }

    #[test]
    fn test_io_permission_denied_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let tide_err: Error = io_err.into();
        match &tide_err {
            Error::Internal { message } => assert!(message.contains("access denied")),
            other => panic!("expected Internal, got {:?}", other),
        }
        assert!(!tide_err.is_retryable());
    }
}

mod error_from_serde {
    use tideorm::error::Error;

    #[test]
    fn test_serde_error_converts_to_conversion() {
        let bad_json = "{ not valid }";
        let serde_err = serde_json::from_str::<serde_json::Value>(bad_json).unwrap_err();
        let tide_err: Error = serde_err.into();
        match &tide_err {
            Error::Conversion { message } => {
                assert!(
                    !message.is_empty(),
                    "message should describe the JSON error"
                );
            }
            other => panic!("expected Conversion, got {:?}", other),
        }
        assert_eq!(tide_err.code(), "TIDE_CONVERSION");
        assert_eq!(tide_err.http_status(), 400);
    }

    #[test]
    fn test_serde_eof_error_converts() {
        let serde_err = serde_json::from_str::<serde_json::Value>("").unwrap_err();
        let tide_err: Error = serde_err.into();
        match &tide_err {
            Error::Conversion { message } => {
                assert!(
                    message.contains("EOF") || message.contains("end of") || !message.is_empty()
                );
            }
            other => panic!("expected Conversion, got {:?}", other),
        }
    }
}

mod config_url_edge_cases {
    use tideorm::config::DatabaseType;

    #[test]
    fn test_from_url_empty_string() {
        assert_eq!(DatabaseType::from_url(""), None);
    }

    #[test]
    fn test_from_url_bare_scheme() {
        assert_eq!(DatabaseType::from_url("://"), None);
    }

    #[test]
    fn test_from_url_unknown_scheme() {
        assert_eq!(DatabaseType::from_url("oracle://host/db"), None);
        assert_eq!(DatabaseType::from_url("mssql://host/db"), None);
        assert_eq!(DatabaseType::from_url("mongodb://host/db"), None);
    }

    #[test]
    fn test_from_url_case_insensitive() {
        assert_eq!(
            DatabaseType::from_url("POSTGRES://host"),
            Some(DatabaseType::Postgres)
        );
        assert_eq!(
            DatabaseType::from_url("MySQL://host"),
            Some(DatabaseType::MySQL)
        );
        assert_eq!(
            DatabaseType::from_url("MARIADB://host"),
            Some(DatabaseType::MariaDB)
        );
        assert_eq!(
            DatabaseType::from_url("SQLite:./db.sqlite"),
            Some(DatabaseType::SQLite)
        );
    }

    #[test]
    fn test_from_url_postgresql_alias() {
        assert_eq!(
            DatabaseType::from_url("postgresql://localhost:5432/db"),
            Some(DatabaseType::Postgres)
        );
    }

    #[test]
    fn test_from_url_no_host() {
        assert_eq!(
            DatabaseType::from_url("postgres://"),
            Some(DatabaseType::Postgres)
        );
        assert_eq!(
            DatabaseType::from_url("mysql://"),
            Some(DatabaseType::MySQL)
        );
    }
}

mod mariadb_feature_parity {
    use tideorm::config::DatabaseType;

    #[test]
    fn test_feature_matrix_parity() {
        let mysql = DatabaseType::MySQL;
        let maria = DatabaseType::MariaDB;

        assert_eq!(mysql.supports_json(), maria.supports_json());
        assert_eq!(
            mysql.supports_native_json_operators(),
            maria.supports_native_json_operators()
        );
        assert_eq!(mysql.supports_arrays(), maria.supports_arrays());
        assert_eq!(mysql.supports_upsert(), maria.supports_upsert());
        assert_eq!(
            mysql.supports_fulltext_search(),
            maria.supports_fulltext_search()
        );
        assert_eq!(
            mysql.supports_window_functions(),
            maria.supports_window_functions()
        );
        assert_eq!(mysql.supports_cte(), maria.supports_cte());
        assert_eq!(mysql.supports_schemas(), maria.supports_schemas());
        assert_eq!(mysql.default_port(), maria.default_port());
        assert_eq!(mysql.param_style(), maria.param_style());
        assert_eq!(mysql.quote_char(), maria.quote_char());
        assert_eq!(mysql.optimal_batch_size(), maria.optimal_batch_size());

        assert!(!mysql.supports_returning());
        assert!(maria.supports_returning());
        assert!(mysql.is_mysql_compatible());
        assert!(maria.is_mysql_compatible());
    }

    #[test]
    fn test_mariadb_display_differs_from_mysql() {
        assert_eq!(format!("{}", DatabaseType::MySQL), "MySQL");
        assert_eq!(format!("{}", DatabaseType::MariaDB), "MariaDB");
    }

    #[test]
    fn test_mariadb_url_scheme() {
        assert_eq!(DatabaseType::MariaDB.url_scheme(), "mariadb");
        assert_eq!(DatabaseType::MySQL.url_scheme(), "mysql");
    }
}

mod cache_capacity_boundary {
    use std::time::Duration;
    use tideorm::cache::{CacheConfig, CacheStrategy, QueryCache};

    #[test]
    fn test_cache_capacity_one_evicts_on_second_insert() {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 1,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::LRU,
            cache_empty_results: true,
            key_prefix: None,
        });

        cache
            .set::<String>("k1", &"val1".to_string(), None, "test")
            .unwrap();
        assert_eq!(cache.len(), 1);
        assert!(cache.contains("k1"));

        cache
            .set::<String>("k2", &"val2".to_string(), None, "test")
            .unwrap();
        assert_eq!(cache.len(), 1);
        assert!(cache.contains("k2"));
        assert!(!cache.contains("k1"));
    }

    #[test]
    fn test_cache_fifo_eviction_order() {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 2,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::FIFO,
            cache_empty_results: true,
            key_prefix: None,
        });

        cache.set::<i32>("a", &1, None, "t").unwrap();
        cache.set::<i32>("b", &2, None, "t").unwrap();
        let _: Option<i32> = cache.get("a");

        cache.set::<i32>("c", &3, None, "t").unwrap();
        assert_eq!(cache.len(), 2);
        assert!(!cache.contains("a"));
        assert!(cache.contains("b"));
        assert!(cache.contains("c"));
    }
}

mod cache_ttl_expiration {
    use std::thread;
    use std::time::Duration;
    use tideorm::cache::{CacheConfig, CacheStrategy, QueryCache};

    #[test]
    fn test_cache_entry_expires_after_ttl() {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 100,
            default_ttl: Duration::from_millis(50),
            strategy: CacheStrategy::LRU,
            cache_empty_results: true,
            key_prefix: None,
        });

        cache
            .set::<String>("k", &"value".to_string(), None, "test")
            .unwrap();
        assert!(cache.get::<String>("k").is_some());

        thread::sleep(Duration::from_millis(80));
        assert!(cache.get::<String>("k").is_none());
    }
}

mod cache_concurrent_stress {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    use tideorm::cache::{CacheConfig, CacheStrategy, QueryCache};

    #[test]
    fn test_cache_concurrent_read_write_50_threads() {
        let cache = Arc::new(QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 100,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::LRU,
            cache_empty_results: true,
            key_prefix: None,
        }));

        let mut handles = Vec::new();

        for i in 0..25 {
            let c = Arc::clone(&cache);
            handles.push(thread::spawn(move || {
                for j in 0..20 {
                    let key = format!("w{}_{}", i, j);
                    c.set::<i32>(&key, &(i * 100 + j), None, "stress").unwrap();
                }
            }));
        }

        for i in 0..25 {
            let c = Arc::clone(&cache);
            handles.push(thread::spawn(move || {
                for j in 0..20 {
                    let key = format!("w{}_{}", i, j);
                    let _: Option<i32> = c.get(&key);
                }
            }));
        }

        for h in handles {
            h.join().expect("thread should not panic");
        }

        let stats = cache.stats();
        assert!(stats.entries <= 100, "should not exceed max_entries");
    }
}

mod cache_invalidate_model {
    use std::time::Duration;
    use tideorm::cache::{CacheConfig, CacheStrategy, QueryCache};

    #[test]
    fn test_invalidate_model_only_removes_matching() {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 100,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::LRU,
            cache_empty_results: true,
            key_prefix: None,
        });

        cache
            .set::<String>("user:1", &"alice".into(), None, "User")
            .unwrap();
        cache
            .set::<String>("user:2", &"bob".into(), None, "User")
            .unwrap();
        cache
            .set::<String>("post:1", &"hello".into(), None, "Post")
            .unwrap();

        assert_eq!(cache.len(), 3);

        cache.invalidate_model("User");

        assert_eq!(cache.len(), 1);
        assert!(!cache.contains("user:1"));
        assert!(!cache.contains("user:2"));
        assert!(cache.contains("post:1"));
    }
}

mod cache_key_prefix {
    use std::time::Duration;
    use tideorm::cache::{CacheConfig, CacheStrategy, QueryCache};

    #[test]
    fn test_generate_key_with_prefix() {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 100,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::LRU,
            cache_empty_results: true,
            key_prefix: Some("v7".into()),
        });

        let key = cache.generate_key("users", 12345);
        assert_eq!(key, "v7:users:12345");
    }

    #[test]
    fn test_generate_key_without_prefix() {
        let cache = QueryCache::new();
        let key = cache.generate_key("posts", 999);
        assert_eq!(key, "posts:999");
    }
}

mod cache_stats_accounting {
    use std::time::Duration;
    use tideorm::cache::{CacheConfig, CacheStrategy, QueryCache};

    #[test]
    fn test_stats_track_hits_misses_evictions() {
        let cache = QueryCache::with_config(CacheConfig {
            enabled: true,
            max_entries: 2,
            default_ttl: Duration::from_secs(60),
            strategy: CacheStrategy::LRU,
            cache_empty_results: true,
            key_prefix: None,
        });

        let _: Option<i32> = cache.get("nope");
        cache.set::<i32>("a", &1, None, "t").unwrap();
        let _: Option<i32> = cache.get("a");
        cache.set::<i32>("b", &2, None, "t").unwrap();
        cache.set::<i32>("c", &3, None, "t").unwrap();

        let stats = cache.stats();
        assert!(stats.misses >= 1);
        assert!(stats.hits >= 1);
        assert!(stats.evictions >= 1);
        assert!(stats.entries <= 2);
        let ratio = stats.hit_ratio();
        assert!((0.0..=1.0).contains(&ratio));
    }
}
