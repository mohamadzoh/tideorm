use super::*;
use crate::internal::OrmColumnType;

/// The SQL type sync would give a column of `rust_type` on `backend`.
///
/// Sync renders the shared mapping table verbatim, so the type arrives as a
/// custom (already-rendered) type rather than one of the engine's own variants.
fn sync_column_type(rust_type: &str, backend: Backend) -> String {
    let mut column = OrmColumnDef::new(Alias::new("value"));
    let _ = apply_column_type(&mut column, rust_type, false, backend);

    match column.get_column_type() {
        Some(OrmColumnType::Custom(iden)) => iden.inner().into_owned(),
        other => panic!("expected a rendered type for '{rust_type}', got {other:?}"),
    }
}

#[test]
fn sync_renders_the_shared_rust_type_table() {
    // The one thing this test really pins: sync must not carry its own opinion
    // about what a Rust type becomes. Whatever `rust_type_to_column_type` says,
    // rendered for the backend, is what the column gets.
    for rust_type in [
        "i64",
        "u32",
        "Decimal",
        "String",
        "Uuid",
        "NaiveDateTime",
        "DateTime<Utc>",
        "Vec<u8>",
        "Vec<String>",
        "serde_json::Value",
        "bool",
    ] {
        for backend in [Backend::Postgres, Backend::MySql, Backend::Sqlite] {
            let expected = crate::schema::rust_type_to_sql(rust_type, backend.as_database_type());
            assert_eq!(
                sync_column_type(rust_type, backend),
                expected,
                "sync disagreed with the shared mapping for '{rust_type}' on {backend:?}"
            );
        }
    }
}

#[test]
fn naive_timestamps_are_not_swallowed_by_the_date_time_arm() {
    // "NaiveDateTime" contains "DateTime", so a broader arm ordered first would
    // give naive columns a TIMESTAMPTZ type that shifts by session timezone.
    for spelling in [
        "NaiveDateTime",
        "chrono::NaiveDateTime",
        "Option<NaiveDateTime>",
    ] {
        assert_eq!(sync_column_type(spelling, Backend::Postgres), "TIMESTAMP");
        // MySQL's TIMESTAMP is the tz-converting type there; a naive value
        // belongs in DATETIME.
        assert_eq!(sync_column_type(spelling, Backend::MySql), "DATETIME");
    }
}

#[test]
fn aware_and_date_only_types_keep_their_own_mapping() {
    assert_eq!(
        sync_column_type("DateTime<Utc>", Backend::Postgres),
        "TIMESTAMPTZ"
    );
    assert_eq!(
        sync_column_type("chrono::DateTime<chrono::Utc>", Backend::Postgres),
        "TIMESTAMPTZ"
    );
    assert_eq!(sync_column_type("NaiveDate", Backend::Postgres), "DATE");
    assert_eq!(sync_column_type("NaiveTime", Backend::Postgres), "TIME");
}

#[test]
fn unsigned_integers_land_in_the_column_the_driver_reads_back() {
    // PostgreSQL has no unsigned column type. sea-orm decodes a `u32` as an
    // `Oid` and then as an `i32`, so a BIGINT column is unreadable however well
    // it would hold the range.
    assert_eq!(sync_column_type("u8", Backend::Postgres), "SMALLINT");
    assert_eq!(sync_column_type("u16", Backend::Postgres), "INTEGER");
    assert_eq!(sync_column_type("u32", Backend::Postgres), "INTEGER");
    assert_eq!(sync_column_type("u64", Backend::Postgres), "BIGINT");
    assert_eq!(
        sync_column_type("Option<u16>", Backend::Postgres),
        "INTEGER"
    );

    // MySQL does have them, so nothing widens there.
    assert_eq!(sync_column_type("u32", Backend::MySql), "INT UNSIGNED");
}

#[test]
fn decimals_land_in_a_column_sea_orm_can_decode() {
    // sea-orm reads Decimal/BigDecimal on SQLite through
    // `try_get::<Option<f64>>`, and sqlx only yields an f64 from REAL affinity,
    // so a DB_SYNC-built TEXT column would fail every read.
    assert_eq!(sync_column_type("Decimal", Backend::Sqlite), "REAL");
    assert_eq!(sync_column_type("Decimal", Backend::Postgres), "DECIMAL");
    assert_eq!(
        sync_column_type("rust_decimal::Decimal", Backend::Sqlite),
        "REAL"
    );
    // i128/u128 ride the same decimal mapping.
    assert_eq!(sync_column_type("i128", Backend::Sqlite), "REAL");
    assert_eq!(
        sync_column_type("i128", Backend::Postgres),
        "DECIMAL(39, 0)"
    );
}

#[test]
fn uuid_columns_match_what_the_driver_binds() {
    // sqlx-mysql encodes a Uuid as 16 raw bytes and refuses to decode anything
    // else, so a CHAR(36) column rejects every insert with error 1366. The
    // other two backends keep their native/text form.
    assert_eq!(sync_column_type("Uuid", Backend::MySql), "BINARY(16)");
    assert_eq!(sync_column_type("Uuid", Backend::Postgres), "UUID");
    assert_eq!(sync_column_type("Uuid", Backend::Sqlite), "TEXT");
}

