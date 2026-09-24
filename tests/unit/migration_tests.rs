use super::*;
use crate::internal::sql_safety::quote_ident;

#[test]
fn test_column_type_postgres() {
    assert_eq!(ColumnType::Integer.to_postgres_sql(), "INTEGER");
    assert_eq!(ColumnType::BigInteger.to_postgres_sql(), "BIGINT");
    assert_eq!(ColumnType::String.to_postgres_sql(), "VARCHAR(255)");
    assert_eq!(ColumnType::Text.to_postgres_sql(), "TEXT");
    assert_eq!(ColumnType::Boolean.to_postgres_sql(), "BOOLEAN");
    assert_eq!(ColumnType::Jsonb.to_postgres_sql(), "JSONB");
    assert_eq!(ColumnType::IntegerArray.to_postgres_sql(), "INTEGER[]");
    assert_eq!(ColumnType::Timestamp.to_postgres_sql(), "TIMESTAMP");
    assert_eq!(ColumnType::TimestampTz.to_postgres_sql(), "TIMESTAMPTZ");
    assert_eq!(ColumnType::Date.to_postgres_sql(), "DATE");
    assert_eq!(ColumnType::Time.to_postgres_sql(), "TIME");
}

#[test]
fn test_column_type_mysql() {
    assert_eq!(ColumnType::Integer.to_mysql_sql(), "INT");
    assert_eq!(ColumnType::BigInteger.to_mysql_sql(), "BIGINT");
    assert_eq!(ColumnType::Boolean.to_mysql_sql(), "TINYINT(1)");
    assert_eq!(ColumnType::Jsonb.to_mysql_sql(), "JSON");
    // DATETIME(6): MySQL's TIMESTAMP only spans 1970-2038, and a column without
    // fractional digits rounds away microseconds.
    assert_eq!(ColumnType::DateTime.to_mysql_sql(), "DATETIME(6)");
    assert_eq!(ColumnType::Timestamp.to_mysql_sql(), "DATETIME(6)");
    assert_eq!(ColumnType::TimestampTz.to_mysql_sql(), "DATETIME(6)");
    assert_eq!(ColumnType::Date.to_mysql_sql(), "DATE");
    assert_eq!(ColumnType::Time.to_mysql_sql(), "TIME(6)");
    // TEXT and BLOB stop at 64 KB.
    assert_eq!(ColumnType::Text.to_mysql_sql(), "LONGTEXT");
    assert_eq!(ColumnType::Binary.to_mysql_sql(), "LONGBLOB");
}

#[test]
fn test_column_type_sqlite() {
    assert_eq!(ColumnType::Integer.to_sqlite_sql(), "INTEGER");
    assert_eq!(ColumnType::BigInteger.to_sqlite_sql(), "INTEGER");
    assert_eq!(ColumnType::String.to_sqlite_sql(), "TEXT");
    assert_eq!(ColumnType::Boolean.to_sqlite_sql(), "INTEGER");
    assert_eq!(ColumnType::Timestamp.to_sqlite_sql(), "TEXT");
    assert_eq!(ColumnType::TimestampTz.to_sqlite_sql(), "TEXT");
    assert_eq!(ColumnType::Date.to_sqlite_sql(), "TEXT");
    assert_eq!(ColumnType::Time.to_sqlite_sql(), "TEXT");
}

/// Assert what `column_type` renders to on every backend, in the order
/// (Postgres, MySQL, MariaDB, SQLite).
#[track_caller]
fn assert_renders(column_type: &ColumnType, postgres: &str, mysql: &str, sqlite: &str) {
    assert_eq!(column_type.to_sql(DatabaseType::Postgres), postgres);
    assert_eq!(column_type.to_sql(DatabaseType::MySQL), mysql);
    assert_eq!(column_type.to_sql(DatabaseType::MariaDB), mysql);
    assert_eq!(column_type.to_sql(DatabaseType::SQLite), sqlite);
}

