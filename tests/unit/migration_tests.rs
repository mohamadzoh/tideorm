use super::*;
use crate::internal::sql_safety::quote_ident;

const BACKENDS: [DatabaseType; 4] = [
    DatabaseType::Postgres,
    DatabaseType::MySQL,
    DatabaseType::MariaDB,
    DatabaseType::SQLite,
];

#[test]
fn test_column_types_render_what_the_drivers_bind_and_decode() {
    // These are pinned per backend because the mapper is shared by migrations,
    // schema export and DB_SYNC: a "nicer" rendering here silently breaks reads
    // on a backend nobody runs in CI. MariaDB renders as MySQL does.
    for (column_type, postgres, mysql, sqlite) in [
        (ColumnType::Integer, "INTEGER", "INT", "INTEGER"),
        (ColumnType::BigInteger, "BIGINT", "BIGINT", "INTEGER"),
        (ColumnType::Double, "DOUBLE PRECISION", "DOUBLE", "REAL"),
        (ColumnType::Boolean, "BOOLEAN", "TINYINT(1)", "INTEGER"),
        (ColumnType::String, "VARCHAR(255)", "VARCHAR(255)", "TEXT"),
        (
            ColumnType::Varchar(64),
            "VARCHAR(64)",
            "VARCHAR(64)",
            "TEXT",
        ),
        // MySQL's TEXT and BLOB stop at 64 KB.
        (ColumnType::Text, "TEXT", "LONGTEXT", "TEXT"),
        (ColumnType::Binary, "BYTEA", "LONGBLOB", "BLOB"),
        (ColumnType::Jsonb, "JSONB", "JSON", "TEXT"),
        (ColumnType::IntegerArray, "INTEGER[]", "JSON", "TEXT"),
        (ColumnType::Date, "DATE", "DATE", "TEXT"),
        // DATETIME(6): MySQL's TIMESTAMP only spans 1970-2038, and a column
        // without fractional digits rounds away microseconds.
        (ColumnType::Time, "TIME", "TIME(6)", "TEXT"),
        (ColumnType::DateTime, "TIMESTAMP", "DATETIME(6)", "TEXT"),
        (ColumnType::Timestamp, "TIMESTAMP", "DATETIME(6)", "TEXT"),
        (
            ColumnType::TimestampTz,
            "TIMESTAMPTZ",
            "DATETIME(6)",
            "TEXT",
        ),
        // Decimal. sea-orm decodes Decimal/BigDecimal on SQLite through
        // `try_get::<Option<f64>>`, and sqlx only yields an f64 from REAL
        // affinity, so a TEXT column - though lossless - cannot be read back.
        (
            ColumnType::Decimal {
                precision: 12,
                scale: 2,
            },
            "DECIMAL(12, 2)",
            "DECIMAL(12, 2)",
            "REAL",
        ),
        (ColumnType::Numeric, "DECIMAL", "DECIMAL(65,30)", "REAL"),
        // i128/u128 map to a 39-digit decimal, so they inherit the decimal
        // rendering - including SQLite's REAL, which cannot hold the range.
        (
            ColumnType::Decimal {
                precision: 39,
                scale: 0,
            },
            "DECIMAL(39, 0)",
            "DECIMAL(39, 0)",
            "REAL",
        ),
        // Uuid. sqlx-mysql encodes a Uuid as 16 raw bytes and refuses to
        // decode anything else, so CHAR(36) rejects every insert with error
        // 1366. These are also what sea-query's own `ColumnDef::uuid()`
        // renders per backend.
        (ColumnType::Uuid, "UUID", "BINARY(16)", "TEXT"),
        // u32. sea-orm reads it back as an `Oid` and then as an `i32`, so an
        // int8 column is unreadable however well it would hold the range.
        (ColumnType::Unsigned, "INTEGER", "INT UNSIGNED", "INTEGER"),
        // u64. Only MySQL can decode one at all, and the binder narrows it to
        // an `i64` on the way in, so BIGINT is the honest PostgreSQL column.
        (
            ColumnType::BigUnsigned,
            "BIGINT",
            "BIGINT UNSIGNED",
            "INTEGER",
        ),
    ] {
        for db_type in BACKENDS {
            let expected = match db_type {
                DatabaseType::Postgres => postgres,
                DatabaseType::MySQL | DatabaseType::MariaDB => mysql,
                DatabaseType::SQLite => sqlite,
            };
            assert_eq!(
                column_type.to_sql(db_type),
                expected,
                "{column_type:?} on {db_type:?}"
            );
        }
    }
}

