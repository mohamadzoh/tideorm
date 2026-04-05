use super::*;

pub(super) async fn run() {
    // =========================================================================
    // JSON TESTS (SQLite JSON1 Extension)
    // =========================================================================
    println!(" Testing: JSON Operations (JSON1 Extension)");
    {
        let _ = Database::execute("DELETE FROM test_products").await;

        // Create products with JSON metadata
        let product = TestProduct {
            id: 0,
            name: "Laptop".to_string(),
            category: "Electronics".to_string(),
            price: 999,
            metadata: Some(serde_json::json!({
                "brand": "TechCorp",
                "features": ["fast", "lightweight"]
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

        // Test that JSON was stored
        let products = TestProduct::query()
            .where_eq("category", "Electronics")
            .get()
            .await
            .expect("Query failed");
        assert_eq!(products.len(), 2);
        assert!(products[0].metadata.is_some());
        println!("   ✓ JSON storage works");

        // Note: Full JSON query operators depend on JSON1 extension
        println!("   ℹ JSON query operators require JSON1 extension");
    }
    println!();
}
