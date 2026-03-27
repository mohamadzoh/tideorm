// =============================================================================
// VALIDATION MODULE TESTS
// =============================================================================

#[cfg(test)]
mod validation_tests {
    use tideorm::validation::{ValidatableValue, ValidationErrors, ValidationRule, Validator};

    #[test]
    fn test_validation_rule_required() {
        let rule = ValidationRule::Required;
        assert!(rule.validate(&"hello".to_string()).is_ok());
        assert!(rule.validate(&"".to_string()).is_err());
        assert!(rule.validate(&"   ".to_string()).is_err());
    }

    #[test]
    fn test_validation_rule_email() {
        let rule = ValidationRule::Email;
        assert!(rule.validate(&"test@example.com".to_string()).is_ok());
        assert!(
            rule.validate(&"user.name+tag@domain.co.uk".to_string())
                .is_ok()
        );
        assert!(rule.validate(&"invalid".to_string()).is_err());
        assert!(rule.validate(&"@nodomain.com".to_string()).is_err());
        assert!(rule.validate(&"noat.com".to_string()).is_err());
    }

    #[test]
    fn test_validation_rule_url() {
        let rule = ValidationRule::Url;
        assert!(rule.validate(&"https://example.com".to_string()).is_ok());
        assert!(
            rule.validate(&"http://localhost:8080/path".to_string())
                .is_ok()
        );
        assert!(rule.validate(&"not-a-url".to_string()).is_err());
        assert!(rule.validate(&"example.com".to_string()).is_err());
    }

    #[test]
    fn test_validation_rule_min_length() {
        let rule = ValidationRule::MinLength(5);
        assert!(rule.validate(&"hello".to_string()).is_ok());
        assert!(rule.validate(&"hello world".to_string()).is_ok());
        assert!(rule.validate(&"hi".to_string()).is_err());
        assert!(rule.validate(&"".to_string()).is_err());
    }

    #[test]
    fn test_validation_rule_max_length() {
        let rule = ValidationRule::MaxLength(10);
        assert!(rule.validate(&"hello".to_string()).is_ok());
        assert!(rule.validate(&"".to_string()).is_ok());
        assert!(rule.validate(&"hello world!".to_string()).is_err());
    }

    #[test]
    fn test_validation_rule_min() {
        let rule = ValidationRule::Min(18.0);
        assert!(rule.validate(&"18".to_string()).is_ok());
        assert!(rule.validate(&"25".to_string()).is_ok());
        assert!(rule.validate(&"17".to_string()).is_err());
        assert!(rule.validate(&"0".to_string()).is_err());
    }

    #[test]
    fn test_validation_rule_max() {
        let rule = ValidationRule::Max(100.0);
        assert!(rule.validate(&"50".to_string()).is_ok());
        assert!(rule.validate(&"100".to_string()).is_ok());
        assert!(rule.validate(&"101".to_string()).is_err());
    }

    #[test]
    fn test_validation_rule_range() {
        let rule = ValidationRule::Range(1.0, 100.0);
        assert!(rule.validate(&"1".to_string()).is_ok());
        assert!(rule.validate(&"50".to_string()).is_ok());
        assert!(rule.validate(&"100".to_string()).is_ok());
        assert!(rule.validate(&"0".to_string()).is_err());
        assert!(rule.validate(&"101".to_string()).is_err());
    }

    #[test]
    fn test_validation_rule_regex() {
        let rule = ValidationRule::Regex(r"^\d{3}-\d{4}$".to_string());
        assert!(rule.validate(&"123-4567".to_string()).is_ok());
        assert!(rule.validate(&"1234567".to_string()).is_err());
        assert!(rule.validate(&"abc-defg".to_string()).is_err());
    }

    #[test]
    fn test_validation_rule_alpha() {
        let rule = ValidationRule::Alpha;
        assert!(rule.validate(&"hello".to_string()).is_ok());
        assert!(rule.validate(&"HelloWorld".to_string()).is_ok());
        assert!(rule.validate(&"hello123".to_string()).is_err());
        assert!(rule.validate(&"hello world".to_string()).is_err());
    }

    #[test]
    fn test_validation_rule_alphanumeric() {
        let rule = ValidationRule::Alphanumeric;
        assert!(rule.validate(&"hello123".to_string()).is_ok());
        assert!(rule.validate(&"ABC123".to_string()).is_ok());
        assert!(rule.validate(&"hello world".to_string()).is_err());
        assert!(rule.validate(&"hello-world".to_string()).is_err());
    }