#[test]
fn test_default_value() {
    let pg = DatabaseType::Postgres;
    assert_eq!(
        DefaultValue::String("test".to_string()).to_sql(pg),
        "'test'"
    );
    assert_eq!(DefaultValue::Integer(42).to_sql(pg), "42");
    assert_eq!(DefaultValue::Boolean(true).to_sql(pg), "TRUE");
    assert_eq!(DefaultValue::Boolean(false).to_sql(pg), "FALSE");
    assert_eq!(DefaultValue::Null.to_sql(pg), "NULL");

    // MySQL reads a backslash as an escape; PostgreSQL and SQLite do not.
    let path = DefaultValue::String(r"C:\temp\ it's".to_string());
    assert_eq!(path.to_sql(pg), r"'C:\temp\ it''s'");
    assert_eq!(path.to_sql(DatabaseType::SQLite), r"'C:\temp\ it''s'");
    assert_eq!(path.to_sql(DatabaseType::MySQL), r"'C:\\temp\\ it''s'");
    assert_eq!(path.to_sql(DatabaseType::MariaDB), r"'C:\\temp\\ it''s'");
}

#[test]
fn ledger_table_defaults_to_underscore_migrations_and_is_configurable() {
    for (migrator, table) in [
        (Migrator::new(), "_migrations"),
        (Migrator::default(), "_migrations"),
        (
            Migrator::new().migrations_table("schema_migrations"),
            "schema_migrations",
        ),
    ] {
        let ledger = migrator.ledger().expect("a valid table name");
        assert_eq!(ledger.table(), table);
    }
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
fn test_table_builder_renders_each_backends_create_table() {
    // The MySQL family's timestamp default matches the column's six fractional
    // digits, or MySQL rejects it as an invalid default. A MySQL table
    // otherwise inherits the database's character set, and a latin1 default
    // rejects `日本語` or an emoji.
    let mysql = "CREATE TABLE `users` (
    `id` BIGINT AUTO_INCREMENT,
    `email` VARCHAR(255) NOT NULL UNIQUE,
    `active` TINYINT(1) DEFAULT TRUE,
    `created_at` DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    `updated_at` DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (`id`)
) DEFAULT CHARSET=utf8mb4";
    for (db_type, expected) in [
        (
            DatabaseType::Postgres,
            r#"CREATE TABLE "users" (
    "id" BIGSERIAL,
    "email" VARCHAR(255) NOT NULL UNIQUE,
    "active" BOOLEAN DEFAULT TRUE,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    "updated_at" TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY ("id")
)"#,
        ),
        (DatabaseType::MySQL, mysql),
        (DatabaseType::MariaDB, mysql),
        (
            DatabaseType::SQLite,
            r#"CREATE TABLE "users" (
    "id" INTEGER NOT NULL,
    "email" TEXT NOT NULL UNIQUE,
    "active" INTEGER DEFAULT TRUE,
    "created_at" TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    "updated_at" TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY ("id")
)"#,
        ),
    ] {
        let mut builder = TableBuilder::new("users", db_type);
        builder.id();
        builder.string("email").unique().not_null();
        builder.boolean("active").default(true);
        builder.timestamps();

        assert_eq!(builder.build_create(false), expected, "{db_type:?}");
    }
}

#[test]
fn test_postgres_column_methods_render_their_types() {
    let mut builder = TableBuilder::new("events", DatabaseType::Postgres);
    // A 4-byte key: `increments` is SERIAL where `id()` is BIGSERIAL.
    builder.increments("id");
    builder.string_with("code", 12).not_null();
    builder.date("event_date");
    builder.time("start_time");
    builder.datetime("local_datetime");
    builder.timestamp("naive_timestamp");
    builder.timestamptz("expires_at").not_null();
    builder.timestamptz("last_activity").nullable();
    builder.decimal("price").check("price >= 0");
    builder.integer("quantity").check("quantity >= 0");
    builder.soft_deletes();
    builder.timestamps_naive();

    assert_eq!(
        builder.build_create(false),
        r#"CREATE TABLE "events" (
    "id" SERIAL,
    "code" VARCHAR(12) NOT NULL,
    "event_date" DATE,
    "start_time" TIME,
    "local_datetime" TIMESTAMP,
    "naive_timestamp" TIMESTAMP,
    "expires_at" TIMESTAMPTZ NOT NULL,
    "last_activity" TIMESTAMPTZ,
    "price" DECIMAL(10, 2) CHECK (price >= 0),
    "quantity" INTEGER CHECK (quantity >= 0),
    "deleted_at" TIMESTAMPTZ,
    "created_at" TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    "updated_at" TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY ("id")
)"#
    );
}

