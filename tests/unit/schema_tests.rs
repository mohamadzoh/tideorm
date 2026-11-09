use super::writer::{CatalogColumn, catalog_table};
use super::*;
use crate::config::DatabaseType;
use crate::migration::{ColumnType, column_type_sql};
use crate::model::IndexDefinition;
use crate::orm::sql_builder::ValueType;

/// The column type a field of type `T` gets: its `ValueType`'s, the one
/// mapping the model's `Column::def()`, schema sync and migrations share.
fn column<T: ValueType>() -> ColumnType {
    T::column_type()
}

/// A newtype takes its inner type's column.
#[derive(Clone, Debug, PartialEq, crate::orm::DeriveValueType)]
struct Cents(i64);

/// `rows` of a column type and its PostgreSQL, MySQL and SQLite SQL.
#[track_caller]
fn assert_renders(rows: &[(ColumnType, &str, &str, &str)]) {
    for (column_type, postgres, mysql, sqlite) in rows {
        for (db_type, expected) in [
            (DatabaseType::Postgres, postgres),
            (DatabaseType::MySQL, mysql),
            (DatabaseType::MariaDB, mysql),
            (DatabaseType::SQLite, sqlite),
        ] {
            assert_eq!(
                column_type_sql(column_type, db_type),
                *expected,
                "{column_type:?} on {db_type:?}"
            );
        }
    }
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
    assert_renders(&[
        (column::<i8>(), "SMALLINT", "TINYINT", "INTEGER"),
        (column::<i16>(), "SMALLINT", "SMALLINT", "INTEGER"),
        (column::<i32>(), "INTEGER", "INT", "INTEGER"),
        (column::<i64>(), "BIGINT", "BIGINT", "INTEGER"),
        (column::<f32>(), "REAL", "FLOAT", "REAL"),
        (column::<f64>(), "DOUBLE PRECISION", "DOUBLE", "REAL"),
        (column::<bool>(), "BOOLEAN", "TINYINT(1)", "INTEGER"),
        (column::<String>(), "TEXT", "LONGTEXT", "TEXT"),
        (
            column::<Option<crate::types::Text>>(),
            "TEXT",
            "LONGTEXT",
            "TEXT",
        ),
        (column::<Vec<u8>>(), "BYTEA", "LONGBLOB", "BLOB"),
        (column::<uuid::Uuid>(), "UUID", "BINARY(16)", "TEXT"),
        (column::<chrono::NaiveDate>(), "DATE", "DATE", "TEXT"),
        (column::<chrono::NaiveTime>(), "TIME", "TIME(6)", "TEXT"),
        (column::<serde_json::Value>(), "JSONB", "JSON", "TEXT"),
        (column::<crate::types::Jsonb>(), "JSONB", "JSON", "TEXT"),
        (column::<Cents>(), "BIGINT", "BIGINT", "INTEGER"),
        // PostgreSQL has no unsigned integers, so each lands on the narrowest
        // signed type holding all its values, which the driver reads it from;
        // a `u64` past `i64::MAX` is refused when it is bound. MySQL does have
        // unsigned column types, so nothing widens there, and SQLite has one
        // integer storage class for them all.
        (column::<u8>(), "SMALLINT", "TINYINT UNSIGNED", "INTEGER"),
        (column::<u16>(), "INTEGER", "SMALLINT UNSIGNED", "INTEGER"),
        (column::<u32>(), "BIGINT", "INT UNSIGNED", "INTEGER"),
        (column::<u64>(), "BIGINT", "BIGINT UNSIGNED", "INTEGER"),
        // TEXT would be the lossless target on SQLite, but a REAL column
        // compares and sorts as a number.
        (
            column::<rust_decimal::Decimal>(),
            "DECIMAL",
            "DECIMAL(65,30)",
            "REAL",
        ),
        // A naive timestamp carries no offset, so it must not land in a column
        // the server shifts by session timezone. MySQL stores every timestamp
        // in DATETIME(6): its TIMESTAMP only spans 1970-2038, and the driver
        // pins the session to UTC.
        (
            column::<chrono::NaiveDateTime>(),
            "TIMESTAMP",
            "DATETIME(6)",
            "TEXT",
        ),
        (
            column::<chrono::DateTime<chrono::Utc>>(),
            "TIMESTAMPTZ",
            "DATETIME(6)",
            "TEXT",
        ),
        (
            column::<chrono::DateTime<chrono::FixedOffset>>(),
            "TIMESTAMPTZ",
            "DATETIME(6)",
            "TEXT",
        ),
    ]);
}

#[cfg(feature = "postgres")]
#[test]
fn test_array_types_map_to_postgres_arrays_and_json_elsewhere() {
    assert_renders(&[
        (column::<Vec<i32>>(), "INTEGER[]", "JSON", "TEXT"),
        (column::<Vec<i64>>(), "BIGINT[]", "JSON", "TEXT"),
        (column::<Vec<f64>>(), "DOUBLE PRECISION[]", "JSON", "TEXT"),
        (column::<Vec<bool>>(), "BOOLEAN[]", "JSON", "TEXT"),
        (column::<Vec<String>>(), "TEXT[]", "JSON", "TEXT"),
        (
            column::<crate::types::TextArray>(),
            "TEXT[]",
            "JSON",
            "TEXT",
        ),
        (
            column::<Vec<serde_json::Value>>(),
            "JSONB[]",
            "JSON",
            "TEXT",
        ),
    ]);
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
