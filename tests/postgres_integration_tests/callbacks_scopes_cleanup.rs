use super::*;

pub(super) async fn run() {
    // CALLBACK TESTS
    // =========================================================================
    println!("Testing: Callbacks");
    {
        CALLBACK_EVENTS.lock().unwrap().clear();
        let created = CallbackUser {
            id: 0,
            email: "UPPER@EXAMPLE.COM".into(),
            name: "Callback User".into(),
        }
        .save()
        .await
        .expect("Callback save should succeed");

        assert_eq!(created.email, "upper@example.com");
        assert_eq!(
            CALLBACK_EVENTS.lock().unwrap().clone(),
            vec![
                "before_validation",
                "after_validation",
                "before_save",
                "before_create",
                "after_create",
                "after_save"
            ]
        );

        CALLBACK_EVENTS.lock().unwrap().clear();
        let updated = CallbackUser {
            id: created.id,
            email: "SECOND@EXAMPLE.COM".into(),
            name: "Callback User Updated".into(),
        }
        .update()
        .await
        .expect("Callback update should succeed");

        assert_eq!(updated.email, "second@example.com");
        assert_eq!(
            CALLBACK_EVENTS.lock().unwrap().clone(),
            vec![
                "before_validation",
                "after_validation",
                "before_save",
                "before_update",
                "after_update",
                "after_save"
            ]
        );

        CALLBACK_EVENTS.lock().unwrap().clear();
        let deleted = updated
            .delete()
            .await
            .expect("Callback delete should succeed");
        assert_eq!(deleted, 1);
        assert_eq!(
            CALLBACK_EVENTS.lock().unwrap().clone(),
            vec!["before_delete", "after_delete"]
        );
        println!("   OK save/update/delete callbacks");
    }
    println!();

    // =========================================================================
    // SCOPES TESTS
    // =========================================================================
    println!("Testing: Scopes");
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        for i in 1..=10 {
            let user = TestUser {
                id: 0,
                email: format!("scope{i}@example.com"),
                name: format!("Scope User {i}"),
                age: 20 + i,
                active: i <= 5,
            };
            user.save().await.expect("Failed to save");
        }

        fn active_scope(q: QueryBuilder<TestUser>) -> QueryBuilder<TestUser> {
            q.where_eq("active", true)
        }

        fn adult_scope(q: QueryBuilder<TestUser>) -> QueryBuilder<TestUser> {
            q.where_gte("age", 25)
        }

        let users = TestUser::query()
            .scope(active_scope)
            .scope(adult_scope)
            .get()
            .await
            .expect("Query failed");

        assert_eq!(users.len(), 1, "Should have 1 user matching both scopes");
        println!("   OK scope chaining");
    }

    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        for i in 1..=5 {
            let user = TestUser {
                id: 0,
                email: format!("cond{i}@example.com"),
                name: format!("Conditional User {i}"),
                age: 20 + i,
                active: i <= 3,
            };
            user.save().await.expect("Failed to save");
        }

        let filter_active = true;
        let users = TestUser::query()
            .when(filter_active, |q| q.where_eq("active", true))
            .get()
            .await
            .expect("Query failed");
        assert_eq!(
            users.len(),
            3,
            "Should have 3 active users when filter is true"
        );

        let filter_active = false;
        let users = TestUser::query()
            .when(filter_active, |q| q.where_eq("active", true))
            .get()
            .await
            .expect("Query failed");
        assert_eq!(users.len(), 5, "Should have 5 users when filter is false");

        let min_age: Option<i32> = Some(23);
        let users = TestUser::query()
            .when_some(min_age, |q, age| q.where_gte("age", age))
            .get()
            .await
            .expect("Query failed");
        assert_eq!(users.len(), 3, "Should have 3 users with age >= 23");
        println!("   OK conditional scopes (when/when_some)");
    }
    println!();

    // =========================================================================
    // CLEANUP
    // =========================================================================
    println!("Cleaning up...");
    let _ = Database::execute("DROP TABLE IF EXISTS test_soft_deletes CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_posts CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_users CASCADE").await;

    println!("\n All PostgreSQL integration tests passed!\n");
}
