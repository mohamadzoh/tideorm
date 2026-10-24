//! `set()` and `coalesce()` values are checked against the column's type and bound
//! in the model's storage type.

use crate::fixtures::{
    ChunkProjection, Item, ProjectionFlag, chunk_projection_database, items_database,
    memory_database,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde_json::json;
use tideorm::prelude::*;

#[tokio::test]
async fn f32_assignments_round_like_model_writes() {
    #[tideorm::model(table = "audit_float_precision")]
    struct FloatRow {
        #[tideorm(primary_key)]
        id: i64,
        amount: Option<f32>,
    }
    let db = memory_database("CREATE TABLE audit_float_precision(id INTEGER PRIMARY KEY, amount REAL); INSERT INTO audit_float_precision VALUES(1,NULL)").await;
    for value in [
        serde_json::json!(0.7_f64),
        serde_json::json!("0.7"),
        serde_json::json!(0.7_f32),
    ] {
        for coalesce in [false, true] {
            FloatRow::query_with(&db)
                .where_eq("id", 1)
                .update_all()
                .set("amount", None::<f32>)
                .execute()
                .await
                .unwrap();
            let update = FloatRow::query_with(&db).where_eq("id", 1).update_all();
            let update = if coalesce {
                update.coalesce("amount", value.clone())
            } else {
                update.set("amount", value.clone())
            };
            assert_eq!(update.execute().await.unwrap(), 1);
            let row = FloatRow::find_with(1, &db).await.unwrap().unwrap();
            assert_eq!(row.amount, Some(0.7_f32));
            assert_eq!(
                FloatRow::query_with(&db)
                    .where_eq("amount", row.amount)
                    .count()
                    .await
                    .unwrap(),
                1
            );
        }
    }
    FloatRow::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .coalesce("amount", 0.9_f64)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        FloatRow::find_with(1, &db).await.unwrap().unwrap().amount,
        Some(0.7_f32)
    );
}

#[tokio::test]
async fn double_assignment_must_not_make_model_unreadable() {
    let db = items_database().await;
    for value in [
        serde_json::json!("not-a-number"),
        serde_json::json!(""),
        serde_json::json!(true),
        serde_json::json!([]),
        serde_json::json!({}),
        serde_json::json!("NaN"),
        serde_json::json!("inf"),
        serde_json::json!("1e400"),
    ] {
        for coalesce in [false, true] {
            let update = Item::query_with(&db).where_eq("id", 1).update_all();
            let result = if coalesce {
                update.coalesce("amount", value.clone())
            } else {
                update.set("amount", value.clone())
            }
            .execute()
            .await;
            assert!(
                result.is_err(),
                "invalid f64 assignment accepted: {value}, coalesce={coalesce}"
            );
        }
    }
    let stored = db
        .query_raw_json("SELECT typeof(amount) AS storage FROM behavior_items WHERE id=1")
        .await
        .unwrap();
    let reload = Item::find_with(1, &db).await;
    assert_eq!(stored[0]["storage"], "real");
    assert_eq!(reload.unwrap().unwrap().amount, 1.5);
    for (value, expected) in [
        (serde_json::json!(4.5), 4.5),
        (serde_json::json!(" 4.5 "), 4.5),
        (serde_json::json!(f64::MAX), f64::MAX),
        (serde_json::json!(-f64::MAX), -f64::MAX),
    ] {
        let updated = Item::query_with(&db)
            .where_eq("id", 1)
            .update_all()
            .set("amount", value.clone())
            .execute()
            .await
            .unwrap();
        assert_eq!(updated, 1, "{value}");
        assert_eq!(
            Item::find_with(1, &db).await.unwrap().unwrap().amount,
            expected,
            "{value}"
        );
    }
}

#[tideorm::model(table = "typed_assignment_rows")]
struct Row {
    #[tideorm(primary_key)]
    id: i64,
    label: String,
    token: Option<uuid::Uuid>,
    day: Option<NaiveDate>,
    time: Option<NaiveTime>,
    local: Option<NaiveDateTime>,
    moment: Option<DateTime<Utc>>,
}