#[test]
fn test_column_types_render_what_the_drivers_bind_and_decode() {
    // These are pinned per backend because the mapper is shared by migrations,
    // schema export and DB_SYNC: a "nicer" rendering here silently breaks reads
    // on a backend nobody runs in CI.

    // Decimal. sea-orm decodes Decimal/BigDecimal on SQLite through
    // `try_get::<Option<f64>>`, and sqlx only yields an f64 from REAL affinity,
    // so a TEXT column - though lossless - cannot be read back at all.
    let money = ColumnType::Decimal {
        precision: 12,
        scale: 2,
    };
    assert_renders(&money, "DECIMAL(12, 2)", "DECIMAL(12, 2)", "REAL");
    assert_renders(&ColumnType::Numeric, "DECIMAL", "DECIMAL(65,30)", "REAL");

    // Uuid. sqlx-mysql encodes a Uuid as 16 raw bytes and refuses to decode
    // anything else, so CHAR(36) rejects every insert with error 1366. These
    // are also what sea-query's own `ColumnDef::uuid()` renders per backend.
    assert_renders(&ColumnType::Uuid, "UUID", "BINARY(16)", "TEXT");

    // u32. sea-orm reads it back as an `Oid` and then as an `i32`, so an int8
    // column is unreadable however well it would hold the range.
    assert_renders(&ColumnType::Unsigned, "INTEGER", "INT UNSIGNED", "INTEGER");

    // u64. Only MySQL can decode one at all, and the binder narrows it to an
    // `i64` on the way in, so BIGINT is the honest PostgreSQL column.
    assert_renders(
        &ColumnType::BigUnsigned,
        "BIGINT",
        "BIGINT UNSIGNED",
        "INTEGER",
    );

    // i128/u128 map to a 39-digit decimal, so they inherit the decimal
    // rendering - including SQLite's REAL, which cannot hold the range.
    let wide = ColumnType::Decimal {
        precision: 39,
        scale: 0,
    };
    assert_renders(&wide, "DECIMAL(39, 0)", "DECIMAL(39, 0)", "REAL");
}

#[test]
fn test_default_value() {
    assert_eq!(DefaultValue::String("test".to_string()).to_sql(), "'test'");
    assert_eq!(DefaultValue::Integer(42).to_sql(), "42");
    assert_eq!(DefaultValue::Boolean(true).to_sql(), "TRUE");
    assert_eq!(DefaultValue::Boolean(false).to_sql(), "FALSE");
    assert_eq!(DefaultValue::Null.to_sql(), "NULL");
}

const BACKENDS: [DatabaseType; 4] = [
    DatabaseType::Postgres,
    DatabaseType::MySQL,
    DatabaseType::MariaDB,
    DatabaseType::SQLite,
];

#[test]
fn ledger_table_defaults_to_underscore_migrations() {
    for migrator in [Migrator::new(), Migrator::default()] {
        let ledger = migrator.ledger().expect("default table name is valid");
        assert_eq!(ledger.table(), "_migrations");
    }
}

#[test]
fn ledger_table_is_configurable() {
    let migrator = Migrator::new().migrations_table("schema_migrations");

    assert_eq!(
        migrator.ledger().expect("valid table name").table(),
        "schema_migrations"
    );
}

#[test]
fn ledger_table_name_is_validated_before_it_reaches_sql() {
    for name in ["schema migrations", "users\"; DROP TABLE users --", "1bad"] {
        let error = match Migrator::new().migrations_table(name).ledger() {
            Ok(_) => panic!("unsafe ledger table name '{}' must be rejected", name),
            Err(error) => error,
        };

        assert!(
            error.to_string().contains(name),
            "Error should name the offending table. Got: {}",
            error
        );
    }
}

