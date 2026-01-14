//! Comprehensive WHERE and OR Query Demo
//!
//! This example demonstrates all WHERE and OR query capabilities in TideORM.
//! It creates test data, runs various query scenarios, and verifies the results.
//!
//! Run with: cargo run --example where_and_or_demo

use tideorm::prelude::*;

// =============================================================================
// TEST MODEL: Product with various field types for comprehensive testing
// =============================================================================

#[tideorm::model]
#[tide(table = "demo_products")]
pub struct Product {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub category: String,
    pub brand: String,
    pub price: f64,
    pub stock: i32,
    pub rating: f64,
    pub active: bool,
    pub featured: bool,
    #[tide(nullable)]
    pub description: Option<String>,
    #[tide(nullable)]
    pub discount_percent: Option<i32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// =============================================================================
// HELPER MACROS AND FUNCTIONS
// =============================================================================

macro_rules! test_section {
    ($name:expr) => {
        println!("\n{}", "=".repeat(70));
        println!(" {}", $name);
        println!("{}", "=".repeat(70));
    };
}

macro_rules! test_case {
    ($name:expr) => {
        println!("\n📋 TEST: {}", $name);
        println!("{}", "-".repeat(50));
    };
}

macro_rules! assert_test {
    ($condition:expr, $msg:expr) => {
        if $condition {
            println!("   ✅ PASS: {}", $msg);
        } else {
            println!("   ❌ FAIL: {}", $msg);
            panic!("Test failed: {}", $msg);
        }
    };
}

fn print_products(products: &[Product], limit: usize) {
    for (i, p) in products.iter().take(limit).enumerate() {
        println!(
            "   {}. {} | {} | {} | ${:.2} | stock:{} | rating:{:.1} | active:{} | featured:{}",
            i + 1, p.name, p.category, p.brand, p.price, p.stock, p.rating, p.active, p.featured
        );
    }
    if products.len() > limit {
        println!("   ... and {} more", products.len() - limit);
    }
}

// =============================================================================
// TEST DATA SETUP
// =============================================================================

async fn setup_test_data() -> tideorm::Result<()> {
    // Drop and recreate table
    let _ = Database::execute("DROP TABLE IF EXISTS demo_products CASCADE").await;
    
    Database::execute(
        r#"
        CREATE TABLE demo_products (
            id BIGSERIAL PRIMARY KEY,
            name VARCHAR(255) NOT NULL,
            category VARCHAR(100) NOT NULL,
            brand VARCHAR(100) NOT NULL,
            price DOUBLE PRECISION NOT NULL,
            stock INTEGER NOT NULL,
            rating DOUBLE PRECISION NOT NULL,
            active BOOLEAN NOT NULL DEFAULT true,
            featured BOOLEAN NOT NULL DEFAULT false,
            description TEXT,
            discount_percent INTEGER,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .await?;
    
    // Insert comprehensive test data
    let products = vec![
        // Electronics - Various brands and prices
        ("iPhone 15 Pro", "Electronics", "Apple", 1199.99, 50, 4.8, true, true, Some("Latest iPhone"), Some(10)),
        ("iPhone 14", "Electronics", "Apple", 799.99, 100, 4.6, true, false, Some("Previous gen iPhone"), Some(15)),
        ("MacBook Pro 16", "Electronics", "Apple", 2499.99, 25, 4.9, true, true, Some("Professional laptop"), None),
        ("Galaxy S24", "Electronics", "Samsung", 999.99, 75, 4.7, true, true, Some("Flagship Android"), Some(5)),
        ("Galaxy A54", "Electronics", "Samsung", 449.99, 200, 4.3, true, false, Some("Mid-range phone"), Some(20)),
        ("Pixel 8 Pro", "Electronics", "Google", 899.99, 40, 4.5, true, false, Some("Google flagship"), None),
        ("ThinkPad X1", "Electronics", "Lenovo", 1599.99, 30, 4.4, true, false, Some("Business laptop"), Some(12)),
        ("Surface Pro 9", "Electronics", "Microsoft", 1299.99, 45, 4.3, false, false, Some("2-in-1 tablet"), Some(8)),
        
        // Clothing - Various brands
        ("Classic T-Shirt", "Clothing", "Nike", 29.99, 500, 4.2, true, false, Some("Cotton tee"), None),
        ("Running Shoes", "Clothing", "Nike", 129.99, 150, 4.6, true, true, Some("Performance shoes"), Some(25)),
        ("Hoodie", "Clothing", "Adidas", 79.99, 200, 4.4, true, false, Some("Warm hoodie"), Some(15)),
        ("Yoga Pants", "Clothing", "Adidas", 59.99, 300, 4.5, true, true, None, Some(10)),
        ("Jeans", "Clothing", "Levi's", 89.99, 250, 4.3, true, false, Some("Classic fit"), None),
        ("Winter Jacket", "Clothing", "North Face", 249.99, 80, 4.7, false, false, Some("Insulated jacket"), Some(30)),
        
        // Home & Kitchen
        ("Coffee Maker", "Home", "Breville", 199.99, 60, 4.6, true, true, Some("Espresso machine"), None),
        ("Blender", "Home", "Vitamix", 449.99, 40, 4.8, true, false, Some("Professional blender"), Some(5)),
        ("Air Fryer", "Home", "Ninja", 149.99, 100, 4.5, true, true, Some("Healthy cooking"), Some(20)),
        ("Vacuum", "Home", "Dyson", 599.99, 35, 4.7, true, false, Some("Cordless vacuum"), None),
        ("Toaster", "Home", "KitchenAid", 89.99, 150, 4.2, true, false, None, None),
        ("Instant Pot", "Home", "Instant Pot", 99.99, 200, 4.6, false, false, Some("Pressure cooker"), Some(15)),
        
        // Books - Various
        ("Rust Programming", "Books", "O'Reilly", 49.99, 500, 4.9, true, true, Some("Learn Rust"), None),
        ("Python Basics", "Books", "O'Reilly", 39.99, 400, 4.7, true, false, Some("Python intro"), Some(10)),
        ("Web Development", "Books", "Manning", 54.99, 300, 4.5, true, false, None, None),
        ("Machine Learning", "Books", "Packt", 44.99, 250, 4.3, true, true, Some("ML fundamentals"), Some(5)),
        ("Data Science", "Books", "O'Reilly", 59.99, 200, 4.6, false, false, Some("Data analysis"), None),
        
        // Sports & Outdoors
        ("Tennis Racket", "Sports", "Wilson", 179.99, 50, 4.4, true, false, Some("Pro racket"), None),
        ("Basketball", "Sports", "Spalding", 29.99, 300, 4.3, true, false, None, None),
        ("Yoga Mat", "Sports", "Manduka", 79.99, 200, 4.7, true, true, Some("Premium mat"), Some(10)),
        ("Dumbbells Set", "Sports", "Bowflex", 349.99, 40, 4.5, true, false, Some("Adjustable weights"), None),
        ("Camping Tent", "Sports", "Coleman", 199.99, 60, 4.2, false, false, Some("4-person tent"), Some(25)),
    ];
    
    for (name, category, brand, price, stock, rating, active, featured, description, discount) in products {
        let escaped_name = name.replace("'", "''");
        let escaped_brand = brand.replace("'", "''");
        let escaped_desc = description.map(|d| d.replace("'", "''"));
        
        Database::execute(&format!(
            r#"INSERT INTO demo_products (name, category, brand, price, stock, rating, active, featured, description, discount_percent)
               VALUES ('{}', '{}', '{}', {}, {}, {}, {}, {}, {}, {})"#,
            escaped_name,
            category,
            escaped_brand,
            price,
            stock,
            rating,
            active,
            featured,
            escaped_desc.map(|d| format!("'{}'", d)).unwrap_or("NULL".to_string()),
            discount.map(|d| d.to_string()).unwrap_or("NULL".to_string())
        ))
        .await?;
    }
    
    println!("✅ Created 30 test products across 5 categories");
    Ok(())
}

// =============================================================================
// MAIN TEST RUNNER
// =============================================================================

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("POSTGRESQL_DATABASE_URL")
        .expect("POSTGRESQL_DATABASE_URL must be set");
    
    // Initialize TideORM
    TideConfig::init()
        .database_type(DatabaseType::Postgres)
        .database(&db_url)
        .max_connections(5)
        .connect()
        .await?;
    
    println!("\n🚀 TideORM WHERE & OR Query Demo");
    println!("================================\n");
    
    // Setup test data
    setup_test_data().await?;
    
    // Run all test sections
    test_basic_where_conditions().await?;
    test_comparison_operators().await?;
    test_string_matching().await?;
    test_in_and_not_in().await?;
    test_null_conditions().await?;
    test_between_conditions().await?;
    test_simple_or_conditions().await?;
    test_fluent_or_api().await?;
    test_complex_combined_queries().await?;
    test_aggregation_with_conditions().await?;
    test_ordering_and_pagination().await?;
    test_advanced_business_scenarios().await?;
    test_nested_or_groups().await?;
    test_edge_cases_and_special_patterns().await?;
    test_reporting_analytics_queries().await?;
    
    // Cleanup
    let _ = Database::execute("DROP TABLE IF EXISTS demo_products CASCADE").await;
    
    println!("\n{}", "=".repeat(70));
    println!(" ✅ ALL TESTS COMPLETED SUCCESSFULLY!");
    println!("{}\n", "=".repeat(70));
    
    Ok(())
}

// =============================================================================
// TEST SECTIONS
// =============================================================================

async fn test_basic_where_conditions() -> tideorm::Result<()> {
    test_section!("1. BASIC WHERE CONDITIONS");
    
    // Test: where_eq (equals)
    test_case!("where_eq - Find all Apple products");
    let results = Product::query()
        .where_eq("brand", "Apple")
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(results.len() == 3, "Should find 3 Apple products");
    assert_test!(results.iter().all(|p| p.brand == "Apple"), "All should be Apple brand");
    
    // Test: where_not (not equals)
    test_case!("where_not - Find all non-Electronics products");
    let results = Product::query()
        .where_not("category", "Electronics")
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(results.iter().all(|p| p.category != "Electronics"), "None should be Electronics");
    assert_test!(results.len() == 22, "Should find 22 non-Electronics products");
    
    // Test: Multiple where_eq (AND logic)
    test_case!("Multiple where_eq - Active AND Featured products");
    let results = Product::query()
        .where_eq("active", true)
        .where_eq("featured", true)
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(results.iter().all(|p| p.active && p.featured), "All should be active AND featured");
    assert_test!(results.len() == 10, "Should find 10 active featured products");
    
    Ok(())
}

async fn test_comparison_operators() -> tideorm::Result<()> {
    test_section!("2. COMPARISON OPERATORS");
    
    // Test: where_gt (greater than)
    test_case!("where_gt - Products with price > $500");
    let results = Product::query()
        .where_gt("price", 500.0)
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(results.iter().all(|p| p.price > 500.0), "All prices should be > $500");
    let expected = 8; // Count products > $500
    assert_test!(results.len() == expected, &format!("Should find {} expensive products", expected));
    
    // Test: where_gte (greater than or equal)
    test_case!("where_gte - Products with rating >= 4.6");
    let results = Product::query()
        .where_gte("rating", 4.6)
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(results.iter().all(|p| p.rating >= 4.6), "All ratings should be >= 4.6");
    
    // Test: where_lt (less than)
    test_case!("where_lt - Products with stock < 50");
    let results = Product::query()
        .where_lt("stock", 50)
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(results.iter().all(|p| p.stock < 50), "All stock should be < 50");
    
    // Test: where_lte (less than or equal)
    test_case!("where_lte - Products with price <= $50");
    let results = Product::query()
        .where_lte("price", 50.0)
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(results.iter().all(|p| p.price <= 50.0), "All prices should be <= $50");
    
    // Test: Combined comparisons
    test_case!("Combined - Price between $100-$500 AND rating > 4.4");
    let results = Product::query()
        .where_gt("price", 100.0)
        .where_lt("price", 500.0)
        .where_gt("rating", 4.4)
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.price > 100.0 && p.price < 500.0 && p.rating > 4.4),
        "All should match combined criteria"
    );
    
