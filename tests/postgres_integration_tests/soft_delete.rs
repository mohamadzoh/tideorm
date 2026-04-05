use super::*;

pub(super) async fn run() {
    // SOFT DELETE TESTS
    // =========================================================================
    println!("Testing: Soft Delete");
    {
        let _ =
            Database::execute("TRUNCATE TABLE test_soft_deletes RESTART IDENTITY CASCADE").await;

        assert!(
            TestSoftDelete::soft_delete_enabled(),
            "soft_delete should be enabled"
        );

        let record1 = TestSoftDelete {
            id: 0,
            name: "Record 1".into(),
            deleted_at: None,
        }
        .save()
        .await
        .expect("Failed to save");
        let record2 = TestSoftDelete {
            id: 0,
            name: "Record 2".into(),
            deleted_at: None,
        }
        .save()
        .await
        .expect("Failed to save");
        let _record3 = TestSoftDelete {
            id: 0,
            name: "Record 3".into(),
            deleted_at: None,
        }
        .save()
        .await
        .expect("Failed to save");

        // Soft delete
        let deleted_record = record1.soft_delete().await.expect("Failed to soft delete");
        assert!(
            deleted_record.deleted_at.is_some(),
            "deleted_at should be set"
        );
        println!("   OK soft_delete sets deleted_at");

        // Query without trashed
        let active = TestSoftDelete::query().get().await.expect("Query failed");
        assert_eq!(active.len(), 2, "Should have 2 active records");
        println!("   OK default query excludes soft deleted");

        // Query with trashed
        let all = TestSoftDelete::query()
            .with_trashed()
            .get()
            .await
            .expect("Query failed");
        assert_eq!(all.len(), 3, "Should have 3 total records");
        println!("   OK with_trashed includes all");

        // Query only trashed
        let trashed = TestSoftDelete::query()
            .only_trashed()
            .get()
            .await
            .expect("Query failed");
        assert_eq!(trashed.len(), 1, "Should have 1 trashed record");
        assert_eq!(trashed[0].name, "Record 1");
        println!("   OK only_trashed works");

        // Restore
        let restored = deleted_record.restore().await.expect("Failed to restore");
        assert!(
            restored.deleted_at.is_none(),
            "deleted_at should be cleared"
        );

        let active_after_restore = TestSoftDelete::query().get().await.expect("Query failed");
        assert_eq!(
            active_after_restore.len(),
            3,
            "Should have 3 active records after restore"
        );
        println!("   OK restore works");

        // Force delete
        record2
            .force_delete()
            .await
            .expect("Failed to force delete");

        let final_count = TestSoftDelete::query()
            .with_trashed()
            .count()
            .await
            .expect("Count failed");
        assert_eq!(final_count, 2, "Should have 2 records after force delete");
        println!("   OK force_delete works");
    }
    println!();

    // =========================================================================
}
