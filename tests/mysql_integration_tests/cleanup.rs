use super::*;

pub(super) async fn run() {
    // =========================================================================
    // CLEANUP
    // =========================================================================
    println!("🧹 Cleanup");
    let _ = Database::execute("DROP TABLE IF EXISTS `test_soft_deletes`").await;
    let _ = Database::execute("DROP TABLE IF EXISTS `test_posts`").await;
    let _ = Database::execute("DROP TABLE IF EXISTS `test_products`").await;
    let _ = Database::execute("DROP TABLE IF EXISTS `test_users`").await;
    println!("   ✓ Tables dropped");

    println!("\n All MySQL integration tests passed!");
}