    Ok(())
}

async fn test_string_matching() -> tideorm::Result<()> {
    test_section!("3. STRING MATCHING (LIKE)");
    
    // Test: where_like - Starts with pattern
    test_case!("where_like - Products starting with 'iPhone'");
    let results = Product::query()
        .where_like("name", "iPhone%")
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(results.iter().all(|p| p.name.starts_with("iPhone")), "All should start with iPhone");
    assert_test!(results.len() == 2, "Should find 2 iPhone products");
    
    // Test: where_like - Contains pattern
    test_case!("where_like - Products containing 'Pro'");
    let results = Product::query()
        .where_like("name", "%Pro%")
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(results.iter().all(|p| p.name.contains("Pro")), "All should contain 'Pro'");
    
    // Test: where_like - Ends with pattern
    test_case!("where_like - Descriptions ending with 'laptop'");
    let results = Product::query()
        .where_like("description", "%laptop")
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.description.as_ref().map(|d| d.ends_with("laptop")).unwrap_or(false)),
        "All descriptions should end with 'laptop'"
    );
    
    // Test: where_not_like
    test_case!("where_not_like - Products NOT containing 'Phone'");
    let results = Product::query()
        .where_not_like("name", "%Phone%")
        .where_eq("category", "Electronics")
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| !p.name.to_lowercase().contains("phone")),
        "None should contain 'Phone'"
    );
    
    Ok(())
}