async fn database() -> Database {
    memory_database("CREATE TABLE typed_assignment_rows(id INTEGER PRIMARY KEY,label TEXT NOT NULL,token BLOB,day TEXT,time TEXT,local TEXT,moment TEXT); INSERT INTO typed_assignment_rows VALUES(1,'original',zeroblob(16),'2026-10-13','09:30:00','2026-10-13 09:30:00','2026-10-13T09:30:00Z'),(2,'original',NULL,NULL,NULL,NULL,NULL)").await
}

#[tokio::test]
async fn invalid_assignments_and_defaults_leave_every_row_unchanged() {
    let db = database().await;
    let before = Row::query_with(&db)
        .order_asc("id")
        .get_json()
        .await
        .unwrap();
    for (column, malformed) in [
        ("token", "not-a-uuid"),
        ("day", "2026-02-30"),
        ("time", "25:00:00"),
        ("local", "2026-02-30T12:00:00"),
        ("moment", "2026-10-13T25:00:00Z"),
    ] {
        for invalid in [
            json!(malformed),
            json!(""),
            json!(true),
            json!(42),
            json!([]),
            json!({}),
        ] {
            for coalesce in [false, true] {
                let update = Row::query_with(&db)
                    .where_gte("id", 1)
                    .update_all()
                    .set("label", "changed");
                let update = if coalesce {
                    update.coalesce(column, invalid.clone())
                } else {
                    update.set(column, invalid.clone())
                };
                let error = match update.execute().await {
                    Ok(rows) => panic!(
                        "{column}={invalid}, coalesce={coalesce} unexpectedly updated {rows} rows"
                    ),
                    Err(error) => error,
                };
                assert!(error.to_string().contains(column), "{error}");
                assert_eq!(
                    Row::query_with(&db)
                        .order_asc("id")
                        .get_json()
                        .await
                        .unwrap(),
                    before
                );
                assert_eq!(Row::query_with(&db).get().await.unwrap().len(), 2);
            }
        }
    }
}

#[tokio::test]
async fn valid_strings_typed_values_and_nulls_round_trip() {
    let db = database().await;
    let uuid = uuid::Uuid::from_u128(1);
    let day = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
    let time = NaiveTime::from_hms_micro_opt(23, 59, 59, 123456).unwrap();
    let local = day.and_time(time);
    let moment = local.and_utc();
    // Existing permissive formats remain accepted, including offsets and bare dates.
    for (local_text, moment_text, expected) in [
        (
            "2024-02-29T23:59:59.123456",
            "2024-02-29 23:59:59.123456+00:00",
            local,
        ),
        (
            "2024-02-29 23:59:59.123456",
            "2024-03-01T01:59:59.123456+02:00",
            local,
        ),
        (
            "2024-03-01T01:59:59.123456+02:00",
            "2024-02-29T23:59:59.123456",
            local,
        ),
        (
            "2024-02-29",
            "2024-02-29",
            day.and_hms_opt(0, 0, 0).unwrap(),
        ),
    ] {
        Row::query_with(&db)
            .where_eq("id", 1)
            .update_all()
            .set("token", uuid.to_string())
            .set("day", "2024-02-29")
            .set("time", "23:59:59.123456")
            .set("local", local_text)
            .set("moment", moment_text)
            .execute()
            .await
            .unwrap();
        let row = Row::find_with(1, &db).await.unwrap().unwrap();
        assert_eq!(
            (row.token, row.day, row.time, row.local, row.moment),
            (
                Some(uuid),
                Some(day),
                Some(time),
                Some(expected),
                Some(expected.and_utc())
            )
        );
    }
    Row::query_with(&db)
        .where_gte("id", 1)
        .update_all()
        .set("token", uuid)
        .set("day", day)
        .set("time", time)
        .set("local", local)
        .set("moment", moment)
        .execute()
        .await
        .unwrap();
    // None remains SQL NULL for every destination type.
    Row::query_with(&db)
        .where_eq("id", 2)
        .update_all()
        .set("token", None::<uuid::Uuid>)
        .set("day", None::<NaiveDate>)
        .set("time", None::<NaiveTime>)
        .set("local", None::<NaiveDateTime>)
        .set("moment", None::<DateTime<Utc>>)
        .execute()
        .await
        .unwrap();
    let row = Row::find_with(2, &db).await.unwrap().unwrap();
    assert_eq!(
        (row.token, row.day, row.time, row.local, row.moment),
        (None, None, None, None, None)
    );
    Row::query_with(&db)
        .where_gte("id", 1)
        .update_all()
        .coalesce("token", uuid.to_string())
        .coalesce("day", "2024-02-29")
        .coalesce("time", "09:30")
        .coalesce("local", local)
        .coalesce("moment", moment)
        .execute()
        .await
        .unwrap();
    for row in Row::query_with(&db).get().await.unwrap() {
        let expected_time = if row.id == 1 {
            time
        } else {
            NaiveTime::from_hms_opt(9, 30, 0).unwrap()
        };
        assert_eq!(
            (row.token, row.day, row.time, row.local, row.moment),
            (
                Some(uuid),
                Some(day),
                Some(expected_time),
                Some(local),
                Some(moment)
            )
        );
    }
}