#[test]
fn ledger_sql_uses_the_configured_table_on_every_backend() {
    let ledger = Ledger::migrations("schema_migrations");

    for db_type in BACKENDS {
        let quoted = quote_ident(db_type, "schema_migrations");

        for sql in [
            ledger.create_table_sql(db_type),
            ledger.keys_sql(db_type),
            ledger
                .insert_sql(db_type, "20260101_001", &["create_users"])
                .0,
            ledger.delete_sql(db_type, "20260101_001").0,
        ] {
            assert!(
                sql.contains(&quoted),
                "{:?} statement should target the configured ledger. Got: {}",
                db_type,
                sql
            );
            assert!(
                !sql.contains(&quote_ident(db_type, "_migrations")),
                "{:?} statement should not fall back to the default ledger. Got: {}",
                db_type,
                sql
            );
        }
    }
}

#[test]
fn ledger_keys_are_ordered_by_insertion_id() {
    for ledger in [Ledger::migrations("_migrations"), Ledger::seeds()] {
        for db_type in BACKENDS {
            let sql = ledger.keys_sql(db_type);

            assert!(
                sql.ends_with(&format!("ORDER BY {} ASC", quote_ident(db_type, "id"))),
                "Rollback order must follow the ledger id, not the key. Got: {}",
                sql
            );
        }
    }
}

#[test]
fn ledger_writes_bind_their_values_with_backend_placeholders() {
    let ledger = Ledger::migrations("_migrations");

    let (insert, params) = ledger.insert_sql(DatabaseType::Postgres, "v1", &["create_users"]);
    assert_eq!(
        insert,
        "INSERT INTO \"_migrations\" (\"version\", \"name\") VALUES ($1, $2)"
    );
    assert_eq!(params.len(), 2);

    let (delete, params) = ledger.delete_sql(DatabaseType::Postgres, "v1");
    assert_eq!(delete, "DELETE FROM \"_migrations\" WHERE \"version\" = $1");
    assert_eq!(params.len(), 1);

    for db_type in [
        DatabaseType::MySQL,
        DatabaseType::MariaDB,
        DatabaseType::SQLite,
    ] {
        let (insert, _) = ledger.insert_sql(db_type, "v1", &["create_users"]);
        assert!(insert.ends_with("VALUES (?, ?)"), "Got: {}", insert);

        let (delete, _) = Ledger::seeds().delete_sql(db_type, "user_seeder");
        assert!(delete.ends_with(" = ?"), "Got: {}", delete);
    }
}

#[test]
fn ledger_table_ddl_keeps_its_shape() {
    // The CLI creates the same tables, so the columns and their order are
    // fixed: a monotonic id, the unique key, the details, the timestamp.
    assert_eq!(
        Ledger::migrations("_migrations").create_table_sql(DatabaseType::Postgres),
        "CREATE TABLE IF NOT EXISTS \"_migrations\" (\"id\" SERIAL PRIMARY KEY, \
         \"version\" VARCHAR(255) NOT NULL UNIQUE, \"name\" VARCHAR(255) NOT NULL, \
         \"applied_at\" TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)"
    );
    assert_eq!(
        Ledger::seeds().create_table_sql(DatabaseType::MySQL),
        "CREATE TABLE IF NOT EXISTS `_seeds` (`id` INT AUTO_INCREMENT PRIMARY KEY, \
         `name` VARCHAR(255) NOT NULL UNIQUE, \
         `executed_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)"
    );
    assert_eq!(
        Ledger::seeds().create_table_sql(DatabaseType::SQLite),
        "CREATE TABLE IF NOT EXISTS \"_seeds\" (\"id\" INTEGER PRIMARY KEY AUTOINCREMENT, \
         \"name\" TEXT NOT NULL UNIQUE, \
         \"executed_at\" TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)"
    );
}

#[test]
fn test_table_builder_create() {
    let mut builder = TableBuilder::new("users", DatabaseType::Postgres);
    builder.id();
    builder.string("email").unique().not_null();
    builder.string("name").not_null();
    builder.boolean("active").default(true);
    builder.timestamps();

    let sql = builder.build_create(false);
    assert!(sql.contains("CREATE TABLE"));
    assert!(sql.contains("\"users\""));
    assert!(sql.contains("\"id\" BIGSERIAL"));
    assert!(sql.contains("\"email\""));
    assert!(sql.contains("\"name\""));
    assert!(sql.contains("\"active\""));
    assert!(sql.contains("\"created_at\""));
    assert!(sql.contains("\"updated_at\""));
}

