use super::*;

pub(super) async fn run() {
    // RAW SQL TESTS
    // =========================================================================
    println!("Testing: Raw SQL");
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        for i in 1..=3 {
            let user = TestUser {
                id: 0,
                email: format!("raw{i}@example.com"),
                name: format!("Raw User {i}"),
                age: 20 + i,
                active: true,
            };
            user.save().await.expect("Failed to save");
        }

        let users: Vec<TestUser> = Database::raw_with_params::<TestUser>(
            "SELECT * FROM test_users WHERE age > $1 ORDER BY age",
            vec![21.into()],
        )
        .await
        .expect("Raw query failed");
        assert_eq!(users.len(), 2, "Should have 2 users with age > 21");
        println!("   OK raw_with_params query");
    }

    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        for i in 1..=5 {
            let user = TestUser {
                id: 0,
                email: format!("exec{i}@example.com"),
                name: format!("Exec User {i}"),
                age: 20 + i,
                active: true,
            };
            user.save().await.expect("Failed to save");
        }

        let affected = Database::execute_with_params(
            "UPDATE test_users SET active = false WHERE age > $1",
            vec![23.into()],
        )
        .await
        .expect("Execute failed");
        assert_eq!(affected, 2, "Should have updated 2 users");

        let inactive = TestUser::query()
            .where_eq("active", false)
            .count()
            .await
            .expect("Count failed");
        assert_eq!(inactive, 2);
        println!("   OK execute_with_params");
    }
    println!();

    // =========================================================================
}
