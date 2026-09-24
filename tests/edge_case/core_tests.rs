mod error_conversions {
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
    fn test_serde_error_converts_to_conversion() {
        let serde_err = serde_json::from_str::<serde_json::Value>("").unwrap_err();
        let expected = serde_err.to_string();
        let tide_err: Error = serde_err.into();
        match &tide_err {
            Error::Conversion { message } => assert_eq!(message, &expected),
            other => panic!("expected Conversion, got {:?}", other),
        }
        assert_eq!(tide_err.code(), "TIDE_CONVERSION");
        assert_eq!(tide_err.http_status(), 400);
    }
}

mod database_type_parsing {
    use tideorm::config::DatabaseType;

    #[test]
    fn test_default_database_type_is_postgres() {
        assert_eq!(DatabaseType::default(), DatabaseType::Postgres);
    }

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
