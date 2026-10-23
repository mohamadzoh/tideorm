use super::*;

use chrono::{Local, NaiveDate, TimeZone};

fn bound_values(db_backend: DbBackend, values: Vec<Value>) -> Vec<Value> {
    let statement = Statement::from_sql_and_values(db_backend, "SELECT 1", values);
    bind_zoned_timestamps_as_datetime(statement)
        .values
        .expect("the statement keeps its values")
        .0
}

#[test]
fn zoned_timestamps_are_bound_as_their_utc_datetime_on_mysql_only() {
    let at = NaiveDate::from_ymd_opt(1969, 7, 20)
        .unwrap()
        .and_hms_micro_opt(20, 17, 40, 123_456)
        .unwrap();
    let values = vec![
        Value::ChronoDateTimeUtc(Some(at.and_utc())),
        Value::ChronoDateTimeLocal(Some(Local.from_utc_datetime(&at))),
        Value::ChronoDateTimeUtc(None),
        Value::ChronoDateTime(Some(at)),
        Value::String(Some("1969".to_string())),
    ];

    assert_eq!(
        bound_values(DbBackend::MySql, values.clone()),
        [
            Value::ChronoDateTime(Some(at)),
            Value::ChronoDateTime(Some(at)),
            Value::ChronoDateTime(None),
            Value::ChronoDateTime(Some(at)),
            Value::String(Some("1969".to_string())),
        ]
    );
    for db_backend in [DbBackend::Postgres, DbBackend::Sqlite] {
        assert_eq!(
            bound_values(db_backend, values.clone()),
            values,
            "{db_backend:?}"
        );
    }
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn deadlocked_transaction_blocks_every_executor_entry_point() {
    use crate::Database;

    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE guarded (id INTEGER)")
        .await
        .unwrap();
    let independent = Database::connect("sqlite::memory:").await.unwrap();
    let result = db
        .transaction(|_| {
            Box::pin(async move {
                // Inject the signal normally recorded by translating the driver's
                // deadlock error. No live MySQL server is needed to verify the gate.
                crate::database::__current_connection()?
                    .source()
                    .record
                    .expect("the transaction installs its record")
                    .lock()
                    .note_failed_statement(true);
                let connection = crate::database::__current_connection()?;
                let executor = connection.executor();
                let statement =
                    || Statement::from_string(DbBackend::Sqlite, "INSERT INTO guarded VALUES (1)");
                assert!(executor.execute_raw(statement()).await.is_err());
                assert!(
                    executor
                        .execute_unprepared("INSERT INTO guarded VALUES (2)")
                        .await
                        .is_err()
                );
                assert!(executor.query_one_raw(statement()).await.is_err());
                assert!(executor.query_all_raw(statement()).await.is_err());
                assert!(executor.begin().await.is_err());
                // A separate pooled database remains usable in the failed scope.
                independent
                    .exec_raw("CREATE TABLE independent (id INTEGER)")
                    .await?;
                Ok(())
            })
        })
        .await;
    assert!(result.unwrap_err().to_string().contains("deadlocked"));
    let rows = db
        .query_raw_json("SELECT COUNT(*) AS n FROM guarded")
        .await
        .unwrap();
    assert_eq!(rows[0]["n"], 0);
    // The next transaction has no stale failure flag.
    db.transaction(|_| {
        Box::pin(async {
            Database::execute("INSERT INTO guarded VALUES (3)").await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    let rows = db
        .query_raw_json("SELECT COUNT(*) AS n FROM guarded")
        .await
        .unwrap();
    assert_eq!(rows[0]["n"], 1);
}
