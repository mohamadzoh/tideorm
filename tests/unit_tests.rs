//! Comprehensive Unit Tests for TideORM
//!
//! These tests verify core functionality without requiring a database connection.
//! Run with: `cargo test --test unit_tests`

// =============================================================================
// ERROR MODULE TESTS
// =============================================================================

#[cfg(test)]
mod error_tests {
    use tideorm::error::{Error, ErrorContext, ValidationErrors};
    
    #[test]
    fn test_error_not_found_creation() {
        let err = Error::not_found("User with id 123 not found");
        assert!(err.is_not_found());
        assert!(!err.is_connection_error());
        assert!(err.to_string().contains("User with id 123"));
    }
    
    #[test]
    fn test_error_not_found_with_context() {
        let ctx = ErrorContext::new()
            .table("users")
            .column("id");
        let err = Error::not_found_with_context("Not found", ctx);
        
        assert!(err.is_not_found());
        let context = err.context().unwrap();
        assert_eq!(context.table.as_ref().unwrap(), "users");
        assert_eq!(context.column.as_ref().unwrap(), "id");
    }
    
    #[test]
    fn test_error_connection() {
        let err = Error::connection("Failed to connect to database");
        assert!(err.is_connection_error());
        assert!(!err.is_not_found());
        assert!(err.to_string().contains("Failed to connect"));
    }
    
    #[test]
    fn test_error_query() {
        let err = Error::query("Invalid SQL syntax");
        assert!(err.is_query_error());
        assert!(err.to_string().contains("Invalid SQL syntax"));
    }
    
    #[test]
    fn test_error_query_with_context() {
        let ctx = ErrorContext::new()
            .table("users")
            .query("SELECT * FROM users WHERE invalid");
        let err = Error::query_with_context("Syntax error", ctx);
        
        assert!(err.is_query_error());
        let context = err.context().unwrap();
        assert_eq!(context.table.as_ref().unwrap(), "users");
        assert!(context.query.as_ref().unwrap().contains("SELECT"));
    }
    
    #[test]
    fn test_error_validation() {
        let err = Error::validation("email", "Invalid email format");
        assert!(err.is_validation_error());
        assert!(err.to_string().contains("email"));
        assert!(err.to_string().contains("Invalid email format"));
    }
    
    #[test]
    fn test_error_conversion() {
        let err = Error::conversion("Cannot convert string to integer");
        assert!(!err.is_validation_error());
        assert!(err.to_string().contains("Cannot convert"));
    }
    
    #[test]
    fn test_error_transaction() {
        let err = Error::transaction("Transaction rolled back due to constraint violation");
        assert!(err.to_string().contains("rolled back"));
    }
    
    #[test]
    fn test_error_configuration() {
        let err = Error::configuration("Missing database URL");
        assert!(err.to_string().contains("Missing database URL"));
    }
    
    #[test]
    fn test_error_internal() {
        let err = Error::internal("Unexpected state");
        assert!(err.to_string().contains("Unexpected state"));
    }
    
    #[test]
    fn test_error_with_context() {
        let err = Error::query("Some query error");
        let ctx = ErrorContext::new().table("posts");
        let err_with_ctx = err.with_context(ctx);
        
        assert!(err_with_ctx.context().is_some());
        assert_eq!(err_with_ctx.context().unwrap().table.as_ref().unwrap(), "posts");
    }
    
    #[test]
    fn test_error_context_builder() {
        let ctx = ErrorContext::new()
            .table("users")
            .column("email")
            .query("INSERT INTO users...");
        
        assert_eq!(ctx.table.unwrap(), "users");
        assert_eq!(ctx.column.unwrap(), "email");
        assert!(ctx.query.unwrap().contains("INSERT"));
    }
    
    #[test]
    fn test_error_context_display() {
        let ctx = ErrorContext::new()
            .table("users")
            .column("id");
        let display = format!("{}", ctx);
        assert!(display.contains("table: users"));
        assert!(display.contains("column: id"));
    }
    
    #[test]
    fn test_validation_errors_empty() {
        let errors = ValidationErrors::new();
        assert!(errors.is_empty());
        assert!(errors.into_error().is_none());
    }
    
    #[test]
    fn test_validation_errors_add() {
        let mut errors = ValidationErrors::new();
        errors.add("email", "Invalid format");
        errors.add("name", "Too short");
        
        assert!(!errors.is_empty());
        assert_eq!(errors.errors().len(), 2);
    }
    
    #[test]
    fn test_validation_errors_into_error() {
        let mut errors = ValidationErrors::new();
        errors.add("email", "Invalid format");
        errors.add("name", "Too short");
        
        let err = errors.into_error().unwrap();
        assert!(err.is_validation_error());
        // Takes the first error
        assert!(err.to_string().contains("email"));
    }
    
    #[test]
    fn test_validation_errors_display() {
        let mut errors = ValidationErrors::new();
        errors.add("email", "Invalid");
        errors.add("name", "Required");
        
        let display = format!("{}", errors);
        assert!(display.contains("email: Invalid"));
        assert!(display.contains("name: Required"));
    }
}

// =============================================================================
// CONFIG MODULE TESTS
// =============================================================================

#[cfg(test)]
mod config_tests {
    use tideorm::config::DatabaseType;
    
    #[test]
    fn test_database_type_default_port() {
        assert_eq!(DatabaseType::Postgres.default_port(), 5432);
        assert_eq!(DatabaseType::MySQL.default_port(), 3306);
        assert_eq!(DatabaseType::SQLite.default_port(), 0);
    }
    
    #[test]
    fn test_database_type_url_scheme() {
        assert_eq!(DatabaseType::Postgres.url_scheme(), "postgres");
        assert_eq!(DatabaseType::MySQL.url_scheme(), "mysql");
        assert_eq!(DatabaseType::SQLite.url_scheme(), "sqlite");
    }
    
    #[test]
    fn test_database_type_supports_json() {
        assert!(DatabaseType::Postgres.supports_json());
        assert!(DatabaseType::MySQL.supports_json());
        assert!(!DatabaseType::SQLite.supports_json());
    }
    
    #[test]
    fn test_database_type_supports_arrays() {
        assert!(DatabaseType::Postgres.supports_arrays());
        assert!(!DatabaseType::MySQL.supports_arrays());
        assert!(!DatabaseType::SQLite.supports_arrays());
    }
    
    #[test]
    fn test_database_type_default() {
        let db_type = DatabaseType::default();
        assert_eq!(db_type, DatabaseType::Postgres);
    }
    
    #[test]
    fn test_database_type_clone() {
        let db_type = DatabaseType::MySQL;
        let cloned = db_type.clone();
        assert_eq!(db_type, cloned);
    }
    
    #[test]
    fn test_database_type_debug() {
        let db_type = DatabaseType::SQLite;
        let debug = format!("{:?}", db_type);
        assert_eq!(debug, "SQLite");
    }
}

// =============================================================================
// SCHEMA MODULE TESTS
// =============================================================================

#[cfg(test)]
mod schema_tests {
    use tideorm::config::DatabaseType;
    use tideorm::schema::{
        rust_type_to_sql, SchemaGenerator, TableSchemaBuilder, ColumnSchema,
    };
    use tideorm::model::IndexDefinition;
    
    #[test]
    fn test_rust_type_to_sql_postgres() {
        assert_eq!(rust_type_to_sql("i64", DatabaseType::Postgres), "BIGINT");
        assert_eq!(rust_type_to_sql("i32", DatabaseType::Postgres), "INTEGER");
        assert_eq!(rust_type_to_sql("String", DatabaseType::Postgres), "TEXT");
        assert_eq!(rust_type_to_sql("bool", DatabaseType::Postgres), "BOOLEAN");
        assert_eq!(rust_type_to_sql("f64", DatabaseType::Postgres), "DOUBLE PRECISION");
        assert_eq!(rust_type_to_sql("Uuid", DatabaseType::Postgres), "UUID");
        assert_eq!(rust_type_to_sql("DateTime", DatabaseType::Postgres), "TIMESTAMP");
        assert_eq!(rust_type_to_sql("NaiveDateTime", DatabaseType::Postgres), "TIMESTAMP");
        assert_eq!(rust_type_to_sql("Value", DatabaseType::Postgres), "JSONB");
    }
    
    #[test]
    fn test_rust_type_to_sql_mysql() {
        assert_eq!(rust_type_to_sql("i64", DatabaseType::MySQL), "BIGINT");
        assert_eq!(rust_type_to_sql("i32", DatabaseType::MySQL), "INT");
        assert_eq!(rust_type_to_sql("u32", DatabaseType::MySQL), "INT UNSIGNED");
        assert_eq!(rust_type_to_sql("String", DatabaseType::MySQL), "TEXT");
        assert_eq!(rust_type_to_sql("bool", DatabaseType::MySQL), "TINYINT(1)");
        assert_eq!(rust_type_to_sql("Uuid", DatabaseType::MySQL), "CHAR(36)");
        assert_eq!(rust_type_to_sql("DateTime", DatabaseType::MySQL), "DATETIME");
        assert_eq!(rust_type_to_sql("Value", DatabaseType::MySQL), "JSON");
    }
    
