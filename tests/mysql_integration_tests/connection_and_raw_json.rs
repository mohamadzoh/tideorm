use super::*;

pub(super) async fn run() {
    // =========================================================================
    // CONNECTION TESTS
    // =========================================================================
    println!("📡 Testing: Database Connection");
    {
        let db = tideorm::require_db().unwrap();
        assert!(db.ping().await.is_ok(), "Database ping failed");
        println!("   ✓ Ping successful");

        let result = Database::execute("SELECT 1").await;
        assert!(result.is_ok(), "Raw SQL execution failed");
        println!("   ✓ Raw SQL execution works");

        // Verify it's MySQL
        assert_eq!(db.backend(), DatabaseType::MySQL);
        println!("   ✓ Database type is MySQL");
    }
    println!();

    // =========================================================================
    // RAW JSON TESTS
    // =========================================================================
    println!("🧪 Testing: Raw JSON Typed Decoding");
    {
        let db = tideorm::require_db().expect("database should be available");

        db.__execute_with_params(
            "INSERT INTO `test_raw_json_types` (`enabled`, `payload`, `amount`, `created_at`) VALUES (?, ?, ?, ?)",
            vec![
                tideorm::internal::Value::Bool(Some(true)),
                tideorm::internal::Value::Json(Some(Box::new(serde_json::json!({
                    "kind": "probe",
                    "count": 2
                })))),
                tideorm::internal::Value::String(Some("12.34".to_string())),
                tideorm::internal::Value::String(Some("2026-03-21 10:11:12".to_string())),
            ],
        )
        .await
        .expect("typed raw-json probe insert should succeed");

        let rows = db
            .__raw_json_with_params(
                "SELECT `enabled`, `payload`, `amount`, `created_at` FROM `test_raw_json_types` ORDER BY `id` ASC",
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
                    chrono::NaiveDateTime::parse_from_str("2026-03-21 10:11:12", "%Y-%m-%d %H:%M:%S")
                        .expect("datetime literal should parse")
                ).expect("datetime should serialize to JSON"),
            })]
        );
        println!("   ✓ raw_json preserves MySQL boolean, JSON, decimal, and datetime types");
    }
    println!();
}
