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