    #[test]
    fn test_rust_type_to_sql_sqlite() {
        assert_eq!(rust_type_to_sql("i64", DatabaseType::SQLite), "INTEGER");
        assert_eq!(rust_type_to_sql("i32", DatabaseType::SQLite), "INTEGER");
        assert_eq!(rust_type_to_sql("String", DatabaseType::SQLite), "TEXT");
        assert_eq!(rust_type_to_sql("bool", DatabaseType::SQLite), "INTEGER");
        assert_eq!(rust_type_to_sql("f64", DatabaseType::SQLite), "REAL");
        assert_eq!(rust_type_to_sql("Uuid", DatabaseType::SQLite), "TEXT");
        assert_eq!(rust_type_to_sql("DateTime", DatabaseType::SQLite), "TEXT");
        assert_eq!(rust_type_to_sql("serde_json::Value", DatabaseType::SQLite), "TEXT");
    }
    
    #[test]
    fn test_rust_type_to_sql_optional() {
        // Option<T> should be handled by stripping Option<>
        assert_eq!(rust_type_to_sql("Option<String>", DatabaseType::Postgres), "TEXT");
        assert_eq!(rust_type_to_sql("Option<i64>", DatabaseType::Postgres), "BIGINT");
    }
    
    #[test]
    fn test_column_schema_builder() {
        let col = ColumnSchema::new("id", "BIGINT")
            .primary_key()
            .auto_increment()
            .not_null();
        
        assert_eq!(col.name, "id");
        assert_eq!(col.sql_type, "BIGINT");
        assert!(col.primary_key);
        assert!(col.auto_increment);
        assert!(!col.nullable);
    }
    
    #[test]
    fn test_column_schema_with_default() {
        let col = ColumnSchema::new("status", "TEXT")
            .not_null()
            .default("'active'");
        
        assert_eq!(col.name, "status");
        assert_eq!(col.default.unwrap(), "'active'");
        assert!(!col.nullable);
    }
    
    #[test]
    fn test_table_schema_builder() {
        let schema = TableSchemaBuilder::new("users")
            .column(ColumnSchema::new("id", "BIGINT").primary_key().auto_increment())
            .column(ColumnSchema::new("email", "TEXT").not_null())
            .column(ColumnSchema::new("name", "TEXT"))
            .index(IndexDefinition::new("idx_users_email", vec!["email".to_string()], false))
            .build();
        
        assert_eq!(schema.name, "users");
        assert_eq!(schema.columns.len(), 3);
        assert_eq!(schema.indexes.len(), 1);
        assert_eq!(schema.primary_key, "id");
    }
    
    #[test]
    fn test_schema_generator_postgres() {
        let mut generator = SchemaGenerator::new(DatabaseType::Postgres);
        
        let schema = TableSchemaBuilder::new("posts")
            .column(ColumnSchema::new("id", "BIGINT").primary_key().auto_increment())
            .column(ColumnSchema::new("title", "TEXT").not_null())
            .build();
        
        generator.add_table(schema);
        let sql = generator.generate();
        
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS"));
        assert!(sql.contains("\"posts\""));  // Postgres uses double quotes
        assert!(sql.contains("BIGSERIAL"));  // Auto-increment in Postgres
        assert!(sql.contains("PRIMARY KEY"));
    }
    
    #[test]
    fn test_schema_generator_mysql() {
        let mut generator = SchemaGenerator::new(DatabaseType::MySQL);
        
        let schema = TableSchemaBuilder::new("posts")
            .column(ColumnSchema::new("id", "BIGINT").primary_key().auto_increment())
            .column(ColumnSchema::new("title", "TEXT").not_null())
            .build();
        
        generator.add_table(schema);
        let sql = generator.generate();
        
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS"));
        assert!(sql.contains("`posts`"));  // MySQL uses backticks
        assert!(sql.contains("AUTO_INCREMENT"));  // MySQL syntax
    }
    
    #[test]
    fn test_schema_generator_sqlite() {
        let mut generator = SchemaGenerator::new(DatabaseType::SQLite);
        
        let schema = TableSchemaBuilder::new("posts")
            .column(ColumnSchema::new("id", "INTEGER").primary_key())
            .column(ColumnSchema::new("title", "TEXT").not_null())
            .build();
        
        generator.add_table(schema);
        let sql = generator.generate();
        
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS"));
        assert!(sql.contains("\"posts\""));  // SQLite uses double quotes
    }
    
    #[test]
    fn test_schema_generator_indexes() {
        let mut generator = SchemaGenerator::new(DatabaseType::Postgres);
        
        let schema = TableSchemaBuilder::new("users")
            .column(ColumnSchema::new("id", "BIGINT").primary_key())
            .column(ColumnSchema::new("email", "TEXT").not_null())
            .index(IndexDefinition::new("idx_users_email", vec!["email".to_string()], false))
            .index(IndexDefinition::new("uidx_users_email", vec!["email".to_string()], true))
            .build();
        
        generator.add_table(schema);
        let sql = generator.generate();
        
        assert!(sql.contains("CREATE INDEX IF NOT EXISTS"));
        assert!(sql.contains("CREATE UNIQUE INDEX IF NOT EXISTS"));
        assert!(sql.contains("idx_users_email"));
        assert!(sql.contains("uidx_users_email"));
    }
    
    #[test]
    fn test_index_definition_parse_single() {
        let indexes = IndexDefinition::parse("users", "email", false);
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].columns, vec!["email"]);
        assert!(!indexes[0].unique);
    }
    
    #[test]
    fn test_index_definition_parse_composite() {
        let indexes = IndexDefinition::parse("users", "first_name,last_name", false);
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].columns, vec!["first_name", "last_name"]);
    }
    
    #[test]
    fn test_index_definition_parse_named() {
        let indexes = IndexDefinition::parse("users", "my_idx:email", false);
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].name, "my_idx");
        assert_eq!(indexes[0].columns, vec!["email"]);
    }
    
    #[test]
    fn test_index_definition_parse_multiple() {
        let indexes = IndexDefinition::parse("users", "email;name:first_name,last_name", false);
        assert_eq!(indexes.len(), 2);
    }
    
    #[test]
    fn test_index_definition_parse_unique() {
        let indexes = IndexDefinition::parse("users", "email", true);
        assert_eq!(indexes.len(), 1);
        assert!(indexes[0].unique);
    }
}

// =============================================================================
// QUERY MODULE TESTS
// =============================================================================

#[cfg(test)]
mod query_tests {
    use tideorm::query::{Order, Operator, ConditionValue, WhereCondition};
    
    #[test]
    fn test_order_as_str() {
        assert_eq!(Order::Asc.as_str(), "ASC");
        assert_eq!(Order::Desc.as_str(), "DESC");
    }
    
