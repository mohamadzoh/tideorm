use super::*;

#[tideorm::model(table = "batch_sql_execution_users")]
struct BatchSqlUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    age: i32,
}

#[tideorm::model(table = "batch_sql_execution_soft_delete_users", soft_delete)]
struct BatchSqlSoftDeleteUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[tideorm::model(table = "batch_sql_invoices", schema = "billing")]
struct BatchSqlInvoice {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    total: i64,
}

#[test]
fn batch_updates_name_the_models_schema() {
    let (sql, _) = BatchUpdateBuilder::<BatchSqlInvoice>::new()
        .set("total", 1)
        .where_eq("id", 1)
        .limit(5)
        .build_update_statement(crate::config::DatabaseType::Postgres)
        .expect("statement should build");
    assert!(
        sql.starts_with(r#"UPDATE "billing"."batch_sql_invoices" SET"#),
        "{sql}"
    );
    // The LIMIT emulation's subquery reads the same table.
    assert!(
        sql.contains(r#"FROM "billing"."batch_sql_invoices""#),
        "{sql}"
    );
}

/// SQLite stores `age * 1.1` as a REAL in an INTEGER column, after which the
/// model cannot read the row; PostgreSQL and MySQL round on assignment.
#[test]
fn scaling_an_integer_column_stays_integral_on_sqlite() {
    use crate::config::DatabaseType;

    let scaled = |db_type| {
        BatchUpdateBuilder::<BatchSqlUser>::new()
            .multiply("age", 1.1)
            .divide("id", 2.0)
            .where_eq("id", 1)
            .build_update_statement(db_type)
            .expect("statement should build")
            .0
    };
    // SQLite scales in integers, by 11 / 10 and 1 / 2, rounding the quotient
    // half away from zero; an `f64` would round a value past 2^53 first. The
    // column is divided before it is multiplied, so no step overflows.
    let sqlite = scaled(DatabaseType::SQLite);
    assert!(
        sqlite.contains(r#""age" = CASE WHEN typeof(("age" / ?) * ?"#),
        "{sqlite}"
    );
    assert!(
        sqlite.contains(r#""id" = CASE WHEN typeof(("id" / ?)"#),
        "{sqlite}"
    );
    assert!(
        sqlite.contains("ELSE abs(-9223372036854775808) END"),
        "{sqlite}"
    );
    // The other backends take the factor as an exact decimal.
    let (postgres, params) = BatchUpdateBuilder::<BatchSqlUser>::new()
        .multiply("age", 1.1)
        .where_eq("id", 1)
        .build_update_statement(DatabaseType::Postgres)
        .expect("statement should build");
    assert!(postgres.contains(r#""age" = "age" * $1"#), "{postgres}");
    assert_eq!(
        params.first(),
        Some(&crate::internal::Value::Decimal(Some(
            "1.1".parse().expect("a decimal")
        )))
    );

    // Arithmetic on text is rejected before SQLite can coerce its contents.
    let error = BatchUpdateBuilder::<BatchSqlUser>::new()
        .multiply("name", 2.0)
        .where_eq("id", 1)
        .build_update_statement(DatabaseType::SQLite)
        .expect_err("text arithmetic must fail");
    assert!(error.to_string().contains("requires a numeric column"));
}

fn filtered_builder() -> BatchUpdateBuilder<BatchSqlUser> {
    BatchUpdateBuilder::<BatchSqlUser>::new()
        .set("name", "updated")
        .set("age", 30)
        .where_eq("id", 1)
}

fn filtered_soft_delete_builder() -> BatchUpdateBuilder<BatchSqlSoftDeleteUser> {
    BatchUpdateBuilder::<BatchSqlSoftDeleteUser>::new()
        .set("name", "updated")
        .where_eq("id", 1)
}

#[test]
fn batch_update_scopes_trashed_rows_as_a_query_does() {
    let sql = |update: BatchUpdateBuilder<BatchSqlSoftDeleteUser>| {
        update
            .build_update_statement(crate::config::DatabaseType::SQLite)
            .expect("statement should build")
            .0
    };

    let default = sql(filtered_soft_delete_builder());
    assert!(
        default.contains(r#""deleted_at" IS NULL"#),
        "the default scope leaves soft-deleted rows out, as a query does: {default}"
    );
    assert_eq!(
        sql(filtered_soft_delete_builder()
            .with_trashed()
            .without_trashed()),
        default
    );

    let with_trashed = sql(filtered_soft_delete_builder().with_trashed());
    assert!(
        !with_trashed.contains("deleted_at"),
        "with_trashed() must not filter soft-deleted rows: {with_trashed}"
    );
}

#[tideorm::model(table = "batch_sql_execution_profiles")]
struct BatchSqlProfile {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[tideorm(column = "name")]
    display_name: String,
}

/// A field and its column name the same assignment, so the last one wins;
/// they were two, and `SET name = ?, name = ?` kept the first on SQLite.
#[test]
fn a_field_and_its_column_are_one_assignment() {
    let (sql, params) = BatchUpdateBuilder::<BatchSqlProfile>::new()
        .set("name", "first")
        .set("display_name", "second")
        .where_eq("id", 1)
        .build_update_statement(crate::config::DatabaseType::SQLite)
        .expect("statement should build");

    assert!(
        sql.starts_with(r#"UPDATE "batch_sql_execution_profiles" SET "name" = ? WHERE"#),
        "{sql}"
    );
    assert_eq!(
        params.first(),
        Some(&crate::internal::Value::String(Some("second".to_string())))
    );
}

#[test]
fn batch_update_set_clause_order_is_deterministic() {
    for _ in 0..16 {
        let (sql, _) = filtered_builder()
            .build_update_statement(crate::config::DatabaseType::SQLite)
            .expect("statement should build");

        assert!(
            sql.starts_with(r#"UPDATE "batch_sql_execution_users" SET "age" = ?, "name" = ?"#),
            "unexpected sql: {sql}"
        );
    }
}

/// PostgreSQL and SQLite refuse `UPDATE .. LIMIT`, so a limit there caps the
/// update through a primary-key subquery; MySQL takes the clause itself.
#[test]
fn batch_update_limit_renders_per_backend() {
    use crate::config::DatabaseType;

    for (db_type, limit, where_clause) in [
        (
            DatabaseType::Postgres,
            Some(5),
            r#" WHERE "id" IN (SELECT "id" FROM "batch_sql_execution_users" WHERE "id" = $3 LIMIT 5)"#,
        ),
        (
            DatabaseType::SQLite,
            Some(5),
            r#" WHERE "id" IN (SELECT "id" FROM "batch_sql_execution_users" WHERE "id" = ? LIMIT 5)"#,
        ),
        (DatabaseType::MySQL, Some(5), " WHERE `id` = ? LIMIT 5"),
        (DatabaseType::Postgres, None, r#" WHERE "id" = $3"#),
    ] {
        let mut update = filtered_builder();
        if let Some(limit) = limit {
            update = update.limit(limit);
        }
        let (sql, params) = update
            .build_update_statement(db_type)
            .expect("statement should build");

        assert!(sql.ends_with(where_clause), "{db_type} {limit:?}: {sql}");
        assert_eq!(params.len(), 3, "{db_type} {limit:?}: {sql}");
    }
}

#[test]
fn batch_update_or_filters_form_one_group_anded_with_the_rest() {
    let (sql, params) = BatchUpdateBuilder::<BatchSqlUser>::new()
        .set("age", 30)
        .where_gt("age", 18)
        .or_where_eq("name", "alice")
        .or_where_eq("name", "bob")
        .build_update_statement(crate::config::DatabaseType::SQLite)
        .expect("statement should build");

    assert!(
        sql.ends_with(r#"WHERE "age" > ? AND ("name" = ? OR "name" = ?)"#),
        "unexpected sql: {sql}"
    );
    assert_eq!(params.len(), 4);
}

#[test]
fn batch_json_set_binds_the_backend_path_then_the_value() {
    let (sql, params) = BatchUpdateBuilder::<BatchJsonUser>::new()
        .json_set("name", "$.profile.city", "Paris")
        .where_eq("id", 1)
        .build_update_statement(crate::config::DatabaseType::Postgres)
        .expect("statement should build");

    assert!(
        sql.starts_with(r#"UPDATE "batch_json_users" SET "name" = jsonb_set(("name")::jsonb, $1::text[], CAST($2 AS jsonb))"#),
        "unexpected sql: {sql}"
    );
    assert_eq!(
        params[..2],
        [
            crate::internal::Value::String(Some("{\"profile\",\"city\"}".to_string())),
            crate::internal::Value::String(Some("\"Paris\"".to_string())),
        ]
    );
}

#[test]
fn batch_json_set_takes_indexes_and_quoted_keys() {
    let path = r#"$.items[0]."a \"b\"""#;
    let bound = |db_type| {
        let (_, params) = BatchUpdateBuilder::<BatchJsonUser>::new()
            .json_set("name", path, 1)
            .where_eq("id", 1)
            .build_update_statement(db_type)
            .expect("statement should build");
        params
    };
    let text = |text: &str| crate::internal::Value::String(Some(text.to_string()));

    assert_eq!(
        bound(crate::config::DatabaseType::Postgres)[0],
        text(r#"{"items","0","a \"b\""}"#)
    );
    assert_eq!(
        bound(crate::config::DatabaseType::MySQL)[0],
        text(r#"$."items"[0]."a \"b\"""#)
    );
    // SQLite checks the parent first, then sets the member.
    let sqlite = bound(crate::config::DatabaseType::SQLite);
    assert_eq!(sqlite[0], text(r#"$."items"[0]"#));
    assert_eq!(sqlite[1], text(r#"$."items"[0]."a \"b\"""#));

    for path in ["$", "$.*", "$.items[*]", "items"] {
        let error = BatchUpdateBuilder::<BatchJsonUser>::new()
            .json_set("name", path, 1)
            .where_eq("id", 1)
            .build_update_statement(crate::config::DatabaseType::MySQL)
            .expect_err("path should be rejected");
        assert!(
            error.to_string().contains("unsupported JSON path"),
            "{path}: {error}"
        );
    }
}

#[tideorm::model(table = "batch_json_users")]
struct BatchJsonUser {
    #[tideorm(primary_key)]
    id: i64,
    name: serde_json::Value,
}

#[test]
fn mysql_array_remove_guards_non_array_documents() {
    for backend in [
        crate::config::DatabaseType::MySQL,
        crate::config::DatabaseType::MariaDB,
    ] {
        let (sql, params) = BatchUpdateBuilder::<BatchJsonUser>::new()
            .array_remove("name", serde_json::json!(999))
            .build_update_statement(backend)
            .unwrap();
        assert!(
            sql.contains("WHEN JSON_TYPE(`name`) <> 'ARRAY' THEN `name` ELSE"),
            "{sql}"
        );
        assert!(!params.is_empty());
    }
}
