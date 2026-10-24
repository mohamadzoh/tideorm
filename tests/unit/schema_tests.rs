use super::writer::{CatalogColumn, catalog_table};
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
fn test_schema_generator_renders_each_backends_create_table() {
    let mysql = "CREATE TABLE IF NOT EXISTS `products` (
    `id` BIGINT AUTO_INCREMENT,
    `name` VARCHAR(255) NOT NULL,
    `price` DECIMAL(10,2) NOT NULL DEFAULT 0.00,
    `description` TEXT,
    PRIMARY KEY (`id`)
) DEFAULT CHARSET=utf8mb4;
";
    // SQLite numbers an `INTEGER` primary key itself, as its rowid.
    for (db_type, id_type, expected) in [
        (
            DatabaseType::Postgres,
            "BIGINT",
            r#"CREATE TABLE IF NOT EXISTS "products" (
    "id" BIGSERIAL,
    "name" VARCHAR(255) NOT NULL,
    "price" DECIMAL(10,2) NOT NULL DEFAULT 0.00,
    "description" TEXT,
    PRIMARY KEY ("id")
);
"#,
        ),
        (DatabaseType::MySQL, "BIGINT", mysql),
        (DatabaseType::MariaDB, "BIGINT", mysql),
        (
            DatabaseType::SQLite,
            "INTEGER",
            r#"CREATE TABLE IF NOT EXISTS "products" (
    "id" INTEGER NOT NULL,
    "name" VARCHAR(255) NOT NULL,
    "price" DECIMAL(10,2) NOT NULL DEFAULT 0.00,
    "description" TEXT,
    PRIMARY KEY ("id")
);
"#,
        ),
    ] {
        let table = TableSchemaBuilder::new("products")
            .column(
                ColumnSchema::new("id", id_type)
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
            .build();

        assert_eq!(
            SchemaGenerator::new(db_type).generate_create_table(&table),
            expected,
            "{db_type:?}"
        );
    }
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
fn test_schema_generator_writes_a_header_then_tables_then_indexes() {
    // MySQL has no CREATE INDEX IF NOT EXISTS; MariaDB does.
    for (db_type, indexes) in [
        (
            DatabaseType::Postgres,
            r#"CREATE INDEX IF NOT EXISTS "idx_users_email" ON "users" ("email");
CREATE UNIQUE INDEX IF NOT EXISTS "uidx_users_email" ON "users" ("email");
"#,
        ),
        (
            DatabaseType::MySQL,
            "CREATE INDEX `idx_users_email` ON `users` (`email`);
CREATE UNIQUE INDEX `uidx_users_email` ON `users` (`email`);
",
        ),
        (
            DatabaseType::MariaDB,
            "CREATE INDEX IF NOT EXISTS `idx_users_email` ON `users` (`email`);
CREATE UNIQUE INDEX IF NOT EXISTS `uidx_users_email` ON `users` (`email`);
",
        ),
        (
            DatabaseType::SQLite,
            r#"CREATE INDEX IF NOT EXISTS "idx_users_email" ON "users" ("email");
CREATE UNIQUE INDEX IF NOT EXISTS "uidx_users_email" ON "users" ("email");
"#,
        ),
    ] {
        let table = TableSchemaBuilder::new("users")
            .column(ColumnSchema::new("email", "TEXT").not_null())
            .index(IndexDefinition::new(
                "idx_users_email",
                vec!["email".to_string()],
                false,
            ))
            .indexes(vec![IndexDefinition::new(
                "uidx_users_email",
                vec!["email".to_string()],
                true,
            )])
            .build();
        let mut generator = SchemaGenerator::new(db_type);
        generator.add_table(table.clone());

        let sql = generator.generate();
        let header =
            format!("-- TideORM Generated Schema\n-- Database: {db_type:?}\n-- Generated at: ");
        assert!(sql.starts_with(&header), "{db_type:?}: {sql}");
        let tables_then_indexes =
            format!("{}\n{indexes}\n", generator.generate_create_table(&table));
        assert!(sql.ends_with(&tables_then_indexes), "{db_type:?}: {sql}");
    }
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

/// A schema given apart and one written into the table name qualify alike.
#[test]
fn test_schema_generator_qualifies_postgres_schema_names() {
    for table in [
        TableSchemaBuilder::new("posts").schema("public"),
        TableSchemaBuilder::new("public.posts"),
    ] {
        let table = table
            .column(
                ColumnSchema::new("id", "BIGINT")
                    .primary_key()
                    .auto_increment(),
            )
            .build();

        let sql = SchemaGenerator::new(DatabaseType::Postgres).generate_create_table(&table);
        assert!(
            sql.starts_with("CREATE TABLE IF NOT EXISTS \"public\".\"posts\""),
            "{sql}"
        );
    }
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
fn test_rust_types_map_to_the_column_each_backend_reads_back() {
    // MariaDB maps as MySQL does.
    for (rust_type, postgres, mysql, sqlite) in [
        ("i32", "INTEGER", "INT", "INTEGER"),
        ("i64", "BIGINT", "BIGINT", "INTEGER"),
        ("f64", "DOUBLE PRECISION", "DOUBLE", "REAL"),
        ("bool", "BOOLEAN", "TINYINT(1)", "INTEGER"),
        ("String", "TEXT", "LONGTEXT", "TEXT"),
        ("Uuid", "UUID", "BINARY(16)", "TEXT"),
        ("NaiveDate", "DATE", "DATE", "TEXT"),
        ("NaiveTime", "TIME", "TIME(6)", "TEXT"),
        ("serde_json::Value", "JSONB", "JSON", "TEXT"),
        ("Vec<i32>", "INTEGER[]", "JSON", "TEXT"),
        ("Vec<i64>", "BIGINT[]", "JSON", "TEXT"),
        ("Vec<String>", "TEXT[]", "JSON", "TEXT"),
        // PostgreSQL has no unsigned integers, so the mapping picks the signed
        // type sea-orm's decoder actually accepts. `u32` is read as an `Oid`
        // and then as an `i32`; widening it to BIGINT makes every read fail.
        // `u64` is only decodable on MySQL, and the binder narrows it to `i64`
        // on the way in, so an exact NUMERIC column buys nothing BIGINT does
        // not already give. MySQL does have unsigned column types, so nothing
        // widens there, and SQLite has one integer storage class for them all.
        ("u8", "SMALLINT", "TINYINT UNSIGNED", "INTEGER"),
        ("u16", "INTEGER", "SMALLINT UNSIGNED", "INTEGER"),
        ("u32", "INTEGER", "INT UNSIGNED", "INTEGER"),
        ("u64", "BIGINT", "BIGINT UNSIGNED", "INTEGER"),
        // TEXT would be the lossless target on SQLite, but sea-orm decodes both
        // Decimal and BigDecimal there through `try_get::<Option<f64>>`, and
        // sqlx only yields an f64 from a REAL-affinity column - a TEXT column
        // cannot be read at all.
        ("Decimal", "DECIMAL", "DECIMAL(65,30)", "REAL"),
        ("BigDecimal", "DECIMAL", "DECIMAL(65,30)", "REAL"),
        // i128/u128 map to Decimal { precision: 39, scale: 0 }, so they inherit
        // the decimal rendering - including SQLite's REAL, which cannot hold
        // the range.
        ("i128", "DECIMAL(39, 0)", "DECIMAL(39, 0)", "REAL"),
        ("u128", "DECIMAL(39, 0)", "DECIMAL(39, 0)", "REAL"),
        // A naive timestamp carries no offset, so it must not land in a column
        // the server shifts by session timezone, although "NaiveDateTime"
        // contains "DateTime". MySQL stores both in DATETIME(6): its TIMESTAMP
        // only spans 1970-2038, and sqlx pins the session to UTC.
        ("NaiveDateTime", "TIMESTAMP", "DATETIME(6)", "TEXT"),
        ("DateTime<Utc>", "TIMESTAMPTZ", "DATETIME(6)", "TEXT"),
    ] {
        for (db_type, expected) in [
            (DatabaseType::Postgres, postgres),
            (DatabaseType::MySQL, mysql),
            (DatabaseType::MariaDB, mysql),
            (DatabaseType::SQLite, sqlite),
        ] {
            assert_eq!(
                sql_for(rust_type, db_type),
                expected,
                "{rust_type} on {db_type:?}"
            );
        }
    }
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
    assert_eq!(sql_for("Option<NaiveDateTime>", pg), "TIMESTAMP");
    assert_eq!(sql_for("rust_decimal::Decimal", pg), "DECIMAL");
    assert_eq!(sql_for("Vec<serde_json::Value>", pg), "JSONB[]");
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
fn qualified_generic_types_have_mapping_parity() {
    // Keep this fixture aligned with the macro's generic-type parity test.
    for (plain, qualified) in [
        ("Vec<String>", "std::vec::Vec<std::string::String>"),
        ("Vec<Json>", "Vec<tideorm::types::Json>"),
        ("Vec<serde_json::Value>", "std::vec::Vec<serde_json::Value>"),
        (
            "Option<Vec<String>>",
            "std::option::Option<std::vec::Vec<std::string::String>>",
        ),
    ] {
        assert_eq!(
            sql_for(plain, DatabaseType::Postgres),
            sql_for(qualified, DatabaseType::Postgres)
        );
    }
}

#[test]
fn mysql_native_ddl_defers_only_foreign_keys() {
    let ddl = "CREATE TABLE `odd(table` (\n `id` int AUTO_INCREMENT,\n `txt` varchar(100) DEFAULT 'a,b)it\\'s',\n UNIQUE KEY `expr` ((lower(`txt`))),\n CONSTRAINT `fk` FOREIGN KEY (`id`) REFERENCES `later` (`id`),\n CONSTRAINT `ck` CHECK (`id` > 0)\n) ENGINE=InnoDB";
    let (table, keys) = super::writer::mysql_defer_foreign_keys(ddl, "odd(table").unwrap();
    assert_eq!(
        keys,
        [
            "ALTER TABLE `odd(table` ADD CONSTRAINT `fk` FOREIGN KEY (`id`) REFERENCES `later` (`id`)"
        ]
    );
    assert!(!table.contains("FOREIGN KEY"));
    assert!(table.contains("UNIQUE KEY `expr` ((lower(`txt`)))"));
    assert!(table.contains("DEFAULT 'a,b)it\\'s'"));
    assert!(table.ends_with(") ENGINE=InnoDB"));
    let (same, keys) = super::writer::mysql_defer_foreign_keys(&table, "odd(table").unwrap();
    assert_eq!(same, table);
    assert!(keys.is_empty());
}
