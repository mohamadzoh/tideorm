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
        let sql_type = rust_type_to_sql("MyCustomType", DatabaseType::Postgres);
        assert_eq!(sql_type, "TEXT");
    }

    #[test]
    fn test_rust_type_deeply_nested_option() {
        let sql_type = rust_type_to_sql("Option<Option<String>>", DatabaseType::Postgres);
        assert_eq!(sql_type, "TEXT");
    }

    #[test]
    fn test_rust_type_vec() {
        let sql_type = rust_type_to_sql("Vec<String>", DatabaseType::Postgres);
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
        assert_eq!(indexes[0].columns[0].trim(), "email");
    }

    #[test]
    fn test_schema_generator_empty() {
        let generator = SchemaGenerator::new(DatabaseType::Postgres);
        let sql = generator.generate();
        let _ = sql;
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

        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