    #[test]
    fn test_order_clone_eq() {
        let order1 = Order::Asc;
        let order2 = order1.clone();
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
        let val = ConditionValue::Range(
            serde_json::json!(1),
            serde_json::json!(100),
        );
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
            value: ConditionValue::Range(
                serde_json::json!(18),
                serde_json::json!(65),
            ),
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
// RELATIONS MODULE TESTS
// =============================================================================

#[cfg(test)]
mod relations_tests {
    use tideorm::relations::{RelationType, RelationInfo};
    
    #[test]
    fn test_relation_type_display() {
        assert_eq!(format!("{}", RelationType::BelongsTo), "belongs_to");
        assert_eq!(format!("{}", RelationType::HasOne), "has_one");
        assert_eq!(format!("{}", RelationType::HasMany), "has_many");
    }
    
    #[test]
    fn test_relation_type_clone_eq() {
        let rt1 = RelationType::HasMany;
        let rt2 = rt1.clone();
        assert_eq!(rt1, rt2);
    }
    
    #[test]
    fn test_relation_type_debug() {
        let debug = format!("{:?}", RelationType::BelongsTo);
        assert_eq!(debug, "BelongsTo");
    }
    
    #[test]
    fn test_relation_info_creation() {
        let info = RelationInfo {
            name: "author".to_string(),
            relation_type: RelationType::BelongsTo,
            related_table: "users".to_string(),
            foreign_key: "user_id".to_string(),
            local_key: "id".to_string(),
        };
        
        assert_eq!(info.name, "author");
        assert_eq!(info.relation_type, RelationType::BelongsTo);
        assert_eq!(info.related_table, "users");
        assert_eq!(info.foreign_key, "user_id");
        assert_eq!(info.local_key, "id");
    }
    
    #[test]
    fn test_relation_info_has_one() {
        let info = RelationInfo {
            name: "profile".to_string(),
            relation_type: RelationType::HasOne,
            related_table: "profiles".to_string(),
            foreign_key: "user_id".to_string(),
            local_key: "id".to_string(),
        };
        
        assert_eq!(info.relation_type, RelationType::HasOne);
    }
    
    #[test]
    fn test_relation_info_has_many() {
        let info = RelationInfo {
            name: "posts".to_string(),
            relation_type: RelationType::HasMany,
            related_table: "posts".to_string(),
            foreign_key: "user_id".to_string(),
            local_key: "id".to_string(),
        };
        
        assert_eq!(info.relation_type, RelationType::HasMany);
    }
}

// =============================================================================
// CALLBACKS MODULE TESTS
// =============================================================================

#[cfg(test)]
mod callbacks_tests {
    // Note: Callbacks has a blanket implementation for all types,
    // so we test the trait structure and types rather than custom implementations
    
    use tideorm::callbacks::Callbacks;
    
    #[test]
    fn test_callbacks_trait_exists() {
        // Verify the trait and its methods exist
        // Due to blanket impl, any type implements Callbacks
        struct TestType;
        
        let mut model = TestType;
        // All default implementations should return Ok(())
        assert!(model.before_validation().is_ok());
        assert!(model.after_validation().is_ok());
        assert!(model.before_save().is_ok());
        assert!(model.after_save().is_ok());
        assert!(model.before_create().is_ok());
        assert!(model.after_create().is_ok());
        assert!(model.before_update().is_ok());
        assert!(model.after_update().is_ok());
        assert!(model.before_delete().is_ok());
        assert!(model.after_delete().is_ok());
    }
    
    #[test]
    fn test_callback_runner_exists() {
        use tideorm::callbacks::CallbackRunner;
        
        struct TestType;
        
        let mut model = TestType;
        // CallbackRunner methods should work via blanket impl
        assert!(model.run_create_callbacks().is_ok());
        assert!(model.run_after_create_callbacks().is_ok());
        assert!(model.run_update_callbacks().is_ok());
        assert!(model.run_after_update_callbacks().is_ok());
        assert!(model.run_delete_callbacks().is_ok());
        assert!(model.run_after_delete_callbacks().is_ok());
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
    use tideorm::types::*;
    use serde_json::json;
    
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
        assert_eq!(bool_array[0], true);
        
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
    use tideorm::query::{JoinType, JoinClause, AggregateFunction};
    
    #[test]
    fn test_join_type_as_sql() {
        assert_eq!(JoinType::Inner.as_sql(), "INNER JOIN");
        assert_eq!(JoinType::Left.as_sql(), "LEFT JOIN");
        assert_eq!(JoinType::Right.as_sql(), "RIGHT JOIN");
    }
    
    #[test]
    fn test_join_type_clone_eq() {
        let jt1 = JoinType::Inner;
        let jt2 = jt1.clone();
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
        assert!(matches!(agg_count_distinct, AggregateFunction::CountDistinct(_)));
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

// =============================================================================
// EDGE CASE TESTS - ERROR HANDLING
// =============================================================================

#[cfg(test)]
mod error_edge_cases {
    use tideorm::error::{Error, ErrorContext, ValidationErrors};
    
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
        let email_errors: Vec<_> = errors.errors()
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
    use tideorm::query::{Operator, ConditionValue, WhereCondition};
    use serde_json::json;
    
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
        let operators = vec![
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
        let operators = vec![
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
        let val = ConditionValue::List(vec![
            json!("string"),
            json!(42),
            json!(true),
            json!(null),
        ]);
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
    use tideorm::schema::{
        rust_type_to_sql, SchemaGenerator, TableSchemaBuilder, ColumnSchema,
    };
    use tideorm::model::IndexDefinition;
    
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
            builder = builder.column(ColumnSchema::new(&format!("col_{}", i), "TEXT"));
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
            let schema = TableSchemaBuilder::new(&format!("table_{}", i))
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
    use tideorm::relations::{RelationType, RelationInfo};
    
    #[test]
    fn test_relation_info_empty_strings() {
        let info = RelationInfo {
            name: "".to_string(),
            relation_type: RelationType::BelongsTo,
            related_table: "".to_string(),
            foreign_key: "".to_string(),
            local_key: "".to_string(),
        };
        
        assert!(info.name.is_empty());
        assert!(info.related_table.is_empty());
    }
    
    #[test]
    fn test_relation_info_unicode_names() {
        let info = RelationInfo {
            name: "作者".to_string(),
            relation_type: RelationType::BelongsTo,
            related_table: "用户".to_string(),
            foreign_key: "用户_id".to_string(),
            local_key: "id".to_string(),
        };
        
        assert_eq!(info.name, "作者");
        assert_eq!(info.related_table, "用户");
    }
    
    #[test]
    fn test_relation_info_clone() {
        let info = RelationInfo {
            name: "author".to_string(),
            relation_type: RelationType::HasMany,
            related_table: "posts".to_string(),
            foreign_key: "author_id".to_string(),
            local_key: "id".to_string(),
        };
        
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
        ];
        
        // All should be different
        assert_ne!(types[0], types[1]);
        assert_ne!(types[1], types[2]);
        assert_ne!(types[0], types[2]);
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
            "",
            " ",
            "\n\t\r",
            "null",
            "true",
            "false",
            "123",
            "[]",
            "{}",
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
    use chrono::{Utc, Duration, TimeZone};
    
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
            let cloned = t.clone();
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
        
        // SQLite supports neither
        assert!(!DatabaseType::SQLite.supports_json());
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
        for db_type in [DatabaseType::Postgres, DatabaseType::MySQL, DatabaseType::SQLite] {
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
    use tideorm::callbacks::{Callbacks, CallbackRunner};
    
    // Struct for potential future callback testing
    #[allow(dead_code)]
    #[derive(Default)]
    struct CallbackCounter {
        before_save_count: u32,
        after_save_count: u32,
    }
    
    // Due to blanket impl, we can't customize callbacks in tests
    // But we can verify the trait structure
    
    #[test]
    fn test_multiple_callback_invocations() {
        let mut model = ();
        
        // Multiple invocations should all succeed
        for _ in 0..100 {
            assert!(model.before_save().is_ok());
            assert!(model.after_save().is_ok());
        }
    }
    
    #[test]
    fn test_callback_runner_chain() {
        let mut model = ();
        
        // Full create lifecycle
        assert!(model.run_create_callbacks().is_ok());
        assert!(model.run_after_create_callbacks().is_ok());
        
        // Full update lifecycle
        assert!(model.run_update_callbacks().is_ok());
        assert!(model.run_after_update_callbacks().is_ok());
        
        // Full delete lifecycle
        assert!(model.run_delete_callbacks().is_ok());
        assert!(model.run_after_delete_callbacks().is_ok());
    }
    
    #[test]
    fn test_all_callback_methods_exist() {
        let mut model = ();
        
        // Validation callbacks
        let _ = model.before_validation();
        let _ = model.after_validation();
        
        // Save callbacks
        let _ = model.before_save();
        let _ = model.after_save();
        
        // Create callbacks
        let _ = model.before_create();
        let _ = model.after_create();
        
        // Update callbacks
        let _ = model.before_update();
        let _ = model.after_update();
        
        // Delete callbacks
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
        let orders = vec![Order::Asc, Order::Desc, Order::Asc];
        
        // Count Asc values
        let asc_count = orders.iter().filter(|&&o| o == Order::Asc).count();
        assert_eq!(asc_count, 2);
        
        // Count Desc values
        let desc_count = orders.iter().filter(|&&o| o == Order::Desc).count();
        assert_eq!(desc_count, 1);
    }
}

// =============================================================================
// MIGRATION MODULE TESTS
// =============================================================================

#[cfg(test)]
mod migration_tests {
    use tideorm::migration::{ColumnType, DefaultValue};
    
    #[test]
    fn test_column_type_variants() {
        let types = vec![
            ColumnType::BigInteger,
            ColumnType::Integer,
            ColumnType::SmallInteger,
            ColumnType::Text,
            ColumnType::String,
            ColumnType::Boolean,
            ColumnType::Float,
            ColumnType::Double,
            ColumnType::Decimal { precision: 10, scale: 2 },
            ColumnType::Date,
            ColumnType::DateTime,
            ColumnType::Timestamp,
            ColumnType::Json,
            ColumnType::Jsonb,
            ColumnType::Uuid,
            ColumnType::Binary,
        ];
        
        // Verify all variants can be constructed
        assert!(types.len() >= 16);
    }
    
    #[test]
    fn test_column_type_string() {
        let string_type = ColumnType::String;
        let sql = string_type.to_postgres_sql();
        assert!(sql.contains("VARCHAR") || sql.contains("TEXT"));
    }
    
    #[test]
    fn test_column_type_decimal_precision() {
        let decimal = ColumnType::Decimal { precision: 12, scale: 4 };
        let sql = decimal.to_postgres_sql();
        assert!(sql.contains("DECIMAL"));
        assert!(sql.contains("12"));
        assert!(sql.contains("4"));
    }
    
    #[test]
    fn test_column_type_to_postgres_sql() {
        assert_eq!(ColumnType::Integer.to_postgres_sql(), "INTEGER");
        assert_eq!(ColumnType::BigInteger.to_postgres_sql(), "BIGINT");
        assert_eq!(ColumnType::SmallInteger.to_postgres_sql(), "SMALLINT");
        assert_eq!(ColumnType::Text.to_postgres_sql(), "TEXT");
        assert_eq!(ColumnType::Boolean.to_postgres_sql(), "BOOLEAN");
        assert_eq!(ColumnType::Uuid.to_postgres_sql(), "UUID");
    }
    
    #[test]
    fn test_column_type_to_mysql_sql() {
        assert_eq!(ColumnType::Integer.to_mysql_sql(), "INT");
        assert_eq!(ColumnType::BigInteger.to_mysql_sql(), "BIGINT");
        assert_eq!(ColumnType::Text.to_mysql_sql(), "TEXT");
    }
    
    #[test]
    fn test_column_type_clone() {
        let ct = ColumnType::Integer;
        let cloned = ct.clone();
        // Both should produce same SQL
        assert_eq!(ct.to_postgres_sql(), cloned.to_postgres_sql());
    }
    
    #[test]
    fn test_default_value_variants() {
        let defaults = vec![
            DefaultValue::String("active".to_string()),
            DefaultValue::Integer(0),
            DefaultValue::Float(0.0),
            DefaultValue::Boolean(true),
            DefaultValue::Null,
            DefaultValue::Raw("CURRENT_TIMESTAMP".to_string()),
        ];
        
        assert_eq!(defaults.len(), 6);
    }
    
    #[test]
    fn test_default_value_string() {
        let default = DefaultValue::String("default_value".to_string());
        let sql = default.to_sql();
        assert!(sql.contains("default_value"));
        assert!(sql.starts_with('\''));
        assert!(sql.ends_with('\''));
    }
    
    #[test]
    fn test_default_value_integer() {
        let default = DefaultValue::Integer(42);
        assert_eq!(default.to_sql(), "42");
    }
    
    #[test]
    fn test_default_value_float() {
        let default = DefaultValue::Float(3.14);
        let sql = default.to_sql();
        assert!(sql.contains("3.14"));
    }
    
    #[test]
    fn test_default_value_boolean() {
        let default_true = DefaultValue::Boolean(true);
        let default_false = DefaultValue::Boolean(false);
        
        assert_eq!(default_true.to_sql(), "TRUE");
        assert_eq!(default_false.to_sql(), "FALSE");
    }
    
    #[test]
    fn test_default_value_null() {
        let default = DefaultValue::Null;
        assert_eq!(default.to_sql(), "NULL");
    }
    
    #[test]
    fn test_default_value_raw() {
        let default = DefaultValue::Raw("CURRENT_TIMESTAMP".to_string());
        assert_eq!(default.to_sql(), "CURRENT_TIMESTAMP");
    }
    
    #[test]
    fn test_default_value_clone() {
        let default = DefaultValue::Integer(99);
        let cloned = default.clone();
        assert_eq!(default.to_sql(), cloned.to_sql());
    }
    
    #[test]
    fn test_column_type_custom() {
        let custom = ColumnType::Custom("CUSTOM_TYPE".to_string());
        assert_eq!(custom.to_postgres_sql(), "CUSTOM_TYPE");
    }
    
    #[test]
    fn test_column_type_arrays() {
        let int_array = ColumnType::IntegerArray;
        let text_array = ColumnType::TextArray;
        
        assert!(int_array.to_postgres_sql().contains("INTEGER"));
        assert!(text_array.to_postgres_sql().contains("TEXT"));
    }
    
    #[test]
    fn test_default_value_string_with_quotes() {
        let default = DefaultValue::String("It's a test".to_string());
        let sql = default.to_sql();
        // Should escape single quotes
        assert!(sql.contains("''"));
    }
}

// =============================================================================
// BATCH OPERATIONS AND BUILDER TESTS
// =============================================================================

#[cfg(test)]
mod batch_operations_tests {
    // These are unit tests for builder patterns (no DB connection)
    // We can't test with () since it doesn't implement Model
    
    #[test]
    fn test_batch_update_builder_type_exists() {
        // Verify the type exists and can be imported
        #[allow(unused_imports)]
        use tideorm::model::BatchUpdateBuilder;
        
        // Type should exist - we just verify it compiles
        // Can't instantiate without a Model type
        let _type_exists = true;
        assert!(_type_exists);
    }
    
    #[test]
    fn test_on_conflict_builder_type_exists() {
        #[allow(unused_imports)]
        use tideorm::model::OnConflictBuilder;
        
        // Type should exist - we just verify it compiles
        let _type_exists = true;
        assert!(_type_exists);
    }
}

// =============================================================================
// COMPREHENSIVE INDEX AND CONSTRAINT TESTS
// =============================================================================

#[cfg(test)]
mod index_constraint_tests {
    use tideorm::model::IndexDefinition;
    
    #[test]
    fn test_index_definition_simple() {
        let idx = IndexDefinition::new("idx_users_email", vec!["email".to_string()], false);
        assert_eq!(idx.name, "idx_users_email");
        assert_eq!(idx.columns.len(), 1);
        assert!(!idx.unique);
    }
    
    #[test]
    fn test_index_definition_composite() {
        let idx = IndexDefinition::new(
            "idx_users_name_age",
            vec!["name".to_string(), "age".to_string()],
            false,
        );
        assert_eq!(idx.columns.len(), 2);
        assert_eq!(idx.columns[0], "name");
        assert_eq!(idx.columns[1], "age");
    }
    
    #[test]
    fn test_index_definition_unique() {
        let idx = IndexDefinition::new("uidx_users_email", vec!["email".to_string()], true);
        assert!(idx.unique);
    }
    
    #[test]
    fn test_index_definition_clone() {
        let idx = IndexDefinition::new("idx_test", vec!["col1".to_string()], false);
        let cloned = idx.clone();
        assert_eq!(idx.name, cloned.name);
        assert_eq!(idx.columns, cloned.columns);
        assert_eq!(idx.unique, cloned.unique);
    }
    
    #[test]
    fn test_index_parse_variations() {
        // Single column
        let idx1 = IndexDefinition::parse("users", "email", false);
        assert_eq!(idx1.len(), 1);
        
        // Composite
        let idx2 = IndexDefinition::parse("users", "first_name,last_name", false);
        assert_eq!(idx2.len(), 1);
        assert_eq!(idx2[0].columns.len(), 2);
        
        // Named
        let idx3 = IndexDefinition::parse("users", "custom_idx:email", false);
        assert_eq!(idx3[0].name, "custom_idx");
        
        // Multiple indexes
        let idx4 = IndexDefinition::parse("users", "email;name", false);
        assert_eq!(idx4.len(), 2);
    }
    
    #[test]
    fn test_index_parse_with_spaces() {
        let idx = IndexDefinition::parse("users", " email , name ", false);
        assert!(!idx.is_empty());
        // Should handle whitespace
    }
    
    #[test]
    fn test_index_auto_naming() {
        let idx = IndexDefinition::parse("users", "email", false);
        // Auto-generated name should include table name
        assert!(idx[0].name.contains("users") || idx[0].name.contains("email"));
    }
}

// =============================================================================
// PRELUDE MODULE TESTS
// =============================================================================

#[cfg(test)]
mod prelude_tests {
    // Test that all common types are exported from prelude
    
    #[test]
    fn test_prelude_error_types() {
        use tideorm::prelude::*;
        
        let err = Error::not_found("test");
        assert!(err.is_not_found());
        
        let ve = ValidationErrors::new();
        assert!(ve.is_empty());
    }
    
    #[test]
    fn test_prelude_config_types() {
        use tideorm::prelude::*;
        
        let db_type = DatabaseType::Postgres;
        assert_eq!(db_type.default_port(), 5432);
    }
    
    #[test]
    fn test_prelude_query_types() {
        use tideorm::prelude::*;
        
        let order = Order::Asc;
        assert_eq!(order.as_str(), "ASC");
        
        let join_type = JoinType::Inner;
        assert_eq!(join_type.as_sql(), "INNER JOIN");
    }
    
    #[test]
    fn test_prelude_json_types() {
        use tideorm::prelude::*;
        
        let j = json!({"key": "value"});
        assert!(j.is_object());
    }
    
    #[test]
    fn test_prelude_datetime_types() {
        use tideorm::prelude::*;
        
        let now = Utc::now();
        let _: DateTime<Utc> = now;
    }
}

// =============================================================================
// COMPREHENSIVE OPERATOR COVERAGE
// =============================================================================

#[cfg(test)]
mod operator_coverage_tests {
    use tideorm::query::Operator;
    
    #[test]
    fn test_all_comparison_operators() {
        let ops = vec![
            Operator::Eq,
            Operator::NotEq,
            Operator::Gt,
            Operator::Gte,
            Operator::Lt,
            Operator::Lte,
        ];
        assert_eq!(ops.len(), 6);
    }
    
    #[test]
    fn test_all_pattern_operators() {
        let ops = vec![
            Operator::Like,
            Operator::NotLike,
        ];
        assert_eq!(ops.len(), 2);
    }
    
    #[test]
    fn test_all_membership_operators() {
        let ops = vec![
            Operator::In,
            Operator::NotIn,
        ];
        assert_eq!(ops.len(), 2);
    }
    
    #[test]
    fn test_all_null_operators() {
        let ops = vec![
            Operator::IsNull,
            Operator::IsNotNull,
        ];
        assert_eq!(ops.len(), 2);
    }
    
    #[test]
    fn test_range_operator() {
        let op = Operator::Between;
        assert!(matches!(op, Operator::Between));
    }
    
    #[test]
    fn test_all_json_operators() {
        let ops = vec![
            Operator::JsonContains,
            Operator::JsonContainedBy,
            Operator::JsonKeyExists,
            Operator::JsonKeyNotExists,
            Operator::JsonPathExists,
            Operator::JsonPathNotExists,
        ];
        assert_eq!(ops.len(), 6);
    }
    
    #[test]
    fn test_all_array_operators() {
        let ops = vec![
            Operator::ArrayContains,
            Operator::ArrayContainedBy,
            Operator::ArrayOverlaps,
            Operator::ArrayContainsAny,
            Operator::ArrayContainsAll,
        ];
        assert_eq!(ops.len(), 5);
    }
    
    #[test]
    fn test_operator_clone() {
        let op = Operator::Eq;
        let cloned = op.clone();
        assert!(matches!(cloned, Operator::Eq));
    }
    
    #[test]
    fn test_operator_debug() {
        let debug_str = format!("{:?}", Operator::Like);
        assert_eq!(debug_str, "Like");
    }
}
// =============================================================================
// ATTACHMENTS MODULE TESTS
// =============================================================================

#[cfg(test)]
mod attachments_tests {
    use tideorm::attachments::{FileAttachment, FilesData, AttachmentError};
    
    #[test]
    fn test_file_attachment_creation() {
        let attachment = FileAttachment::new("uploads/2024/01/image.jpg");
        assert_eq!(attachment.key, "uploads/2024/01/image.jpg");
        assert_eq!(attachment.filename, "image.jpg");
        assert!(!attachment.created_at.is_empty());
    }
    
    #[test]
    fn test_file_attachment_simple_key() {
        let attachment = FileAttachment::new("photo.png");
        assert_eq!(attachment.key, "photo.png");
        assert_eq!(attachment.filename, "photo.png");
    }
    
    #[test]
    fn test_file_attachment_with_metadata() {
        let attachment = FileAttachment::with_metadata(
            "uploads/doc.pdf",
            Some("My Document.pdf"),
            Some(1024),
            Some("application/pdf"),
        );
        assert_eq!(attachment.key, "uploads/doc.pdf");
        assert_eq!(attachment.filename, "doc.pdf");
        assert_eq!(attachment.original_filename, Some("My Document.pdf".to_string()));
        assert_eq!(attachment.size, Some(1024));
        assert_eq!(attachment.mime_type, Some("application/pdf".to_string()));
    }
    
    #[test]
    fn test_file_attachment_add_metadata() {
        let attachment = FileAttachment::new("test.jpg")
            .add_metadata("width", 1920)
            .add_metadata("height", 1080)
            .add_metadata("format", "jpeg");
        
        assert_eq!(attachment.metadata.get("width"), Some(&serde_json::json!(1920)));
        assert_eq!(attachment.metadata.get("height"), Some(&serde_json::json!(1080)));
        assert_eq!(attachment.metadata.get("format"), Some(&serde_json::json!("jpeg")));
    }
    
    #[test]
    fn test_file_attachment_to_json() {
        let attachment = FileAttachment::new("test.jpg");
        let json = attachment.to_json();
        
        assert_eq!(json.get("key").unwrap(), "test.jpg");
        assert_eq!(json.get("filename").unwrap(), "test.jpg");
        assert!(json.get("created_at").is_some());
    }
    
    #[test]
    fn test_files_data_new() {
        let files = FilesData::new();
        assert!(!files.has_files("any"));
        assert_eq!(files.count_files("any"), 0);
    }
    
    #[test]
    fn test_files_data_set_one() {
        let mut files = FilesData::new();
        files.set_one("thumbnail", FileAttachment::new("thumb.jpg"));
        
        assert!(files.has_files("thumbnail"));
        assert_eq!(files.count_files("thumbnail"), 1);
        
        let thumb = files.get_one("thumbnail").unwrap();
        assert_eq!(thumb.key, "thumb.jpg");
    }
    
    #[test]
    fn test_files_data_remove_one() {
        let mut files = FilesData::new();
        files.set_one("thumbnail", FileAttachment::new("thumb.jpg"));
        
        assert!(files.has_files("thumbnail"));
        
        files.remove_one("thumbnail");
        assert!(!files.has_files("thumbnail"));
        assert!(files.get_one("thumbnail").is_none());
    }
    
    #[test]
    fn test_files_data_replace_one() {
        let mut files = FilesData::new();
        files.set_one("thumbnail", FileAttachment::new("old.jpg"));
        files.set_one("thumbnail", FileAttachment::new("new.jpg"));
        
        let thumb = files.get_one("thumbnail").unwrap();
        assert_eq!(thumb.key, "new.jpg");
        assert_eq!(files.count_files("thumbnail"), 1);
    }
    
    #[test]
    fn test_files_data_add_many() {
        let mut files = FilesData::new();
        files.add_many("images", FileAttachment::new("img1.jpg"));
        files.add_many("images", FileAttachment::new("img2.jpg"));
        files.add_many("images", FileAttachment::new("img3.jpg"));
        
        assert!(files.has_files("images"));
        assert_eq!(files.count_files("images"), 3);
        
        let images = files.get_many("images");
        assert_eq!(images.len(), 3);
        assert_eq!(images[0].key, "img1.jpg");
        assert_eq!(images[1].key, "img2.jpg");
        assert_eq!(images[2].key, "img3.jpg");
    }
    
    #[test]
    fn test_files_data_remove_from_many() {
        let mut files = FilesData::new();
        files.add_many("images", FileAttachment::new("img1.jpg"));
        files.add_many("images", FileAttachment::new("img2.jpg"));
        files.add_many("images", FileAttachment::new("img3.jpg"));
        
        files.remove_from_many("images", "img2.jpg");
        
        assert_eq!(files.count_files("images"), 2);
        let images = files.get_many("images");
        assert_eq!(images[0].key, "img1.jpg");
        assert_eq!(images[1].key, "img3.jpg");
    }
    
    #[test]
    fn test_files_data_clear_many() {
        let mut files = FilesData::new();
        files.add_many("images", FileAttachment::new("img1.jpg"));
        files.add_many("images", FileAttachment::new("img2.jpg"));
        
        files.clear_many("images");
        
        assert!(!files.has_files("images"));
        assert_eq!(files.count_files("images"), 0);
    }
    
    #[test]
    fn test_files_data_from_json() {
        let json = serde_json::json!({
            "thumbnail": {
                "key": "thumb.jpg",
                "filename": "thumb.jpg",
                "created_at": "2024-01-01T00:00:00Z"
            },
            "images": [
                {"key": "img1.jpg", "filename": "img1.jpg", "created_at": "2024-01-01T00:00:00Z"},
                {"key": "img2.jpg", "filename": "img2.jpg", "created_at": "2024-01-01T00:00:00Z"}
            ]
        });
        
        let files = FilesData::from_json(&json);
        
        assert!(files.has_files("thumbnail"));
        assert_eq!(files.count_files("thumbnail"), 1);
        assert_eq!(files.count_files("images"), 2);
    }
    
    #[test]
    fn test_files_data_to_json() {
        let mut files = FilesData::new();
        files.set_one("thumbnail", FileAttachment::new("thumb.jpg"));
        files.add_many("images", FileAttachment::new("img1.jpg"));
        
        let json = files.to_json();
        
        assert!(json.get("thumbnail").is_some());
        assert!(json.get("images").is_some());
        assert!(json.get("images").unwrap().is_array());
    }
    
    #[test]
    fn test_files_data_multiple_relations() {
        let mut files = FilesData::new();
        files.set_one("avatar", FileAttachment::new("avatar.jpg"));
        files.set_one("cover", FileAttachment::new("cover.jpg"));
        files.add_many("gallery", FileAttachment::new("photo1.jpg"));
        files.add_many("documents", FileAttachment::new("doc.pdf"));
        
        assert_eq!(files.count_files("avatar"), 1);
        assert_eq!(files.count_files("cover"), 1);
        assert_eq!(files.count_files("gallery"), 1);
        assert_eq!(files.count_files("documents"), 1);
    }
    
    #[test]
    fn test_attachment_error_invalid_relation() {
        let err = AttachmentError::InvalidRelation("unknown".to_string());
        assert!(err.to_string().contains("Invalid relation"));
        assert!(err.to_string().contains("unknown"));
    }
    
    #[test]
    fn test_attachment_error_parse_error() {
        let err = AttachmentError::ParseError("failed to parse".to_string());
        assert!(err.to_string().contains("Parse error"));
    }
    
    #[test]
    fn test_attachment_error_not_supported() {
        let err = AttachmentError::NotSupported;
        assert!(err.to_string().contains("does not support"));
    }
    
    #[test]
    fn test_files_data_get_nonexistent() {
        let files = FilesData::new();
        
        assert!(files.get_one("nonexistent").is_none());
        assert!(files.get_many("nonexistent").is_empty());
        assert!(!files.has_files("nonexistent"));
        assert_eq!(files.count_files("nonexistent"), 0);
    }
}

// =============================================================================
// TRANSLATIONS MODULE TESTS
// =============================================================================

#[cfg(test)]
mod translations_tests {
    use tideorm::translations::{
        TranslationsData, FieldTranslations, TranslationInput, TranslationError,
    };
    
    #[test]
    fn test_field_translations_new() {
        let trans = FieldTranslations::new();
        assert!(!trans.has("en"));
        assert!(trans.languages().is_empty());
    }
    
    #[test]
    fn test_field_translations_set_get() {
        let mut trans = FieldTranslations::new();
        trans.set("en", "Hello");
        trans.set("ar", "مرحبا");
        
        assert_eq!(trans.get("en"), Some(&serde_json::json!("Hello")));
        assert_eq!(trans.get("ar"), Some(&serde_json::json!("مرحبا")));
        assert_eq!(trans.get("fr"), None);
    }
    
    #[test]
    fn test_field_translations_has() {
        let mut trans = FieldTranslations::new();
        trans.set("en", "Hello");
        
        assert!(trans.has("en"));
        assert!(!trans.has("fr"));
    }
    
    #[test]
    fn test_field_translations_remove() {
        let mut trans = FieldTranslations::new();
        trans.set("en", "Hello");
        trans.set("ar", "مرحبا");
        
        trans.remove("en");
        
        assert!(!trans.has("en"));
        assert!(trans.has("ar"));
    }
    
    #[test]
    fn test_field_translations_languages() {
        let mut trans = FieldTranslations::new();
        trans.set("en", "Hello");
        trans.set("ar", "مرحبا");
        trans.set("fr", "Bonjour");
        
        let langs = trans.languages();
        assert_eq!(langs.len(), 3);
    }
    
    #[test]
    fn test_field_translations_all() {
        let mut trans = FieldTranslations::new();
        trans.set("en", "Hello");
        trans.set("ar", "مرحبا");
        
        let all = trans.all();
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("en"), Some(&serde_json::json!("Hello")));
    }
    
    #[test]
    fn test_translations_data_new() {
        let data = TranslationsData::new();
        assert!(!data.has_translations("any"));
        assert!(data.fields().is_empty());
    }
    
    #[test]
    fn test_translations_data_set_get() {
        let mut data = TranslationsData::new();
        data.set("name", "en", "Product");
        data.set("name", "ar", "منتج");
        
        assert_eq!(data.get("name", "en"), Some(&serde_json::json!("Product")));
        assert_eq!(data.get("name", "ar"), Some(&serde_json::json!("منتج")));
        assert_eq!(data.get("name", "fr"), None);
        assert_eq!(data.get("other", "en"), None);
    }
    
    #[test]
    fn test_translations_data_multiple_fields() {
        let mut data = TranslationsData::new();
        data.set("name", "en", "Product");
        data.set("name", "ar", "منتج");
        data.set("description", "en", "A great product");
        data.set("description", "ar", "منتج رائع");
        
        assert!(data.has_translations("name"));
        assert!(data.has_translations("description"));
        assert!(!data.has_translations("other"));
        
        assert_eq!(data.fields().len(), 2);
    }
    
    #[test]
    fn test_translations_data_remove() {
        let mut data = TranslationsData::new();
        data.set("name", "en", "Product");
        data.set("name", "ar", "منتج");
        
        data.remove("name", "ar");
        
        assert!(data.get("name", "en").is_some());
        assert!(data.get("name", "ar").is_none());
    }
    
    #[test]
    fn test_translations_data_remove_field() {
        let mut data = TranslationsData::new();
        data.set("name", "en", "Product");
        data.set("name", "ar", "منتج");
        
        data.remove_field("name");
        
        assert!(!data.has_translations("name"));
        assert!(data.get("name", "en").is_none());
    }
    
    #[test]
    fn test_translations_data_from_json() {
        let json = serde_json::json!({
            "name": {"en": "Product", "ar": "منتج"},
            "description": {"en": "A great product"}
        });
        
        let data = TranslationsData::from_json(&json);
        
        assert_eq!(data.get("name", "en"), Some(&serde_json::json!("Product")));
        assert_eq!(data.get("name", "ar"), Some(&serde_json::json!("منتج")));
        assert_eq!(data.get("description", "en"), Some(&serde_json::json!("A great product")));
    }
    
    #[test]
    fn test_translations_data_to_json() {
        let mut data = TranslationsData::new();
        data.set("name", "en", "Product");
        data.set("name", "ar", "منتج");
        
        let json = data.to_json();
        
        assert!(json.get("name").is_some());
        let name = json.get("name").unwrap();
        assert_eq!(name.get("en"), Some(&serde_json::json!("Product")));
        assert_eq!(name.get("ar"), Some(&serde_json::json!("منتج")));
    }
    
    #[test]
    fn test_translations_data_get_field() {
        let mut data = TranslationsData::new();
        data.set("name", "en", "Product");
        data.set("name", "ar", "منتج");
        
        let field = data.get_field("name");
        assert!(field.is_some());
        assert_eq!(field.unwrap().languages().len(), 2);
        
        assert!(data.get_field("nonexistent").is_none());
    }
    
    #[test]
    fn test_translation_input_new() {
        let input = TranslationInput::new();
        assert!(input.fields.is_empty());
    }
    
    #[test]
    fn test_translation_input_add() {
        let mut input = TranslationInput::new();
        input.add("name", "en", "Product");
        input.add("name", "ar", "منتج");
        input.add("description", "en", "Description");
        
        assert_eq!(input.fields.len(), 2);
        assert_eq!(input.fields.get("name").unwrap().len(), 2);
        assert_eq!(input.fields.get("description").unwrap().len(), 1);
    }
    
    #[test]
    fn test_translation_input_from_json() {
        let json = serde_json::json!({
            "name": {"en": "Product", "ar": "منتج"},
            "description": {"en": "A product"}
        });
        
        let input = TranslationInput::from_json(&json).unwrap();
        
        assert_eq!(input.fields.len(), 2);
        assert_eq!(
            input.fields.get("name").unwrap().get("en"),
            Some(&serde_json::json!("Product"))
        );
    }
    
    #[test]
    fn test_translation_input_from_invalid_json() {
        let json = serde_json::json!("not an object");
        
        let result = TranslationInput::from_json(&json);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_translation_error_invalid_field() {
        let err = TranslationError::InvalidField("unknown".to_string());
        assert!(err.to_string().contains("Invalid field"));
        assert!(err.to_string().contains("unknown"));
    }
    
    #[test]
    fn test_translation_error_invalid_language() {
        let err = TranslationError::InvalidLanguage("xx".to_string());
        assert!(err.to_string().contains("Invalid language"));
        assert!(err.to_string().contains("xx"));
    }
    
    #[test]
    fn test_translation_error_parse_error() {
        let err = TranslationError::ParseError("failed".to_string());
        assert!(err.to_string().contains("Parse error"));
    }
    
    #[test]
    fn test_translation_error_not_supported() {
        let err = TranslationError::NotSupported;
        assert!(err.to_string().contains("does not support"));
    }
    
    #[test]
    fn test_translations_data_update_existing() {
        let mut data = TranslationsData::new();
        data.set("name", "en", "Old Value");
        data.set("name", "en", "New Value");
        
        assert_eq!(data.get("name", "en"), Some(&serde_json::json!("New Value")));
    }
    
    #[test]
    fn test_translations_data_with_complex_values() {
        let mut data = TranslationsData::new();
        
        // Test with numbers
        data.set("count", "en", 42);
        assert_eq!(data.get("count", "en"), Some(&serde_json::json!(42)));
        
        // Test with booleans
        data.set("active", "en", true);
        assert_eq!(data.get("active", "en"), Some(&serde_json::json!(true)));
        
        // Test with arrays
        data.set("tags", "en", serde_json::json!(["tag1", "tag2"]));
        assert_eq!(data.get("tags", "en"), Some(&serde_json::json!(["tag1", "tag2"])));
    }
    
    #[test]
    fn test_translations_roundtrip() {
        let mut original = TranslationsData::new();
        original.set("name", "en", "English Name");
        original.set("name", "ar", "الاسم العربي");
        original.set("description", "en", "English Description");
        original.set("description", "fr", "Description en français");
        
        // Convert to JSON and back
        let json = original.to_json();
        let restored = TranslationsData::from_json(&json);
        
        // Verify all values preserved
        assert_eq!(restored.get("name", "en"), original.get("name", "en"));
        assert_eq!(restored.get("name", "ar"), original.get("name", "ar"));
        assert_eq!(restored.get("description", "en"), original.get("description", "en"));
        assert_eq!(restored.get("description", "fr"), original.get("description", "fr"));
    }
}

// =============================================================================
// EXTENDED ATTACHMENTS TESTS
// =============================================================================

#[cfg(test)]
mod attachments_extended_tests {
    use tideorm::attachments::{FileAttachment, FilesData, HasAttachments, AttachmentError};
    use serde::{Serialize, Deserialize};
    
    // Test model implementing HasAttachments
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct TestProduct {
        id: i64,
        name: String,
        files: Option<serde_json::Value>,
    }
    
    impl HasAttachments for TestProduct {
        fn has_one_files() -> Vec<&'static str> {
            vec!["thumbnail", "cover"]
        }
        
        fn has_many_files() -> Vec<&'static str> {
            vec!["images", "documents"]
        }
        
        fn get_files_data(&self) -> Result<FilesData, AttachmentError> {
            match &self.files {
                Some(json) => Ok(FilesData::from_json(json)),
                None => Ok(FilesData::new()),
            }
        }
        
        fn set_files_data(&mut self, data: FilesData) -> Result<(), AttachmentError> {
            self.files = Some(data.to_json());
            Ok(())
        }
    }
    
    impl TestProduct {
        fn new(id: i64, name: &str) -> Self {
            Self {
                id,
                name: name.to_string(),
                files: None,
            }
        }
    }
    
    #[test]
    fn test_has_attachments_attach_single() {
        let mut product = TestProduct::new(1, "Test Product");
        
        product.attach("thumbnail", "uploads/thumb.jpg").unwrap();
        
        let thumb = product.get_file("thumbnail").unwrap().unwrap();
        assert_eq!(thumb.key, "uploads/thumb.jpg");
        assert_eq!(thumb.filename, "thumb.jpg");
    }
    
    #[test]
    fn test_has_attachments_attach_replaces_has_one() {
        let mut product = TestProduct::new(1, "Test Product");
        
        product.attach("thumbnail", "uploads/old.jpg").unwrap();
        product.attach("thumbnail", "uploads/new.jpg").unwrap();
        
        let thumb = product.get_file("thumbnail").unwrap().unwrap();
        assert_eq!(thumb.key, "uploads/new.jpg");
        assert_eq!(product.count_files("thumbnail").unwrap(), 1);
    }
    
    #[test]
    fn test_has_attachments_attach_many() {
        let mut product = TestProduct::new(1, "Test Product");
        
        product.attach_many("images", vec!["img1.jpg", "img2.jpg", "img3.jpg"]).unwrap();
        
        let images = product.get_files("images").unwrap();
        assert_eq!(images.len(), 3);
        assert_eq!(images[0].key, "img1.jpg");
        assert_eq!(images[1].key, "img2.jpg");
        assert_eq!(images[2].key, "img3.jpg");
    }
    
    #[test]
    fn test_has_attachments_attach_many_accumulates() {
        let mut product = TestProduct::new(1, "Test Product");
        
        product.attach("images", "img1.jpg").unwrap();
        product.attach("images", "img2.jpg").unwrap();
        product.attach_many("images", vec!["img3.jpg", "img4.jpg"]).unwrap();
        
        assert_eq!(product.count_files("images").unwrap(), 4);
    }
    
    #[test]
    fn test_has_attachments_detach_specific() {
        let mut product = TestProduct::new(1, "Test Product");
        
        product.attach_many("images", vec!["img1.jpg", "img2.jpg", "img3.jpg"]).unwrap();
        product.detach("images", Some("img2.jpg")).unwrap();
        
        let images = product.get_files("images").unwrap();
        assert_eq!(images.len(), 2);
        assert!(images.iter().all(|f| f.key != "img2.jpg"));
    }
    
    #[test]
    fn test_has_attachments_detach_all() {
        let mut product = TestProduct::new(1, "Test Product");
        
        product.attach_many("images", vec!["img1.jpg", "img2.jpg", "img3.jpg"]).unwrap();
        product.detach("images", None).unwrap();
        
        assert!(!product.has_files("images").unwrap());
        assert_eq!(product.count_files("images").unwrap(), 0);
    }
    
    #[test]
    fn test_has_attachments_detach_has_one() {
        let mut product = TestProduct::new(1, "Test Product");
        
        product.attach("thumbnail", "thumb.jpg").unwrap();
        assert!(product.has_files("thumbnail").unwrap());
        
        product.detach("thumbnail", None).unwrap();
        assert!(!product.has_files("thumbnail").unwrap());
    }
    
    #[test]
    fn test_has_attachments_sync_replaces_all() {
        let mut product = TestProduct::new(1, "Test Product");
        
        product.attach_many("images", vec!["old1.jpg", "old2.jpg", "old3.jpg"]).unwrap();
        product.sync("images", vec!["new1.jpg", "new2.jpg"]).unwrap();
        
        let images = product.get_files("images").unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].key, "new1.jpg");
        assert_eq!(images[1].key, "new2.jpg");
    }
    
    #[test]
    fn test_has_attachments_sync_empty_clears() {
        let mut product = TestProduct::new(1, "Test Product");
        
        product.attach_many("images", vec!["img1.jpg", "img2.jpg"]).unwrap();
        product.sync("images", vec![]).unwrap();
        
        assert!(!product.has_files("images").unwrap());
    }
    
    #[test]
    fn test_has_attachments_sync_has_one() {
        let mut product = TestProduct::new(1, "Test Product");
        
        product.attach("thumbnail", "old.jpg").unwrap();
        product.sync("thumbnail", vec!["new.jpg"]).unwrap();
        
        let thumb = product.get_file("thumbnail").unwrap().unwrap();
        assert_eq!(thumb.key, "new.jpg");
    }
    
    #[test]
    fn test_has_attachments_with_metadata() {
        let mut product = TestProduct::new(1, "Test Product");
        
        let attachment = FileAttachment::with_metadata(
            "uploads/doc.pdf",
            Some("My Document.pdf"),
            Some(1024 * 1024),
            Some("application/pdf"),
        );
        
        product.attach_with_metadata("documents", attachment).unwrap();
        
        let docs = product.get_files("documents").unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].original_filename, Some("My Document.pdf".to_string()));
        assert_eq!(docs[0].size, Some(1024 * 1024));
        assert_eq!(docs[0].mime_type, Some("application/pdf".to_string()));
    }
    
    #[test]
    fn test_has_attachments_invalid_relation() {
        let mut product = TestProduct::new(1, "Test Product");
        
        let result = product.attach("unknown_relation", "file.jpg");
        assert!(result.is_err());
        
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Unknown file relation"));
    }
    
    #[test]
    fn test_has_attachments_attach_many_on_has_one() {
        let mut product = TestProduct::new(1, "Test Product");
        
        let result = product.attach_many("thumbnail", vec!["img1.jpg", "img2.jpg"]);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_has_attachments_multiple_relations() {
        let mut product = TestProduct::new(1, "Test Product");
        
        product.attach("thumbnail", "thumb.jpg").unwrap();
        product.attach("cover", "cover.jpg").unwrap();
        product.attach_many("images", vec!["img1.jpg", "img2.jpg"]).unwrap();
        product.attach_many("documents", vec!["doc1.pdf", "doc2.pdf"]).unwrap();
        
        assert_eq!(product.count_files("thumbnail").unwrap(), 1);
        assert_eq!(product.count_files("cover").unwrap(), 1);
        assert_eq!(product.count_files("images").unwrap(), 2);
        assert_eq!(product.count_files("documents").unwrap(), 2);
    }
    
    #[test]
    fn test_has_attachments_json_persistence() {
        let mut product = TestProduct::new(1, "Test Product");
        
        product.attach("thumbnail", "thumb.jpg").unwrap();
        product.attach_many("images", vec!["img1.jpg", "img2.jpg"]).unwrap();
        
        // Simulate save/load by serializing and deserializing
        let json = serde_json::to_string(&product).unwrap();
        let loaded: TestProduct = serde_json::from_str(&json).unwrap();
        
        assert_eq!(loaded.count_files("thumbnail").unwrap(), 1);
        assert_eq!(loaded.count_files("images").unwrap(), 2);
        
        let thumb = loaded.get_file("thumbnail").unwrap().unwrap();
        assert_eq!(thumb.key, "thumb.jpg");
    }
    
    #[test]
    fn test_file_attachment_deep_path() {
        let attachment = FileAttachment::new("uploads/2024/01/15/user_123/profile/avatar.png");
        assert_eq!(attachment.filename, "avatar.png");
        assert_eq!(attachment.key, "uploads/2024/01/15/user_123/profile/avatar.png");
    }
    
    #[test]
    fn test_file_attachment_unicode_filename() {
        let attachment = FileAttachment::new("uploads/文档/图片.jpg");
        assert_eq!(attachment.filename, "图片.jpg");
    }
    
    #[test]
    fn test_file_attachment_special_characters() {
        let attachment = FileAttachment::new("uploads/file with spaces (1).pdf");
        assert_eq!(attachment.filename, "file with spaces (1).pdf");
    }
}

// =============================================================================
// EXTENDED TRANSLATIONS TESTS
// =============================================================================

#[cfg(test)]
mod translations_extended_tests {
    use tideorm::translations::{
        TranslationsData, TranslationInput, HasTranslations, TranslationError, ApplyTranslations,
    };
    use serde::{Serialize, Deserialize};
    use std::collections::HashMap;
    
    // Test model implementing HasTranslations
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct TestProduct {
        id: i64,
        name: String,
        description: String,
        translations: Option<serde_json::Value>,
    }
    
    impl HasTranslations for TestProduct {
        fn translatable_fields() -> Vec<&'static str> {
            vec!["name", "description"]
        }
        
        fn allowed_languages() -> Vec<String> {
            vec!["en".to_string(), "ar".to_string(), "fr".to_string(), "es".to_string()]
        }
        
        fn fallback_language() -> String {
            "en".to_string()
        }
        
        fn get_translations_data(&self) -> Result<TranslationsData, TranslationError> {
            match &self.translations {
                Some(json) => Ok(TranslationsData::from_json(json)),
                None => Ok(TranslationsData::new()),
            }
        }
        
        fn set_translations_data(&mut self, data: TranslationsData) -> Result<(), TranslationError> {
            self.translations = Some(data.to_json());
            Ok(())
        }
        
        fn get_default_value(&self, field: &str) -> Result<serde_json::Value, TranslationError> {
            match field {
                "name" => Ok(serde_json::json!(self.name)),
                "description" => Ok(serde_json::json!(self.description)),
                _ => Err(TranslationError::InvalidField(format!("Unknown field: {}", field))),
            }
        }
    }
    
    impl TestProduct {
        fn new(id: i64, name: &str, description: &str) -> Self {
            Self {
                id,
                name: name.to_string(),
                description: description.to_string(),
                translations: None,
            }
        }
    }
    
    #[test]
    fn test_has_translations_set_single() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        product.set_translation("name", "ar", "منتج").unwrap();
        
        let trans = product.get_translation("name", "ar").unwrap();
        assert_eq!(trans, Some(serde_json::json!("منتج")));
    }
    
    #[test]
    fn test_has_translations_set_multiple() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        let mut translations = HashMap::new();
        translations.insert("en", "Product Name");
        translations.insert("ar", "اسم المنتج");
        translations.insert("fr", "Nom du produit");
        
        product.set_translations("name", translations).unwrap();
        
        assert_eq!(product.get_translation("name", "en").unwrap(), Some(serde_json::json!("Product Name")));
        assert_eq!(product.get_translation("name", "ar").unwrap(), Some(serde_json::json!("اسم المنتج")));
        assert_eq!(product.get_translation("name", "fr").unwrap(), Some(serde_json::json!("Nom du produit")));
    }
    
