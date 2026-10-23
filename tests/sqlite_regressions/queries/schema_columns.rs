use crate::fixtures::memory_database;
use tideorm::prelude::*;

#[cfg(feature = "encrypted-fields")]
#[tideorm::model(table = "schema_secrets", schema = "aux", encrypted = "value")]
struct SchemaSecret {
    #[tideorm(primary_key)]
    id: i64,
    value: String,
}

#[cfg(feature = "encrypted-fields")]
#[tokio::test]
async fn schema_typed_columns_keep_joined_encryption_checks() {
    let db = memory_database(
        "ATTACH DATABASE ':memory:' AS aux;
        CREATE TABLE main.schema_records(id INTEGER PRIMARY KEY,label TEXT);
        CREATE TABLE aux.schema_secrets(id INTEGER PRIMARY KEY,value TEXT);",
    )
    .await;
    crate::fixtures::encrypted_error(
        SchemaSecret::query_with(&db)
            .sum::<f64>(SchemaSecret::VALUE)
            .await,
    );
    crate::fixtures::encrypted_error(
        MainRow::query_with(&db)
            .inner_join(
                "aux.schema_secrets",
                "schema_records.id",
                "schema_secrets.id",
            )
            .sum::<f64>(SchemaSecret::VALUE)
            .await,
    );
    assert_eq!(
        MainRow::query_with(&db)
            .inner_join(
                "aux.schema_secrets",
                "schema_records.id",
                "schema_secrets.id"
            )
            .where_eq(SchemaSecret::ID, 1)
            .count()
            .await
            .unwrap(),
        0
    );
}

#[tideorm::model(table = "schema_records", schema = "main")]
struct MainRow {
    #[tideorm(primary_key)]
    id: i64,
    label: String,
}

#[tideorm::model(table = "schema_records", schema = "aux")]
struct AuxRow {
    #[tideorm(primary_key)]
    id: i64,
    label: String,
}

#[tideorm::model(table = "schema_records")]
struct UnqualifiedRow {
    #[tideorm(primary_key)]
    id: i64,
    label: String,
}

#[tokio::test]
async fn typed_columns_preserve_schema_ownership_for_reads_and_writes() {
    let db = memory_database(
        "ATTACH DATABASE ':memory:' AS aux;
        CREATE TABLE main.schema_records(id INTEGER PRIMARY KEY,label TEXT);
        CREATE TABLE aux.schema_records(id INTEGER PRIMARY KEY,label TEXT);
        INSERT INTO main.schema_records VALUES(1,'main');
        INSERT INTO aux.schema_records VALUES(1,'aux');",
    )
    .await;
    for update in [
        MainRow::query_with(&db)
            .update_all()
            .set(AuxRow::LABEL, "wrong"),
        MainRow::query_with(&db)
            .update_all()
            .set(AuxRow::columns.label, "wrong"),
        MainRow::query_with(&db)
            .update_all()
            .set(UnqualifiedRow::LABEL, "wrong"),
    ] {
        assert!(
            update
                .execute()
                .await
                .unwrap_err()
                .to_string()
                .contains("typed column")
        );
    }
    assert_eq!(
        MainRow::find_with(1, &db).await.unwrap().unwrap().label,
        "main"
    );
    assert_eq!(
        AuxRow::find_with(1, &db).await.unwrap().unwrap().label,
        "aux"
    );
    for query in [
        MainRow::query_with(&db).where_eq(AuxRow::LABEL, "aux"),
        MainRow::query_with(&db).where_col(AuxRow::LABEL.eq("aux")),
        MainRow::query_with(&db).order_asc(AuxRow::LABEL),
    ] {
        assert!(query.validate().is_err());
    }
    MainRow::query_with(&db)
        .where_col(MainRow::ID.eq(1))
        .update_all()
        .set(MainRow::columns.label, "updated")
        .execute()
        .await
        .unwrap();
    assert_eq!(
        MainRow::query_with(&db)
            .where_eq(MainRow::LABEL, "updated")
            .order_asc(MainRow::ID)
            .pluck::<String>(MainRow::LABEL)
            .await
            .unwrap(),
        vec!["updated"]
    );
    assert_eq!(
        AuxRow::query_with(&db)
            .where_col(AuxRow::LABEL.eq("aux"))
            .count()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        MainRow::query_with(&db)
            .max::<i64>(MainRow::ID)
            .await
            .unwrap(),
        Some(1)
    );
}
