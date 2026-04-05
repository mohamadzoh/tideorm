use super::*;

pub(super) async fn run() {
    // CONNECTION TESTS
    // =========================================================================
    println!("Testing: Database Connection");
    {
        let db = tideorm::require_db().unwrap();
        assert!(db.ping().await.is_ok(), "Database ping failed");
        println!("   OK Ping successful");

        let result = Database::execute("SELECT 1").await;
        assert!(result.is_ok(), "Raw SQL execution failed");
        println!("   OK Raw SQL execution works");
    }
    println!();

    // =========================================================================
}
