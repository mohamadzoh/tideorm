// =============================================================================
// EDGE CASE TESTS - TYPE CONVERSIONS
// =============================================================================

#[cfg(test)]
mod type_conversion_edge_cases {
    use serde_json::json;
    use tideorm::types::Castable;

    #[test]
    fn test_json_empty_object() {
        let val = json!({});
        assert!(val.is_object());
        assert!(val.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_json_empty_array() {
        let val = json!([]);
        assert!(val.is_array());
        assert!(val.as_array().unwrap().is_empty());
    }

    #[test]
    fn test_json_deeply_nested() {
        let val = json!({
            "a": {"b": {"c": {"d": {"e": "deep"}}}}
        });
        assert_eq!(val["a"]["b"]["c"]["d"]["e"], "deep");
    }

    #[test]
    fn test_json_large_array() {
        let arr: Vec<i32> = (0..1000).collect();
        let val = json!(arr);
        assert_eq!(val.as_array().unwrap().len(), 1000);
    }

    #[test]
    fn test_json_special_strings() {
        let special_strings = vec![
            "", " ", "\n\t\r", "null", "true", "false", "123", "[]", "{}",
        ];

        for s in special_strings {
            let val = json!(s);
            assert!(val.is_string());
            assert_eq!(val.as_str().unwrap(), s);
        }
    }

    #[test]
    fn test_castable_empty_array() {
        let json_array = json!([]);
        let result: Result<Vec<String>, String> = Castable::from_json(&json_array);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_castable_invalid_type() {
        let json_obj = json!({"key": "value"});
        let result: Result<Vec<String>, String> = Castable::from_json(&json_obj);
        assert!(result.is_err());
    }

    #[test]
    fn test_number_boundaries() {
        let max_i64 = json!(i64::MAX);
        assert_eq!(max_i64.as_i64(), Some(i64::MAX));

        let min_i64 = json!(i64::MIN);
        assert_eq!(min_i64.as_i64(), Some(i64::MIN));

        let infinity_test = json!(f64::MAX);
        assert!(infinity_test.as_f64().is_some());
    }
}

// =============================================================================
// EDGE CASE TESTS - SOFT DELETE
// =============================================================================

#[cfg(test)]
mod soft_delete_edge_cases {
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn test_deleted_at_far_future() {
        let far_future = Utc.with_ymd_and_hms(2100, 12, 31, 23, 59, 59).unwrap();
        let now = Utc::now();
        assert!(far_future > now);
    }

    #[test]
    fn test_deleted_at_far_past() {
        let far_past = Utc.with_ymd_and_hms(1970, 1, 1, 0, 0, 0).unwrap();
        let now = Utc::now();
        assert!(far_past < now);
    }

    #[test]
    fn test_deleted_at_nanosecond_precision() {
        let ts1 = Utc::now();
        let ts2 = ts1 + Duration::nanoseconds(1);
        assert!(ts2 > ts1);
    }

    #[test]
    fn test_optional_timestamp_transitions() {
        let mut deleted_at: Option<chrono::DateTime<Utc>> = None;
        assert!(deleted_at.is_none());

        deleted_at = Some(Utc::now());
        assert!(deleted_at.is_some());

        deleted_at = None;
        assert!(deleted_at.is_none());
    }
}

// =============================================================================
// EDGE CASE TESTS - CONFIG
// =============================================================================

#[cfg(test)]
mod config_edge_cases {
    use tideorm::config::DatabaseType;

    #[test]
    fn test_database_type_all_variants() {
        let types = [
            DatabaseType::Postgres,
            DatabaseType::MySQL,
            DatabaseType::SQLite,
        ];

        for t in &types {
            let cloned = *t;
            assert_eq!(*t, cloned);
        }
    }

    #[test]
    fn test_database_type_features_matrix() {
        assert!(DatabaseType::Postgres.supports_json());
        assert!(DatabaseType::Postgres.supports_arrays());

        assert!(DatabaseType::MySQL.supports_json());
        assert!(!DatabaseType::MySQL.supports_arrays());

        assert!(DatabaseType::SQLite.supports_json());
        assert!(!DatabaseType::SQLite.supports_arrays());
    }

    #[test]
    fn test_database_type_ports() {
        assert!(DatabaseType::Postgres.default_port() > 0);
        assert!(DatabaseType::MySQL.default_port() > 0);
        assert_eq!(DatabaseType::SQLite.default_port(), 0);
    }

    #[test]
    fn test_database_type_schemes_valid() {
        for db_type in [
            DatabaseType::Postgres,
            DatabaseType::MySQL,
            DatabaseType::SQLite,
        ] {
            let scheme = db_type.url_scheme();
            assert!(!scheme.is_empty());
            assert_eq!(scheme, scheme.to_lowercase());
        }
    }
}

// =============================================================================
// EDGE CASE TESTS - CALLBACKS
// =============================================================================

#[cfg(test)]
mod callback_edge_cases {
    use tideorm::callbacks::{CallbackRunner, Callbacks};
    use tideorm::validation::{Validate, ValidationErrors};

    struct TestType;

    impl Callbacks for TestType {}

    impl Validate for TestType {
        fn validate(&self) -> std::result::Result<(), ValidationErrors> {
            Ok(())
        }
    }

    #[test]
    fn test_multiple_callback_invocations() {
        let mut model = TestType;

        for _ in 0..100 {
            assert!(model.before_save().is_ok());
            assert!(model.after_save().is_ok());
        }
    }

    #[test]
    fn test_callback_runner_chain() {
        let mut model = TestType;

        assert!(model.run_create_callbacks().is_ok());
        assert!(model.run_after_create_callbacks().is_ok());

        assert!(model.run_update_callbacks().is_ok());
        assert!(model.run_after_update_callbacks().is_ok());

        assert!(model.run_delete_callbacks().is_ok());
        assert!(model.run_after_delete_callbacks().is_ok());
    }

    #[test]
    fn test_all_callback_methods_exist() {
        let mut model = TestType;

        let _ = model.before_validation();
        let _ = model.after_validation();
        let _ = model.before_save();
        let _ = model.after_save();
        let _ = model.before_create();
        let _ = model.after_create();
        let _ = model.before_update();
        let _ = model.after_update();
        let _ = model.before_delete();
        let _ = model.after_delete();
    }
}

// =============================================================================
// EDGE CASE TESTS - ORDER OPERATIONS
// =============================================================================

#[cfg(test)]
mod order_edge_cases {
    use tideorm::query::Order;

    #[test]
    fn test_order_as_str_uppercase() {
        assert!(Order::Asc.as_str().chars().all(|c| c.is_uppercase()));
        assert!(Order::Desc.as_str().chars().all(|c| c.is_uppercase()));
    }

    #[test]
    fn test_order_copy_semantics() {
        let order1 = Order::Asc;
        let order2 = order1;
        let order3 = order1;

        assert_eq!(order1, order2);
        assert_eq!(order2, order3);
    }

    #[test]
    fn test_order_eq_behavior() {
        let orders = [Order::Asc, Order::Desc, Order::Asc];

        let asc_count = orders.iter().filter(|&&o| o == Order::Asc).count();
        assert_eq!(asc_count, 2);

        let desc_count = orders.iter().filter(|&&o| o == Order::Desc).count();
        assert_eq!(desc_count, 1);
    }
}