    #[test]
    fn test_validation_rule_numeric() {
        let rule = ValidationRule::Numeric;
        assert!(rule.validate(&"12345".to_string()).is_ok());
        assert!(rule.validate(&"0".to_string()).is_ok());
        assert!(rule.validate(&"12.34".to_string()).is_ok()); // Decimals are valid numbers
        assert!(rule.validate(&"-123".to_string()).is_ok()); // Negative numbers are valid
        assert!(rule.validate(&"abc".to_string()).is_err());
        assert!(rule.validate(&"12abc".to_string()).is_err());
    }

    #[test]
    fn test_validation_rule_uuid() {
        let rule = ValidationRule::Uuid;
        assert!(
            rule.validate(&"550e8400-e29b-41d4-a716-446655440000".to_string())
                .is_ok()
        );
        assert!(
            rule.validate(&"550E8400-E29B-41D4-A716-446655440000".to_string())
                .is_ok()
        );
        assert!(rule.validate(&"not-a-uuid".to_string()).is_err());
        assert!(
            rule.validate(&"550e8400-e29b-41d4-a716".to_string())
                .is_err()
        );
    }

    #[test]
    fn test_validation_rule_in() {
        let rule = ValidationRule::In(vec![
            "red".to_string(),
            "green".to_string(),
            "blue".to_string(),
        ]);
        assert!(rule.validate(&"red".to_string()).is_ok());
        assert!(rule.validate(&"green".to_string()).is_ok());
        assert!(rule.validate(&"yellow".to_string()).is_err());
    }

    #[test]
    fn test_validation_rule_not_in() {
        let rule = ValidationRule::NotIn(vec!["admin".to_string(), "root".to_string()]);
        assert!(rule.validate(&"user".to_string()).is_ok());
        assert!(rule.validate(&"guest".to_string()).is_ok());
        assert!(rule.validate(&"admin".to_string()).is_err());
        assert!(rule.validate(&"root".to_string()).is_err());
    }

    #[test]
    fn test_validation_errors_collection() {
        let mut errors = ValidationErrors::new();
        assert!(errors.is_empty());

        errors.add("email", "Invalid email format");
        assert!(!errors.is_empty());
        assert!(errors.has_errors());

        errors.add("email", "Email already taken");
        errors.add("password", "Too short");

        let all_errors = errors.errors();
        assert_eq!(all_errors.len(), 3);
    }

    #[test]
    fn test_validation_errors_field_errors() {
        let mut errors = ValidationErrors::new();
        errors.add("email", "Invalid format");
        errors.add("email", "Already taken");
        errors.add("name", "Required");

        let email_errors = errors.field_errors("email");
        assert_eq!(email_errors.len(), 2);

        let name_errors = errors.field_errors("name");
        assert_eq!(name_errors.len(), 1);

        let missing_errors = errors.field_errors("missing");
        assert_eq!(missing_errors.len(), 0);
    }

    #[test]
    fn test_validation_errors_display() {
        let mut errors = ValidationErrors::new();
        errors.add("email", "Invalid email");
        errors.add("password", "Too short");

        let display = format!("{}", errors);
        assert!(
            display.contains("email")
                || display.contains("Invalid email")
                || display.contains("password")
                || display.contains("Too short")
        );
    }

    #[test]
    fn test_validator_validate_rule() {
        let rule = ValidationRule::Email;
        let result = Validator::validate_rule(&"test@example.com".to_string(), &rule, "email");
        assert!(result.is_none());

        let result = Validator::validate_rule(&"invalid".to_string(), &rule, "email");
        assert!(result.is_some());
    }

    #[test]
    fn test_validatable_value_string() {
        let value = "hello".to_string();
        assert!(!value.is_empty_value());
        assert_eq!(value.as_str_value(), Some("hello"));

        let empty = "".to_string();
        assert!(empty.is_empty_value());
    }

    #[test]
    fn test_validatable_value_option() {
        let some_value: Option<String> = Some("test".to_string());
        assert!(!some_value.is_empty_value());

        let none_value: Option<String> = None;
        assert!(none_value.is_empty_value());
    }

    #[test]
    fn test_validatable_value_numbers() {
        let int_val: i32 = 42;
        assert!(!int_val.is_empty_value());
        assert_eq!(int_val.as_f64_value(), Some(42.0));

        let float_val: f64 = 3.14;
        assert_eq!(float_val.as_f64_value(), Some(3.14));
    }

