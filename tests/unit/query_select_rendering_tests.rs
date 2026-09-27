use crate::config::DatabaseType;
use crate::model::Model;
use crate::query::CTE;

#[tideorm::model(table = "select_render_users")]
struct SelectRenderUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "select_render_accounts")]
struct SelectRenderAccount {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[tideorm(column = "user_name")]
    user_name_field: String,
}

#[tideorm::model(table = "invoices", schema = "billing")]
struct BillingInvoice {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    total: i64,
}

/// A model with a schema is reached as `schema.table` by every statement, or it
/// reads and writes a same-named table on the search path instead.
#[test]
fn test_schema_qualified_models_render_their_schema_everywhere() {
    use crate::model::ModelMeta;

    assert_eq!(BillingInvoice::schema_name(), Some("billing"));
    assert_eq!(SelectRenderUser::schema_name(), None);

    let select = BillingInvoice::query()
        .where_eq("total", 5)
        .build_select_sql_for_db(DatabaseType::Postgres);
    assert!(
        select.contains(r#"FROM "billing"."invoices" WHERE "invoices"."total""#)
            || select.contains(r#"FROM "billing"."invoices" WHERE "total""#),
        "{select}"
    );

    let joined = SelectRenderUser::query()
        .inner_join("billing.invoices", "select_render_users.id", "invoices.id")
        .where_eq("invoices.total", 5)
        .build_select_sql_for_db(DatabaseType::Postgres);
    assert!(
        joined.contains(r#"INNER JOIN "billing"."invoices" ON"#),
        "{joined}"
    );
}

fn cte() -> CTE {
    CTE::new("recent", "SELECT id FROM select_render_users".to_string())
}

#[test]
fn test_typed_and_raw_selections_compose_in_one_projection() {
    let sql = SelectRenderUser::query()
        .select(vec!["id", "name"])
        .select_raw("COUNT(*) AS total")
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert_eq!(
        sql,
        "SELECT \"select_render_users\".\"id\", \"select_render_users\".\"name\", COUNT(*) AS total FROM \"select_render_users\""
    );
}

#[test]
fn test_distinct_prefixes_the_projection() {
    let sql = SelectRenderUser::query()
        .distinct()
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert_eq!(
        sql,
        "SELECT DISTINCT \"select_render_users\".\"id\", \"select_render_users\".\"name\" FROM \"select_render_users\""
    );
}

#[test]
fn test_distinct_composes_with_every_projection_source() {
    let sql = SelectRenderUser::query()
        .select(vec!["id", "name"])
        .select_raw("COUNT(*) AS total")
        .distinct()
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert_eq!(
        sql,
        "SELECT DISTINCT \"select_render_users\".\"id\", \"select_render_users\".\"name\", COUNT(*) AS total FROM \"select_render_users\""
    );
}

#[test]
fn test_distinct_sentinel_never_reaches_parameterized_sql() {
    let (sql, _) = SelectRenderUser::query()
        .distinct()
        .where_eq("name", "ada")
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);

    assert!(
        sql.starts_with(
            "SELECT DISTINCT \"select_render_users\".\"id\", \"select_render_users\".\"name\" "
        ),
        "{sql}"
    );
    assert!(!sql.contains('\u{1}'), "{sql}");
}

#[test]
fn test_distinct_count_counts_the_deduplicated_rows() {
    let (sql, _) = SelectRenderUser::query()
        .distinct()
        .build_count_sql_with_params_for_db(DatabaseType::Postgres);

    // The `SELECT COUNT(*)` fast path would count the duplicate rows that
    // DISTINCT exists to collapse, so a distinct query has to count the
    // deduplicated result set through the derived table instead.
    assert_eq!(
        sql,
        "SELECT COUNT(*) AS count FROM (SELECT DISTINCT \"select_render_users\".\"id\", \"select_render_users\".\"name\" FROM \"select_render_users\") AS \"tideorm_count_subquery\""
    );
}

#[test]
fn test_distinct_does_not_change_the_exists_projection() {
    let (sql, _) = SelectRenderUser::query()
        .distinct()
        .build_exists_sql_with_params_for_db(DatabaseType::Postgres);

    assert_eq!(
        sql,
        "SELECT EXISTS(SELECT 1 FROM \"select_render_users\" LIMIT 1) AS \"exists_result\""
    );
}

#[test]
fn test_distinct_survives_a_fragment_round_trip_without_duplicating() {
    let fragment = SelectRenderUser::query().distinct().consolidate();

    assert!(!fragment.is_empty());

    let sql = SelectRenderUser::query()
        .distinct()
        .apply(&fragment)
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert_eq!(
        sql,
        "SELECT DISTINCT \"select_render_users\".\"id\", \"select_render_users\".\"name\" FROM \"select_render_users\""
    );
}

#[test]
fn test_empty_selection_falls_back_to_the_model_projection() {
    let sql = SelectRenderUser::query()
        .select(Vec::new())
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert_eq!(
        sql,
        "SELECT \"select_render_users\".\"id\", \"select_render_users\".\"name\" FROM \"select_render_users\""
    );
}

#[test]
fn test_cte_name_uses_backend_identifier_quoting() {
    let query = SelectRenderUser::query().with_cte(cte());

    let mysql_sql = query.build_select_sql_for_db(DatabaseType::MySQL);
    assert!(
        mysql_sql.starts_with("WITH `recent` AS ("),
        "MySQL rejects a double-quoted CTE name: {mysql_sql}"
    );

    let postgres_sql = query.build_select_sql_for_db(DatabaseType::Postgres);
    assert!(
        postgres_sql.starts_with("WITH \"recent\" AS ("),
        "{postgres_sql}"
    );
}

#[test]
fn test_self_qualified_rust_field_name_renders_the_database_column() {
    let sql = SelectRenderAccount::query()
        .select(vec!["select_render_accounts.user_name_field"])
        .group_by("select_render_accounts.user_name_field")
        .order_desc("select_render_accounts.user_name_field")
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert!(
        !sql.contains("user_name_field"),
        "the Rust field name must not reach SQL: {sql}"
    );
    assert!(
        sql.starts_with("SELECT \"select_render_accounts\".\"user_name\" "),
        "{sql}"
    );
    assert!(
        sql.contains("GROUP BY \"select_render_accounts\".\"user_name\""),
        "{sql}"
    );
    assert!(
        sql.contains("ORDER BY \"select_render_accounts\".\"user_name\" DESC"),
        "{sql}"
    );
}

#[test]
fn test_unqualified_rust_field_name_still_renders_the_database_column() {
    let sql = SelectRenderAccount::query()
        .order_asc("user_name_field")
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert!(sql.ends_with("ORDER BY \"user_name\" ASC"), "{sql}");
}

#[test]
fn test_foreign_qualifier_is_left_untouched() {
    let sql = SelectRenderAccount::query()
        .inner_join(
            "select_render_logins",
            "select_render_accounts.id",
            "select_render_logins.account_id",
        )
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert!(
        sql.contains(
            "ON \"select_render_accounts\".\"id\" = \"select_render_logins\".\"account_id\""
        ),
        "{sql}"
    );
}

#[test]
fn test_select_subquery_binds_its_values_ahead_of_the_where_clause() {
    // The scalar subquery sits in the projection, left of the WHERE clause, so
    // its value is bound first and the outer filter's placeholder follows it.
    let (sql, params) = SelectRenderUser::query()
        .select_subquery(
            SelectRenderAccount::query()
                .select(vec!["id"])
                .where_eq("user_name_field", "o'brien")
                .limit(1),
            "account_id",
        )
        .where_eq("name", "ada")
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);

    assert_eq!(
        sql,
        "SELECT (SELECT \"select_render_accounts\".\"id\" FROM \"select_render_accounts\" WHERE \"user_name\" = $1 LIMIT 1) AS \"account_id\" FROM \"select_render_users\" WHERE \"name\" = $2"
    );
    assert_eq!(
        params,
        vec![
            crate::internal::Value::String(Some("o'brien".to_string())),
            crate::internal::Value::String(Some("ada".to_string())),
        ]
    );
}

#[test]
fn test_lock_for_update_closes_the_statement_and_survives_a_fragment() {
    let locked = || {
        SelectRenderUser::query()
            .where_eq("name", "ada")
            .lock_for_update()
    };

    let (sql, _) = locked()
        .limit(1)
        .build_select_sql_with_params_for_db(DatabaseType::MySQL);
    assert_eq!(
        sql,
        "SELECT `select_render_users`.`id`, `select_render_users`.`name` FROM `select_render_users` WHERE `name` = ? LIMIT 1 FOR UPDATE"
    );
    // SQLite has no row locks and no FOR UPDATE syntax.
    let (sql, _) = locked().build_select_sql_with_params_for_db(DatabaseType::SQLite);
    assert!(!sql.contains("FOR UPDATE"), "{sql}");

    // PostgreSQL refuses FOR UPDATE next to an aggregate, so a locked count
    // locks the rows it reads through a derived table.
    let (sql, _) = locked().build_count_sql_with_params_for_db(DatabaseType::Postgres);
    assert_eq!(
        sql,
        "SELECT COUNT(*) AS count FROM (SELECT \"select_render_users\".\"id\", \"select_render_users\".\"name\" FROM \"select_render_users\" WHERE \"name\" = $1 FOR UPDATE) AS \"tideorm_count_subquery\""
    );

    let fragment = SelectRenderUser::query().lock_for_update().consolidate();
    assert!(!fragment.is_empty());
    let (sql, _) = SelectRenderUser::query()
        .apply(&fragment)
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);
    assert!(sql.ends_with(" FOR UPDATE"), "{sql}");
}

