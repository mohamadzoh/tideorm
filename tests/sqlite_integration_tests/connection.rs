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

        // Verify it's SQLite
        assert_eq!(db.backend(), DatabaseType::SQLite);
        println!("   ✓ Database type is SQLite");
    }
    println!();
}
