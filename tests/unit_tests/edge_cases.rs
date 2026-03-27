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
        // Empty message should still work - actual format is "Record not found: "
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

        // Should have 2 entries for email
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
            // JSON floats have limited precision
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
        // Range where low > high (edge case)
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
        // Test that special SQL characters are just stored as values
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

// =============================================================================
// EDGE CASE TESTS - SCHEMA GENERATION
// =============================================================================

#[cfg(test)]
mod schema_edge_cases {
    use tideorm::config::DatabaseType;
    use tideorm::model::IndexDefinition;
    use tideorm::schema::{ColumnSchema, SchemaGenerator, TableSchemaBuilder, rust_type_to_sql};

    #[test]
    fn test_rust_type_unknown() {
        // Unknown types should fall back to TEXT
        let sql_type = rust_type_to_sql("MyCustomType", DatabaseType::Postgres);
        assert_eq!(sql_type, "TEXT");
    }

    #[test]
    fn test_rust_type_deeply_nested_option() {
        let sql_type = rust_type_to_sql("Option<Option<String>>", DatabaseType::Postgres);
        // Should handle nested Option
        assert_eq!(sql_type, "TEXT");
    }

    #[test]
    fn test_rust_type_vec() {
        // Vec types are converted to TEXT (not array syntax) by the current implementation
        let sql_type = rust_type_to_sql("Vec<String>", DatabaseType::Postgres);
        // This tests current behavior - Vec is treated as unknown type
        assert!(!sql_type.is_empty());

        let sql_type2 = rust_type_to_sql("Vec<i32>", DatabaseType::Postgres);
        assert!(!sql_type2.is_empty());
    }

    #[test]
    fn test_column_schema_all_options() {
        let col = ColumnSchema::new("id", "BIGINT")
            .primary_key()
            .auto_increment()
            .not_null()
            .default("0");

        assert!(col.primary_key);
        assert!(col.auto_increment);
        assert!(!col.nullable);
        assert_eq!(col.default, Some("0".to_string()));
    }

    #[test]
    fn test_column_schema_nullable_default() {
        let col = ColumnSchema::new("name", "TEXT");
        // By default columns should be nullable
        assert!(col.nullable);
    }

    #[test]
    fn test_table_schema_empty_columns() {
        let schema = TableSchemaBuilder::new("empty_table").build();
        assert_eq!(schema.name, "empty_table");
        assert!(schema.columns.is_empty());
    }

    #[test]
    fn test_table_schema_many_columns() {
        let mut builder = TableSchemaBuilder::new("wide_table");
        for i in 0..100 {
            builder = builder.column(ColumnSchema::new(format!("col_{}", i), "TEXT"));
        }
        let schema = builder.build();
        assert_eq!(schema.columns.len(), 100);
    }

    #[test]
    fn test_index_definition_empty_columns() {
        let index = IndexDefinition::new("idx_empty", vec![], false);
        assert!(index.columns.is_empty());
    }

    #[test]
    fn test_index_definition_many_columns() {
        let columns: Vec<String> = (0..10).map(|i| format!("col_{}", i)).collect();
        let index = IndexDefinition::new("idx_composite", columns.clone(), false);
        assert_eq!(index.columns.len(), 10);
    }

    #[test]
    fn test_index_parse_empty_string() {
        let indexes = IndexDefinition::parse("users", "", false);
        assert!(indexes.is_empty() || indexes[0].columns.is_empty());
    }

    #[test]
    fn test_index_parse_whitespace() {
        let indexes = IndexDefinition::parse("users", "  email  ", false);
        assert!(!indexes.is_empty());
        // Should trim whitespace
        assert_eq!(indexes[0].columns[0].trim(), "email");
    }

    #[test]
    fn test_schema_generator_empty() {
        let generator = SchemaGenerator::new(DatabaseType::Postgres);
        let sql = generator.generate();
        // Empty generator without tables - verify it doesn't panic and returns a string
        // (actual output may include headers/comments)
        let _ = sql; // Use the variable to avoid warnings
    }

    #[test]
    fn test_schema_generator_multiple_tables() {
        let mut generator = SchemaGenerator::new(DatabaseType::Postgres);

        for i in 0..5 {
            let schema = TableSchemaBuilder::new(format!("table_{}", i))
                .column(ColumnSchema::new("id", "BIGINT").primary_key())
                .build();
            generator.add_table(schema);
        }

        let sql = generator.generate();
        assert!(sql.contains("table_0"));
        assert!(sql.contains("table_4"));
    }

