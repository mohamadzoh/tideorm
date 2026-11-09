use super::*;

#[test]
fn only_postgres_reads_a_schema_from_a_dotted_name() {
    assert_eq!(
        table_parts(DatabaseType::Postgres, "app.users"),
        (Some("app"), "users")
    );
    assert_eq!(
        table_parts(DatabaseType::Postgres, "users"),
        (None, "users")
    );
    assert_eq!(
        table_parts(DatabaseType::MySQL, "app.users"),
        (None, "app.users")
    );
    assert_eq!(
        table_parts(DatabaseType::SQLite, "app.users"),
        (None, "app.users")
    );
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
mod sqlite {
    use super::*;

    async fn database() -> Database {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("an in-memory SQLite database");
        db.exec_raw(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT NOT NULL UNIQUE, \
                 name TEXT DEFAULT 'x', label TEXT GENERATED ALWAYS AS (name || email) VIRTUAL);
             CREATE TABLE pairs (a INT, b INT, PRIMARY KEY (a, b));
             CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INT REFERENCES users, a INT, b INT, \
                 slug TEXT, FOREIGN KEY (a, b) REFERENCES pairs);
             CREATE INDEX idx_posts_expr ON posts (lower(slug), user_id);
             CREATE UNIQUE INDEX idx_posts_slug ON posts (slug);
             CREATE VIEW user_ids AS SELECT id FROM users;
             CREATE TABLE ledger (id INTEGER PRIMARY KEY AUTOINCREMENT);",
        )
        .await
        .expect("the catalog fixture");
        db
    }

    fn column(name: &str, data_type: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable: true,
            default: None,
            primary_key: false,
            unique: false,
            foreign_key: false,
            extra: None,
        }
    }

    /// `sqlite_sequence`, which `AUTOINCREMENT` creates, is SQLite's own.
    #[tokio::test]
    async fn listings_name_the_databases_own_tables_and_views() {
        let db = database().await;
        let catalog = db.catalog();

        assert_eq!(
            catalog.tables().await.unwrap(),
            ["ledger", "pairs", "posts", "users"]
        );
        assert_eq!(catalog.views().await.unwrap(), ["user_ids"]);
        assert!(catalog.sequences().await.unwrap().is_empty());
        assert!(catalog.enum_types().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn columns_carry_their_keys_defaults_and_generation() {
        let db = database().await;

        assert_eq!(
            db.catalog().columns("users").await.unwrap(),
            [
                ColumnInfo {
                    primary_key: true,
                    ..column("id", "INTEGER")
                },
                ColumnInfo {
                    nullable: false,
                    unique: true,
                    ..column("email", "TEXT")
                },
                ColumnInfo {
                    default: Some("'x'".to_string()),
                    ..column("name", "TEXT")
                },
                ColumnInfo {
                    extra: Some("VIRTUAL GENERATED".to_string()),
                    ..column("label", "TEXT")
                },
            ]
        );

        let posts = db.catalog().columns("posts").await.unwrap();
        let keyed: Vec<_> = posts
            .iter()
            .filter(|column| column.foreign_key)
            .map(|column| column.name.as_str())
            .collect();
        assert_eq!(keyed, ["user_id", "a", "b"]);
        // A unique index of several columns makes none of them unique.
        assert!(
            posts
                .iter()
                .all(|column| column.unique == (column.name == "slug"))
        );
    }

    #[tokio::test]
    async fn a_view_has_columns_and_a_missing_table_none() {
        let db = database().await;

        assert_eq!(
            db.catalog().columns("user_ids").await.unwrap(),
            [column("id", "INTEGER")]
        );
        assert!(db.catalog().columns("missing").await.unwrap().is_empty());
    }

    /// An expression key has no column to name; a primary key of several
    /// columns has an index, an `INTEGER PRIMARY KEY` none.
    #[tokio::test]
    async fn indexes_group_their_key_columns() {
        let db = database().await;
        let index = |name: &str, columns: &[&str], unique, primary| IndexInfo {
            name: name.to_string(),
            columns: columns.iter().map(ToString::to_string).collect(),
            unique,
            primary,
            whole_columns: true,
        };

        assert_eq!(
            db.catalog().indexes("posts").await.unwrap(),
            [
                IndexInfo {
                    whole_columns: false,
                    ..index("idx_posts_expr", &["user_id"], false, false)
                },
                index("idx_posts_slug", &["slug"], true, false),
            ]
        );
        assert_eq!(
            db.catalog().indexes("pairs").await.unwrap(),
            [index("sqlite_autoindex_pairs_1", &["a", "b"], true, true)]
        );
    }

    /// A key written `REFERENCES parent` references the parent's primary key.
    #[tokio::test]
    async fn foreign_keys_name_the_parents_key_columns() {
        let db = database().await;
        let key = |column: &str, table: &str, references: &str| ForeignKeyInfo {
            column: column.to_string(),
            references_table: table.to_string(),
            references_column: references.to_string(),
        };

        assert_eq!(
            db.catalog().foreign_keys("posts").await.unwrap(),
            [
                key("a", "pairs", "a"),
                key("b", "pairs", "b"),
                key("user_id", "users", "id"),
            ]
        );
    }

    #[tokio::test]
    async fn probes_ignore_the_case_of_names() {
        let db = database().await;
        let connection = db.__get_connection().unwrap();
        let executor = connection.executor();
        let backend = DatabaseType::SQLite;

        assert!(
            table_exists(&executor, backend, None, "USERS")
                .await
                .unwrap()
        );
        assert!(
            !table_exists(&executor, backend, None, "user_ids")
                .await
                .unwrap()
        );
        assert_eq!(
            index_table(&executor, backend, None, "IDX_POSTS_SLUG")
                .await
                .unwrap()
                .as_deref(),
            Some("posts")
        );
        assert_eq!(
            index_table(&executor, backend, None, "missing")
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            column_names(&executor, backend, None, "pairs")
                .await
                .unwrap(),
            ["a", "b"]
        );
    }
}