async fn test_in_and_not_in() -> tideorm::Result<()> {
    test_section!("4. IN AND NOT IN");
    
    // Test: where_in - Multiple categories
    test_case!("where_in - Products in Electronics or Books");
    let results = Product::query()
        .where_in("category", vec!["Electronics", "Books"])
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.category == "Electronics" || p.category == "Books"),
        "All should be Electronics or Books"
    );
    assert_test!(results.len() == 13, "Should find 13 products (8 Electronics + 5 Books)");
    
    // Test: where_in - Multiple brands
    test_case!("where_in - Products from Apple, Samsung, or Google");
    let results = Product::query()
        .where_in("brand", vec!["Apple", "Samsung", "Google"])
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| ["Apple", "Samsung", "Google"].contains(&p.brand.as_str())),
        "All should be from specified brands"
    );
    
    // Test: where_not_in - Exclude categories
    test_case!("where_not_in - Products NOT in Electronics or Sports");
    let results = Product::query()
        .where_not_in("category", vec!["Electronics", "Sports"])
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.category != "Electronics" && p.category != "Sports"),
        "None should be Electronics or Sports"
    );
    
    // Test: where_in with numeric values
    test_case!("where_in - Products with specific discount percentages");
    let results = Product::query()
        .where_in("discount_percent", vec![10, 15, 20])
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.discount_percent.map(|d| [10, 15, 20].contains(&d)).unwrap_or(false)),
        "All should have 10%, 15%, or 20% discount"
    );
    
    Ok(())
}

async fn test_null_conditions() -> tideorm::Result<()> {
    test_section!("5. NULL CONDITIONS");
    
    // Test: where_null
    test_case!("where_null - Products with no description");
    let results = Product::query()
        .where_null("description")
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(results.iter().all(|p| p.description.is_none()), "All descriptions should be null");
    
    // Test: where_not_null
    test_case!("where_not_null - Products with description");
    let results = Product::query()
        .where_not_null("description")
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(results.iter().all(|p| p.description.is_some()), "All should have descriptions");
    
    // Test: where_null on discount
    test_case!("where_null - Products with no discount");
    let results = Product::query()
        .where_null("discount_percent")
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(results.iter().all(|p| p.discount_percent.is_none()), "All discounts should be null");
    
    // Test: Combine null with other conditions
    test_case!("Combined - Active products with discount");
    let results = Product::query()
        .where_eq("active", true)
        .where_not_null("discount_percent")
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.active && p.discount_percent.is_some()),
        "All should be active with discount"
    );
    
    Ok(())
}

async fn test_between_conditions() -> tideorm::Result<()> {
    test_section!("6. BETWEEN CONDITIONS");
    
    // Test: where_between - Price range
    test_case!("where_between - Products priced $50 to $100");
    let results = Product::query()
        .where_between("price", 50.0, 100.0)
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.price >= 50.0 && p.price <= 100.0),
        "All prices should be between $50 and $100"
    );
    
    // Test: where_between - Stock range
    test_case!("where_between - Products with stock 100 to 300");
    let results = Product::query()
        .where_between("stock", 100, 300)
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.stock >= 100 && p.stock <= 300),
        "All stock should be between 100 and 300"
    );
    
    // Test: where_between - Rating range
    test_case!("where_between - Products with rating 4.5 to 4.7");
    let results = Product::query()
        .where_between("rating", 4.5, 4.7)
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.rating >= 4.5 && p.rating <= 4.7),
        "All ratings should be between 4.5 and 4.7"
    );
    
    // Test: Combine between with other conditions
    test_case!("Combined - Electronics priced $500-$1500");
    let results = Product::query()
        .where_eq("category", "Electronics")
        .where_between("price", 500.0, 1500.0)
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.category == "Electronics" && p.price >= 500.0 && p.price <= 1500.0),
        "All should be Electronics in price range"
    );
    
    Ok(())
}

async fn test_simple_or_conditions() -> tideorm::Result<()> {
    test_section!("7. SIMPLE OR CONDITIONS");
    
    // Test: or_where_eq
    test_case!("or_where_eq - Apple OR Samsung products");
    let results = Product::query()
        .where_eq("brand", "Apple")
        .or_where_eq("brand", "Samsung")
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| p.brand == "Apple" || p.brand == "Samsung"),
        "All should be Apple or Samsung"
    );
    println!("   Found {} Apple/Samsung products", results.len());
    
    // Test: or_where_gt
    test_case!("or_where_gt - Cheap (<$50) OR expensive (>$1000)");
    let results = Product::query()
        .where_lt("price", 50.0)
        .or_where_gt("price", 1000.0)
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.price < 50.0 || p.price > 1000.0),
        "All should be cheap or expensive"
    );
    
    // Test: or_where_like
    test_case!("or_where_like - Name contains 'Pro' OR 'Premium'");
    let results = Product::query()
        .where_like("name", "%Pro%")
        .or_where_like("description", "%Premium%")
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| 
            p.name.contains("Pro") || 
            p.description.as_ref().map(|d| d.contains("Premium")).unwrap_or(false)
        ),
        "All should contain Pro or Premium"
    );
    
    // Test: or_where_in
    test_case!("or_where_in - Nike/Adidas clothing OR O'Reilly books");
    let results = Product::query()
        .where_eq("category", "Clothing")
        .where_in("brand", vec!["Nike", "Adidas"])
        .or_where_eq("brand", "O'Reilly")
        .get()
        .await?;
    print_products(&results, 10);
    
    // Test: or_where_null
    test_case!("or_where_null - Featured OR no description");
    let results = Product::query()
        .where_eq("featured", true)
        .or_where_null("description")
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.featured || p.description.is_none()),
        "All should be featured or have no description"
    );
    
    // Test: or_where_between
    test_case!("or_where_between - Low stock (<50) OR high stock (>300)");
    let results = Product::query()
        .where_lt("stock", 50)
        .or_where_gt("stock", 300)
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.stock < 50 || p.stock > 300),
        "All should have low or high stock"
    );
    
    Ok(())
}