#[test]
fn test_timestamps_feature() {
    let mut builder = TableBuilder::new("posts", DatabaseType::Postgres);
    builder.id();
    builder.string("title").not_null();
    builder.timestamps();

    let sql = builder.build_create(false);
    assert!(
        sql.contains("\"created_at\" TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP"),
        "PostgreSQL should have created_at with TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP. Got: {}",
        sql
    );
    assert!(
        sql.contains("\"updated_at\" TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP"),
        "PostgreSQL should have updated_at with TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP. Got: {}",
        sql
    );

    let mut builder = TableBuilder::new("posts", DatabaseType::MySQL);
    builder.id();
    builder.string("title").not_null();
    builder.timestamps();

    let sql = builder.build_create(false);
    // The default has to match the column's six fractional digits, or MySQL
    // rejects it as an invalid default.
    assert!(
        sql.contains("`created_at` DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6)"),
        "MySQL should have created_at with DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6). Got: {}",
        sql
    );
    assert!(
        sql.contains("`updated_at` DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6)"),
        "MySQL should have updated_at with DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6). Got: {}",
        sql
    );

    let mut builder = TableBuilder::new("posts", DatabaseType::MariaDB);
    builder.id();
    builder.string("title").not_null();
    builder.timestamps();

    let sql = builder.build_create(false);
    assert!(
        sql.contains("`created_at` DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6)"),
        "MariaDB should have created_at with DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6). Got: {}",
        sql
    );
    assert!(
        sql.contains("`updated_at` DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6)"),
        "MariaDB should have updated_at with DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6). Got: {}",
        sql
    );

    let mut builder = TableBuilder::new("posts", DatabaseType::SQLite);
    builder.id();
    builder.string("title").not_null();
    builder.timestamps();

    let sql = builder.build_create(false);
    assert!(
        sql.contains("\"created_at\" TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP"),
        "SQLite should have created_at with TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP. Got: {}",
        sql
    );
    assert!(
        sql.contains("\"updated_at\" TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP"),
        "SQLite should have updated_at with TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP. Got: {}",
        sql
    );
}

#[test]
fn test_timestamps_naive_feature() {
    let mut builder = TableBuilder::new("logs", DatabaseType::Postgres);
    builder.id();
    builder.text("message").not_null();
    builder.timestamps_naive();

    let sql = builder.build_create(false);
    assert!(
        sql.contains("\"created_at\" TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP"),
        "PostgreSQL timestamps_naive should use TIMESTAMP. Got: {}",
        sql
    );
    assert!(
        sql.contains("\"updated_at\" TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP"),
        "PostgreSQL timestamps_naive should use TIMESTAMP. Got: {}",
        sql
    );
}

#[test]
fn test_timestamptz_column() {
    let mut builder = TableBuilder::new("sessions", DatabaseType::Postgres);
    builder.id();
    builder.string("token").not_null();
    builder.timestamptz("expires_at").not_null();
    builder.timestamptz("last_activity").nullable();

    let sql = builder.build_create(false);
    assert!(
        sql.contains("\"expires_at\" TIMESTAMPTZ NOT NULL"),
        "Should have expires_at as TIMESTAMPTZ NOT NULL. Got: {}",
        sql
    );
    assert!(
        sql.contains("\"last_activity\" TIMESTAMPTZ"),
        "Should have last_activity as TIMESTAMPTZ. Got: {}",
        sql
    );
    assert!(
        !sql.contains("\"last_activity\" TIMESTAMPTZ NOT NULL"),
        "last_activity should be nullable. Got: {}",
        sql
    );
}

