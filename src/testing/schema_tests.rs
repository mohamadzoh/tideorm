use super::*;

#[test]
fn test_index_definition_parse() {
    let indexes = IndexDefinition::parse("users", "email", false);
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].columns, vec!["email"]);
    assert!(!indexes[0].unique);

    let indexes = IndexDefinition::parse("users", "first_name,last_name", false);
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].columns, vec!["first_name", "last_name"]);

    let indexes = IndexDefinition::parse("users", "my_idx:email", false);
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].name, "my_idx");
    assert_eq!(indexes[0].columns, vec!["email"]);

    let indexes = IndexDefinition::parse("users", "email;name:first_name,last_name", false);
    assert_eq!(indexes.len(), 2);
}

#[test]
fn test_schema_generation() {
    let mut generator = SchemaGenerator::new(DatabaseType::Postgres);

    let table = TableSchemaBuilder::new("users")
        .column(
            ColumnSchema::new("id", "BIGINT")
                .primary_key()
                .auto_increment(),
        )
        .column(ColumnSchema::new("email", "TEXT").not_null())
        .column(ColumnSchema::new("name", "TEXT"))
        .index(IndexDefinition::new(
            "idx_users_email",
            vec!["email".to_string()],
            false,
        ))
        .index(IndexDefinition::new(
            "uidx_users_email",
            vec!["email".to_string()],
            true,
        ))
        .build();

    generator.add_table(table);

    let sql = generator.generate();
    assert!(sql.contains("CREATE TABLE IF NOT EXISTS"));
    assert!(sql.contains("CREATE INDEX IF NOT EXISTS"));
    assert!(sql.contains("CREATE UNIQUE INDEX IF NOT EXISTS"));
}

#[test]
fn test_schema_generator_postgres() {
    let mut generator = SchemaGenerator::new(DatabaseType::Postgres);

    let table = TableSchemaBuilder::new("products")
        .column(
            ColumnSchema::new("id", "BIGINT")
                .primary_key()
                .auto_increment(),
        )
        .column(ColumnSchema::new("name", "VARCHAR(255)").not_null())
        .column(
            ColumnSchema::new("price", "DECIMAL(10,2)")
                .not_null()
                .default("0.00"),
        )
        .column(ColumnSchema::new("description", "TEXT"))
        .column(
            ColumnSchema::new("created_at", "TIMESTAMPTZ")
                .not_null()
                .default("NOW()"),
        )
        .build();

    generator.add_table(table);

    let sql = generator.generate();

    assert!(sql.contains("\"products\""));
    assert!(sql.contains("BIGSERIAL"));
    assert!(sql.contains("NOT NULL"));
    assert!(sql.contains("DEFAULT"));
}

#[test]
fn test_schema_generator_mysql() {
    let mut generator = SchemaGenerator::new(DatabaseType::MySQL);

    let table = TableSchemaBuilder::new("products")
        .column(
            ColumnSchema::new("id", "BIGINT")
                .primary_key()
                .auto_increment(),
        )
        .column(ColumnSchema::new("name", "VARCHAR(255)").not_null())
        .build();

    generator.add_table(table);

    let sql = generator.generate();

    assert!(sql.contains("`products`"));
    assert!(sql.contains("AUTO_INCREMENT"));
}

#[test]
fn test_schema_generator_mariadb() {
    let mut generator = SchemaGenerator::new(DatabaseType::MariaDB);

    let table = TableSchemaBuilder::new("products")
        .column(
            ColumnSchema::new("id", "BIGINT")
                .primary_key()
                .auto_increment(),
        )
        .column(ColumnSchema::new("name", "VARCHAR(255)").not_null())
        .build();

    generator.add_table(table);

    let sql = generator.generate();

    assert!(sql.contains("`products`"));
    assert!(sql.contains("AUTO_INCREMENT"));
}

#[test]
fn test_schema_generator_sqlite() {
    let mut generator = SchemaGenerator::new(DatabaseType::SQLite);

    let table = TableSchemaBuilder::new("products")
        .column(
            ColumnSchema::new("id", "INTEGER")
                .primary_key()
                .auto_increment(),
        )
        .column(ColumnSchema::new("name", "TEXT").not_null())
        .build();

    generator.add_table(table);

    let sql = generator.generate();

    assert!(sql.contains("\"products\""));
    assert!(sql.contains("INTEGER"));
}

#[test]
fn test_column_schema_builder() {
    let col = ColumnSchema::new("email", "VARCHAR(255)")
        .not_null()
        .default("''");

    assert_eq!(col.name, "email");
    assert_eq!(col.sql_type, "VARCHAR(255)");
    assert!(!col.nullable);
    assert_eq!(col.default, Some("''".to_string()));
    assert!(!col.primary_key);
    assert!(!col.auto_increment);
}

#[test]
fn test_column_schema_primary_key() {
    let col = ColumnSchema::new("id", "BIGINT")
        .primary_key()
        .auto_increment();

    assert!(col.primary_key);
    assert!(col.auto_increment);
    assert!(!col.nullable);
}

#[test]
fn test_table_schema_builder() {
    let table = TableSchemaBuilder::new("users")
        .column(ColumnSchema::new("id", "BIGINT").primary_key())
        .column(ColumnSchema::new("email", "TEXT").not_null())
        .index(IndexDefinition::new(
            "idx_email",
            vec!["email".to_string()],
            false,
        ))
        .build();

    assert_eq!(table.name, "users");
    assert_eq!(table.columns.len(), 2);
    assert_eq!(table.indexes.len(), 1);
    assert_eq!(table.primary_key, "id");
}

