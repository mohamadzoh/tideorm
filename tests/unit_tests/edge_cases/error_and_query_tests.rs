// =============================================================================
// EDGE CASE TESTS - ERROR HANDLING
// =============================================================================

#[cfg(test)]
mod error_edge_cases {
    use tideorm::error::{Error, ErrorContext};
    use tideorm::validation::ValidationErrors;

    #[test]
    fn test_error_empty_message() {
        let err = Error::not_found("");
        assert!(err.is_not_found());
        assert_eq!(err.to_string(), "Record not found: ");
    }

    #[test]
    fn test_error_message_with_special_chars() {
        let err = Error::query("Error: 'invalid' \"syntax\" <tag> & more");
        assert!(err.to_string().contains("'invalid'"));
        assert!(err.to_string().contains("\"syntax\""));
        assert!(err.to_string().contains("<tag>"));
    }

    #[test]
    fn test_error_message_with_newlines() {
        let err = Error::query("Line 1\nLine 2\nLine 3");
        let msg = err.to_string();
        assert!(msg.contains("Line 1"));
        assert!(msg.contains("Line 2"));
    }

    #[test]
    fn test_error_message_with_unicode() {
        let err = Error::not_found("用户未找到 🔍");
        assert!(err.to_string().contains("用户未找到"));
        assert!(err.to_string().contains("🔍"));
    }

    #[test]
    fn test_error_context_empty() {
        let ctx = ErrorContext::new();
        assert!(ctx.table.is_none());
        assert!(ctx.column.is_none());
        assert!(ctx.query.is_none());
    }

    #[test]
    fn test_error_context_partial() {
        let ctx = ErrorContext::new().table("users");
        assert_eq!(ctx.table, Some("users".to_string()));
        assert!(ctx.column.is_none());
        assert!(ctx.query.is_none());
    }

    #[test]
    fn test_error_context_all_fields() {
        let ctx = ErrorContext::new()
            .table("users")
            .column("email")
            .query("SELECT * FROM users WHERE id = $1");

        assert!(ctx.table.is_some());
        assert!(ctx.column.is_some());
        assert!(ctx.query.is_some());
    }

    #[test]
    fn test_validation_errors_multiple_same_field() {
        let mut errors = ValidationErrors::new();
        errors.add("email", "Invalid format");
        errors.add("email", "Already taken");

        let all_errors = errors.errors();
        let email_errors: Vec<_> = all_errors
            .iter()
            .filter(|(field, _)| *field == "email")
            .collect();
        assert_eq!(email_errors.len(), 2);
    }

    #[test]
    fn test_validation_errors_empty_field_name() {
        let mut errors = ValidationErrors::new();
        errors.add("", "Some error");

        assert!(!errors.is_empty());
    }

    #[test]
    fn test_validation_errors_empty_message() {
        let mut errors = ValidationErrors::new();
        errors.add("field", "");

        assert!(!errors.is_empty());
    }

    #[test]
    fn test_validation_errors_unicode_field() {
        let mut errors = ValidationErrors::new();
        errors.add("用户名", "必填字段");

        assert!(!errors.is_empty());
        let display = format!("{}", errors);
        assert!(display.contains("用户名"));
    }
}

// =============================================================================
// EDGE CASE TESTS - QUERY CONDITIONS
// =============================================================================

#[cfg(test)]
mod query_edge_cases {
    use serde_json::json;
    use tideorm::query::{ConditionValue, Operator, WhereCondition};

    #[test]
    fn test_condition_value_empty_list() {
        let val = ConditionValue::List(vec![]);
        if let ConditionValue::List(v) = val {
            assert!(v.is_empty());
        }
    }

    #[test]
    fn test_condition_value_single_null() {
        let val = ConditionValue::Single(json!(null));
        if let ConditionValue::Single(v) = val {
            assert!(v.is_null());
        }
    }