#[test]
fn test_timestamp_vs_timestamptz() {
    let mut builder = TableBuilder::new("events", DatabaseType::Postgres);
    builder.id();
    builder.timestamp("local_time");
    builder.timestamptz("utc_time");

    let sql = builder.build_create(false);
    assert!(
        sql.contains("\"local_time\" TIMESTAMP"),
        "timestamp() should produce TIMESTAMP. Got: {}",
        sql
    );
    assert!(
        sql.contains("\"utc_time\" TIMESTAMPTZ"),
        "timestamptz() should produce TIMESTAMPTZ. Got: {}",
        sql
    );
}

#[test]
fn test_date_time_columns() {
    let mut builder = TableBuilder::new("schedules", DatabaseType::Postgres);
    builder.id();
    builder.date("event_date");
    builder.time("start_time");
    builder.datetime("local_datetime");
    builder.timestamp("naive_timestamp");
    builder.timestamptz("utc_timestamp");

    let sql = builder.build_create(false);
    assert!(
        sql.contains("\"event_date\" DATE"),
        "Should have DATE column. Got: {}",
        sql
    );
    assert!(
        sql.contains("\"start_time\" TIME"),
        "Should have TIME column. Got: {}",
        sql
    );
    assert!(
        sql.contains("\"local_datetime\" TIMESTAMP"),
        "Should have TIMESTAMP for datetime. Got: {}",
        sql
    );
    assert!(
        sql.contains("\"naive_timestamp\" TIMESTAMP"),
        "Should have TIMESTAMP. Got: {}",
        sql
    );
    assert!(
        sql.contains("\"utc_timestamp\" TIMESTAMPTZ"),
        "Should have TIMESTAMPTZ. Got: {}",
        sql
    );
}

#[test]
fn test_soft_deletes_feature() {
    let mut builder = TableBuilder::new("posts", DatabaseType::Postgres);
    builder.id();
    builder.soft_deletes();

    let sql = builder.build_create(false);
    assert!(
        sql.contains("\"deleted_at\" TIMESTAMPTZ"),
        "Should have deleted_at TIMESTAMPTZ column. Got: {}",
        sql
    );
    assert!(
        !sql.contains("\"deleted_at\" TIMESTAMPTZ NOT NULL"),
        "deleted_at should be nullable (no NOT NULL). Got: {}",
        sql
    );
}

#[test]
fn test_alter_table_builder() {
    let mut builder = AlterTableBuilder::new("users", DatabaseType::Postgres);
    builder.add_column("phone", ColumnType::String).nullable();
    builder.drop_column("legacy");
    builder.rename_column("name", "full_name");

    let statements = builder.build().expect("alter statements");
    assert_eq!(statements.len(), 3);
    assert!(statements[0].contains("ADD COLUMN"));
    assert!(statements[1].contains("DROP COLUMN"));
    assert!(statements[2].contains("RENAME COLUMN"));
}

#[test]
fn test_alter_add_column_renders_every_setting() {
    let mut builder = AlterTableBuilder::new("users", DatabaseType::Postgres);
    builder
        .add_column("age", ColumnType::Integer)
        .not_null()
        .default(0)
        .unique();
    builder
        .add_column("seen_at", ColumnType::TimestampTz)
        .default_now();

    let statements = builder.build().expect("alter statements");
    assert_eq!(
        statements,
        vec![
            "ALTER TABLE \"users\" ADD COLUMN \"age\" INTEGER NOT NULL DEFAULT 0 UNIQUE",
            "ALTER TABLE \"users\" ADD COLUMN \"seen_at\" TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP",
        ]
    );
}

#[test]
fn test_increments_keep_their_width_on_postgres() {
    let mut builder = TableBuilder::new("counters", DatabaseType::Postgres);
    builder.increments("id");

    let sql = builder.build_create(false);
    assert!(sql.contains("\"id\" SERIAL"), "Got: {}", sql);
    assert!(!sql.contains("BIGSERIAL"), "Got: {}", sql);
    assert!(sql.contains("PRIMARY KEY (\"id\")"), "Got: {}", sql);
}