async fn test_fluent_or_api() -> tideorm::Result<()> {
    test_section!("8. FLUENT OR API (begin_or / end_or)");
    
    // Test: Simple fluent OR
    test_case!("begin_or - Electronics OR Home category");
    let results = Product::query()
        .begin_or()
            .or_where_eq("category", "Electronics")
            .or_where_eq("category", "Home")
        .end_or()
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.category == "Electronics" || p.category == "Home"),
        "All should be Electronics or Home"
    );
    assert_test!(results.len() == 14, "Should find 14 products (8 Electronics + 6 Home)");
    
    // Test: OR with AND conditions in each branch
    test_case!("Fluent OR with AND - (Apple AND active) OR (Samsung AND featured)");
    let results = Product::query()
        .begin_or()
            .or_where_eq("brand", "Apple").and_where_eq("active", true)
            .or_where_eq("brand", "Samsung").and_where_eq("featured", true)
        .end_or()
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| 
            (p.brand == "Apple" && p.active) || 
            (p.brand == "Samsung" && p.featured)
        ),
        "All should match (Apple AND active) OR (Samsung AND featured)"
    );
    
    // Test: Complex business logic
    test_case!("Business logic - Premium Electronics OR Discounted Clothing");
    let results = Product::query()
        .begin_or()
            .or_where_eq("category", "Electronics").and_where_gt("price", 1000.0)
            .or_where_eq("category", "Clothing").and_where_not_null("discount_percent")
        .end_or()
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| 
            (p.category == "Electronics" && p.price > 1000.0) || 
            (p.category == "Clothing" && p.discount_percent.is_some())
        ),
        "All should be premium electronics or discounted clothing"
    );
    
    // Test: Multiple AND per branch
    test_case!("Multiple AND - (Electronics AND active AND rating>4.5) OR (Books AND featured)");
    let results = Product::query()
        .begin_or()
            .or_where_eq("category", "Electronics")
                .and_where_eq("active", true)
                .and_where_gt("rating", 4.5)
            .or_where_eq("category", "Books")
                .and_where_eq("featured", true)
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| 
            (p.category == "Electronics" && p.active && p.rating > 4.5) || 
            (p.category == "Books" && p.featured)
        ),
        "All should match complex criteria"
    );
    
    // Test: Outer WHERE with OR group
    test_case!("Outer WHERE + OR - Active AND (Electronics OR Books)");
    let results = Product::query()
        .where_eq("active", true)
        .begin_or()
            .or_where_eq("category", "Electronics")
            .or_where_eq("category", "Books")
        .end_or()
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.active && (p.category == "Electronics" || p.category == "Books")),
        "All should be active AND (Electronics OR Books)"
    );
    
    // Test: OR with LIKE
    test_case!("OR with LIKE - Name starts with 'iPhone' OR contains 'Galaxy'");
    let results = Product::query()
        .begin_or()
            .or_where_like("name", "iPhone%")
            .or_where_like("name", "%Galaxy%")
        .end_or()
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.name.starts_with("iPhone") || p.name.contains("Galaxy")),
        "All should match name patterns"
    );
    assert_test!(results.len() == 4, "Should find 4 products (2 iPhone + 2 Galaxy)");
    
    // Test: OR with IN
    test_case!("OR with IN - Apple products OR products with 20-25% discount");
    let results = Product::query()
        .begin_or()
            .or_where_eq("brand", "Apple")
            .or_where_in("discount_percent", vec![20, 25])
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| 
            p.brand == "Apple" || 
            p.discount_percent.map(|d| d == 20 || d == 25).unwrap_or(false)
        ),
        "All should be Apple or have 20-25% discount"
    );
    
    // Test: OR with BETWEEN
    test_case!("OR with BETWEEN - Cheap ($20-50) OR Premium ($1000-2000)");
    let results = Product::query()
        .begin_or()
            .or_where_between("price", 20.0, 50.0)
            .or_where_between("price", 1000.0, 2000.0)
        .end_or()
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| 
            (p.price >= 20.0 && p.price <= 50.0) || 
            (p.price >= 1000.0 && p.price <= 2000.0)
        ),
        "All should be in cheap or premium price range"
    );
    
    Ok(())
}

async fn test_complex_combined_queries() -> tideorm::Result<()> {
    test_section!("9. COMPLEX COMBINED QUERIES");
    
    // Test: Real-world e-commerce filter
    test_case!("E-commerce filter - Active electronics under $1000 with good rating");
    let results = Product::query()
        .where_eq("category", "Electronics")
        .where_eq("active", true)
        .where_lt("price", 1000.0)
        .where_gte("rating", 4.3)
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| 
            p.category == "Electronics" && 
            p.active && 
            p.price < 1000.0 && 
            p.rating >= 4.3
        ),
        "All should match e-commerce filter"
    );
    
    // Test: Inventory management query
    test_case!("Inventory - Low stock (<50) active products needing reorder");
    let results = Product::query()
        .where_eq("active", true)
        .where_lt("stock", 50)
        .where_gte("rating", 4.0)
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| p.active && p.stock < 50 && p.rating >= 4.0),
        "All should need reorder"
    );
    
    // Test: Marketing query - Featured or on sale
    test_case!("Marketing - Featured OR discounted active products");
    let results = Product::query()
        .where_eq("active", true)
        .begin_or()
            .or_where_eq("featured", true)
            .or_where_not_null("discount_percent")
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| p.active && (p.featured || p.discount_percent.is_some())),
        "All should be featured or discounted"
    );
    
    // Test: Complex price/brand filter
    test_case!("Complex - (Apple >$500) OR (Samsung active) OR (Nike/Adidas)");
    let results = Product::query()
        .begin_or()
            .or_where_eq("brand", "Apple").and_where_gt("price", 500.0)
            .or_where_eq("brand", "Samsung").and_where_eq("active", true)
            .or_where_in("brand", vec!["Nike", "Adidas"])
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| 
            (p.brand == "Apple" && p.price > 500.0) || 
            (p.brand == "Samsung" && p.active) ||
            (p.brand == "Nike" || p.brand == "Adidas")
        ),
        "All should match complex brand/price criteria"
    );
    
    // Test: Search with multiple criteria
    test_case!("Search - Name contains 'Pro' AND (Electronics OR Home) AND rating >= 4.5");
    let results = Product::query()
        .where_like("name", "%Pro%")
        .where_gte("rating", 4.5)
        .begin_or()
            .or_where_eq("category", "Electronics")
            .or_where_eq("category", "Home")
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| 
            p.name.contains("Pro") && 
            p.rating >= 4.5 && 
            (p.category == "Electronics" || p.category == "Home")
        ),
        "All should match search criteria"
    );
    
    Ok(())
}

