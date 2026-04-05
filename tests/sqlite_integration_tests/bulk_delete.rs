use super::*;

pub(super) async fn run() {
    // =========================================================================
    // BULK DELETE TESTS
    // =========================================================================
    println!("🗑️  Testing: Bulk Delete");
    {
        let deleted = TestUser::query()
            .where_eq("active", false)
            .delete()
            .await
            .expect("Bulk delete failed");
        assert_eq!(deleted, 5);

        let remaining = TestUser::query().count().await.expect("Count failed");
        assert_eq!(remaining, 5);
        println!("   ✓ Bulk delete works");
    }
    println!();
}
