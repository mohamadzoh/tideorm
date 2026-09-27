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

#[test]
fn empty_negative_list_does_not_count_as_an_explicit_filter() {
    // Same hazard as the QueryBuilder mutation terminals: an empty candidate
    // set for a negative membership test renders constant-true, so counting
    // conditions would let a caller whose filter list came back empty rewrite
    // every row in the table.
    let err = BatchUpdateBuilder::<BatchSqlUser>::new()
        .set("name", "updated")
        .where_not_in("id", Vec::<i64>::new())
        .ensure_explicit_filters("update")
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("unfiltered bulk mutations are blocked"),
        "vacuous filter was accepted: {err}"
    );

    // One real predicate alongside it is still enough.
    assert!(
        BatchUpdateBuilder::<BatchSqlUser>::new()
            .set("name", "updated")
            .where_eq("name", "alice")
            .where_not_in("id", Vec::<i64>::new())
            .ensure_explicit_filters("update")
            .is_ok()
    );
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
    // half away from zero; an `f64` would round a value past 2^53 first.
    let sqlite = scaled(DatabaseType::SQLite);
    assert!(
        sqlite.contains(
            r#""age" = CAST((("age" * ?) + CASE WHEN ("age" * ?) < 0 THEN -? ELSE ? END) / ? AS INTEGER)"#
        ),
        "{sqlite}"
    );
    assert!(
        sqlite.contains(
            r#""id" = CAST((("id") + CASE WHEN ("id") < 0 THEN -? ELSE ? END) / ? AS INTEGER)"#
        ),
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

    // A text column is not touched, whatever it holds.
    let (sql, _) = BatchUpdateBuilder::<BatchSqlUser>::new()
        .multiply("name", 2.0)
        .where_eq("id", 1)
        .build_update_statement(DatabaseType::SQLite)
        .expect("statement should build");
    assert!(sql.contains(r#""name" = "name" * ?"#), "{sql}");
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
fn batch_update_leaves_trashed_rows_out_by_default() {
    let (sql, _) = filtered_soft_delete_builder()
        .build_update_statement(crate::config::DatabaseType::SQLite)
        .expect("statement should build");

    assert!(
        sql.contains(r#""deleted_at" IS NULL"#),
        "the default scope leaves soft-deleted rows out, as a query does: {sql}"
    );
    let (explicit_sql, _) = filtered_soft_delete_builder()
        .with_trashed()
        .without_trashed()
        .build_update_statement(crate::config::DatabaseType::SQLite)
        .expect("statement should build");
    assert_eq!(sql, explicit_sql);
}

#[test]
fn batch_update_with_trashed_reaches_the_trash_too() {
    let (sql, _) = filtered_soft_delete_builder()
        .with_trashed()
        .build_update_statement(crate::config::DatabaseType::SQLite)
        .expect("statement should build");

    assert!(
        !sql.contains("deleted_at"),
        "with_trashed() must not filter soft-deleted rows: {sql}"
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

#[test]
fn batch_update_limit_is_scoped_by_primary_key_subquery_on_postgres() {
    let (sql, params) = filtered_builder()
        .limit(5)
        .build_update_statement(crate::config::DatabaseType::Postgres)
        .expect("statement should build");

    assert!(
        sql.contains(
            r#"WHERE "id" IN (SELECT "id" FROM "batch_sql_execution_users" WHERE "id" = $3 LIMIT 5)"#
        ),
        "unexpected sql: {sql}"
    );
    assert_eq!(params.len(), 3);
}

#[test]
fn batch_update_limit_is_scoped_by_primary_key_subquery_on_sqlite() {
    let (sql, _) = filtered_builder()
        .limit(5)
        .build_update_statement(crate::config::DatabaseType::SQLite)
        .expect("statement should build");

    assert!(
        sql.contains(
            r#"WHERE "id" IN (SELECT "id" FROM "batch_sql_execution_users" WHERE "id" = ? LIMIT 5)"#
        ),
        "unexpected sql: {sql}"
    );
}

#[test]
fn batch_update_limit_uses_the_native_clause_on_mysql() {
    let (sql, _) = filtered_builder()
        .limit(5)
        .build_update_statement(crate::config::DatabaseType::MySQL)
        .expect("statement should build");

    assert!(sql.ends_with(" LIMIT 5"), "unexpected sql: {sql}");
    assert!(!sql.contains("SELECT"), "unexpected sql: {sql}");
}

#[test]
fn batch_update_without_limit_keeps_a_plain_where_clause() {
    let (sql, _) = filtered_builder()
        .build_update_statement(crate::config::DatabaseType::Postgres)
        .expect("statement should build");

    assert!(
        sql.ends_with(r#" WHERE "id" = $3"#),
        "unexpected sql: {sql}"
    );
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
    let (sql, params) = BatchUpdateBuilder::<BatchSqlUser>::new()
        .json_set("name", "$.profile.city", "Paris")
        .where_eq("id", 1)
        .build_update_statement(crate::config::DatabaseType::Postgres)
        .expect("statement should build");

    assert!(
        sql.starts_with(r#"UPDATE "batch_sql_execution_users" SET "name" = jsonb_set(("name")::jsonb, $1::text[], CAST($2 AS jsonb))"#),
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