async fn test_aggregation_with_conditions() -> tideorm::Result<()> {
    test_section!("10. AGGREGATION WITH CONDITIONS");
    
    // Test: count() with simple condition
    test_case!("count() - Number of active products");
    let count = Product::query()
        .where_eq("active", true)
        .count()
        .await?;
    println!("   Active products: {}", count);
    assert_test!(count > 20, "Should have many active products");
    
    // Test: count() with OR conditions
    test_case!("count() - Electronics OR Books count");
    let count = Product::query()
        .begin_or()
            .or_where_eq("category", "Electronics")
            .or_where_eq("category", "Books")
        .end_or()
        .count()
        .await?;
    println!("   Electronics or Books: {}", count);
    assert_test!(count == 13, "Should have 13 products");
    
    // Test: exists() with condition
    test_case!("exists() - Check if any Apple products exist");
    let exists = Product::query()
        .where_eq("brand", "Apple")
        .exists()
        .await?;
    println!("   Apple products exist: {}", exists);
    assert_test!(exists, "Apple products should exist");
    
    // Test: exists() with non-matching condition
    test_case!("exists() - Check if any 'NonExistent' brand exists");
    let exists = Product::query()
        .where_eq("brand", "NonExistent")
        .exists()
        .await?;
    println!("   NonExistent brand: {}", exists);
    assert_test!(!exists, "NonExistent brand should not exist");
    
    // Test: first() with conditions
    test_case!("first() - First Apple product");
    let first = Product::query()
        .where_eq("brand", "Apple")
        .order_by("price", Order::Asc)
        .first()
        .await?;
    if let Some(p) = &first {
        println!("   First Apple (cheapest): {} at ${:.2}", p.name, p.price);
    }
    assert_test!(first.is_some(), "Should find an Apple product");
    assert_test!(first.as_ref().map(|p| p.brand == "Apple").unwrap_or(false), "Should be Apple brand");
    
    // Test: first() with OR conditions
    test_case!("first() - First expensive (>$1000) product");
    let first = Product::query()
        .where_gt("price", 1000.0)
        .order_by("price", Order::Desc)
        .first()
        .await?;
    if let Some(p) = &first {
        println!("   Most expensive >$1000: {} at ${:.2}", p.name, p.price);
    }
    assert_test!(first.is_some(), "Should find expensive product");
    
    Ok(())
}

async fn test_ordering_and_pagination() -> tideorm::Result<()> {
    test_section!("11. ORDERING AND PAGINATION WITH CONDITIONS");
    
    // Test: order_by ASC
    test_case!("order_by ASC - Products ordered by price (cheapest first)");
    let results = Product::query()
        .where_eq("category", "Electronics")
        .order_by("price", Order::Asc)
        .limit(5)
        .get()
        .await?;
    print_products(&results, 5);
    for i in 1..results.len() {
        assert_test!(results[i-1].price <= results[i].price, "Should be in ascending price order");
    }
    
    // Test: order_by DESC
    test_case!("order_by DESC - Products ordered by rating (best first)");
    let results = Product::query()
        .where_eq("active", true)
        .order_by("rating", Order::Desc)
        .limit(5)
        .get()
        .await?;
    print_products(&results, 5);
    for i in 1..results.len() {
        assert_test!(results[i-1].rating >= results[i].rating, "Should be in descending rating order");
    }
    
    // Test: Multiple order_by
    test_case!("Multiple order_by - By category ASC, then price DESC");
    let results = Product::query()
        .where_eq("active", true)
        .order_by("category", Order::Asc)
        .order_by("price", Order::Desc)
        .limit(10)
        .get()
        .await?;
    print_products(&results, 10);
    
    // Test: limit
    test_case!("limit - Get top 3 products");
    let results = Product::query()
        .order_by("rating", Order::Desc)
        .limit(3)
        .get()
        .await?;
    print_products(&results, 3);
    assert_test!(results.len() == 3, "Should return exactly 3 products");
    
    // Test: offset (pagination)
    test_case!("offset - Page 2 of products (skip first 5)");
    let page1 = Product::query()
        .where_eq("active", true)
        .order_by("name", Order::Asc)
        .limit(5)
        .get()
        .await?;
    let page2 = Product::query()
        .where_eq("active", true)
        .order_by("name", Order::Asc)
        .limit(5)
        .offset(5)
        .get()
        .await?;
    println!("   Page 1:");
    print_products(&page1, 5);
    println!("   Page 2:");
    print_products(&page2, 5);
    assert_test!(page1.len() == 5, "Page 1 should have 5 products");
    assert_test!(page2.len() == 5, "Page 2 should have 5 products");
    assert_test!(
        page1.iter().all(|p1| page2.iter().all(|p2| p1.id != p2.id)),
        "Pages should not overlap"
    );
    
    // Test: Order + Limit with OR conditions
    test_case!("Order + Limit + OR - Top 5 cheapest Electronics or Home");
    let results = Product::query()
        .begin_or()
            .or_where_eq("category", "Electronics")
            .or_where_eq("category", "Home")
        .end_or()
        .order_by("price", Order::Asc)
        .limit(5)
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(results.len() == 5, "Should return 5 products");
    assert_test!(
        results.iter().all(|p| p.category == "Electronics" || p.category == "Home"),
        "All should be Electronics or Home"
    );
    for i in 1..results.len() {
        assert_test!(results[i-1].price <= results[i].price, "Should be price ordered");
    }
    
    Ok(())
}

// =============================================================================
// 12. ADVANCED BUSINESS SCENARIOS
// =============================================================================

