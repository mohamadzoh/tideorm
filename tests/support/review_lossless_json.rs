use super::{Database, DatabaseType, backend};
use tideorm::prelude::*;

#[tideorm::model(table = "review_multiranges")]
struct MultirangeRow {
    #[tideorm(primary_key)]
    id: i64,
    value: String,
}

#[tokio::test]
async fn postgres_multiranges_reach_raw_and_model_json() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    let rows = db
        .query_raw_json("SELECT '{[1,3)}'::int4multirange::text AS m")
        .await
        .unwrap();
    assert_eq!(rows[0]["m"], "{[1,3)}");
    let rows = db
        .query_raw_json("SELECT '{[1,3)}'::int4multirange AS m")
        .await
        .unwrap();
    assert_eq!(rows[0]["m"], "{[1,3)}");
    db.exec_raw("DROP TABLE IF EXISTS review_multiranges")
        .await
        .unwrap();
    db.exec_raw("CREATE TABLE review_multiranges (id BIGINT PRIMARY KEY, value int4multirange); INSERT INTO review_multiranges VALUES (1, '{[1,3),[5,7)}'), (2, '{}'), (3, NULL)").await.unwrap();
    let expected = serde_json::json!([
        {"id":1,"value":"{[1,3),[5,7)}"}, {"id":2,"value":"{}"}, {"id":3,"value":null}
    ]);
    let raw = db
        .query_raw_json("SELECT id, value FROM review_multiranges ORDER BY id")
        .await
        .unwrap();
    assert_eq!(serde_json::json!(raw), expected);
    let model = MultirangeRow::query_with(&db)
        .order_asc("id")
        .get_json()
        .await
        .unwrap();
    assert_eq!(serde_json::json!(model), expected);
    // Compare all six multirange families with server-rendered text. UTC is
    // the established range output policy, independent of connection timezone.
    db.transaction(async |_| {
        Database::execute("SET LOCAL TIME ZONE 'UTC'").await?;
        let rows = Database::raw_json("SELECT m, m::text AS expected FROM (VALUES ('{[1,3),[5,7)}'::int4multirange)) v(m)").await?;
        assert_eq!(rows[0]["m"], rows[0]["expected"]);
        for sql in [
            "SELECT m, m::text AS expected FROM (VALUES ('{[9000000000,9000000002)}'::int8multirange)) v(m)",
            "SELECT m, m::text AS expected FROM (VALUES ('{[1.12345678901234567890123456789,2)}'::nummultirange)) v(m)",
            "SELECT m, m::text AS expected FROM (VALUES ('{[2020-01-01,infinity)}'::datemultirange)) v(m)",
            "SELECT m, m::text AS expected FROM (VALUES ('{[2020-01-01 10:00,2020-01-01 11:00)}'::tsmultirange)) v(m)",
            "SELECT m, m::text AS expected FROM (VALUES ('{[2020-01-01 10:00+00,2020-01-01 11:00+00)}'::tstzmultirange)) v(m)",
        ] {
            let rows = Database::raw_json(sql).await?;
            assert_eq!(rows[0]["m"], rows[0]["expected"]);
        }
        let rows = Database::raw_json("SELECT ARRAY['{[1,3)}'::int4multirange, NULL, '{}'::int4multirange] AS m").await?;
        assert_eq!(rows[0]["m"], serde_json::json!(["{[1,3)}", null, "{}"]));
        Ok(())
    }).await.unwrap();
    db.exec_raw("DROP TABLE review_multiranges").await.unwrap();
}

#[tideorm::model(table = "review_exact_decimals")]
struct ExactDecimalRow {
    #[tideorm(primary_key)]
    id: i64,
    value: Option<rust_decimal::Decimal>,
}

#[tokio::test]
async fn mysql_decimal_65_30_keeps_every_digit() {
    if !matches!(
        backend::DATABASE_TYPE,
        DatabaseType::MySQL | DatabaseType::MariaDB
    ) || !backend::connect().await
    {
        return;
    }
    let db = tideorm::require_db().unwrap();
    let digits = "12345678901234567890123456789012345.123456789012345678901234567890";
    db.exec_raw("DROP TABLE IF EXISTS review_exact_decimals")
        .await
        .unwrap();
    db.exec_raw("CREATE TABLE review_exact_decimals (id BIGINT PRIMARY KEY, value DECIMAL(65,30))")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_exact_decimals VALUES (1, 12345678901234567890123456789012345.123456789012345678901234567890), (2, -12345678901234567890123456789012345.123456789012345678901234567890), (3, NULL)").await.unwrap();
    let expected = serde_json::json!([
        {"id":1,"value":digits}, {"id":2,"value":format!("-{digits}")}, {"id":3,"value":null}
    ]);
    let raw = db
        .query_raw_json("SELECT id, value FROM review_exact_decimals ORDER BY id")
        .await
        .unwrap();
    assert_eq!(serde_json::json!(raw), expected);
    let model = ExactDecimalRow::query_with(&db)
        .order_asc("id")
        .get_json()
        .await
        .unwrap();
    assert_eq!(serde_json::json!(model), expected);
    // Text under a decimal field's name is not a DECIMAL, so it stays text.
    let aliased = ExactDecimalRow::query_with(&db)
        .select_raw("'00123' AS value")
        .where_eq("id", 1)
        .get_json()
        .await
        .unwrap();
    assert_eq!(aliased[0]["value"], "00123");
    db.exec_raw("DROP TABLE review_exact_decimals")
        .await
        .unwrap();
}