    #[test]
    fn test_validation_error_messages() {
        let rule = ValidationRule::MinLength(5);
        let result = rule.validate(&"hi".to_string());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("5"));
    }

    #[test]
    fn test_validation_errors_into_error() {
        let mut errors = ValidationErrors::new();
        errors.add("field1", "error1");
        errors.add("field2", "error2");

        let error: tideorm::error::Error = errors.into();
        assert!(error.is_validation_error());
    }
}

// =============================================================================
// QUERY MODULE TESTS
// =============================================================================

#[cfg(test)]
mod query_tests {
    use tideorm::query::{ConditionValue, Operator, Order, WhereCondition};

    #[test]
    fn test_order_as_str() {
        assert_eq!(Order::Asc.as_str(), "ASC");
        assert_eq!(Order::Desc.as_str(), "DESC");
    }

    #[test]
    fn test_order_clone_eq() {
        let order1 = Order::Asc;
        let order2 = order1;
        assert_eq!(order1, order2);
    }

    #[test]
    fn test_order_debug() {
        let debug = format!("{:?}", Order::Desc);
        assert_eq!(debug, "Desc");
    }

    #[test]
    fn test_operator_variants() {
        // Verify all operators exist and are constructible
        let operators = vec![
            Operator::Eq,
            Operator::NotEq,
            Operator::Gt,
            Operator::Gte,
            Operator::Lt,
            Operator::Lte,
            Operator::Like,
            Operator::NotLike,
            Operator::In,
            Operator::NotIn,
            Operator::IsNull,
            Operator::IsNotNull,
            Operator::Between,
        ];
        assert_eq!(operators.len(), 13);
    }

    #[test]
    fn test_condition_value_single() {
        let val = ConditionValue::Single(serde_json::json!("test"));
        if let ConditionValue::Single(v) = val {
            assert_eq!(v, serde_json::json!("test"));
        } else {
            panic!("Expected Single variant");
        }
    }

    #[test]
    fn test_condition_value_single_numbers() {
        let val_int = ConditionValue::Single(serde_json::json!(42));
        let val_float = ConditionValue::Single(serde_json::json!(3.14));
        let val_bool = ConditionValue::Single(serde_json::json!(true));

        if let ConditionValue::Single(v) = val_int {
            assert_eq!(v.as_i64(), Some(42));
        }
        if let ConditionValue::Single(v) = val_float {
            assert_eq!(v.as_f64(), Some(3.14));
        }
        if let ConditionValue::Single(v) = val_bool {
            assert_eq!(v.as_bool(), Some(true));
        }
    }

    #[test]
    fn test_condition_value_list() {
        let val = ConditionValue::List(vec![
            serde_json::json!("a"),
            serde_json::json!("b"),
            serde_json::json!("c"),
        ]);
        if let ConditionValue::List(v) = val {
            assert_eq!(v.len(), 3);
        } else {
            panic!("Expected List variant");
        }
    }

    #[test]
    fn test_condition_value_range() {
        let val = ConditionValue::Range(serde_json::json!(1), serde_json::json!(100));
        if let ConditionValue::Range(low, high) = val {
            assert_eq!(low.as_i64(), Some(1));
            assert_eq!(high.as_i64(), Some(100));
        } else {
            panic!("Expected Range variant");
        }
    }

    #[test]
    fn test_condition_value_none() {
        let val = ConditionValue::None;
        assert!(matches!(val, ConditionValue::None));
    }

    #[test]
    fn test_where_condition_struct() {
        let condition = WhereCondition {
            column: "status".to_string(),
            operator: Operator::Eq,
            value: ConditionValue::Single(serde_json::json!("active")),
        };

        assert_eq!(condition.column, "status");
        assert!(matches!(condition.operator, Operator::Eq));
    }

    #[test]
    fn test_where_condition_in_operator() {
        let condition = WhereCondition {
            column: "role".to_string(),
            operator: Operator::In,
            value: ConditionValue::List(vec![
                serde_json::json!("admin"),
                serde_json::json!("moderator"),
            ]),
        };

        assert_eq!(condition.column, "role");
        assert!(matches!(condition.operator, Operator::In));
    }

    #[test]
    fn test_where_condition_between_operator() {
        let condition = WhereCondition {
            column: "age".to_string(),
            operator: Operator::Between,
            value: ConditionValue::Range(serde_json::json!(18), serde_json::json!(65)),
        };

        assert_eq!(condition.column, "age");
        assert!(matches!(condition.operator, Operator::Between));
    }

    #[test]
    fn test_where_condition_is_null() {
        let condition = WhereCondition {
            column: "deleted_at".to_string(),
            operator: Operator::IsNull,
            value: ConditionValue::None,
        };

        assert!(matches!(condition.operator, Operator::IsNull));
        assert!(matches!(condition.value, ConditionValue::None));
    }
}

