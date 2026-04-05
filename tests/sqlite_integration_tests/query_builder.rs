use super::*;

pub(super) async fn run() {
    // =========================================================================
    // QUERY BUILDER TESTS
    // =========================================================================
    println!("🔍 Testing: Query Builder");

    // Clear and seed data
    let _ = Database::execute("DELETE FROM test_users").await;

    for i in 1..=10 {
        TestUser {
            id: 0,
            email: format!("user{}@example.com", i),
            name: format!("User {}", i),
            age: 20 + i,
            active: i % 2 == 0,
        }
        .save()
        .await
        .expect("Failed to seed user");
    }

    // WHERE conditions
    {
        let active_users = TestUser::query()
            .where_eq("active", true)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(active_users.len(), 5, "Should have 5 active users");
        println!("   ✓ where_eq works");

        let young_users = TestUser::query()
            .where_lt("age", 25)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(young_users.len(), 4);
        println!("   ✓ where_lt works");

        let range_users = TestUser::query()
            .where_between("age", 23, 27)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(range_users.len(), 5);
        println!("   ✓ where_between works");
    }

    // Ordering
    {
        let ordered = TestUser::query()
            .order_by("age", Order::Desc)
            .limit(3)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(ordered.len(), 3);
        assert_eq!(ordered[0].age, 30);
        println!("   ✓ order_by works");
    }

    // Pagination
    {
        let page1 = TestUser::query()
            .order_by("id", Order::Asc)
            .page(1, 3)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(page1.len(), 3);

        let page2 = TestUser::query()
            .order_by("id", Order::Asc)
            .page(2, 3)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(page2.len(), 3);
        assert_ne!(page1[0].id, page2[0].id);
        println!("   ✓ Pagination works");
    }

    // Count
    {
        let count = TestUser::query().count().await.expect("Count failed");
        assert_eq!(count, 10);

        let active_count = TestUser::query()
            .where_eq("active", true)
            .count()
            .await
            .expect("Count failed");
        assert_eq!(active_count, 5);
        println!("   ✓ count works");
    }

    // Exists
    {
        let exists = TestUser::query()
            .where_eq("email", "user1@example.com")
            .exists()
            .await
            .expect("Exists failed");
        assert!(exists);

        let not_exists = TestUser::query()
            .where_eq("email", "nonexistent@example.com")
            .exists()
            .await
            .expect("Exists failed");
        assert!(!not_exists);
        println!("   ✓ exists works");
    }

    // Pattern matching
    {
        let like_users = TestUser::query()
            .where_like("email", "%@example.com")
            .get()
            .await
            .expect("Query failed");
        assert_eq!(like_users.len(), 10);
        println!("   ✓ where_like works");
    }

    // IN clause
    {
        let in_users = TestUser::query()
            .where_in("age", vec![21, 23, 25])
            .get()
            .await
            .expect("Query failed");
        assert_eq!(in_users.len(), 3);
        println!("   ✓ where_in works");
    }
    println!();
}