async fn test_advanced_business_scenarios() -> tideorm::Result<()> {
    test_section!("12. ADVANCED BUSINESS SCENARIOS");
    
    // Scenario 1: Flash Sale Eligibility
    // Products that qualify for flash sale: high stock + good rating + not already discounted
    test_case!("Flash Sale - High stock (>100), rating>=4.3, no existing discount");
    let results = Product::query()
        .where_eq("active", true)
        .where_gt("stock", 100)
        .where_gte("rating", 4.3)
        .where_null("discount_percent")
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| p.active && p.stock > 100 && p.rating >= 4.3 && p.discount_percent.is_none()),
        "All should qualify for flash sale"
    );
    println!("   {} products qualify for flash sale", results.len());
    
    // Scenario 2: Clearance Candidates
    // Low stock OR inactive OR low rating - potential clearance items
    test_case!("Clearance Candidates - Low stock (<50) OR inactive OR low rating (<4.3)");
    let results = Product::query()
        .begin_or()
            .or_where_lt("stock", 50)
            .or_where_eq("active", false)
            .or_where_lt("rating", 4.3)
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| p.stock < 50 || !p.active || p.rating < 4.3),
        "All should be clearance candidates"
    );
    println!("   {} products are clearance candidates", results.len());
    
    // Scenario 3: Premium Bundle Candidates
    // (Electronics > $500 with rating >= 4.5) OR (Home > $200 with rating >= 4.6)
    test_case!("Premium Bundle - Expensive high-rated Electronics OR Home items");
    let results = Product::query()
        .where_eq("active", true)
        .begin_or()
            .or_where_eq("category", "Electronics")
                .and_where_gt("price", 500.0)
                .and_where_gte("rating", 4.5)
            .or_where_eq("category", "Home")
                .and_where_gt("price", 200.0)
                .and_where_gte("rating", 4.6)
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| 
            p.active && (
                (p.category == "Electronics" && p.price > 500.0 && p.rating >= 4.5) ||
                (p.category == "Home" && p.price > 200.0 && p.rating >= 4.6)
            )
        ),
        "All should qualify for premium bundle"
    );
    
    // Scenario 4: Cross-sell Recommendations
    // If customer bought Apple, recommend: other Apple products OR Samsung flagships OR premium accessories
    test_case!("Cross-sell - Apple products OR Samsung flagships (>$500) OR featured items");
    let results = Product::query()
        .where_eq("active", true)
        .begin_or()
            .or_where_eq("brand", "Apple")
            .or_where_eq("brand", "Samsung").and_where_gt("price", 500.0)
            .or_where_eq("featured", true).and_where_not("category", "Electronics")
        .end_or()
        .order_by("rating", Order::Desc)
        .limit(10)
        .get()
        .await?;
    print_products(&results, 10);
    println!("   Top 10 cross-sell recommendations");
    
    // Scenario 5: Supplier Reorder Alert
    // Active products with (low stock AND high rating) OR (very low stock regardless of rating)
    test_case!("Reorder Alert - (stock<50 AND rating>4.5) OR (stock<30)");
    let results = Product::query()
        .where_eq("active", true)
        .begin_or()
            .or_where_lt("stock", 50).and_where_gt("rating", 4.5)
            .or_where_lt("stock", 30)
        .end_or()
        .order_by("stock", Order::Asc)
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| 
            p.active && ((p.stock < 50 && p.rating > 4.5) || p.stock < 30)
        ),
        "All should need reordering"
    );
    println!("   {} products need reordering", results.len());
    
    // Scenario 6: Promotional Email Targeting
    // Users who might be interested in: discounted items in their favorite categories
    test_case!("Promo Targeting - Discounted Electronics/Clothing/Home with good ratings");
    let results = Product::query()
        .where_eq("active", true)
        .where_not_null("discount_percent")
        .where_gte("rating", 4.0)
        .begin_or()
            .or_where_eq("category", "Electronics")
            .or_where_eq("category", "Clothing")
            .or_where_eq("category", "Home")
        .end_or()
        .order_by("discount_percent", Order::Desc)
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| 
            p.active && 
            p.discount_percent.is_some() && 
            p.rating >= 4.0 &&
            ["Electronics", "Clothing", "Home"].contains(&p.category.as_str())
        ),
        "All should be promotional candidates"
    );
    
    // Scenario 7: Competitive Pricing Analysis
    // Find products that are either premium (>$1000) or budget (<$50) in tech categories
    test_case!("Price Segmentation - Premium (>$1000) OR Budget (<$50) in Electronics/Books");
    let results = Product::query()
        .begin_or()
            .or_where_eq("category", "Electronics")
            .or_where_eq("category", "Books")
        .end_or()
        .begin_or()
            .or_where_gt("price", 1000.0)
            .or_where_lt("price", 50.0)
        .end_or()
        .order_by("price", Order::Desc)
        .get()
        .await?;
    print_products(&results, 10);
    
    // Scenario 8: Quality Assurance Check
    // Find products with concerning patterns: low rating despite being featured, or high price with low rating
    test_case!("QA Check - (featured AND rating<4.5) OR (price>$500 AND rating<4.3)");
    let results = Product::query()
        .begin_or()
            .or_where_eq("featured", true).and_where_lt("rating", 4.5)
            .or_where_gt("price", 500.0).and_where_lt("rating", 4.3)
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    println!("   {} products need quality review", results.len());
    
    Ok(())
}

// =============================================================================
// 13. NESTED OR GROUPS
// =============================================================================

async fn test_nested_or_groups() -> tideorm::Result<()> {
    test_section!("13. NESTED OR GROUPS & COMPLEX LOGIC");
    
    // Test: Multiple sequential OR groups
    test_case!("Sequential OR groups - Category filter AND Brand filter");
    let results = Product::query()
        .where_eq("active", true)
        .begin_or()
            .or_where_eq("category", "Electronics")
            .or_where_eq("category", "Home")
        .end_or()
        .begin_or()
            .or_where_eq("brand", "Apple")
            .or_where_eq("brand", "Samsung")
            .or_where_eq("brand", "Breville")
            .or_where_eq("brand", "Dyson")
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| 
            p.active &&
            (p.category == "Electronics" || p.category == "Home") &&
            ["Apple", "Samsung", "Breville", "Dyson"].contains(&p.brand.as_str())
        ),
        "All should match both OR groups"
    );
    
    // Test: Triple OR group chain
    test_case!("Triple OR chain - Category AND Brand AND Price tier");
    let results = Product::query()
        .begin_or()
            .or_where_eq("category", "Electronics")
            .or_where_eq("category", "Clothing")
            .or_where_eq("category", "Home")
        .end_or()
        .begin_or()
            .or_where_eq("featured", true)
            .or_where_not_null("discount_percent")
        .end_or()
        .begin_or()
            .or_where_between("price", 50.0, 200.0)
            .or_where_gt("price", 500.0)
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    println!("   Products matching all three OR group criteria: {}", results.len());
    
    // Test: Complex AND + OR combination
    test_case!("Complex - Active AND rating>=4.0 AND ((Electronics AND price<1000) OR (Home AND featured))");
    let results = Product::query()
        .where_eq("active", true)
        .where_gte("rating", 4.0)
        .begin_or()
            .or_where_eq("category", "Electronics").and_where_lt("price", 1000.0)
            .or_where_eq("category", "Home").and_where_eq("featured", true)
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| 
            p.active && 
            p.rating >= 4.0 &&
            ((p.category == "Electronics" && p.price < 1000.0) || 
             (p.category == "Home" && p.featured))
        ),
        "All should match complex criteria"
    );
    
    // Test: Multiple conditions before and after OR group
    test_case!("Sandwich pattern - where + where + OR group + where");
    let results = Product::query()
        .where_eq("active", true)
        .where_gte("rating", 4.3)
        .begin_or()
            .or_where_eq("brand", "Apple")
            .or_where_eq("brand", "Samsung")
            .or_where_eq("brand", "Nike")
        .end_or()
        .where_gt("stock", 30)
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| 
            p.active && 
            p.rating >= 4.3 &&
            ["Apple", "Samsung", "Nike"].contains(&p.brand.as_str()) &&
            p.stock > 30
        ),
        "All should match sandwich pattern"
    );
    
    // Test: OR groups with different operators
    test_case!("Mixed operators in OR - (name LIKE 'iPhone%') OR (price BETWEEN) OR (stock IN)");
    let results = Product::query()
        .begin_or()
            .or_where_like("name", "iPhone%")
            .or_where_between("price", 75.0, 85.0)
            .or_where_in("stock", vec![500, 400, 300])
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| 
            p.name.starts_with("iPhone") ||
            (p.price >= 75.0 && p.price <= 85.0) ||
            [500, 400, 300].contains(&p.stock)
        ),
        "All should match one of the mixed conditions"
    );
    
    // Test: Deeply nested business logic
    test_case!("Deep nesting - Premium segment analysis");
    // ((Electronics AND (Apple OR Samsung) AND price>500) OR (Home AND Dyson)) AND active AND rating>4.0
    let results = Product::query()
        .where_eq("active", true)
        .where_gt("rating", 4.0)
        .begin_or()
            .or_where_eq("category", "Electronics")
                .and_where_gt("price", 500.0)
                .and_where_in("brand", vec!["Apple", "Samsung"])
            .or_where_eq("category", "Home")
                .and_where_eq("brand", "Dyson")
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| 
            p.active && 
            p.rating > 4.0 &&
            ((p.category == "Electronics" && p.price > 500.0 && 
              ["Apple", "Samsung"].contains(&p.brand.as_str())) ||
             (p.category == "Home" && p.brand == "Dyson"))
        ),
        "All should match deep nesting criteria"
    );
    
    Ok(())
}

