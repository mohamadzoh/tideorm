use super::*;

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
    assert_eq!(ColumnType::Timestamp.to_mysql_sql(), "TIMESTAMP");
    assert_eq!(ColumnType::TimestampTz.to_mysql_sql(), "TIMESTAMP");
    assert_eq!(ColumnType::Date.to_mysql_sql(), "DATE");
    assert_eq!(ColumnType::Time.to_mysql_sql(), "TIME");
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

#[test]
fn test_default_value() {
    assert_eq!(DefaultValue::String("test".to_string()).to_sql(), "'test'");
    assert_eq!(DefaultValue::Integer(42).to_sql(), "42");
    assert_eq!(DefaultValue::Boolean(true).to_sql(), "TRUE");
    assert_eq!(DefaultValue::Boolean(false).to_sql(), "FALSE");
    assert_eq!(DefaultValue::Null.to_sql(), "NULL");
}

#[test]
fn test_migration_parameter_placeholders_are_backend_aware() {
    assert_eq!(
        migration_parameter_list(DatabaseType::Postgres, 2),
        "$1, $2"
    );
    assert_eq!(
        migration_parameter_placeholder(DatabaseType::Postgres, 1),
        "$1"
    );

    assert_eq!(migration_parameter_list(DatabaseType::MySQL, 2), "?, ?");
    assert_eq!(migration_parameter_list(DatabaseType::MariaDB, 2), "?, ?");
    assert_eq!(migration_parameter_list(DatabaseType::SQLite, 2), "?, ?");
    assert_eq!(
        migration_parameter_placeholder(DatabaseType::SQLite, 1),
        "?"
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

    let sql = builder.build_create();
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

    let sql = builder.build_create();
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

    let sql = builder.build_create();
    assert!(
        sql.contains("`created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP"),
        "MySQL should have created_at with TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP. Got: {}",
        sql
    );
    assert!(
        sql.contains("`updated_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP"),
        "MySQL should have updated_at with TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP. Got: {}",
        sql
    );

    let mut builder = TableBuilder::new("posts", DatabaseType::MariaDB);
    builder.id();
    builder.string("title").not_null();
    builder.timestamps();

    let sql = builder.build_create();
    assert!(
        sql.contains("`created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP"),
        "MariaDB should have created_at with TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP. Got: {}",
        sql
    );
    assert!(
        sql.contains("`updated_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP"),
        "MariaDB should have updated_at with TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP. Got: {}",
        sql
    );

    let mut builder = TableBuilder::new("posts", DatabaseType::SQLite);
    builder.id();
    builder.string("title").not_null();
    builder.timestamps();

    let sql = builder.build_create();
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

    let sql = builder.build_create();
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

    let sql = builder.build_create();
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

    let sql = builder.build_create();
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

    let sql = builder.build_create();
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

    let sql = builder.build_create();
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

    let statements = builder.build();
    assert_eq!(statements.len(), 3);
    assert!(statements[0].contains("ADD COLUMN"));
    assert!(statements[1].contains("DROP COLUMN"));
    assert!(statements[2].contains("RENAME COLUMN"));
}

#[test]
fn test_multi_column_unique_constraint() {
    let mut builder = TableBuilder::new("user_roles", DatabaseType::Postgres);
    builder.big_integer("user_id").not_null();
    builder.big_integer("role_id").not_null();
    builder.unique(&["user_id", "role_id"]);

    let sql = builder.build_create();
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

    let sql = builder.build_create();
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

    let sql = builder.build_create();
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
fn test_check_constraint() {
    let mut builder = TableBuilder::new("products", DatabaseType::Postgres);
    builder.id();
    builder.decimal("price").check("price >= 0");
    builder.integer("quantity").check("quantity >= 0");

    let sql = builder.build_create();
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

    let sql = builder.build_create();
    assert!(
        sql.contains("COLLATE utf8mb4_unicode_ci"),
        "Should include extra SQL. Got: {}",
        sql
    );

    let mut builder = TableBuilder::new("logs", DatabaseType::MariaDB);
    builder.id();
    builder.text("message").extra("COLLATE utf8mb4_unicode_ci");

    let sql = builder.build_create();
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
    assert_eq!(ColumnType::Timestamp.to_mysql_sql(), "TIMESTAMP");
    assert_eq!(ColumnType::TimestampTz.to_mysql_sql(), "TIMESTAMP");
    assert_eq!(ColumnType::Date.to_mysql_sql(), "DATE");
    assert_eq!(ColumnType::Time.to_mysql_sql(), "TIME");
}

#[test]
fn test_mariadb_table_builder_create() {
    let mut builder = TableBuilder::new("users", DatabaseType::MariaDB);
    builder.id();
    builder.string("email").unique().not_null();
    builder.string("name").not_null();
    builder.boolean("active").default(true);
    builder.timestamps();

    let sql = builder.build_create();
    assert!(sql.contains("CREATE TABLE"));
    assert!(sql.contains("`users`"));
    assert!(sql.contains("`id` BIGINT AUTO_INCREMENT"));
    assert!(sql.contains("`email`"));
    assert!(sql.contains("`name`"));
    assert!(sql.contains("`active`"));
    assert!(sql.contains("`created_at`"));
    assert!(sql.contains("`updated_at`"));
}

#[test]
fn test_mariadb_alter_table_builder() {
    let mut builder = AlterTableBuilder::new("users", DatabaseType::MariaDB);
    builder.add_column("phone", ColumnType::String).nullable();
    builder.drop_column("legacy");
    builder.rename_column("name", "full_name");

    let statements = builder.build();
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
fn test_mariadb_quoting() {
    let schema = Schema::new(DatabaseType::MariaDB);
    assert_eq!(schema.quote_identifier("users"), "`users`");
    assert_eq!(schema.quote_identifier("email"), "`email`");
}

#[test]
fn test_postgres_identifier_quoting_escapes_inner_quotes() {
    let schema = Schema::new(DatabaseType::Postgres);
    assert_eq!(schema.quote_identifier("user\"roles"), "\"user\"\"roles\"");
    assert_eq!(
        quote_migration_identifier("col\"name", DatabaseType::Postgres),
        "\"col\"\"name\""
    );

    let mut builder = TableBuilder::new("user\"roles", DatabaseType::Postgres);
    builder.string("display\"name");
    let sql = builder.build_create();

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
    let schema = Schema::new(DatabaseType::MySQL);
    assert_eq!(schema.quote_identifier("user`roles"), "`user``roles`");
    assert_eq!(
        quote_migration_identifier("col`name", DatabaseType::MySQL),
        "`col``name`"
    );

    let mut builder = AlterTableBuilder::new("user`roles", DatabaseType::MySQL);
    builder.rename_column("old`name", "new`name");
    let statements = builder.build();

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
