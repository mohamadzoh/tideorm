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

    /// The scheme alone decides, in any case and with no host; anything else
    /// is unknown.
    #[test]
    fn test_from_url_reads_the_scheme_only() {
        for (url, expected) in [
            ("POSTGRES://host", Some(DatabaseType::Postgres)),
            ("MySQL://host", Some(DatabaseType::MySQL)),
            ("MARIADB://host", Some(DatabaseType::MariaDB)),
            ("SQLite:./db.sqlite", Some(DatabaseType::SQLite)),
            ("postgres://", Some(DatabaseType::Postgres)),
            ("mysql://", Some(DatabaseType::MySQL)),
            ("", None),
            ("://", None),
            ("oracle://host/db", None),
            ("mssql://host/db", None),
            ("mongodb://host/db", None),
        ] {
            assert_eq!(DatabaseType::from_url(url), expected, "{url:?}");
        }
    }
}