// =============================================================================
// SOFT DELETE TESTS
// =============================================================================

#[cfg(test)]
mod soft_delete_tests {
    // Note: SoftDelete trait requires Model, so we test the trait structure
    // using the public API types. Full soft delete tests are in integration tests.
    use chrono::Utc;

    #[test]
    fn test_deleted_at_timestamp() {
        // Test that deleted_at timestamps work as expected
        let now = Utc::now();
        let later = now + chrono::Duration::seconds(1);
        assert!(later > now);
    }

    #[test]
    fn test_optional_deleted_at() {
        // Soft delete uses Option<DateTime<Utc>>
        let deleted_at: Option<chrono::DateTime<Utc>> = None;
        assert!(deleted_at.is_none());

        let deleted_at: Option<chrono::DateTime<Utc>> = Some(Utc::now());
        assert!(deleted_at.is_some());
    }
}

// =============================================================================
// DATABASE TYPE CONVERSION TESTS
// =============================================================================

#[cfg(test)]
mod type_conversion_tests {
    use serde_json::json;

    #[test]
    fn test_json_value_conversions() {
        // Test that common Rust types convert to JSON properly
        let str_val = json!("hello");
        assert_eq!(str_val.as_str(), Some("hello"));

        let int_val = json!(42i64);
        assert_eq!(int_val.as_i64(), Some(42));

        let float_val = json!(3.14f64);
        assert_eq!(float_val.as_f64(), Some(3.14));

        let bool_val = json!(true);
        assert_eq!(bool_val.as_bool(), Some(true));

        let null_val = json!(null);
        assert!(null_val.is_null());

        let array_val = json!([1, 2, 3]);
        assert!(array_val.is_array());
        assert_eq!(array_val.as_array().unwrap().len(), 3);

        let object_val = json!({"key": "value"});
        assert!(object_val.is_object());
    }

    #[test]
    fn test_json_from_primitives() {
        // These are the conversions that happen in query builder
        let from_str: serde_json::Value = "test".into();
        assert_eq!(from_str, json!("test"));

        let from_string: serde_json::Value = String::from("test").into();
        assert_eq!(from_string, json!("test"));

        let from_i32: serde_json::Value = json!(42i32);
        assert_eq!(from_i32.as_i64(), Some(42));

        let from_i64: serde_json::Value = json!(42i64);
        assert_eq!(from_i64.as_i64(), Some(42));

        let from_bool: serde_json::Value = json!(true);
        assert_eq!(from_bool.as_bool(), Some(true));
    }
}

// =============================================================================
// JSON AND ARRAY TYPES TESTS
// =============================================================================

#[cfg(test)]
mod json_array_types_tests {
    use serde_json::json;
    use tideorm::types::*;

    #[test]
    fn test_json_type_casting() {
        let json_value = json!({"key": "value", "number": 42});

        // Test Json type
        let json_data: Json = json_value.clone();
        assert_eq!(json_data["key"], "value");
        assert_eq!(json_data["number"], 42);

        // Test Jsonb type (alias)
        let jsonb_data: Jsonb = json_value.clone();
        assert_eq!(jsonb_data["key"], "value");
        assert_eq!(jsonb_data["number"], 42);
    }

    #[test]
    fn test_array_types() {
        // Test IntArray
        let int_array: IntArray = vec![1, 2, 3, 4, 5];
        assert_eq!(int_array.len(), 5);
        assert_eq!(int_array[0], 1);

        // Test TextArray
        let text_array: TextArray = vec!["hello".to_string(), "world".to_string()];
        assert_eq!(text_array.len(), 2);
        assert_eq!(text_array[0], "hello");

        // Test BoolArray
        let bool_array: BoolArray = vec![true, false, true];
        assert_eq!(bool_array.len(), 3);
        assert!(bool_array[0]);

        // Test FloatArray
        let float_array: FloatArray = vec![1.1, 2.2, 3.3];
        assert_eq!(float_array.len(), 3);
        assert_eq!(float_array[0], 1.1);

        // Test JsonArray
        let json_array: JsonArray = vec![json!({"id": 1}), json!({"id": 2})];
        assert_eq!(json_array.len(), 2);
        assert_eq!(json_array[0]["id"], 1);
    }