// =============================================================================
// 14. EDGE CASES AND SPECIAL PATTERNS
// =============================================================================

async fn test_edge_cases_and_special_patterns() -> tideorm::Result<()> {
    test_section!("14. EDGE CASES & SPECIAL PATTERNS");
    
    // Test: Empty string matching
    test_case!("Edge case - LIKE with empty pattern prefix");
    let results = Product::query()
        .where_like("name", "%")
        .limit(5)
        .get()
        .await?;
    print_products(&results, 5);
    println!("   Pattern '%' matches {} products", results.len());
    
    // Test: Single value IN clause
    test_case!("Edge case - IN with single value (equivalent to =)");
    let results = Product::query()
        .where_in("brand", vec!["Apple"])
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(results.iter().all(|p| p.brand == "Apple"), "All should be Apple");
    
    // Test: NOT IN with many values
    test_case!("Edge case - NOT IN with many exclusions");
    let results = Product::query()
        .where_not_in("category", vec!["Electronics", "Clothing", "Home", "Sports"])
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| p.category == "Books"),
        "Only Books should remain"
    );
    
    // Test: Boundary values for BETWEEN
    test_case!("Edge case - BETWEEN with exact boundary values");
    let results = Product::query()
        .where_between("price", 29.99, 29.99) // Exact match
        .get()
        .await?;
    print_products(&results, 5);
    assert_test!(
        results.iter().all(|p| p.price == 29.99),
        "All should have price exactly 29.99"
    );
    
    // Test: Chained same-column conditions
    test_case!("Pattern - Same column multiple conditions (rating window)");
    let results = Product::query()
        .where_gt("rating", 4.3)
        .where_lt("rating", 4.7)
        .where_not("rating", 4.5) // Exclude exactly 4.5
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| p.rating > 4.3 && p.rating < 4.7 && p.rating != 4.5),
        "All should be in rating window excluding 4.5"
    );
    
    // Test: NULL with OR
    test_case!("Pattern - Complex NULL logic in OR");
    let results = Product::query()
        .begin_or()
            .or_where_null("description").and_where_eq("active", true)
            .or_where_null("discount_percent").and_where_gt("price", 100.0)
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| 
            (p.description.is_none() && p.active) ||
            (p.discount_percent.is_none() && p.price > 100.0)
        ),
        "All should match NULL pattern"
    );
    
    // Test: Overlapping OR conditions
    test_case!("Pattern - Overlapping conditions (some products match multiple)");
    let results = Product::query()
        .begin_or()
            .or_where_eq("brand", "Apple") // Matches Apple
            .or_where_gt("price", 1000.0)  // Also matches expensive Apple
            .or_where_eq("featured", true) // Also matches featured Apple
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    
    // Count how many match multiple conditions
    let multi_match: Vec<_> = results.iter().filter(|p| {
        let matches = (p.brand == "Apple") as i32 + 
                     (p.price > 1000.0) as i32 + 
                     p.featured as i32;
        matches > 1
    }).collect();
    println!("   {} products match multiple OR conditions", multi_match.len());
    
    // Test: Case sensitivity in LIKE
    test_case!("Pattern - LIKE patterns (case sensitive)");
    let results_upper = Product::query()
        .where_like("name", "%PRO%")
        .get()
        .await?;
    let results_lower = Product::query()
        .where_like("name", "%pro%")
        .get()
        .await?;
    let results_mixed = Product::query()
        .where_like("name", "%Pro%")
        .get()
        .await?;
    println!("   'PRO' matches: {}, 'pro' matches: {}, 'Pro' matches: {}", 
        results_upper.len(), results_lower.len(), results_mixed.len());
    
    // Test: Combining multiple LIKE with OR
    test_case!("Pattern - Multiple LIKE patterns");
    let results = Product::query()
        .begin_or()
            .or_where_like("name", "iPhone%")
            .or_where_like("name", "Galaxy%")
            .or_where_like("name", "MacBook%")
            .or_where_like("name", "%Pro%")
        .end_or()
        .get()
        .await?;
    print_products(&results, 10);
    println!("   {} products match any name pattern", results.len());
    
    // Test: Negation patterns
    test_case!("Pattern - Multiple negations");
    let results = Product::query()
        .where_not("category", "Electronics")
        .where_not("category", "Clothing")
        .where_not("active", false)
        .where_not_null("description")
        .get()
        .await?;
    print_products(&results, 10);
    assert_test!(
        results.iter().all(|p| 
            p.category != "Electronics" && 
            p.category != "Clothing" && 
            p.active && 
            p.description.is_some()
        ),
        "All should pass negation filters"
    );
    
    // Test: Large IN clause
    test_case!("Performance - Large IN clause with many values");
    let many_stocks: Vec<i32> = (1..=500).collect();
    let results = Product::query()
        .where_in("stock", many_stocks)
        .get()
        .await?;
    println!("   Large IN (1-500) matches {} products", results.len());
    
    Ok(())
}

// =============================================================================
// 15. REPORTING & ANALYTICS QUERIES
// =============================================================================

