use super::*;

pub(super) async fn run() {
    // =========================================================================
    // CRUD TESTS
    // =========================================================================
    println!("📝 Testing: CRUD Operations");

    // Create and Find
    {
        let user = TestUser {
            id: 0,
            email: "test@example.com".to_string(),
            name: "Test User".to_string(),
            age: 25,
            active: true,
        };

        let saved_user = user.save().await.expect("Failed to save user");

        let _ = assert_profiled_operation("TestUser::query().get()", TestUser::query().get()).await;
        assert!(saved_user.id > 0, "User ID should be auto-generated");
        println!("   ✓ Create works (id: {})", saved_user.id);

        let found_user = TestUser::find(saved_user.id).await.expect("Find failed");
        assert!(found_user.is_some(), "User should be found");
        assert_eq!(found_user.unwrap().email, "test@example.com");
        println!("   ✓ Find by ID works");
    }

    // Update
    {
        let user = TestUser {
            id: 0,
            email: "update@example.com".to_string(),
            name: "Original Name".to_string(),
            age: 30,
            active: true,
        };
        let mut saved_user = user.save().await.expect("Failed to save");

        saved_user.name = "Updated Name".to_string();
        saved_user.age = 31;
        let updated = saved_user.update().await.expect("Update failed");

        assert_eq!(updated.name, "Updated Name");
        assert_eq!(updated.age, 31);
        println!("   ✓ Update works");
    }

    // Delete
    {
        let user = TestUser {
            id: 0,
            email: "delete@example.com".to_string(),
            name: "To Delete".to_string(),
            age: 40,
            active: true,
        };
        let saved_user = user.save().await.expect("Failed to save");
        let user_id = saved_user.id;

        saved_user.delete().await.expect("Delete failed");

        let found = TestUser::find(user_id).await.expect("Find failed");
        assert!(found.is_none(), "User should be deleted");
        println!("   ✓ Delete works");
    }
    println!();
}