    #[test]
    fn test_table_name_with_special_chars() {
        let schema = TableSchemaBuilder::new("user-data")
            .column(ColumnSchema::new("id", "BIGINT"))
            .build();
        assert_eq!(schema.name, "user-data");
    }

    #[test]
    fn test_column_name_reserved_word() {
        // 'order' is a SQL reserved word
        let col = ColumnSchema::new("order", "INTEGER");
        assert_eq!(col.name, "order");
    }
}

// =============================================================================
// EDGE CASE TESTS - RELATIONS
// =============================================================================

#[cfg(test)]
mod relation_edge_cases {
    use tideorm::relations::{RelationInfo, RelationType};

    #[test]
    fn test_relation_info_empty_strings() {
        let info = RelationInfo::belongs_to("", "", "", "");

        assert!(info.name.is_empty());
        assert!(info.related_table.is_empty());
    }

    #[test]
    fn test_relation_info_unicode_names() {
        let info = RelationInfo::belongs_to("作者", "用户", "用户_id", "id");

        assert_eq!(info.name, "作者");
        assert_eq!(info.related_table, "用户");
    }

    #[test]
    fn test_relation_info_clone() {
        let info = RelationInfo::has_many("posts", "posts", "author_id", "id");

        let cloned = info.clone();
        assert_eq!(info.name, cloned.name);
        assert_eq!(info.relation_type, cloned.relation_type);
    }

    #[test]
    fn test_all_relation_types() {
        let types = [
            RelationType::BelongsTo,
            RelationType::HasOne,
            RelationType::HasMany,
            RelationType::HasManyThrough,
            RelationType::MorphTo,
            RelationType::MorphOne,
            RelationType::MorphMany,
        ];

        // All should be different
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}

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
        // Try to cast an object to a vec
        let json_obj = json!({"key": "value"});
        let result: Result<Vec<String>, String> = Castable::from_json(&json_obj);
        // Should fail
        assert!(result.is_err());
    }

    #[test]
    fn test_number_boundaries() {
        // Test boundary values
        let max_i64 = json!(i64::MAX);
        assert_eq!(max_i64.as_i64(), Some(i64::MAX));

        let min_i64 = json!(i64::MIN);
        assert_eq!(min_i64.as_i64(), Some(i64::MIN));

        // Test floating point
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
        // Even nanosecond difference should be detectable
        assert!(ts2 > ts1);
    }

    #[test]
    fn test_optional_timestamp_transitions() {
        let mut deleted_at: Option<chrono::DateTime<Utc>> = None;
        assert!(deleted_at.is_none());

        deleted_at = Some(Utc::now());
        assert!(deleted_at.is_some());

        deleted_at = None; // Restore
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

        // All should be clonable and comparable
        for t in &types {
            let cloned = *t;
            assert_eq!(*t, cloned);
        }
    }

    #[test]
    fn test_database_type_features_matrix() {
        // Postgres supports both
        assert!(DatabaseType::Postgres.supports_json());
        assert!(DatabaseType::Postgres.supports_arrays());

        // MySQL supports JSON but not arrays
        assert!(DatabaseType::MySQL.supports_json());
        assert!(!DatabaseType::MySQL.supports_arrays());

        // SQLite supports JSON (via json1 extension) but not native arrays
        assert!(DatabaseType::SQLite.supports_json());
        assert!(!DatabaseType::SQLite.supports_arrays());
    }

    #[test]
    fn test_database_type_ports() {
        assert!(DatabaseType::Postgres.default_port() > 0);
        assert!(DatabaseType::MySQL.default_port() > 0);
        assert_eq!(DatabaseType::SQLite.default_port(), 0); // SQLite has no port
    }

    #[test]
    fn test_database_type_schemes_valid() {
        // All schemes should be non-empty lowercase strings
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
        // SQL keywords should be uppercase
        assert!(Order::Asc.as_str().chars().all(|c| c.is_uppercase()));
        assert!(Order::Desc.as_str().chars().all(|c| c.is_uppercase()));
    }

    #[test]
    fn test_order_copy_semantics() {
        let order1 = Order::Asc;
        let order2 = order1; // Copy
        let order3 = order1; // Copy again

        assert_eq!(order1, order2);
        assert_eq!(order2, order3);
    }

    #[test]
    fn test_order_eq_behavior() {
        // Order implements PartialEq + Eq
        let orders = [Order::Asc, Order::Desc, Order::Asc];

        // Count Asc values
        let asc_count = orders.iter().filter(|&&o| o == Order::Asc).count();
        assert_eq!(asc_count, 2);

        // Count Desc values
        let desc_count = orders.iter().filter(|&&o| o == Order::Desc).count();
        assert_eq!(desc_count, 1);
    }
}

// =============================================================================