    #[test]
    fn test_array_castable_implementation() {
        use tideorm::types::Castable;

        // Test Vec<String> casting
        let json_array = json!(["hello", "world"]);
        let result: Result<Vec<String>, String> = Castable::from_json(&json_array);
        assert!(result.is_ok());
        let vec = result.unwrap();
        assert_eq!(vec, vec!["hello".to_string(), "world".to_string()]);

        // Test Vec<i32> casting
        let json_int_array = json!([1, 2, 3]);
        let result: Result<Vec<i32>, String> = Castable::from_json(&json_int_array);
        assert!(result.is_ok());
        let vec = result.unwrap();
        assert_eq!(vec, vec![1, 2, 3]);
    }
}

// =============================================================================
// JOIN AND AGGREGATION TESTS
// =============================================================================

#[cfg(test)]
mod join_aggregation_tests {
    use tideorm::query::{AggregateFunction, JoinClause, JoinType};

    #[test]
    fn test_join_type_as_sql() {
        assert_eq!(JoinType::Inner.as_sql(), "INNER JOIN");
        assert_eq!(JoinType::Left.as_sql(), "LEFT JOIN");
        assert_eq!(JoinType::Right.as_sql(), "RIGHT JOIN");
    }

    #[test]
    fn test_join_type_clone_eq() {
        let jt1 = JoinType::Inner;
        let jt2 = jt1;
        assert_eq!(jt1, jt2);

        assert_ne!(JoinType::Inner, JoinType::Left);
        assert_ne!(JoinType::Left, JoinType::Right);
    }

    #[test]
    fn test_join_type_debug() {
        assert_eq!(format!("{:?}", JoinType::Inner), "Inner");
        assert_eq!(format!("{:?}", JoinType::Left), "Left");
        assert_eq!(format!("{:?}", JoinType::Right), "Right");
    }

    #[test]
    fn test_join_clause_creation() {
        let clause = JoinClause {
            join_type: JoinType::Inner,
            table: "users".to_string(),
            alias: None,
            left_column: "posts.user_id".to_string(),
            right_column: "users.id".to_string(),
        };

        assert_eq!(clause.join_type, JoinType::Inner);
        assert_eq!(clause.table, "users");
        assert!(clause.alias.is_none());
        assert_eq!(clause.left_column, "posts.user_id");
        assert_eq!(clause.right_column, "users.id");
    }

    #[test]
    fn test_join_clause_with_alias() {
        let clause = JoinClause {
            join_type: JoinType::Left,
            table: "users".to_string(),
            alias: Some("u".to_string()),
            left_column: "posts.user_id".to_string(),
            right_column: "u.id".to_string(),
        };

        assert_eq!(clause.alias, Some("u".to_string()));
    }

    #[test]
    fn test_join_clause_clone() {
        let clause = JoinClause {
            join_type: JoinType::Right,
            table: "comments".to_string(),
            alias: Some("c".to_string()),
            left_column: "posts.id".to_string(),
            right_column: "c.post_id".to_string(),
        };

        let cloned = clause.clone();
        assert_eq!(cloned.table, "comments");
        assert_eq!(cloned.alias, Some("c".to_string()));
    }

    #[test]
    fn test_aggregate_function_variants() {
        let agg_count = AggregateFunction::Count;
        let agg_count_distinct = AggregateFunction::CountDistinct("category".to_string());
        let agg_sum = AggregateFunction::Sum("price".to_string());
        let agg_avg = AggregateFunction::Avg("rating".to_string());
        let agg_min = AggregateFunction::Min("created_at".to_string());
        let agg_max = AggregateFunction::Max("updated_at".to_string());

        // Verify they exist and can be matched
        assert!(matches!(agg_count, AggregateFunction::Count));
        assert!(matches!(
            agg_count_distinct,
            AggregateFunction::CountDistinct(_)
        ));
        assert!(matches!(agg_sum, AggregateFunction::Sum(_)));
        assert!(matches!(agg_avg, AggregateFunction::Avg(_)));
        assert!(matches!(agg_min, AggregateFunction::Min(_)));
        assert!(matches!(agg_max, AggregateFunction::Max(_)));
    }

    #[test]
    fn test_aggregate_function_clone() {
        let agg = AggregateFunction::Sum("amount".to_string());
        let cloned = agg.clone();

        if let AggregateFunction::Sum(col) = cloned {
            assert_eq!(col, "amount");
        } else {
            panic!("Expected Sum variant");
        }
    }

    #[test]
    fn test_aggregate_function_debug() {
        let agg = AggregateFunction::Avg("score".to_string());
        let debug = format!("{:?}", agg);
        assert!(debug.contains("Avg"));
        assert!(debug.contains("score"));
    }
}