#[test]
fn test_change_column_type_is_rejected_on_sqlite() {
    let mut builder = AlterTableBuilder::new("users", DatabaseType::SQLite);
    builder.change_column("age", ColumnType::BigInteger);

    let error = builder
        .build()
        .expect_err("SQLite cannot alter a column type");
    assert!(
        matches!(error, crate::error::Error::BackendNotSupported { .. }),
        "Got: {}",
        error
    );

    let message = error.to_string();
    assert!(
        message.contains("age") && message.contains("users"),
        "Error should name the column and table. Got: {}",
        message
    );

    for database_type in [
        DatabaseType::Postgres,
        DatabaseType::MySQL,
        DatabaseType::MariaDB,
    ] {
        let mut builder = AlterTableBuilder::new("users", database_type);
        builder.change_column("age", ColumnType::BigInteger);
        let statements = builder.build().expect("alter statements");
        assert_eq!(statements.len(), 1);
    }
}

#[test]
fn test_multi_column_unique_constraint() {
    let mut builder = TableBuilder::new("user_roles", DatabaseType::Postgres);
    builder.big_integer("user_id").not_null();
    builder.big_integer("role_id").not_null();
    builder.unique(&["user_id", "role_id"]);

    let sql = builder.build_create(false);
    assert!(
        sql.contains("UNIQUE (\"user_id\", \"role_id\")"),
        "Should have multi-column unique constraint. Got: {}",
        sql
    );

    let mut builder = TableBuilder::new("users", DatabaseType::Postgres);
    builder.id();
    builder.string("email").not_null();
    builder.big_integer("tenant_id").not_null();
    builder.unique_named("uq_user_email_tenant", &["email", "tenant_id"]);

    let sql = builder.build_create(false);
    assert!(
        sql.contains("CONSTRAINT \"uq_user_email_tenant\" UNIQUE (\"email\", \"tenant_id\")"),
        "Should have named unique constraint. Got: {}",
        sql
    );
}

#[test]
fn test_composite_primary_key() {
    let mut builder = TableBuilder::new("user_roles", DatabaseType::Postgres);
    builder.big_integer("user_id").not_null();
    builder.big_integer("role_id").not_null();
    builder.timestamps();
    builder.primary_key(&["user_id", "role_id"]);

    let sql = builder.build_create(false);
    assert!(
        sql.contains("PRIMARY KEY (\"user_id\", \"role_id\")"),
        "Should have composite primary key. Got: {}",
        sql
    );
    assert!(
        !sql.contains("BIGINT PRIMARY KEY"),
        "Individual columns should not be marked as primary key. Got: {}",
        sql
    );
}

#[test]
fn test_composite_primary_key_replaces_column_level_key() {
    let mut builder = TableBuilder::new("user_roles", DatabaseType::Postgres);
    builder.id();
    builder.big_integer("user_id").not_null();
    builder.big_integer("role_id").not_null();
    builder.primary_key(&["user_id", "role_id"]);

    let sql = builder.build_create(false);
    assert_eq!(
        sql.matches("PRIMARY KEY").count(),
        1,
        "Exactly one PRIMARY KEY clause may be emitted. Got: {}",
        sql
    );
    assert!(
        sql.contains("PRIMARY KEY (\"user_id\", \"role_id\")"),
        "The explicit composite key should win. Got: {}",
        sql
    );
}

#[test]
fn test_create_index_if_not_exists_is_omitted_for_mysql() {
    let mut builder = TableBuilder::new("users", DatabaseType::MySQL);
    builder.id();
    builder.string("email").not_null();
    builder.index(&["email"]);

    let indexes = builder.build_indexes(true);
    assert_eq!(indexes.len(), 1);
    assert!(
        !indexes[0].contains("IF NOT EXISTS"),
        "MySQL has no CREATE INDEX IF NOT EXISTS. Got: {}",
        indexes[0]
    );
    assert!(indexes[0].starts_with("CREATE INDEX `idx_users_email` ON `users`"));

    for database_type in [
        DatabaseType::Postgres,
        DatabaseType::MariaDB,
        DatabaseType::SQLite,
    ] {
        let mut builder = TableBuilder::new("users", database_type);
        builder.index(&["email"]);
        let indexes = builder.build_indexes(true);
        assert!(
            indexes[0].contains("IF NOT EXISTS"),
            "{:?} supports CREATE INDEX IF NOT EXISTS. Got: {}",
            database_type,
            indexes[0]
        );
    }
}

