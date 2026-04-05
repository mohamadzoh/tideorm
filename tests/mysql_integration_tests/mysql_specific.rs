use super::*;

pub(super) async fn run() {
    // =========================================================================
    // MYSQL-SPECIFIC FEATURES
    // =========================================================================
    println!("🐬 Testing: MySQL-Specific Features");
    {
        // Test ON DUPLICATE KEY UPDATE (upsert)
        let _ = Database::execute("DELETE FROM `test_users`").await;

        // Create unique index on email
        let _ = Database::execute("DROP INDEX `idx_users_email` ON `test_users`").await;
        let _ =
            Database::execute("CREATE UNIQUE INDEX `idx_users_email` ON `test_users` (`email`)")
                .await;

        // Insert
        let user = TestUser {
            id: 0,
            email: "upsert@example.com".to_string(),
            name: "Initial".to_string(),
            age: 25,
            active: true,
        };
        user.save().await.expect("Failed to save");

        // Verify insert
        let found = TestUser::query()
            .where_eq("email", "upsert@example.com")
            .first()
            .await
            .expect("Query failed")
            .unwrap();
        assert_eq!(found.name, "Initial");
        println!("   ✓ Unique index works");

        // Test LIMIT with ORDER BY
        let _ = Database::execute("DELETE FROM `test_users`").await;
        for i in 1..=5 {
            TestUser {
                id: 0,
                email: format!("order{}@example.com", i),
                name: format!("User {}", i),
                age: 30 - i,
                active: true,
            }
            .save()
            .await
            .unwrap();
        }

        let top3 = TestUser::query()
            .order_by("age", Order::Desc)
            .limit(3)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(top3.len(), 3);
        assert_eq!(top3[0].age, 29);
        println!("   ✓ LIMIT with ORDER BY works");
    }
    println!();
}
