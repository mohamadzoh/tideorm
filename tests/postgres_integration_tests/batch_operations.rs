use super::*;

pub(super) async fn run() {
    // BATCH OPERATIONS TESTS
    // =========================================================================
    println!("Testing: Batch Operations");
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        let users = vec![
            TestUser {
                id: 0,
                email: "batch1@example.com".into(),
                name: "Batch 1".into(),
                age: 25,
                active: true,
            },
            TestUser {
                id: 0,
                email: "batch2@example.com".into(),
                name: "Batch 2".into(),
                age: 30,
                active: true,
            },
            TestUser {
                id: 0,
                email: "batch3@example.com".into(),
                name: "Batch 3".into(),
                age: 35,
                active: false,
            },
        ];

        let inserted = TestUser::insert_all(users)
            .await
            .expect("Insert all failed");

        assert_eq!(inserted.len(), 3, "Should have inserted 3 users");
        for user in &inserted {
            assert!(user.id > 0, "Each user should have an ID");
        }

        let count = TestUser::count().await.expect("Count failed");
        assert_eq!(count, 3, "Should have 3 users in database");
        println!("   OK insert_all");
    }
    println!();

    // =========================================================================
}
