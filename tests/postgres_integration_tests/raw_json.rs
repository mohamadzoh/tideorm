pub(super) async fn run() {
    // RAW JSON TESTS
    // =========================================================================
    println!("Testing: Raw JSON Typed Decoding");
    {
        let probe_uuid = uuid::Uuid::parse_str("6d8f4a4e-5f60-4c5f-b8fb-7ddc7310df2a")
            .expect("UUID literal should parse");
        let db = tideorm::require_db().expect("database should be available");

        db.__execute_with_params(
            "INSERT INTO test_raw_json_types (enabled, payload, amount, created_at, uuid_value) VALUES ($1, $2, $3::numeric, $4::timestamptz, $5::uuid)",
            vec![
                tideorm::internal::Value::Bool(Some(true)),
                tideorm::internal::Value::Json(Some(Box::new(serde_json::json!({
                    "kind": "probe",
                    "count": 2
                })))),
                tideorm::internal::Value::String(Some("12.34".to_string())),
                tideorm::internal::Value::String(Some("2026-03-21T10:11:12+00:00".to_string())),
                tideorm::internal::Value::String(Some(probe_uuid.to_string())),
            ],
        )
        .await
        .expect("typed raw-json probe insert should succeed");

        let rows = db
            .__raw_json_with_params(
                "SELECT enabled, payload, amount, created_at, uuid_value FROM test_raw_json_types ORDER BY id ASC",
                vec![],
            )
            .await
            .expect("typed raw-json probe query should succeed");

        assert_eq!(
            rows,
            vec![serde_json::json!({
                "enabled": true,
                "payload": {
                    "kind": "probe",
                    "count": 2
                },
                "amount": serde_json::to_value(
                    rust_decimal::Decimal::from_str_exact("12.34")
                        .expect("decimal literal should parse")
                ).expect("decimal should serialize to JSON"),
                "created_at": serde_json::to_value(
                    chrono::DateTime::parse_from_rfc3339("2026-03-21T10:11:12+00:00")
                        .expect("timestamp literal should parse")
                ).expect("timestamp should serialize to JSON"),
                "uuid_value": serde_json::to_value(probe_uuid)
                    .expect("uuid should serialize to JSON"),
            })]
        );
        println!(
            "   OK raw_json preserves PostgreSQL boolean, JSONB, numeric, timestamptz, and UUID types"
        );
    }
    println!();

    // =========================================================================
}