#[test]
fn test_check_constraint() {
    let mut builder = TableBuilder::new("products", DatabaseType::Postgres);
    builder.id();
    builder.decimal("price").check("price >= 0");
    builder.integer("quantity").check("quantity >= 0");

    let sql = builder.build_create(false);
    assert!(
        sql.contains("CHECK (price >= 0)"),
        "Should have CHECK constraint on price. Got: {}",
        sql
    );
    assert!(
        sql.contains("CHECK (quantity >= 0)"),
        "Should have CHECK constraint on quantity. Got: {}",
        sql
    );
}

#[test]
fn test_extra_sql_attribute() {
    let mut builder = TableBuilder::new("logs", DatabaseType::MySQL);
    builder.id();
    builder.text("message").extra("COLLATE utf8mb4_unicode_ci");

    let sql = builder.build_create(false);
    assert!(
        sql.contains("COLLATE utf8mb4_unicode_ci"),
        "Should include extra SQL. Got: {}",
        sql
    );

    let mut builder = TableBuilder::new("logs", DatabaseType::MariaDB);
    builder.id();
    builder.text("message").extra("COLLATE utf8mb4_unicode_ci");

    let sql = builder.build_create(false);
    assert!(
        sql.contains("COLLATE utf8mb4_unicode_ci"),
        "MariaDB should include extra SQL. Got: {}",
        sql
    );
}

#[test]
fn test_column_type_mariadb() {
    assert_eq!(ColumnType::Integer.to_mysql_sql(), "INT");
    assert_eq!(ColumnType::BigInteger.to_mysql_sql(), "BIGINT");
    assert_eq!(ColumnType::Boolean.to_mysql_sql(), "TINYINT(1)");
    assert_eq!(ColumnType::Jsonb.to_mysql_sql(), "JSON");
    // DATETIME(6): MySQL's TIMESTAMP only spans 1970-2038, and a column without
    // fractional digits rounds away microseconds.
    assert_eq!(ColumnType::DateTime.to_mysql_sql(), "DATETIME(6)");
    assert_eq!(ColumnType::Timestamp.to_mysql_sql(), "DATETIME(6)");
    assert_eq!(ColumnType::TimestampTz.to_mysql_sql(), "DATETIME(6)");
    assert_eq!(ColumnType::Date.to_mysql_sql(), "DATE");
    assert_eq!(ColumnType::Time.to_mysql_sql(), "TIME(6)");
    // TEXT and BLOB stop at 64 KB.
    assert_eq!(ColumnType::Text.to_mysql_sql(), "LONGTEXT");
    assert_eq!(ColumnType::Binary.to_mysql_sql(), "LONGBLOB");
}

#[test]
fn test_mariadb_table_builder_create() {
    let mut builder = TableBuilder::new("users", DatabaseType::MariaDB);
    builder.id();
    builder.string("email").unique().not_null();
    builder.string("name").not_null();
    builder.boolean("active").default(true);
    builder.timestamps();

    let sql = builder.build_create(false);
    assert!(sql.contains("CREATE TABLE"));
    assert!(sql.contains("`users`"));
    assert!(sql.contains("`id` BIGINT AUTO_INCREMENT"));
    assert!(sql.contains("`email`"));
    assert!(sql.contains("`name`"));
    assert!(sql.contains("`active`"));
    assert!(sql.contains("`created_at`"));
    assert!(sql.contains("`updated_at`"));
}