#[tokio::test]
async fn batch_setters_reject_foreign_typed_columns() {
    let db = chunk_projection_database().await;
    for operation in 0..10 {
        let update = ChunkProjection::query_with(&db)
            .where_eq("id", 7)
            .update_all();
        let update = match operation {
            0 => update.set(ProjectionFlag::ID, 123),
            1 => update.increment(ProjectionFlag::ID, 1),
            2 => update.decrement(ProjectionFlag::ID, 1),
            3 => update.multiply(ProjectionFlag::ID, 2.0),
            4 => update.divide(ProjectionFlag::ID, 2.0),
            5 => update.coalesce(ProjectionFlag::ID, 123),
            6 => update.array_append(ProjectionFlag::ID, 123),
            7 => update.array_remove(ProjectionFlag::ID, 123),
            8 => update.json_set(ProjectionFlag::ID, "$.a", 123),
            _ => update.set_trusted_raw(ProjectionFlag::ID, "123"),
        };
        assert!(
            update
                .execute()
                .await
                .unwrap_err()
                .to_string()
                .contains("typed column"),
            "operation {operation}"
        );
    }
    assert_eq!(
        ChunkProjection::find_with(7, &db)
            .await
            .unwrap()
            .unwrap()
            .rank,
        80
    );
    for typed in [true, false] {
        let update = ChunkProjection::query_with(&db)
            .where_eq("id", 7)
            .update_all();
        let update = if typed {
            update.set(ChunkProjection::RANK, 81)
        } else {
            update.set("rank", 82)
        };
        assert_eq!(update.execute().await.unwrap(), 1);
    }
    assert_eq!(
        ChunkProjection::find_with(7, &db)
            .await
            .unwrap()
            .unwrap()
            .rank,
        82
    );
}

#[tokio::test]
async fn literal_assignments_preserve_sqlite_model_storage_types() {
    #[tideorm::model(table = "review_literal_bounds")]
    struct BoundRow {
        #[tideorm(primary_key)]
        id: i64,
        n: Option<i32>,
        amount: Option<f64>,
    }
    let db = memory_database("CREATE TABLE review_literal_bounds(id INTEGER PRIMARY KEY,n INTEGER,amount REAL); INSERT INTO review_literal_bounds VALUES(1,123,123),(2,NULL,NULL)").await;
    for value in [
        serde_json::json!(u64::MAX),
        serde_json::json!(i32::MAX as i64 + 1),
        serde_json::json!(1.5),
    ] {
        for coalesce in [false, true] {
            let query = BoundRow::query_with(&db).where_gte("id", 1).update_all();
            let query = if coalesce {
                query.coalesce("n", value.clone())
            } else {
                query.set("n", value.clone())
            };
            assert!(
                query.execute().await.is_err(),
                "{value}, coalesce={coalesce}"
            );
        }
    }
    for value in ["invalid", "NaN", "inf", "-inf"] {
        for coalesce in [false, true] {
            let query = BoundRow::query_with(&db).where_gte("id", 1).update_all();
            let query = if coalesce {
                query.coalesce("amount", value)
            } else {
                query.set("amount", value)
            };
            assert!(
                query.execute().await.is_err(),
                "{value}, coalesce={coalesce}"
            );
        }
    }
    let rows = BoundRow::query_with(&db)
        .order_asc("id")
        .get()
        .await
        .unwrap();
    assert_eq!((rows[0].n, rows[0].amount), (Some(123), Some(123.0)));
    assert_eq!((rows[1].n, rows[1].amount), (None, None));
    BoundRow::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .set("n", i32::MIN)
        .set("amount", "1.25")
        .execute()
        .await
        .unwrap();
    BoundRow::query_with(&db)
        .where_eq("id", 2)
        .update_all()
        .coalesce("n", i32::MAX)
        .coalesce("amount", "2.5")
        .execute()
        .await
        .unwrap();
    let rows = BoundRow::query_with(&db)
        .order_asc("id")
        .get()
        .await
        .unwrap();
    assert_eq!((rows[0].n, rows[0].amount), (Some(i32::MIN), Some(1.25)));
    assert_eq!((rows[1].n, rows[1].amount), (Some(i32::MAX), Some(2.5)));
    // `None` stays SQL NULL.
    BoundRow::query_with(&db)
        .where_eq("id", 2)
        .update_all()
        .set("n", None::<i32>)
        .set("amount", None::<f64>)
        .execute()
        .await
        .unwrap();
    let row = BoundRow::find_with(2, &db).await.unwrap().unwrap();
    assert_eq!((row.n, row.amount), (None, None));
}