    #[test]
    fn test_has_translations_get_translated_with_fallback() {
        let mut product = TestProduct::new(1, "Default Product", "Default Description");
        
        product.set_translation("name", "en", "English Name").unwrap();
        product.set_translation("name", "ar", "الاسم العربي").unwrap();
        
        // Get existing translation
        let ar = product.get_translated("name", "ar").unwrap();
        assert_eq!(ar, serde_json::json!("الاسم العربي"));
        
        // Get fallback language
        let es = product.get_translated("name", "es").unwrap();
        assert_eq!(es, serde_json::json!("English Name"));
    }
    
    #[test]
    fn test_has_translations_fallback_to_default() {
        let product = TestProduct::new(1, "Default Product", "Default Description");
        
        // No translations set, should fall back to default
        let name = product.get_translated("name", "ar").unwrap();
        assert_eq!(name, serde_json::json!("Default Product"));
    }
    
    #[test]
    fn test_has_translations_get_all() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        product.set_translation("name", "en", "English").unwrap();
        product.set_translation("name", "ar", "عربي").unwrap();
        product.set_translation("name", "fr", "Français").unwrap();
        
        let all = product.get_all_translations("name").unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all.get("en"), Some(&serde_json::json!("English")));
        assert_eq!(all.get("ar"), Some(&serde_json::json!("عربي")));
        assert_eq!(all.get("fr"), Some(&serde_json::json!("Français")));
    }
    
    #[test]
    fn test_has_translations_get_for_language() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        product.set_translation("name", "ar", "منتج").unwrap();
        product.set_translation("description", "ar", "الوصف").unwrap();
        product.set_translation("name", "en", "Product").unwrap();
        
        let ar_trans = product.get_translations_for_language("ar").unwrap();
        assert_eq!(ar_trans.len(), 2);
        assert_eq!(ar_trans.get("name"), Some(&serde_json::json!("منتج")));
        assert_eq!(ar_trans.get("description"), Some(&serde_json::json!("الوصف")));
    }
    
    #[test]
    fn test_has_translations_remove_single() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        product.set_translation("name", "ar", "منتج").unwrap();
        product.set_translation("name", "fr", "Produit").unwrap();
        
        product.remove_translation("name", "ar").unwrap();
        
        assert!(!product.has_translation("name", "ar").unwrap());
        assert!(product.has_translation("name", "fr").unwrap());
    }
    
    #[test]
    fn test_has_translations_remove_field() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        product.set_translation("name", "ar", "منتج").unwrap();
        product.set_translation("name", "fr", "Produit").unwrap();
        
        product.remove_field_translations("name").unwrap();
        
        assert!(!product.has_any_translation("name").unwrap());
    }
    
    #[test]
    fn test_has_translations_clear_all() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        product.set_translation("name", "ar", "منتج").unwrap();
        product.set_translation("description", "ar", "الوصف").unwrap();
        
        product.clear_translations().unwrap();
        
        assert!(!product.has_any_translation("name").unwrap());
        assert!(!product.has_any_translation("description").unwrap());
    }
    
    #[test]
    fn test_has_translations_sync() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        product.set_translation("name", "ar", "قديم").unwrap();
        product.set_translation("name", "fr", "Ancien").unwrap();
        
        let mut new_trans = HashMap::new();
        new_trans.insert("en", "New");
        new_trans.insert("es", "Nuevo");
        
        product.sync_translations("name", new_trans).unwrap();
        
        // Old translations should be gone
        assert!(!product.has_translation("name", "ar").unwrap());
        assert!(!product.has_translation("name", "fr").unwrap());
        
        // New translations should exist
        assert!(product.has_translation("name", "en").unwrap());
        assert!(product.has_translation("name", "es").unwrap());
    }
    
    #[test]
    fn test_has_translations_available_languages() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        product.set_translation("name", "en", "English").unwrap();
        product.set_translation("name", "ar", "عربي").unwrap();
        product.set_translation("name", "fr", "Français").unwrap();
        
        let langs = product.available_languages("name").unwrap();
        assert_eq!(langs.len(), 3);
        assert!(langs.contains(&"en".to_string()));
        assert!(langs.contains(&"ar".to_string()));
        assert!(langs.contains(&"fr".to_string()));
    }
    
    #[test]
    fn test_has_translations_invalid_field() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        let result = product.set_translation("invalid_field", "en", "value");
        assert!(result.is_err());
    }
    
    #[test]
    fn test_has_translations_invalid_language() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        let result = product.set_translation("name", "invalid_lang", "value");
        assert!(result.is_err());
    }
    
    #[test]
    fn test_has_translations_to_translated_json() {
        let mut product = TestProduct::new(1, "Default Product", "Default Description");
        
        product.set_translation("name", "ar", "منتج عربي").unwrap();
        product.set_translation("description", "ar", "وصف عربي").unwrap();
        product.set_translation("name", "en", "English Product").unwrap();
        product.set_translation("description", "en", "English Description").unwrap();
        
        // Get Arabic JSON
        let mut opts = HashMap::new();
        opts.insert("language".to_string(), "ar".to_string());
        let json = product.to_translated_json(Some(opts));
        
        assert_eq!(json.get("name"), Some(&serde_json::json!("منتج عربي")));
        assert_eq!(json.get("description"), Some(&serde_json::json!("وصف عربي")));
        assert!(json.get("translations").is_none()); // translations column should be removed
    }
    
    #[test]
    fn test_has_translations_to_json_with_all() {
        let mut product = TestProduct::new(1, "Default Product", "Default Description");
        
        product.set_translation("name", "ar", "منتج").unwrap();
        product.set_translation("name", "en", "Product").unwrap();
        
        let json = product.to_json_with_all_translations();
        
        // Should include translations column
        assert!(json.get("translations").is_some());
    }
    
    #[test]
    fn test_apply_translations() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        let mut input = TranslationInput::new();
        input.add("name", "ar", "منتج");
        input.add("name", "fr", "Produit");
        input.add("description", "ar", "الوصف");
        
        product.apply_translations(input).unwrap();
        
        assert_eq!(product.get_translation("name", "ar").unwrap(), Some(serde_json::json!("منتج")));
        assert_eq!(product.get_translation("name", "fr").unwrap(), Some(serde_json::json!("Produit")));
        assert_eq!(product.get_translation("description", "ar").unwrap(), Some(serde_json::json!("الوصف")));
    }
    
    #[test]
    fn test_apply_translations_from_json() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        let api_data = serde_json::json!({
            "name": {"ar": "منتج", "fr": "Produit"},
            "description": {"ar": "الوصف"}
        });
        
        let input = TranslationInput::from_json(&api_data).unwrap();
        product.apply_translations(input).unwrap();
        
        assert_eq!(product.get_translation("name", "ar").unwrap(), Some(serde_json::json!("منتج")));
        assert_eq!(product.get_translation("description", "ar").unwrap(), Some(serde_json::json!("الوصف")));
    }
    
    #[test]
    fn test_translations_json_persistence() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        product.set_translation("name", "ar", "منتج").unwrap();
        product.set_translation("description", "ar", "الوصف").unwrap();
        
        // Simulate save/load by serializing and deserializing
        let json = serde_json::to_string(&product).unwrap();
        let loaded: TestProduct = serde_json::from_str(&json).unwrap();
        
        assert_eq!(loaded.get_translation("name", "ar").unwrap(), Some(serde_json::json!("منتج")));
        assert_eq!(loaded.get_translation("description", "ar").unwrap(), Some(serde_json::json!("الوصف")));
    }
    
    #[test]
    fn test_translations_rtl_languages() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        // Arabic
        product.set_translation("name", "ar", "منتج رائع جداً").unwrap();
        
        let ar = product.get_translated("name", "ar").unwrap();
        assert_eq!(ar, serde_json::json!("منتج رائع جداً"));
    }
    
    #[test]
    fn test_translations_with_html() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        product.set_translation("description", "en", "<p>Product <strong>description</strong></p>").unwrap();
        
        let desc = product.get_translated("description", "en").unwrap();
        assert_eq!(desc, serde_json::json!("<p>Product <strong>description</strong></p>"));
    }
    
    #[test]
    fn test_translations_with_emoji() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        product.set_translation("name", "en", "Product 🎉 Special Edition").unwrap();
        
        let name = product.get_translated("name", "en").unwrap();
        assert_eq!(name, serde_json::json!("Product 🎉 Special Edition"));
    }
    
    #[test]
    fn test_translations_empty_string() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        product.set_translation("name", "en", "").unwrap();
        
        let name = product.get_translation("name", "en").unwrap();
        assert_eq!(name, Some(serde_json::json!("")));
    }
    
    #[test]
    fn test_translations_long_text() {
        let mut product = TestProduct::new(1, "Product", "Description");
        
        let long_text = "A".repeat(10000);
        product.set_translation("description", "en", long_text.clone()).unwrap();
        
        let desc = product.get_translated("description", "en").unwrap();
        assert_eq!(desc, serde_json::json!(long_text));
    }
}
