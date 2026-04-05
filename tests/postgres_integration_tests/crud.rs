use super::*;

pub(super) async fn run() {
    // CRUD TESTS
    // =========================================================================
    println!("Testing: CRUD Operations");

    // Create and Find
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        let user = TestUser {
            id: 0,
            email: "test@example.com".to_string(),
            name: "Test User".to_string(),
            age: 25,
            active: true,
        };

        let saved_user = user.save().await.expect("Failed to save user");
        assert!(saved_user.id > 0, "User should have an auto-generated ID");
        assert_eq!(saved_user.email, "test@example.com");

        let found = TestUser::find(saved_user.id)
            .await
            .expect("Failed to find user");
        assert!(found.is_some(), "User should be found");
        let found_user = found.unwrap();
        assert_eq!(found_user.email, "test@example.com");
        assert_eq!(found_user.name, "Test User");
        println!("   OK Create and Find");
    }

    // Update
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        let user = TestUser {
            id: 0,
            email: "update@example.com".to_string(),
            name: "Original Name".to_string(),
            age: 30,
            active: true,
        };
        let mut saved_user = user.save().await.expect("Failed to save user");

        saved_user.name = "Updated Name".to_string();
        saved_user.age = 31;
        let updated_user = saved_user.update().await.expect("Failed to update user");

        assert_eq!(updated_user.name, "Updated Name");
        assert_eq!(updated_user.age, 31);

        let reloaded = TestUser::find(updated_user.id)
            .await
            .expect("Failed to reload")
            .unwrap();
        assert_eq!(reloaded.name, "Updated Name");
        println!("   OK Update");
    }

    // Delete
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        let user = TestUser {
            id: 0,
            email: "delete@example.com".to_string(),
            name: "To Delete".to_string(),
            age: 25,
            active: true,
        };
        let saved_user = user.save().await.expect("Failed to save user");
        let user_id = saved_user.id;

        let deleted_count = saved_user.delete().await.expect("Failed to delete");
        assert_eq!(deleted_count, 1);

        let found = TestUser::find(user_id).await.expect("Find failed");
        assert!(found.is_none(), "User should be deleted");
        println!("   OK Delete");
    }

    // Destroy by ID
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        let user = TestUser {
            id: 0,
            email: "destroy@example.com".to_string(),
            name: "To Destroy".to_string(),
            age: 25,
            active: true,
        };
        let saved_user = user.save().await.expect("Failed to save user");

        let deleted = TestUser::destroy(saved_user.id)
            .await
            .expect("Failed to destroy");
        assert_eq!(deleted, 1);

        let found = TestUser::find(saved_user.id).await.expect("Find failed");
        assert!(found.is_none());
        println!("   OK Destroy by ID");
    }
    println!();

    // =========================================================================
}