#[tokio::test]
async fn assignments_reject_invalid_boolean_bytes_and_float_width() {
    #[tideorm::model(table = "review_assignment_types")]
    struct AssignmentRow {
        #[tideorm(primary_key)]
        id: i64,
        active: Option<bool>,
        bytes: Option<Vec<u8>>,
        amount: Option<f32>,
    }
    let db = memory_database("CREATE TABLE review_assignment_types(id INTEGER PRIMARY KEY, active BOOLEAN, bytes BLOB, amount REAL); INSERT INTO review_assignment_types VALUES(1,1,x'0102',1),(2,NULL,NULL,NULL)").await;
    for (column, value) in [
        ("active", serde_json::json!("false")),
        ("active", serde_json::json!(2)),
        ("bytes", serde_json::json!([256])),
        ("bytes", serde_json::json!([-1])),
        ("bytes", serde_json::json!([1.5])),
        ("bytes", serde_json::json!("abc")),
        ("amount", serde_json::json!(1e100)),
        ("amount", serde_json::json!(-1e100)),
        ("amount", serde_json::json!("1e100")),
    ] {
        for coalesce in [false, true] {
            let update = AssignmentRow::query_with(&db)
                .where_gte("id", 1)
                .update_all();
            let update = if coalesce {
                update.coalesce(column, value.clone())
            } else {
                update.set(column, value.clone())
            };
            assert!(
                update.execute().await.is_err(),
                "{column} {value} coalesce={coalesce}"
            );
        }
    }
    let rows = AssignmentRow::query_with(&db)
        .order_asc("id")
        .get()
        .await
        .unwrap();
    assert_eq!(
        (rows[0].active, rows[0].bytes.as_deref(), rows[0].amount),
        (Some(true), Some([1u8, 2].as_slice()), Some(1.0))
    );
    assert_eq!(
        (rows[1].active, rows[1].bytes.as_deref(), rows[1].amount),
        (None, None, None)
    );
    AssignmentRow::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .set("active", false)
        .set("bytes", vec![0u8, 255])
        .set("amount", f32::MAX)
        .execute()
        .await
        .unwrap();
    AssignmentRow::query_with(&db)
        .where_eq("id", 2)
        .update_all()
        .coalesce("active", true)
        .coalesce("bytes", Vec::<u8>::new())
        .coalesce("amount", -f32::MAX)
        .execute()
        .await
        .unwrap();
    let rows = AssignmentRow::query_with(&db)
        .order_asc("id")
        .get()
        .await
        .unwrap();
    assert_eq!(
        (rows[0].active, rows[0].bytes.as_deref(), rows[0].amount),
        (Some(false), Some([0u8, 255].as_slice()), Some(f32::MAX))
    );
    assert_eq!(
        (rows[1].active, rows[1].bytes.as_deref(), rows[1].amount),
        (Some(true), Some([].as_slice()), Some(-f32::MAX))
    );
    AssignmentRow::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .set("active", None::<bool>)
        .set("bytes", None::<Vec<u8>>)
        .set("amount", None::<f32>)
        .execute()
        .await
        .unwrap();
    let row = AssignmentRow::find_with(1, &db).await.unwrap().unwrap();
    assert_eq!((row.active, row.bytes, row.amount), (None, None, None));
}