#[test]
fn test_alter_table_builder_renders_add_drop_and_rename_on_every_backend() {
    for db_type in BACKENDS {
        let mut builder = AlterTableBuilder::new("users", db_type);
        builder.add_column("phone", ColumnType::String).nullable();
        builder.drop_column("legacy");
        builder.rename_column("name", "full_name");

        let quoted = |name| quote_ident(db_type, name);
        assert_eq!(
            builder.build().expect("alter statements"),
            [
                format!(
                    "ALTER TABLE {} ADD COLUMN {} {}",
                    quoted("users"),
                    quoted("phone"),
                    ColumnType::String.to_sql(db_type)
                ),
                format!(
                    "ALTER TABLE {} DROP COLUMN {}",
                    quoted("users"),
                    quoted("legacy")
                ),
                format!(
                    "ALTER TABLE {} RENAME COLUMN {} TO {}",
                    quoted("users"),
                    quoted("name"),
                    quoted("full_name")
                ),
            ],
            "{db_type:?}"
        );
    }
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
fn test_change_column_keeps_what_a_mysql_modify_would_drop() {
    // What `SHOW CREATE TABLE` writes, MySQL's spelling and MariaDB's.
    let create_table = "CREATE TABLE `users` (
  `id` bigint NOT NULL AUTO_INCREMENT,
  `age` int(11) NOT NULL DEFAULT 5 COMMENT 'years',
  `code` varchar(20) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin DEFAULT NULL,
  `kind` enum('a','b c') NOT NULL,
  `plain` int,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB";
    let mut builder = AlterTableBuilder::new("users", DatabaseType::MySQL);
    for column in ["id", "age", "code", "kind", "plain", "missing"] {
        builder.change_column(column, ColumnType::BigInteger);
    }
    builder.keep_column_attributes(create_table);

    assert_eq!(
        builder.build().unwrap(),
        [
            "ALTER TABLE `users` MODIFY COLUMN `id` BIGINT NOT NULL AUTO_INCREMENT",
            "ALTER TABLE `users` MODIFY COLUMN `age` BIGINT NOT NULL DEFAULT 5 COMMENT 'years'",
            "ALTER TABLE `users` MODIFY COLUMN `code` BIGINT DEFAULT NULL",
            "ALTER TABLE `users` MODIFY COLUMN `kind` BIGINT NOT NULL",
            "ALTER TABLE `users` MODIFY COLUMN `plain` BIGINT",
            "ALTER TABLE `users` MODIFY COLUMN `missing` BIGINT",
        ]
    );
}

#[test]
fn test_change_column_keeps_a_text_columns_character_set_and_collation() {
    // MySQL writes both clauses where they differ from the table's; MariaDB
    // may write the collation alone.
    let create_table = "CREATE TABLE `users` (
  `code` varchar(20) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
  `tag` varchar(20) COLLATE utf8mb4_bin DEFAULT NULL,
  PRIMARY KEY (`code`)
) ENGINE=InnoDB";
    let mut builder = AlterTableBuilder::new("users", DatabaseType::MySQL);
    builder
        .change_column("code", ColumnType::Varchar(50))
        .change_column("tag", ColumnType::Text);
    builder.keep_column_attributes(create_table);

    assert_eq!(
        builder.build().unwrap(),
        [
            "ALTER TABLE `users` MODIFY COLUMN `code` VARCHAR(50) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL",
            "ALTER TABLE `users` MODIFY COLUMN `tag` LONGTEXT COLLATE utf8mb4_bin DEFAULT NULL",
        ]
    );
}

#[test]
fn test_change_column_after_a_rename_keeps_the_renamed_columns_attributes() {
    let create_table = "CREATE TABLE `users` (
  `old` int NOT NULL DEFAULT 5 COMMENT 'kept',
  `gone` int NOT NULL DEFAULT 1,
  PRIMARY KEY (`old`)
) ENGINE=InnoDB";
    let mut builder = AlterTableBuilder::new("users", DatabaseType::MySQL);
    builder
        .rename_column("old", "new")
        .change_column("new", ColumnType::BigInteger)
        .drop_column("gone");
    builder.add_column("gone", ColumnType::Integer).nullable();
    builder.change_column("gone", ColumnType::BigInteger);
    builder.keep_column_attributes(create_table);

    let statements = builder.build().unwrap();
    assert_eq!(
        statements[1],
        "ALTER TABLE `users` MODIFY COLUMN `new` BIGINT NOT NULL DEFAULT 5 COMMENT 'kept'"
    );
    // The column added in its place has none of the dropped one's.
    assert_eq!(
        statements.last().unwrap(),
        "ALTER TABLE `users` MODIFY COLUMN `gone` BIGINT"
    );
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
    let (name, sql) = &indexes[0];
    assert_eq!(*name, "idx_users_email");
    assert!(
        !sql.contains("IF NOT EXISTS"),
        "MySQL has no CREATE INDEX IF NOT EXISTS. Got: {}",
        sql
    );
    assert!(sql.starts_with("CREATE INDEX `idx_users_email` ON `users`"));

    for database_type in [
        DatabaseType::Postgres,
        DatabaseType::MariaDB,
        DatabaseType::SQLite,
    ] {
        let mut builder = TableBuilder::new("users", database_type);
        builder.index(&["email"]);
        let indexes = builder.build_indexes(true);
        assert!(
            indexes[0].1.contains("IF NOT EXISTS"),
            "{:?} supports CREATE INDEX IF NOT EXISTS. Got: {}",
            database_type,
            indexes[0].1
        );
    }
}

