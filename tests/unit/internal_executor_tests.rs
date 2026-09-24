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