#[test]
fn auto_increment_keys_keep_a_native_integer_type() {
    // sea-query builds SERIAL/IDENTITY/AUTOINCREMENT out of the column's native
    // integer type and panics on a custom one.
    let mut column = OrmColumnDef::new(Alias::new("id"));
    let integer_key = apply_column_type(&mut column, "i64", true, Backend::Postgres);
    assert!(integer_key);
    assert!(matches!(
        column.get_column_type(),
        Some(OrmColumnType::BigInteger)
    ));

    // A key sync cannot auto-increment must say so, or sea-query panics while
    // rendering the IDENTITY clause.
    let mut text_key = OrmColumnDef::new(Alias::new("id"));
    let uuid_key = apply_column_type(&mut text_key, "Uuid", true, Backend::Postgres);
    assert!(!uuid_key);
}

#[test]
fn auto_increment_keys_keep_the_width_the_plain_column_would_get() {
    // A u32 key built as a big integer becomes BIGSERIAL on PostgreSQL, which
    // sea-orm cannot decode back into a u32 - and it disagrees with the INTEGER
    // a plain u32 column gets. The two widths have to match.
    let mut key = OrmColumnDef::new(Alias::new("id"));
    let unsigned_key = apply_column_type(&mut key, "u32", true, Backend::Postgres);
    assert!(unsigned_key);
    assert!(matches!(
        key.get_column_type(),
        Some(OrmColumnType::Integer)
    ));

    let mut small = OrmColumnDef::new(Alias::new("id"));
    let tiny_key = apply_column_type(&mut small, "u8", true, Backend::Postgres);
    assert!(tiny_key);
    assert!(matches!(
        small.get_column_type(),
        Some(OrmColumnType::SmallInteger)
    ));
}

#[test]
fn create_table_never_emits_auto_increment_for_a_non_integer_key() {
    // A UUID key marked auto-increment used to reach sea-query's Postgres
    // renderer, which panics on anything but a native integer.
    let model = ModelSchema::new("users")
        .schema("")
        .column(ColumnDef::new("id", "Uuid").primary_key().auto_increment());

    let sql = build_create_table_sql(&model, Backend::Postgres);
    assert!(sql.contains("UUID"), "Got: {}", sql);
    assert!(!sql.to_uppercase().contains("SERIAL"), "Got: {}", sql);
}

#[test]
fn create_table_targets_the_schema_the_model_declares() {
    let model = ModelSchema::new("users")
        .schema("tenant_a")
        .column(ColumnDef::new("id", "i64").primary_key());

    // The existence probe and the force DROP both filter on `schema_name`, so
    // CREATE TABLE has to name the same schema or every run recreates the table
    // somewhere neither of them looks.
    let postgres = build_create_table_sql(&model, Backend::Postgres);
    assert!(
        postgres.contains("\"tenant_a\".\"users\""),
        "CREATE TABLE must name the schema the model declares. Got: {}",
        postgres
    );

    // MySQL's "schema" is the connected database and SQLite has none, so
    // qualifying there would name a database that does not exist.
    let mysql = build_create_table_sql(&model, Backend::MySql);
    assert!(mysql.contains("`users`"), "Got: {}", mysql);
    assert!(!mysql.contains("tenant_a"), "Got: {}", mysql);

    let sqlite = build_create_table_sql(&model, Backend::Sqlite);
    assert!(!sqlite.contains("tenant_a"), "Got: {}", sqlite);
}

#[test]
fn qualifying_schema_is_postgres_only_and_skips_empty_names() {
    let pg = Backend::Postgres;
    let model = ModelSchema::new("users").schema("tenant_a");
    assert_eq!(qualifying_schema(&model, pg), Some("tenant_a"));
    assert_eq!(qualifying_schema(&model, Backend::MySql), None);
    assert_eq!(qualifying_schema(&model, Backend::Sqlite), None);

    let unqualified = ModelSchema::new("users").schema("");
    assert_eq!(qualifying_schema(&unqualified, pg), None);
}

#[test]
fn column_diff_reports_missing_and_extra_columns() {
    let model = ModelSchema::new("users")
        .column(ColumnDef::new("id", "i64").primary_key())
        .column(ColumnDef::new("email", "String").not_null())
        .column(ColumnDef::new("phone", "Option<String>"));

    let existing = vec![
        "id".to_string(),
        "EMAIL".to_string(),
        "legacy_note".to_string(),
    ];

    let (missing, extra) = diff_columns(&model, &existing);

    assert_eq!(
        missing
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec!["phone"]
    );
    assert_eq!(extra, vec!["legacy_note"]);
}

#[test]
fn column_diff_is_empty_when_the_table_matches_the_model() {
    let model = ModelSchema::new("users").column(ColumnDef::new("id", "i64").primary_key());
    let existing = vec!["id".to_string()];

    let (missing, extra) = diff_columns(&model, &existing);

    assert!(missing.is_empty());
    assert!(extra.is_empty());
}
