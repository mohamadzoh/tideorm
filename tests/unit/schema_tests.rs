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
fn test_schema_generator_postgres_preserves_serial_width() {
    let mut generator = SchemaGenerator::new(DatabaseType::Postgres);
    generator.add_table(
        TableSchemaBuilder::new("small")
            .column(
                ColumnSchema::new("id", "INTEGER")
                    .primary_key()
                    .auto_increment(),
            )
            .build(),
    );
    generator.add_table(
        TableSchemaBuilder::new("tiny")
            .column(
                ColumnSchema::new("id", "SMALLINT")
                    .primary_key()
                    .auto_increment(),
            )
            .build(),
    );

    let sql = generator.generate();

    assert!(
        sql.contains("\"id\" SERIAL"),
        "An INTEGER key must stay 4 bytes. Got: {}",
        sql
    );
    assert!(
        sql.contains("\"id\" SMALLSERIAL"),
        "A SMALLINT key must stay 2 bytes. Got: {}",
        sql
    );
    assert!(
        !sql.contains("BIGSERIAL"),
        "No declared width here widens to 8 bytes. Got: {}",
        sql
    );
}

#[test]
fn test_schema_generator_omits_index_if_not_exists_for_mysql() {
    let table = TableSchemaBuilder::new("users")
        .column(ColumnSchema::new("email", "TEXT").not_null())
        .index(IndexDefinition::new(
            "idx_users_email",
            vec!["email".to_string()],
            false,
        ))
        .build();

    let mut mysql = SchemaGenerator::new(DatabaseType::MySQL);
    mysql.add_table(table.clone());
    let mysql_sql = mysql.generate();
    assert!(
        mysql_sql.contains("CREATE INDEX `idx_users_email`"),
        "Got: {}",
        mysql_sql
    );
    assert!(
        !mysql_sql.contains("IF NOT EXISTS `idx_users_email`"),
        "MySQL has no CREATE INDEX IF NOT EXISTS. Got: {}",
        mysql_sql
    );

    let mut mariadb = SchemaGenerator::new(DatabaseType::MariaDB);
    mariadb.add_table(table);
    let mariadb_sql = mariadb.generate();
    assert!(
        mariadb_sql.contains("CREATE INDEX IF NOT EXISTS `idx_users_email`"),
        "MariaDB does support it. Got: {}",
        mariadb_sql
    );
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
fn test_schema_generator_escapes_embedded_identifier_quotes() {
    let mut postgres = SchemaGenerator::new(DatabaseType::Postgres);
    postgres.add_table(
        TableSchemaBuilder::new("user\"roles")
            .column(ColumnSchema::new("display\"name", "TEXT").not_null())
            .build(),
    );
    let postgres_sql = postgres.generate();
    assert!(postgres_sql.contains("\"user\"\"roles\""));
    assert!(postgres_sql.contains("\"display\"\"name\" TEXT"));

    let mut mysql = SchemaGenerator::new(DatabaseType::MySQL);
    mysql.add_table(
        TableSchemaBuilder::new("user`roles")
            .column(ColumnSchema::new("display`name", "TEXT").not_null())
            .build(),
    );
    let mysql_sql = mysql.generate();
    assert!(mysql_sql.contains("`user``roles`"));
    assert!(mysql_sql.contains("`display``name` TEXT"));
}

#[test]
fn test_schema_generator_qualifies_postgres_schema_names() {
    let mut generator = SchemaGenerator::new(DatabaseType::Postgres);
    generator.add_table(
        TableSchemaBuilder::new("posts")
            .schema("public")
            .column(
                ColumnSchema::new("id", "BIGINT")
                    .primary_key()
                    .auto_increment(),
            )
            .build(),
    );

    let sql = generator.generate();
    assert!(sql.contains("CREATE TABLE IF NOT EXISTS \"public\".\"posts\""));
}

#[test]
fn test_schema_generator_supports_identifier_references() {
    let mut generator = SchemaGenerator::new(DatabaseType::Postgres);
    generator.add_table(
        TableSchemaBuilder::new("public.posts")
            .column(
                ColumnSchema::new("id", "BIGINT")
                    .primary_key()
                    .auto_increment(),
            )
            .build(),
    );

    let sql = generator.generate();
    assert!(sql.contains("CREATE TABLE IF NOT EXISTS \"public\".\"posts\""));
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
fn test_rust_type_to_sql_maps_unsigned_types_to_what_postgres_reads_back() {
    // PostgreSQL has no unsigned integers, so the mapping picks the signed type
    // sea-orm's decoder actually accepts. `u32` is read as an `Oid` and then as
    // an `i32`; widening it to BIGINT makes every read fail. `u64` is only
    // decodable on MySQL, and the binder narrows it to `i64` on the way in, so
    // an exact NUMERIC column buys nothing BIGINT does not already give.
    let pg = DatabaseType::Postgres;
    assert_eq!(rust_type_to_sql("u8", pg), "SMALLINT");
    assert_eq!(rust_type_to_sql("u16", pg), "INTEGER");
    assert_eq!(rust_type_to_sql("u32", pg), "INTEGER");
    assert_eq!(rust_type_to_sql("u64", pg), "BIGINT");
    assert_eq!(rust_type_to_sql("Option<u32>", pg), "INTEGER");

    // MySQL does have unsigned column types, so nothing widens there.
    let mysql = DatabaseType::MySQL;
    assert_eq!(rust_type_to_sql("u32", mysql), "INT UNSIGNED");
    assert_eq!(rust_type_to_sql("u64", mysql), "BIGINT UNSIGNED");

    // SQLite has one integer storage class for all of them.
    let sqlite = DatabaseType::SQLite;
    assert_eq!(rust_type_to_sql("u32", sqlite), "INTEGER");
    assert_eq!(rust_type_to_sql("u64", sqlite), "INTEGER");
}

#[test]
fn test_rust_type_to_sql_keeps_decimals_readable() {
    // TEXT would be the lossless target on SQLite, but sea-orm decodes both
    // Decimal and BigDecimal there through `try_get::<Option<f64>>`, and sqlx
    // only yields an f64 from a REAL-affinity column - a TEXT column cannot be
    // read at all.
    let pg = DatabaseType::Postgres;
    let sqlite = DatabaseType::SQLite;
    assert_eq!(rust_type_to_sql("Decimal", pg), "DECIMAL");
    assert_eq!(rust_type_to_sql("Decimal", sqlite), "REAL");
    assert_eq!(rust_type_to_sql("BigDecimal", sqlite), "REAL");
    assert_eq!(rust_type_to_sql("rust_decimal::Decimal", sqlite), "REAL");
}

#[test]
fn test_rust_type_to_sql_maps_128_bit_integers_per_backend() {
    // i128/u128 map to Decimal { precision: 39, scale: 0 }, so they inherit the
    // decimal rendering - including SQLite's REAL, which cannot hold the range.
    let pg = DatabaseType::Postgres;
    assert_eq!(rust_type_to_sql("i128", pg), "DECIMAL(39, 0)");
    assert_eq!(rust_type_to_sql("u128", pg), "DECIMAL(39, 0)");
    assert_eq!(
        rust_type_to_sql("i128", DatabaseType::MySQL),
        "DECIMAL(39, 0)"
    );
    assert_eq!(rust_type_to_sql("i128", DatabaseType::SQLite), "REAL");
}

#[test]
fn test_rust_type_to_sql_mysql() {
    assert_eq!(rust_type_to_sql("i64", DatabaseType::MySQL), "BIGINT");
    assert_eq!(rust_type_to_sql("bool", DatabaseType::MySQL), "TINYINT(1)");
    assert_eq!(rust_type_to_sql("f64", DatabaseType::MySQL), "DOUBLE");
    assert_eq!(rust_type_to_sql("Uuid", DatabaseType::MySQL), "BINARY(16)");
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
    assert_eq!(
        rust_type_to_sql("Uuid", DatabaseType::MariaDB),
        "BINARY(16)"
    );
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
fn test_rust_type_to_column_type_is_the_single_mapping_table() {
    // `rust_type_to_sql` must be nothing but this lookup plus a render, so the
    // schema writer, sync and migrations cannot drift apart.
    for rust_type in ["i64", "u32", "Decimal", "DateTime<Utc>", "Vec<String>"] {
        let mapped = rust_type_to_column_type(rust_type).expect("mapped type");
        for db_type in [
            DatabaseType::Postgres,
            DatabaseType::MySQL,
            DatabaseType::MariaDB,
            DatabaseType::SQLite,
        ] {
            let rendered = rust_type_to_sql(rust_type, db_type);
            assert_eq!(rendered, mapped.to_sql(db_type));
        }
    }
}

#[test]
fn test_rust_type_to_column_type_reports_unknown_types() {
    // The fallback belongs to the caller: schema export uses TEXT silently,
    // sync warns first.
    assert!(rust_type_to_column_type("MyCustomType").is_none());
    assert_eq!(
        rust_type_to_sql("MyCustomType", DatabaseType::Postgres),
        "TEXT"
    );
}

#[test]
fn test_rust_type_normalization_strips_paths_lifetimes_and_options() {
    let pg = DatabaseType::Postgres;
    assert_eq!(
        rust_type_to_sql("chrono::DateTime<chrono::Utc>", pg),
        "TIMESTAMPTZ"
    );
    assert_eq!(rust_type_to_sql("&'static str", pg), "TEXT");
    assert_eq!(rust_type_to_sql("Option < i32 >", pg), "INTEGER");
    assert_eq!(rust_type_to_sql("Option<Option<String>>", pg), "TEXT");
    assert_eq!(rust_type_to_sql("rust_decimal::Decimal", pg), "DECIMAL");
    assert_eq!(rust_type_to_sql("Vec<serde_json::Value>", pg), "JSONB[]");
}

#[test]
fn test_naive_and_aware_timestamps_get_different_columns() {
    // A naive timestamp carries no offset, so it must not land in a column the
    // server shifts by session timezone.
    let mysql = DatabaseType::MySQL;
    assert_eq!(rust_type_to_sql("NaiveDateTime", mysql), "DATETIME");
    assert_eq!(rust_type_to_sql("DateTime<Utc>", mysql), "TIMESTAMP");

    let pg = DatabaseType::Postgres;
    assert_eq!(rust_type_to_sql("NaiveDateTime", pg), "TIMESTAMP");
    assert_eq!(rust_type_to_sql("DateTime<Utc>", pg), "TIMESTAMPTZ");
}

#[test]
fn test_migration_decimals_stay_readable_on_sqlite() {
    // sea-orm decodes Decimal/BigDecimal on SQLite through
    // `try_get::<Option<f64>>`, and sqlx only produces an f64 from a
    // REAL-affinity column. A TEXT column is exact but unreadable, so REAL is
    // forced - the precision loss is a documented limitation.
    use crate::migration::ColumnType;

    let scaled = ColumnType::Decimal {
        precision: 12,
        scale: 2,
    };
    assert_eq!(scaled.to_sqlite_sql(), "REAL");
    assert_eq!(ColumnType::Numeric.to_sqlite_sql(), "REAL");
    assert_eq!(scaled.to_postgres_sql(), "DECIMAL(12, 2)");
    assert_eq!(ColumnType::Numeric.to_mysql_sql(), "DECIMAL(65,30)");

    // Floats still render as floats.
    assert_eq!(ColumnType::Double.to_sqlite_sql(), "REAL");
}

#[test]
fn test_migration_column_type_to_sql_dispatches_per_backend() {
    use crate::migration::ColumnType;

    assert_eq!(
        ColumnType::Unsigned.to_sql(DatabaseType::Postgres),
        ColumnType::Unsigned.to_postgres_sql()
    );
    assert_eq!(
        ColumnType::Unsigned.to_sql(DatabaseType::MariaDB),
        ColumnType::Unsigned.to_mysql_sql()
    );
    assert_eq!(
        ColumnType::Unsigned.to_sql(DatabaseType::SQLite),
        ColumnType::Unsigned.to_sqlite_sql()
    );
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
    assert_eq!(schemas[0].schema_name, None);

    SchemaWriter::register_schema(table);
    let schemas = SchemaWriter::get_registered_schemas();
    assert_eq!(schemas.len(), 1);

    SchemaWriter::clear_registry();
    let schemas = SchemaWriter::get_registered_schemas();
    assert!(schemas.is_empty());
}

#[test]
fn test_schema_writer_registry_keeps_distinct_schemas() {
    SchemaWriter::clear_registry();

    let public_table = TableSchemaBuilder::new("posts")
        .schema("public")
        .column(ColumnSchema::new("id", "BIGINT").primary_key())
        .build();
    let audit_table = TableSchemaBuilder::new("posts")
        .schema("audit")
        .column(ColumnSchema::new("id", "BIGINT").primary_key())
        .build();

    SchemaWriter::register_schema(public_table);
    SchemaWriter::register_schema(audit_table);

    let schemas = SchemaWriter::get_registered_schemas();
    assert_eq!(schemas.len(), 2);
    assert!(schemas.iter().any(|schema| {
        schema.name == "posts" && schema.schema_name.as_deref() == Some("public")
    }));
    assert!(schemas.iter().any(|schema| {
        schema.name == "posts" && schema.schema_name.as_deref() == Some("audit")
    }));

    SchemaWriter::clear_registry();
}