/// The page size is written into the SQL and the offset bound after every
/// other value, so each page of a paged query reuses one prepared statement.
#[test]
fn test_offset_is_bound_last_and_the_limit_written_in() {
    use crate::internal::Value;

    let (sql, params) = SelectRenderUser::query()
        .where_eq("name", "ada")
        .limit(20)
        .offset(40)
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);
    assert!(sql.ends_with(" LIMIT 20 OFFSET $2"), "{sql}");
    assert_eq!(params.last(), Some(&Value::BigInt(Some(40))));

    let (sql, params) = SelectRenderUser::query()
        .offset(40)
        .build_select_sql_with_params_for_db(DatabaseType::SQLite);
    assert!(sql.ends_with(" LIMIT -1 OFFSET ?"), "{sql}");
    assert_eq!(params, vec![Value::BigInt(Some(40))]);

    let preview = SelectRenderUser::query()
        .limit(20)
        .offset(40)
        .build_select_sql_for_db(DatabaseType::MySQL);
    assert!(preview.ends_with(" LIMIT 20 OFFSET 40"), "{preview}");
}

/// A long list is one array parameter on PostgreSQL and one JSON value on
/// SQLite, so it fits under every parameter limit; a short one stays `IN (..)`.
#[test]
fn test_long_lists_bind_one_parameter_where_the_backend_can() {
    let names: Vec<String> = (0..1_500).map(|i| format!("name{i}")).collect();

    // The array value only exists with PostgreSQL support compiled in.
    #[cfg(feature = "postgres")]
    {
        let (sql, params) = SelectRenderUser::query()
            .where_in("name", names.clone())
            .build_select_sql_with_params_for_db(DatabaseType::Postgres);
        assert!(sql.contains(r#""name" = ANY($1)"#), "{sql}");
        assert_eq!(params.len(), 1);
    }

    let (sql, params) = SelectRenderUser::query()
        .where_not_in("name", names.clone())
        .build_select_sql_with_params_for_db(DatabaseType::SQLite);
    assert!(
        sql.contains(r#""name" NOT IN (SELECT value FROM json_each(?))"#),
        "{sql}"
    );
    assert_eq!(params.len(), 1);

    let (sql, params) = SelectRenderUser::query()
        .where_in("name", names[..3].to_vec())
        .build_select_sql_with_params_for_db(DatabaseType::Postgres);
    assert!(sql.contains(r#""name" IN ($1, $2, $3)"#), "{sql}");
    assert_eq!(params.len(), 3);
}