    #[test]
    fn test_condition_value_nested_json() {
        let nested = json!({
            "level1": {
                "level2": {
                    "level3": [1, 2, 3]
                }
            }
        });
        let val = ConditionValue::Single(nested.clone());
        if let ConditionValue::Single(v) = val {
            assert!(v["level1"]["level2"]["level3"].is_array());
        }
    }

    #[test]
    fn test_condition_value_large_number() {
        let large_num = json!(i64::MAX);
        let val = ConditionValue::Single(large_num);
        if let ConditionValue::Single(v) = val {
            assert_eq!(v.as_i64(), Some(i64::MAX));
        }
    }

    #[test]
    fn test_condition_value_negative_number() {
        let neg = json!(-999999);
        let val = ConditionValue::Single(neg);
        if let ConditionValue::Single(v) = val {
            assert_eq!(v.as_i64(), Some(-999999));
        }
    }

    #[test]
    fn test_condition_value_float_precision() {
        let precise = json!(0.123456789012345);
        let val = ConditionValue::Single(precise);
        if let ConditionValue::Single(v) = val {
            assert!(v.as_f64().is_some());
        }
    }

    #[test]
    fn test_condition_value_range_same_values() {
        let val = ConditionValue::Range(json!(5), json!(5));
        if let ConditionValue::Range(low, high) = val {
            assert_eq!(low, high);
        }
    }

    #[test]
    fn test_condition_value_range_reversed() {
        let val = ConditionValue::Range(json!(100), json!(1));
        if let ConditionValue::Range(low, high) = val {
            assert!(low.as_i64().unwrap() > high.as_i64().unwrap());
        }
    }

    #[test]
    fn test_where_condition_empty_column() {
        let condition = WhereCondition {
            column: "".to_string(),
            operator: Operator::Eq,
            value: ConditionValue::Single(json!("test")),
        };
        assert!(condition.column.is_empty());
    }

    #[test]
    fn test_where_condition_column_with_table() {
        let condition = WhereCondition {
            column: "users.email".to_string(),
            operator: Operator::Eq,
            value: ConditionValue::Single(json!("test@example.com")),
        };
        assert!(condition.column.contains('.'));
    }

    #[test]
    fn test_where_condition_column_with_special_chars() {
        let condition = WhereCondition {
            column: "user-name".to_string(),
            operator: Operator::Eq,
            value: ConditionValue::Single(json!("test")),
        };
        assert!(condition.column.contains('-'));
    }

    #[test]
    fn test_json_operators_exist() {
        let operators = [
            Operator::JsonContains,
            Operator::JsonContainedBy,
            Operator::JsonKeyExists,
            Operator::JsonKeyNotExists,
            Operator::JsonPathExists,
            Operator::JsonPathNotExists,
        ];
        assert_eq!(operators.len(), 6);
    }

    #[test]
    fn test_array_operators_exist() {
        let operators = [
            Operator::ArrayContains,
            Operator::ArrayContainedBy,
            Operator::ArrayOverlaps,
            Operator::ArrayContainsAny,
            Operator::ArrayContainsAll,
        ];
        assert_eq!(operators.len(), 5);
    }

    #[test]
    fn test_condition_with_sql_injection_attempt() {
        let condition = WhereCondition {
            column: "name".to_string(),
            operator: Operator::Eq,
            value: ConditionValue::Single(json!("'; DROP TABLE users; --")),
        };

        if let ConditionValue::Single(v) = &condition.value {
            assert_eq!(v.as_str().unwrap(), "'; DROP TABLE users; --");
        }
    }

    #[test]
    fn test_list_with_mixed_types() {
        let val = ConditionValue::List(vec![json!("string"), json!(42), json!(true), json!(null)]);
        if let ConditionValue::List(v) = val {
            assert_eq!(v.len(), 4);
            assert!(v[0].is_string());
            assert!(v[1].is_number());
            assert!(v[2].is_boolean());
            assert!(v[3].is_null());
        }
    }
}
