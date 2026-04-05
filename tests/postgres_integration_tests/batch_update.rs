use super::*;

pub(super) async fn run() {
    // BATCH UPDATE TESTS
    // =========================================================================
    println!("Testing: Batch Update");
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        for i in 0..5 {
            let user = TestUser {
                id: 0,
                email: format!("batch-update-{i}@example.com"),
                name: format!("Batch Update {i}"),
                age: 24 + (i * 3), // 24, 27, 30, 33, 36
                active: true,
            };
            user.save()
                .await
                .expect("Failed to seed user for batch update");
        }

        let affected = TestUser::update_all()
            .set("active", false)
            .where_gt("age", 30)
            .execute()
            .await
            .expect("Batch update should succeed");
        assert_eq!(affected, 2, "Two users have age > 30");

        let inactive = TestUser::query()
            .where_eq("active", false)
            .count()
            .await
            .expect("Count inactive failed");
        assert_eq!(inactive, 2, "Two users should now be inactive");

        let active = TestUser::query()
            .where_eq("active", true)
            .count()
            .await
            .expect("Count active failed");
        assert_eq!(active, 3, "Three users should remain active");

        let trusted_raw_affected = TestUser::update_all()
            .set_trusted_raw("name", "'trusted-batch-update'")
            .where_eq("id", 1)
            .execute()
            .await
            .expect("set_trusted_raw should execute trusted SQL");
        assert_eq!(trusted_raw_affected, 1, "One row should be updated");

        let trusted_name = TestUser::find_or_fail(1)
            .await
            .expect("Reload trusted raw update failed");
        assert_eq!(trusted_name.name, "trusted-batch-update");

        println!("   OK batch update builder");
    }
    println!();

    // =========================================================================
}