/// A MySQL table otherwise inherits the database's character set, and a latin1
/// default rejects `日本語` or an emoji.
#[test]
fn mysql_tables_are_created_utf8mb4() {
    for db_type in [DatabaseType::MySQL, DatabaseType::MariaDB] {
        let mut builder = TableBuilder::new("notes", db_type);
        builder.id();
        assert!(
            builder
                .build_create(false)
                .ends_with(") DEFAULT CHARSET=utf8mb4"),
            "{db_type:?}"
        );
    }
    for db_type in [DatabaseType::Postgres, DatabaseType::SQLite] {
        let mut builder = TableBuilder::new("notes", db_type);
        builder.id();
        assert!(
            !builder.build_create(false).contains("CHARSET"),
            "{db_type:?}"
        );
    }
}

#[test]
fn test_mariadb_alter_table_builder() {
    let mut builder = AlterTableBuilder::new("users", DatabaseType::MariaDB);
    builder.add_column("phone", ColumnType::String).nullable();
    builder.drop_column("legacy");
    builder.rename_column("name", "full_name");

    let statements = builder.build().expect("alter statements");
    assert_eq!(statements.len(), 3);
    assert!(statements[0].contains("ADD COLUMN"));
    assert!(statements[0].contains("`phone`"));
    assert!(statements[1].contains("DROP COLUMN"));
    assert!(statements[1].contains("`legacy`"));
    assert!(statements[2].contains("RENAME COLUMN"));
    assert!(statements[2].contains("`name`"));
    assert!(statements[2].contains("`full_name`"));
}

#[test]
fn test_postgres_identifier_quoting_escapes_inner_quotes() {
    let mut builder = TableBuilder::new("user\"roles", DatabaseType::Postgres);
    builder.string("display\"name");
    let sql = builder.build_create(false);

    assert!(
        sql.contains("CREATE TABLE \"user\"\"roles\""),
        "SQL should escape embedded double quotes in table names. Got: {}",
        sql
    );
    assert!(
        sql.contains("\"display\"\"name\" VARCHAR(255)"),
        "SQL should escape embedded double quotes in column names. Got: {}",
        sql
    );
}

#[test]
fn test_mysql_identifier_quoting_escapes_inner_backticks() {
    let mut builder = AlterTableBuilder::new("user`roles", DatabaseType::MySQL);
    builder.rename_column("old`name", "new`name");
    let statements = builder.build().expect("alter statements");

    assert!(
        statements[0].contains("ALTER TABLE `user``roles`"),
        "SQL should escape embedded backticks in table names. Got: {}",
        statements[0]
    );
    assert!(
        statements[0].contains("`old``name` TO `new``name`"),
        "SQL should escape embedded backticks in column names. Got: {}",
        statements[0]
    );
}

#[test]
fn test_varchar_columns_carry_their_length() {
    assert_eq!(
        ColumnType::Varchar(64).to_sql(DatabaseType::Postgres),
        "VARCHAR(64)"
    );
    assert_eq!(
        ColumnType::Varchar(64).to_sql(DatabaseType::MySQL),
        "VARCHAR(64)"
    );
    assert_eq!(ColumnType::Varchar(64).to_sql(DatabaseType::SQLite), "TEXT");

    let mut builder = TableBuilder::new("users", DatabaseType::Postgres);
    builder.id();
    builder.string_with("code", 12).not_null();
    let sql = builder.build_create(false);
    assert!(sql.contains("\"code\" VARCHAR(12) NOT NULL"), "{sql}");
}

#[test]
fn a_mysql_array_column_default_is_written_as_an_expression() {
    // An array column is `JSON` on MySQL, which takes a default only as an
    // expression; `DEFAULT '[]'` fails with error 1101.
    let mut column = super::ddl::ColumnDefinition::new("tags", ColumnType::TextArray);
    column.default = Some("'[]'".to_string());
    let sql = column.to_sql(DatabaseType::MySQL);
    assert!(sql.contains("JSON DEFAULT ('[]')"), "{sql}");
    assert!(
        column
            .to_sql(DatabaseType::Postgres)
            .contains("TEXT[] DEFAULT '[]'")
    );
}