#[test]
fn test_extra_sql_attribute() {
    for db_type in [DatabaseType::MySQL, DatabaseType::MariaDB] {
        let mut builder = TableBuilder::new("logs", db_type);
        builder.id();
        builder.text("message").extra("COLLATE utf8mb4_unicode_ci");

        let sql = builder.build_create(false);
        assert!(
            sql.contains("`message` LONGTEXT COLLATE utf8mb4_unicode_ci,"),
            "{db_type:?}: {sql}"
        );
    }
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

#[test]
fn test_rename_table_uses_each_backends_statement() {
    assert_eq!(
        ddl::rename_table(DatabaseType::Postgres, "users", "members"),
        "ALTER TABLE \"users\" RENAME TO \"members\""
    );
    assert_eq!(
        ddl::rename_table(DatabaseType::SQLite, "users", "members"),
        "ALTER TABLE \"users\" RENAME TO \"members\""
    );
    for db_type in [DatabaseType::MySQL, DatabaseType::MariaDB] {
        assert_eq!(
            ddl::rename_table(db_type, "users", "members"),
            "RENAME TABLE `users` TO `members`"
        );
    }
    assert_eq!(
        ddl::rename_table(DatabaseType::Postgres, "we\"ird", "members"),
        "ALTER TABLE \"we\"\"ird\" RENAME TO \"members\""
    );
}

/// Generated index names fit PostgreSQL's 63 bytes and MySQL's 64
/// characters: past that PostgreSQL cut two names with a common prefix to
/// one, skipping the second index, and MySQL refused them.
#[test]
fn generated_index_names_fit_every_backend() {
    let mut builder = TableBuilder::new(
        "organization_membership_invitations",
        DatabaseType::Postgres,
    );
    builder.index(&["organization_id", "invited_at"]);
    builder.unique_index(&["organization_id", "invited_email"]);
    builder.unique_index(&["organization_id", "invited_phone"]);
    builder.index(&["email"]);
    let names: Vec<String> = builder
        .build_indexes(true)
        .into_iter()
        .map(|(name, _)| name.to_string())
        .collect();

    assert_eq!(
        names[0],
        "idx_organization_membership_invitations_organization_i_75abc36e"
    );
    assert_ne!(names[1], names[2]);
    assert!(names.iter().all(|name| name.len() <= 63), "{names:?}");
    // A name that fits is left as it was.
    assert_eq!(names[3], "idx_organization_membership_invitations_email");
}

/// A model's unnamed index is named as a migration names it, so sync finds
/// the index a migration created instead of adding a second one.
#[test]
fn a_model_names_its_indexes_as_a_migration_does() {
    let table = "organization_membership_invitations";
    let mut builder = TableBuilder::new(table, DatabaseType::Postgres);
    builder.unique_index(&["organization_id", "invited_email"]);
    builder.index(&["email"]);
    let migration_names: Vec<String> = builder
        .build_indexes(true)
        .into_iter()
        .map(|(name, _)| name.to_string())
        .collect();

    let columns = |columns: &[&str]| columns.iter().map(|column| column.to_string()).collect();
    let model_names = [
        crate::model::IndexDefinition::__generated(
            table,
            columns(&["organization_id", "invited_email"]),
            true,
        )
        .name,
        crate::model::IndexDefinition::__generated(table, columns(&["email"]), false).name,
    ];

    assert_eq!(migration_names, model_names);
}

/// Every spelling SQLite reads as turning foreign keys off is caught, and
/// none that leaves them on or never reaches a pragma is.
#[test]
fn foreign_keys_off_is_recognized_in_every_spelling() {
    use super::sqlite_foreign_keys::disables_foreign_keys;

    for sql in [
        "PRAGMA foreign_keys(OFF)",
        "\u{feff}PRAGMA foreign_keys = OFF",
        "PRAGMA main.foreign_keys = false",
        "-- table rebuild\nPRAGMA foreign_keys /* disable */ = 0; -- end",
        "PRAGMA [foreign_keys] = 'no'",
        "PRAGMA \"main\".\"foreign_keys\"('OFF')",
        "PRAGMA foreign_keys = +0",
        "PRAGMA foreign_keys = 256",
        "PRAGMA foreign_keys = 2147483648",
        "PRAGMA foreign_keys = 0x100",
        "PRAGMA foreign_keys = '+1'",
        "/* outer /* inner */ PRAGMA foreign_keys=OFF; /* end */",
        "SELECT $x$; PRAGMA foreign_keys=OFF; SELECT $x$;",
        "PRAGMA foreign_keys = 'unrecognized'",
        "PRAGMA foreign_keys = ON; PRAGMA foreign_keys = OFF;",
        "pragma foreign_keys=0;",
        "PRAGMA [foreign_keys] = `off`",
        "/* rebuild */ PRAGMA -- off\n foreign_keys /* now */ = OFF",
        "PRAGMA foreign_keys = 4294967297",
        "PRAGMA foreign_keys = -1",
        "PRAGMA foreign_keys = maybe",
        "CREATE TABLE t (id INTEGER); PRAGMA foreign_keys = OFF; DROP TABLE t",
    ] {
        assert!(disables_foreign_keys(sql), "{sql}");
    }
    for sql in [
        "SELECT $name(;PRAGMA/**/foreign_keys=OFF;)",
        "SELECT :name(;PRAGMA/**/foreign_keys=OFF;)",
        "SELECT #name(;PRAGMA/**/foreign_keys=OFF;)",
        "PRAGMA foreign_keys",
        "PRAGMA main.foreign_keys(ON)",
        "PRAGMA foreign_keys = 2",
        "PRAGMA foreign_keys = 257",
        "PRAGMA foreign_keys = +1",
        "PRAGMA foreign_keys = 1.0",
        "PRAGMA foreign_keys = 0x101",
        "SELECT 'PRAGMA foreign_keys=OFF;'; -- PRAGMA foreign_keys=OFF",
        "PRAGMA foreign_keys = 0x1",
        "PRAGMA foreign_keys('yes')",
        "PRAGMA foreign_key_check",
        "PRAGMA defer_foreign_keys = OFF",
        "EXPLAIN PRAGMA foreign_keys = OFF",
        "-- PRAGMA foreign_keys = OFF\nSELECT 1",
        "/* PRAGMA foreign_keys = OFF; */ SELECT 1",
        "INSERT INTO notes (body) VALUES ('it''s; PRAGMA foreign_keys = OFF')",
        "SELECT :ns::name(;PRAGMA/**/foreign_keys=OFF;)",
    ] {
        assert!(!disables_foreign_keys(sql), "{sql}");
    }
}

/// The hidden wrapper the CLI reads a table's index DDL through renders what
/// the migrator runs.
#[test]
fn cli_index_wrapper_renders_what_the_migrator_runs() {
    let mut builder = TableBuilder::new("posts", DatabaseType::MySQL);
    builder.id();
    builder.integer("user_id");
    builder.index(&["user_id"]);

    let indexes: Vec<String> = builder
        .build_indexes(false)
        .into_iter()
        .map(|(_, sql)| sql)
        .collect();
    assert_eq!(indexes.len(), 1);
    assert_eq!(builder.__index_sql(false), indexes);
}

/// `DROP INDEX` names the table on the MySQL family and the index elsewhere,
/// each in the schema given apart; `IF EXISTS` only where it exists.
#[test]
fn drop_index_names_what_each_backend_drops() {
    for (db_type, schema, if_exists, expected) in [
        (DatabaseType::Postgres, None, false, r#"DROP INDEX "idx""#),
        (
            DatabaseType::Postgres,
            Some("app"),
            true,
            r#"DROP INDEX IF EXISTS "app"."idx""#,
        ),
        (
            DatabaseType::SQLite,
            None,
            true,
            r#"DROP INDEX IF EXISTS "idx""#,
        ),
        (
            DatabaseType::MySQL,
            None,
            true,
            "DROP INDEX `idx` ON `users`",
        ),
        (
            DatabaseType::MariaDB,
            Some("app"),
            false,
            "DROP INDEX `idx` ON `app`.`users`",
        ),
    ] {
        assert_eq!(
            crate::migration::__drop_index_sql(db_type, schema, "users", "idx", if_exists),
            expected
        );
    }
}