#[test]
fn test_table_schema_multiple_indexes() {
    let indexes = vec![
        IndexDefinition::new("idx_email", vec!["email".to_string()], false),
        IndexDefinition::new(
            "idx_name",
            vec!["first_name".to_string(), "last_name".to_string()],
            false,
        ),
        IndexDefinition::new("uidx_email", vec!["email".to_string()], true),
    ];

    let table = TableSchemaBuilder::new("users")
        .column(ColumnSchema::new("id", "BIGINT").primary_key())
        .indexes(indexes)
        .build();

    assert_eq!(table.indexes.len(), 3);
}

#[test]
fn test_schema_generator_supports_composite_primary_keys() {
    let table = TableSchemaBuilder::new("user_roles")
        .column(ColumnSchema::new("user_id", "BIGINT").primary_key())
        .column(ColumnSchema::new("role_id", "BIGINT").primary_key())
        .build();

    assert_eq!(table.primary_keys, vec!["user_id", "role_id"]);

    let mut generator = SchemaGenerator::new(DatabaseType::Postgres);
    generator.add_table(table);
    let sql = generator.generate();

    assert!(sql.contains("PRIMARY KEY (\"user_id\", \"role_id\")"));
}

#[test]
fn test_rust_type_to_sql_postgres() {
    assert_eq!(rust_type_to_sql("i64", DatabaseType::Postgres), "BIGINT");
    assert_eq!(rust_type_to_sql("i32", DatabaseType::Postgres), "INTEGER");
    assert_eq!(rust_type_to_sql("String", DatabaseType::Postgres), "TEXT");
    assert_eq!(rust_type_to_sql("bool", DatabaseType::Postgres), "BOOLEAN");
    assert_eq!(
        rust_type_to_sql("f64", DatabaseType::Postgres),
        "DOUBLE PRECISION"
    );
    assert_eq!(
        rust_type_to_sql("Option<i64>", DatabaseType::Postgres),
        "BIGINT"
    );
    assert_eq!(
        rust_type_to_sql("serde_json::Value", DatabaseType::Postgres),
        "JSONB"
    );
}

#[test]
fn test_rust_type_to_sql_mysql() {
    assert_eq!(rust_type_to_sql("i64", DatabaseType::MySQL), "BIGINT");
    assert_eq!(rust_type_to_sql("bool", DatabaseType::MySQL), "TINYINT(1)");
    assert_eq!(rust_type_to_sql("f64", DatabaseType::MySQL), "DOUBLE");
    assert_eq!(rust_type_to_sql("Uuid", DatabaseType::MySQL), "CHAR(36)");
    assert_eq!(rust_type_to_sql("Vec<i32>", DatabaseType::MySQL), "JSON");
    assert_eq!(rust_type_to_sql("Vec<i64>", DatabaseType::MySQL), "JSON");
    assert_eq!(rust_type_to_sql("Vec<String>", DatabaseType::MySQL), "JSON");
}

#[test]
fn test_rust_type_to_sql_mariadb() {
    assert_eq!(rust_type_to_sql("i64", DatabaseType::MariaDB), "BIGINT");
    assert_eq!(
        rust_type_to_sql("bool", DatabaseType::MariaDB),
        "TINYINT(1)"
    );
    assert_eq!(rust_type_to_sql("f64", DatabaseType::MariaDB), "DOUBLE");
    assert_eq!(rust_type_to_sql("Uuid", DatabaseType::MariaDB), "CHAR(36)");
    assert_eq!(rust_type_to_sql("Vec<i32>", DatabaseType::MariaDB), "JSON");
    assert_eq!(rust_type_to_sql("Vec<i64>", DatabaseType::MariaDB), "JSON");
    assert_eq!(
        rust_type_to_sql("Vec<String>", DatabaseType::MariaDB),
        "JSON"
    );
}

#[test]
fn test_rust_type_to_sql_sqlite() {
    assert_eq!(rust_type_to_sql("i64", DatabaseType::SQLite), "INTEGER");
    assert_eq!(rust_type_to_sql("i32", DatabaseType::SQLite), "INTEGER");
    assert_eq!(rust_type_to_sql("bool", DatabaseType::SQLite), "INTEGER");
    assert_eq!(rust_type_to_sql("f64", DatabaseType::SQLite), "REAL");
    assert_eq!(rust_type_to_sql("String", DatabaseType::SQLite), "TEXT");
}

#[test]
fn test_schema_generator_header() {
    let generator = SchemaGenerator::new(DatabaseType::Postgres);
    let sql = generator.generate();

    assert!(sql.contains("-- TideORM Generated Schema"));
    assert!(sql.contains("-- Database:"));
    assert!(sql.contains("-- Generated at:"));
}

#[test]
fn test_schema_writer_registry() {
    SchemaWriter::clear_registry();

    let table = TableSchemaBuilder::new("test_table")
        .column(ColumnSchema::new("id", "BIGINT").primary_key())
        .build();

    SchemaWriter::register_schema(table.clone());

    let schemas = SchemaWriter::get_registered_schemas();
    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].name, "test_table");

    SchemaWriter::register_schema(table);
    let schemas = SchemaWriter::get_registered_schemas();
    assert_eq!(schemas.len(), 1);

    SchemaWriter::clear_registry();
    let schemas = SchemaWriter::get_registered_schemas();
    assert!(schemas.is_empty());
}
