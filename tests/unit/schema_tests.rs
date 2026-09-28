use super::writer::{CatalogColumn, CatalogIndexColumn, catalog_table, group_indexes};
use super::*;
use crate::config::DatabaseType;
use crate::model::IndexDefinition;

/// What a Rust type renders to on `db_type` through the shared mapping table.
#[track_caller]
fn sql_for(rust_type: &str, db_type: DatabaseType) -> String {
    rust_type_to_column_type(rust_type)
        .unwrap_or_else(|| panic!("'{rust_type}' should be mapped"))
        .to_sql(db_type)
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
fn test_schema_generator_mysql_family() {
    for database_type in [DatabaseType::MySQL, DatabaseType::MariaDB] {
        let mut generator = SchemaGenerator::new(database_type);

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

        assert!(sql.contains("`products`"), "{database_type:?}");
        assert!(sql.contains("AUTO_INCREMENT"), "{database_type:?}");
    }
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
    assert_eq!(table.primary_keys, vec!["id"]);
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
fn test_rust_type_mapping_postgres() {
    assert_eq!(sql_for("i64", DatabaseType::Postgres), "BIGINT");
    assert_eq!(sql_for("i32", DatabaseType::Postgres), "INTEGER");
    assert_eq!(sql_for("String", DatabaseType::Postgres), "TEXT");
    assert_eq!(sql_for("bool", DatabaseType::Postgres), "BOOLEAN");
    assert_eq!(sql_for("f64", DatabaseType::Postgres), "DOUBLE PRECISION");
    assert_eq!(sql_for("Option<i64>", DatabaseType::Postgres), "BIGINT");
    assert_eq!(sql_for("Uuid", DatabaseType::Postgres), "UUID");
    assert_eq!(sql_for("Uuid", DatabaseType::SQLite), "TEXT");
    assert_eq!(sql_for("NaiveDate", DatabaseType::Postgres), "DATE");
    assert_eq!(sql_for("NaiveTime", DatabaseType::Postgres), "TIME");
    // "NaiveDateTime" contains "DateTime": a naive column must not become the
    // session-shifted TIMESTAMPTZ.
    assert_eq!(
        sql_for("Option<NaiveDateTime>", DatabaseType::Postgres),
        "TIMESTAMP"
    );
    assert_eq!(
        sql_for("serde_json::Value", DatabaseType::Postgres),
        "JSONB"
    );
}

#[test]
fn test_rust_type_mapping_maps_unsigned_types_to_what_postgres_reads_back() {
    // PostgreSQL has no unsigned integers, so the mapping picks the signed type
    // sea-orm's decoder actually accepts. `u32` is read as an `Oid` and then as
    // an `i32`; widening it to BIGINT makes every read fail. `u64` is only
    // decodable on MySQL, and the binder narrows it to `i64` on the way in, so
    // an exact NUMERIC column buys nothing BIGINT does not already give.
    let pg = DatabaseType::Postgres;
    assert_eq!(sql_for("u8", pg), "SMALLINT");
    assert_eq!(sql_for("u16", pg), "INTEGER");
    assert_eq!(sql_for("u32", pg), "INTEGER");
    assert_eq!(sql_for("u64", pg), "BIGINT");
    assert_eq!(sql_for("Option<u32>", pg), "INTEGER");

    // MySQL does have unsigned column types, so nothing widens there.
    let mysql = DatabaseType::MySQL;
    assert_eq!(sql_for("u32", mysql), "INT UNSIGNED");
    assert_eq!(sql_for("u64", mysql), "BIGINT UNSIGNED");

    // SQLite has one integer storage class for all of them.
    let sqlite = DatabaseType::SQLite;
    assert_eq!(sql_for("u32", sqlite), "INTEGER");
    assert_eq!(sql_for("u64", sqlite), "INTEGER");
}

#[test]
fn test_rust_type_mapping_keeps_decimals_readable() {
    // TEXT would be the lossless target on SQLite, but sea-orm decodes both
    // Decimal and BigDecimal there through `try_get::<Option<f64>>`, and sqlx
    // only yields an f64 from a REAL-affinity column - a TEXT column cannot be
    // read at all.
    let pg = DatabaseType::Postgres;
    let sqlite = DatabaseType::SQLite;
    assert_eq!(sql_for("Decimal", pg), "DECIMAL");
    assert_eq!(sql_for("Decimal", sqlite), "REAL");
    assert_eq!(sql_for("BigDecimal", sqlite), "REAL");
    assert_eq!(sql_for("rust_decimal::Decimal", sqlite), "REAL");
}

#[test]
fn test_rust_type_mapping_maps_128_bit_integers_per_backend() {
    // i128/u128 map to Decimal { precision: 39, scale: 0 }, so they inherit the
    // decimal rendering - including SQLite's REAL, which cannot hold the range.
    let pg = DatabaseType::Postgres;
    assert_eq!(sql_for("i128", pg), "DECIMAL(39, 0)");
    assert_eq!(sql_for("u128", pg), "DECIMAL(39, 0)");
    assert_eq!(sql_for("i128", DatabaseType::MySQL), "DECIMAL(39, 0)");
    assert_eq!(sql_for("i128", DatabaseType::SQLite), "REAL");
}

#[test]
fn test_rust_type_mapping_mysql_family() {
    for database_type in [DatabaseType::MySQL, DatabaseType::MariaDB] {
        assert_eq!(sql_for("i64", database_type), "BIGINT");
        assert_eq!(sql_for("bool", database_type), "TINYINT(1)");
        assert_eq!(sql_for("f64", database_type), "DOUBLE");
        assert_eq!(sql_for("Uuid", database_type), "BINARY(16)");
        assert_eq!(sql_for("Vec<i32>", database_type), "JSON");
        assert_eq!(sql_for("Vec<i64>", database_type), "JSON");
        assert_eq!(sql_for("Vec<String>", database_type), "JSON");
    }
}

#[test]
fn test_rust_type_mapping_sqlite() {
    assert_eq!(sql_for("i64", DatabaseType::SQLite), "INTEGER");
    assert_eq!(sql_for("i32", DatabaseType::SQLite), "INTEGER");
    assert_eq!(sql_for("bool", DatabaseType::SQLite), "INTEGER");
    assert_eq!(sql_for("f64", DatabaseType::SQLite), "REAL");
    assert_eq!(sql_for("String", DatabaseType::SQLite), "TEXT");
}

#[test]
fn test_rust_type_to_column_type_reports_unknown_types() {
    // The fallback belongs to the caller: sync warns and uses TEXT.
    assert!(rust_type_to_column_type("MyCustomType").is_none());
}

#[test]
fn test_rust_type_normalization_strips_paths_lifetimes_and_options() {
    let pg = DatabaseType::Postgres;
    assert_eq!(sql_for("chrono::DateTime<chrono::Utc>", pg), "TIMESTAMPTZ");
    assert_eq!(sql_for("&'static str", pg), "TEXT");
    assert_eq!(sql_for("Option < i32 >", pg), "INTEGER");
    assert_eq!(sql_for("Option<Option<String>>", pg), "TEXT");
    assert_eq!(sql_for("rust_decimal::Decimal", pg), "DECIMAL");
    assert_eq!(sql_for("Vec<serde_json::Value>", pg), "JSONB[]");
}

#[test]
fn test_naive_and_aware_timestamps_get_different_columns() {
    // A naive timestamp carries no offset, so it must not land in a column the
    // server shifts by session timezone. MySQL stores both in DATETIME(6): its
    // TIMESTAMP only spans 1970-2038, and sqlx pins the session to UTC.
    let mysql = DatabaseType::MySQL;
    assert_eq!(sql_for("NaiveDateTime", mysql), "DATETIME(6)");
    assert_eq!(sql_for("DateTime<Utc>", mysql), "DATETIME(6)");

    let pg = DatabaseType::Postgres;
    assert_eq!(sql_for("NaiveDateTime", pg), "TIMESTAMP");
    assert_eq!(sql_for("DateTime<Utc>", pg), "TIMESTAMPTZ");
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

fn catalog_column(name: &str, sql_type: &str) -> CatalogColumn {
    CatalogColumn {
        name: name.to_string(),
        sql_type: sql_type.to_string(),
        nullable: false,
        default: None,
        auto_increment: false,
    }
}

fn index_column(index: &str, unique: bool, column: &str) -> CatalogIndexColumn {
    CatalogIndexColumn {
        index: index.to_string(),
        unique,
        column: column.to_string(),
    }
}

#[test]
fn test_catalog_indexes_come_out_sorted_and_keep_key_order() {
    // Grouping used to go through a HashMap, so the same database exported
    // its indexes in a different order on every run.
    let indexes = group_indexes([
        index_column("idx_users_tenant_email", false, "tenant_id"),
        index_column("idx_users_tenant_email", false, "email"),
        index_column("idx_b", true, "b"),
        index_column("idx_a", false, "a"),
    ]);

    let names: Vec<&str> = indexes.iter().map(|index| index.name.as_str()).collect();
    assert_eq!(names, vec!["idx_a", "idx_b", "idx_users_tenant_email"]);
    assert!(indexes[1].unique);
    assert_eq!(indexes[2].columns, vec!["tenant_id", "email"]);
}

#[test]
fn test_catalog_table_keeps_every_primary_key_column_in_key_order() {
    // Only the first key column used to survive introspection, which exported
    // a composite key as a single-column one.
    let table = catalog_table(
        "user_roles",
        Some("public"),
        vec![
            catalog_column("role_id", "bigint"),
            catalog_column("user_id", "bigint"),
            catalog_column("note", "text"),
        ],
        vec!["user_id".to_string(), "role_id".to_string()],
        Vec::new(),
        true,
    );

    assert_eq!(table.primary_keys, vec!["user_id", "role_id"]);
    assert!(table.columns[0].primary_key && table.columns[1].primary_key);
    assert!(!table.columns[2].primary_key);

    let mut generator = SchemaGenerator::new(DatabaseType::Postgres);
    generator.add_table(table);
    let sql = generator.generate();
    assert!(
        sql.contains("PRIMARY KEY (\"user_id\", \"role_id\")"),
        "Got: {}",
        sql
    );
}

#[test]
fn test_catalog_table_auto_increments_key_columns_unless_any_column_counts() {
    let columns = || {
        let mut id = catalog_column("id", "bigint");
        id.auto_increment = true;
        let mut counter = catalog_column("counter", "integer");
        counter.auto_increment = true;
        counter.default = Some("0".to_string());
        vec![id, counter]
    };

    let table = catalog_table(
        "events",
        None,
        columns(),
        vec!["id".to_string()],
        Vec::new(),
        false,
    );

    assert!(table.columns[0].auto_increment);
    assert!(!table.columns[1].auto_increment);
    assert_eq!(table.columns[1].default.as_deref(), Some("0"));
    assert_eq!(table.schema_name, None);

    // PostgreSQL's serial type numbers a column that is not the key, as an
    // identity column does.
    let table = catalog_table(
        "events",
        Some("public"),
        columns(),
        vec!["id".to_string()],
        Vec::new(),
        true,
    );
    assert!(table.columns[0].auto_increment && table.columns[1].auto_increment);
}

#[test]
fn test_mysql_defaults_are_restored_to_a_default_clause() {
    use super::writer::mysql_default;

    let default = |value: &str, sql_type: &str, extra: &str| {
        mysql_default(Some(value.to_string()), sql_type, extra, false)
    };
    // MySQL 8 reports a literal bare and an expression without parentheses.
    assert_eq!(
        default("draft", "varchar(20)", "").as_deref(),
        Some("'draft'")
    );
    assert_eq!(default("", "varchar(20)", "").as_deref(), Some("''"));
    assert_eq!(default("it's", "text", "").as_deref(), Some("'it''s'"));
    assert_eq!(
        default(r"C:\temp", "varchar(20)", "").as_deref(),
        Some(r"'C:\\temp'")
    );
    assert_eq!(default("5", "int", "").as_deref(), Some("5"));
    assert_eq!(
        default("1.50", "decimal(10,2)", "").as_deref(),
        Some("1.50")
    );
    assert_eq!(
        default("CURRENT_TIMESTAMP(6)", "datetime(6)", "DEFAULT_GENERATED").as_deref(),
        Some("CURRENT_TIMESTAMP(6)")
    );
    assert_eq!(
        default("CURRENT_TIMESTAMP", "timestamp", "DEFAULT_GENERATED").as_deref(),
        Some("CURRENT_TIMESTAMP")
    );
    // Text that only starts like one is a literal, as is the same text in a
    // column that takes no current-time default.
    assert_eq!(
        default("CURRENT_TIMESTAMP is text", "varchar(100)", "").as_deref(),
        Some("'CURRENT_TIMESTAMP is text'")
    );
    assert_eq!(
        default("CURRENT_TIMESTAMP", "varchar(20)", "").as_deref(),
        Some("'CURRENT_TIMESTAMP'")
    );
    assert_eq!(
        default(r"_utf8mb4\'x\'", "longtext", "DEFAULT_GENERATED").as_deref(),
        Some("(_utf8mb4'x')")
    );
    assert_eq!(
        default("uuid()", "char(36)", "DEFAULT_GENERATED").as_deref(),
        Some("(uuid())")
    );
    assert_eq!(mysql_default(None, "varchar(20)", "", false), None);
    // MariaDB reports the clause's own spelling.
    assert_eq!(
        mysql_default(Some("'draft'".to_string()), "varchar(20)", "", true).as_deref(),
        Some("'draft'")
    );
}