async fn test_reporting_analytics_queries() -> tideorm::Result<()> {
    test_section!("15. REPORTING & ANALYTICS QUERIES");
    
    // Report 1: Category Performance Summary
    test_case!("Report - Best performing products per category");
    for category in &["Electronics", "Clothing", "Home", "Books", "Sports"] {
        let top = Product::query()
            .where_eq("category", *category)
            .where_eq("active", true)
            .order_by("rating", Order::Desc)
            .order_by("stock", Order::Desc)
            .limit(3)
            .get()
            .await?;
        println!("   Top 3 in {}:", category);
        for p in &top {
            println!("      - {} (rating: {:.1}, stock: {})", p.name, p.rating, p.stock);
        }
    }
    
    // Report 2: Inventory Health by Category
    test_case!("Report - Low inventory alerts by category");
    let low_inventory = Product::query()
        .where_eq("active", true)
        .where_lt("stock", 50)
        .order_by("category", Order::Asc)
        .order_by("stock", Order::Asc)
        .get()
        .await?;
    
    let mut current_category = String::new();
    for p in &low_inventory {
        if p.category != current_category {
            current_category = p.category.clone();
            println!("   {}:", current_category);
        }
        println!("      ⚠️ {} - only {} in stock", p.name, p.stock);
    }
    
    // Report 3: Discount Analysis
    test_case!("Report - Discount distribution analysis");
    for range in &[(1, 10), (11, 20), (21, 30)] {
        let count = Product::query()
            .where_between("discount_percent", range.0, range.1)
            .count()
            .await?;
        println!("   {}%-{}% discount: {} products", range.0, range.1, count);
    }
    let no_discount = Product::query()
        .where_null("discount_percent")
        .count()
        .await?;
    println!("   No discount: {} products", no_discount);
    
    // Report 4: Price Tier Analysis
    test_case!("Report - Products by price tier");
    let tiers = [
        ("Budget (< $50)", 0.0, 50.0),
        ("Mid-range ($50-200)", 50.0, 200.0),
        ("Premium ($200-500)", 200.0, 500.0),
        ("Luxury ($500-1000)", 500.0, 1000.0),
        ("Ultra-premium (> $1000)", 1000.0, 10000.0),
    ];
    for (name, min, max) in &tiers {
        let count = Product::query()
            .where_gte("price", *min)
            .where_lt("price", *max)
            .count()
            .await?;
        println!("   {}: {} products", name, count);
    }
    
    // Report 5: Featured Products Performance
    test_case!("Report - Featured vs Non-featured comparison");
    let featured = Product::query()
        .where_eq("featured", true)
        .where_eq("active", true)
        .get()
        .await?;
    let non_featured = Product::query()
        .where_eq("featured", false)
        .where_eq("active", true)
        .get()
        .await?;
    
    let avg_rating_featured: f64 = featured.iter().map(|p| p.rating).sum::<f64>() / featured.len() as f64;
    let avg_rating_non: f64 = non_featured.iter().map(|p| p.rating).sum::<f64>() / non_featured.len() as f64;
    let avg_price_featured: f64 = featured.iter().map(|p| p.price).sum::<f64>() / featured.len() as f64;
    let avg_price_non: f64 = non_featured.iter().map(|p| p.price).sum::<f64>() / non_featured.len() as f64;
    
    println!("   Featured ({} products):", featured.len());
    println!("      Avg rating: {:.2}, Avg price: ${:.2}", avg_rating_featured, avg_price_featured);
    println!("   Non-featured ({} products):", non_featured.len());
    println!("      Avg rating: {:.2}, Avg price: ${:.2}", avg_rating_non, avg_price_non);
    
    // Report 6: Brand Portfolio Analysis
    test_case!("Report - Brand presence by category");
    let brands = ["Apple", "Samsung", "Nike", "Adidas", "O'Reilly"];
    for brand in &brands {
        let products = Product::query()
            .where_eq("brand", *brand)
            .get()
            .await?;
        
        let categories: std::collections::HashSet<_> = products.iter().map(|p| &p.category).collect();
        let total_value: f64 = products.iter().map(|p| p.price * p.stock as f64).sum();
        
        println!("   {} ({} products):", brand, products.len());
        println!("      Categories: {:?}", categories);
        println!("      Total inventory value: ${:.2}", total_value);
    }
    
    // Report 7: Quality Score Distribution
    test_case!("Report - Rating distribution");
    let rating_ranges = [
        ("Excellent (4.7+)", 4.7, 5.0),
        ("Very Good (4.5-4.7)", 4.5, 4.7),
        ("Good (4.3-4.5)", 4.3, 4.5),
        ("Average (4.0-4.3)", 4.0, 4.3),
        ("Below Average (<4.0)", 0.0, 4.0),
    ];
    for (label, min, max) in &rating_ranges {
        let count = Product::query()
            .where_gte("rating", *min)
            .where_lt("rating", *max)
            .count()
            .await?;
        let pct = (count as f64 / 30.0 * 100.0) as i32;
        println!("   {}: {} products ({}%)", label, count, pct);
    }
    
    // Report 8: Active vs Inactive Product Analysis
    test_case!("Report - Active status breakdown");
    let inactive = Product::query()
        .where_eq("active", false)
        .get()
        .await?;
    println!("   Inactive products ({}):", inactive.len());
    for p in &inactive {
        let reason = if p.stock < 50 { "low stock" }
            else if p.rating < 4.0 { "low rating" }
            else { "other" };
        println!("      - {} ({}) - possible reason: {}", p.name, p.category, reason);
    }
    
    // Report 9: High Value Opportunities
    test_case!("Report - High value products needing attention");
    let high_value = Product::query()
        .where_gt("price", 500.0)
        .where_eq("active", true)
        .begin_or()
            .or_where_lt("stock", 50)
            .or_where_lt("rating", 4.5)
            .or_where_eq("featured", false)
        .end_or()
        .order_by("price", Order::Desc)
        .get()
        .await?;
    println!("   High-value products needing attention:");
    for p in &high_value {
        let issues: Vec<&str> = [
            if p.stock < 50 { Some("low stock") } else { None },
            if p.rating < 4.5 { Some("rating < 4.5") } else { None },
            if !p.featured { Some("not featured") } else { None },
        ].into_iter().flatten().collect();
        println!("      - {} (${:.2}): {:?}", p.name, p.price, issues);
    }
    
    // Report 10: Comprehensive Product Search Dashboard
    test_case!("Dashboard - Multi-filter product search simulation");
    
    // Simulate: Electronics OR Home, active, good rating, in stock, sorted by value
    let dashboard_results = Product::query()
        .where_eq("active", true)
        .where_gte("rating", 4.0)
        .where_gt("stock", 0)
        .begin_or()
            .or_where_eq("category", "Electronics")
            .or_where_eq("category", "Home")
        .end_or()
        .order_by("rating", Order::Desc)
        .limit(15)
        .get()
        .await?;
    
    println!("   Dashboard Results (Electronics/Home, Active, Rating>=4.0):");
    println!("   {:<25} {:<12} {:>8} {:>6} {:>6}", "Name", "Category", "Price", "Stock", "Rating");
    println!("   {}", "-".repeat(65));
    for p in &dashboard_results {
        println!("   {:<25} {:<12} {:>8.2} {:>6} {:>6.1}", 
            &p.name[..p.name.len().min(24)], p.category, p.price, p.stock, p.rating);
    }
    
    Ok(())
}
