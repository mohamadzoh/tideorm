use super::*;

pub(super) async fn run() {
    // QUERY BUILDER TESTS
    // =========================================================================
    println!("Testing: Query Builder");

    // Where Equal
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        for i in 1..=5 {
            let user = TestUser {
                id: 0,
                email: format!("user{i}@example.com"),
                name: format!("User {i}"),
                age: 20 + i,
                active: i % 2 == 0,
            };
            user.save().await.expect("Failed to save");
        }

        let active_users = TestUser::query()
            .where_eq("active", true)
            .get()
            .await
            .expect("Query failed");

        assert_eq!(active_users.len(), 2, "Should have 2 active users");
        for user in &active_users {
            assert!(user.active, "All users should be active");
        }
        println!("   OK where_eq");
    }

    // Where Greater Than / Less Than
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        for i in 1..=5 {
            let user = TestUser {
                id: 0,
                email: format!("age{i}@example.com"),
                name: format!("User {i}"),
                age: 20 + (i * 5), // Ages: 25, 30, 35, 40, 45
                active: true,
            };
            user.save().await.expect("Failed to save");
        }

        let older_users = TestUser::query()
            .where_gt("age", 30)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(older_users.len(), 3, "Should have 3 users with age > 30");

        let younger_users = TestUser::query()
            .where_lt("age", 35)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(younger_users.len(), 2, "Should have 2 users with age < 35");
        println!("   OK where_gt / where_lt");
    }

    // Where Like
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        TestUser {
            id: 0,
            email: "john@gmail.com".into(),
            name: "John Doe".into(),
            age: 25,
            active: true,
        }
        .save()
        .await
        .ok();
        TestUser {
            id: 0,
            email: "jane@gmail.com".into(),
            name: "Jane Doe".into(),
            age: 30,
            active: true,
        }
        .save()
        .await
        .ok();
        TestUser {
            id: 0,
            email: "bob@yahoo.com".into(),
            name: "Bob Smith".into(),
            age: 35,
            active: true,
        }
        .save()
        .await
        .ok();

        let gmail_users = TestUser::query()
            .where_like("email", "%gmail%")
            .get()
            .await
            .expect("Query failed");
        assert_eq!(gmail_users.len(), 2, "Should have 2 gmail users");

        let doe_users = TestUser::query()
            .where_like("name", "%Doe%")
            .get()
            .await
            .expect("Query failed");
        assert_eq!(doe_users.len(), 2, "Should have 2 Doe users");
        println!("   OK where_like");
    }

    // Where In
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        for i in 1..=5 {
            let user = TestUser {
                id: 0,
                email: format!("in_test{i}@example.com"),
                name: format!("User {i}"),
                age: 20 + i,
                active: true,
            };
            user.save().await.expect("Failed to save");
        }

        let users = TestUser::query()
            .where_in("age", vec![21, 23, 25])
            .get()
            .await
            .expect("Query failed");
        assert_eq!(users.len(), 3, "Should have 3 users with ages 21, 23, 25");
        println!("   OK where_in");
    }

    // Order and Limit
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        for i in 1..=10 {
            let user = TestUser {
                id: 0,
                email: format!("order{i}@example.com"),
                name: format!("User {i:02}"),
                age: 20 + i,
                active: true,
            };
            user.save().await.expect("Failed to save");
        }

        let users = TestUser::query()
            .order_by("age", Order::Desc)
            .limit(3)
            .get()
            .await
            .expect("Query failed");

        assert_eq!(users.len(), 3, "Should have 3 users");
        assert_eq!(users[0].age, 30, "First user should be oldest");
        assert_eq!(users[1].age, 29);
        assert_eq!(users[2].age, 28);
        println!("   OK order_by / limit");
    }

    // Pagination
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        for i in 1..=20 {
            let user = TestUser {
                id: 0,
                email: format!("page{i}@example.com"),
                name: format!("User {i:02}"),
                age: 20 + i,
                active: true,
            };
            user.save().await.expect("Failed to save");
        }

        let page2 = TestUser::query()
            .order_by("age", Order::Asc)
            .page(2, 5)
            .get()
            .await
            .expect("Query failed");

        assert_eq!(page2.len(), 5, "Should have 5 users on page 2");
        assert_eq!(page2[0].age, 26, "First user on page 2 should have age 26");
        println!("   OK pagination");
    }

    // Count
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        for i in 1..=10 {
            let user = TestUser {
                id: 0,
                email: format!("count{i}@example.com"),
                name: format!("User {i}"),
                age: 20 + i,
                active: i <= 6,
            };
            user.save().await.expect("Failed to save");
        }

        let total = TestUser::count().await.expect("Count failed");
        assert_eq!(total, 10, "Should have 10 total users");

        let active_count = TestUser::query()
            .where_eq("active", true)
            .count()
            .await
            .expect("Count failed");
        assert_eq!(active_count, 6, "Should have 6 active users");
        println!("   OK count");
    }

    // First
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        for i in 1..=5 {
            let user = TestUser {
                id: 0,
                email: format!("first{i}@example.com"),
                name: format!("User {i}"),
                age: 20 + i,
                active: true,
            };
            user.save().await.expect("Failed to save");
        }

        let first = TestUser::query()
            .where_gt("age", 22)
            .order_by("age", Order::Asc)
            .first()
            .await
            .expect("Query failed");

        assert!(first.is_some());
        assert_eq!(
            first.unwrap().age,
            23,
            "First matching user should have age 23"
        );
        println!("   OK first");
    }

    // Bulk Delete
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        for i in 1..=10 {
            let user = TestUser {
                id: 0,
                email: format!("bulk{i}@example.com"),
                name: format!("User {i}"),
                age: 20 + i,
                active: i <= 5,
            };
            user.save().await.expect("Failed to save");
        }

        let deleted = TestUser::query()
            .where_eq("active", false)
            .delete()
            .await
            .expect("Delete failed");
        assert_eq!(deleted, 5, "Should have deleted 5 inactive users");

        let remaining = TestUser::count().await.expect("Count failed");
        assert_eq!(remaining, 5, "Should have 5 remaining users");
        println!("   OK bulk delete");
    }
    println!();

    // =========================================================================
}
