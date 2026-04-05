use super::*;

pub(super) async fn run() {
    // =========================================================================
    // SOFT DELETE TESTS
    // =========================================================================
    println!("🗄️  Testing: Soft Deletes");
    {
        let _ = Database::execute("DELETE FROM `test_soft_deletes`").await;

        // Create records
        for i in 1..=5 {
            TestSoftDelete {
                id: 0,
                name: format!("Item {}", i),
                deleted_at: None,
            }
            .save()
            .await
            .expect("Failed to create");
        }

        // Soft delete some
        let item = TestSoftDelete::query()
            .where_eq("name", "Item 1")
            .first()
            .await
            .expect("Query failed")
            .expect("Item not found");
        item.soft_delete().await.expect("Soft delete failed");

        let item2 = TestSoftDelete::query()
            .where_eq("name", "Item 2")
            .first()
            .await
            .expect("Query failed")
            .expect("Item not found");
        item2.soft_delete().await.expect("Soft delete failed");

        // Query without trashed
        let active = TestSoftDelete::query().get().await.expect("Query failed");
        assert_eq!(active.len(), 3);
        println!("   ✓ Default excludes soft deleted");

        // Query with trashed
        let all = TestSoftDelete::query()
            .with_trashed()
            .get()
            .await
            .expect("Query failed");
        assert_eq!(all.len(), 5);
        println!("   ✓ with_trashed includes all");

        // Query only trashed
        let trashed = TestSoftDelete::query()
            .only_trashed()
            .get()
            .await
            .expect("Query failed");
        assert_eq!(trashed.len(), 2);
        println!("   ✓ only_trashed works");
    }
    println!();
}
