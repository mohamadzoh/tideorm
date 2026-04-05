use super::*;

pub(super) async fn run() {
    // =========================================================================
    // JSON TESTS (MySQL JSON Type)
    // =========================================================================
    println!(" Testing: JSON Operations");
    {
        let _ = Database::execute("DELETE FROM `test_products`").await;

        // Create products with JSON metadata
        let product = TestProduct {
            id: 0,
            name: "Laptop".to_string(),
            category: "Electronics".to_string(),
            price: 999,
            metadata: Some(serde_json::json!({
                "brand": "TechCorp",
                "features": ["fast", "lightweight"],
                "specs": {
                    "ram": 16,
                    "storage": 512
                }
            })),
        };
        product.save().await.expect("Failed to save product");

        let product2 = TestProduct {
            id: 0,
            name: "Phone".to_string(),
            category: "Electronics".to_string(),
            price: 699,
            metadata: Some(serde_json::json!({
                "brand": "MobileCo",
                "features": ["compact", "durable"]
            })),
        };
        product2.save().await.expect("Failed to save product");

        // Test that JSON was stored and retrieved correctly
        let products = TestProduct::query()
            .where_eq("category", "Electronics")
            .get()
            .await
            .expect("Query failed");
        assert_eq!(products.len(), 2);

        let laptop = &products.iter().find(|p| p.name == "Laptop").unwrap();
        let metadata = laptop.metadata.as_ref().unwrap();
        assert_eq!(metadata["brand"], "TechCorp");
        println!("   ✓ JSON storage and retrieval works");

        // MySQL native JSON query (using raw SQL for now)
        // MySQL supports JSON_EXTRACT and ->> operators
        let result = Database::raw_json(
            "SELECT COUNT(*) as cnt FROM `test_products` WHERE JSON_EXTRACT(metadata, '$.brand') = 'TechCorp'",
        ).await;
        assert!(result.is_ok(), "JSON query should work");
        println!("   ✓ MySQL JSON_EXTRACT works");
    }
    println!();

    // =========================================================================
    // FIRST AND FIRST_OR_FAIL TESTS
    // =========================================================================
    println!("🎯 Testing: First Methods");
    {
        let _ = Database::execute("DELETE FROM `test_users`").await;

        TestUser {
            id: 0,
            email: "first@example.com".to_string(),
            name: "First".to_string(),
            age: 25,
            active: true,
        }
        .save()
        .await
        .unwrap();

        let first = TestUser::query().first().await.expect("First failed");
        assert!(first.is_some());
        println!("   ✓ first works");

        let first_or_fail = TestUser::query()
            .where_eq("email", "first@example.com")
            .first_or_fail()
            .await;
        assert!(first_or_fail.is_ok());
        println!("   ✓ first_or_fail works for existing");

        let not_found = TestUser::query()
            .where_eq("email", "nonexistent@example.com")
            .first_or_fail()
            .await;
        assert!(not_found.is_err());
        println!("   ✓ first_or_fail returns error for missing");
    }
    println!();
}
