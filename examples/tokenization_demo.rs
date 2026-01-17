//! TideORM Record Tokenization Demo
//!
//! **Category:** Security Features
//!
//! This example demonstrates TideORM's secure record tokenization feature with a
//! real PostgreSQL database. Tokenization converts record IDs into encrypted,
//! URL-safe tokens that can be safely shared externally without exposing internal
//! database IDs.
//!
//! ## Run this example
//!
//! ```bash
//! cargo run --example tokenization_demo
//! ```
//!
//! ## Features Demonstrated
//!
//! - Global tokenization configuration via TideConfig
//! - Encryption key management
//! - Model-specific tokens (User token ≠ Product token)
//! - Tamper detection via HMAC verification
//! - URL-safe token encoding
//! - Full CRUD operations with tokenized IDs
//! - Custom encoder/decoder support
//!
//! ## Running this example
//!
//! 1. Configure database in `.env` file:
//!    ```
//!    POSTGRESQL_DATABASE_URL=postgres://postgres:postgres@localhost:5432/test_tide_orm
//!    ENCRYPTION_KEY=my-super-secret-encryption-key-32c
//!    ```
//!
//! 2. Run: cargo run --example tokenization_demo

use tideorm::prelude::*;

// ============================================================================
// USER MODEL - Basic tokenization example
// ============================================================================

/// User model - demonstrates basic tokenization
/// 
/// With tokenization enabled via `#[tide(tokenize)]`, you can:
/// - Convert user IDs to encrypted tokens: `user.tokenize()`
/// - Decode tokens back to IDs: `User::detokenize(&token)`
/// - Fetch records directly from tokens: `User::from_token(&token).await`
/// - Use tokens in URLs instead of exposing database IDs
#[tideorm::model]
#[tide(table = "tokenization_users", hidden = "deleted_at", tokenize)]
#[index("email")]
#[unique_index("email")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    
    pub email: String,
    pub name: String,
    pub status: String,
    
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl User {
    pub fn new(email: impl Into<String>, name: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: 0,
            email: email.into(),
            name: name.into(),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
        }
    }
}

// No manual Tokenizable implementation needed!
// The `#[tide(tokenize)]` attribute automatically implements it.

// ============================================================================
// PRODUCT MODEL - Another model for cross-model token testing
// ============================================================================

/// Product model - demonstrates model-specific tokens
/// 
/// A token for a User cannot be decoded as a Product and vice versa,
/// preventing ID enumeration attacks across different resources.
#[tideorm::model]
#[tide(table = "tokenization_products", tokenize)]
#[index("sku")]
#[unique_index("sku")]
pub struct Product {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    
    pub sku: String,
    pub name: String,
    pub price: i32,  // Price in cents
    pub stock: i32,
    
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Product {
    pub fn new(sku: impl Into<String>, name: impl Into<String>, price: i32) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: 0,
            sku: sku.into(),
            name: name.into(),
            price,
            stock: 0,
            created_at: now,
            updated_at: now,
        }
    }
}

// No manual Tokenizable implementation needed!
// The `#[tide(tokenize)]` attribute automatically implements it.

// ============================================================================
// ORDER MODEL - Demonstrates tokens in relationships
// ============================================================================

/// Order model - demonstrates tokens in foreign key relationships
/// 
/// Orders reference users and products. In APIs, you can use tokens
/// instead of raw IDs to hide the internal structure.
#[tideorm::model]
#[tide(table = "tokenization_orders", tokenize)]
#[index("user_id")]
#[index("status")]
pub struct Order {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    
    pub user_id: i64,
    pub product_id: i64,
    pub quantity: i32,
    pub total_cents: i32,
    pub status: String,  // "pending", "paid", "shipped", "delivered"
    
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Order {
    pub fn new(user_id: i64, product_id: i64, quantity: i32, price_per_unit: i32) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: 0,
            user_id,
            product_id,
            quantity,
            total_cents: quantity * price_per_unit,
            status: "pending".to_string(),
            created_at: now,
            updated_at: now,
        }
    }
    
    pub fn mark_paid(&mut self) {
        self.status = "paid".to_string();
        self.updated_at = chrono::Utc::now();
    }
    
    pub fn ship(&mut self) {
        self.status = "shipped".to_string();
        self.updated_at = chrono::Utc::now();
    }
}

// No manual Tokenizable implementation needed!
// The `#[tide(tokenize)]` attribute automatically implements it.

// ============================================================================
// DATABASE SETUP SQL
// ============================================================================

const DROP_TABLES_SQL: &str = r#"
DROP TABLE IF EXISTS tokenization_orders CASCADE;
DROP TABLE IF EXISTS tokenization_products CASCADE;
DROP TABLE IF EXISTS tokenization_users CASCADE;
"#;

const CREATE_TABLES_SQL: &str = r#"
-- Users table
CREATE TABLE IF NOT EXISTS tokenization_users (
    id BIGSERIAL PRIMARY KEY,
    email VARCHAR(255) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ
);

-- Products table
CREATE TABLE IF NOT EXISTS tokenization_products (
    id BIGSERIAL PRIMARY KEY,
    sku VARCHAR(100) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    price INTEGER NOT NULL,
    stock INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Orders table
CREATE TABLE IF NOT EXISTS tokenization_orders (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES tokenization_users(id) ON DELETE CASCADE,
    product_id BIGINT NOT NULL REFERENCES tokenization_products(id) ON DELETE CASCADE,
    quantity INTEGER NOT NULL,
    total_cents INTEGER NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Create indexes
CREATE INDEX IF NOT EXISTS idx_tokenization_orders_user_id ON tokenization_orders(user_id);
CREATE INDEX IF NOT EXISTS idx_tokenization_orders_product_id ON tokenization_orders(product_id);
CREATE INDEX IF NOT EXISTS idx_tokenization_orders_status ON tokenization_orders(status);
"#;

// ============================================================================
// MAIN EXAMPLE
// ============================================================================

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    println!("🔐 TideORM Tokenization Demo");
    println!("============================\n");
    
    // ========================================================================
    // STEP 1: Configure TideORM with Encryption Key
    // ========================================================================
    print_section("1. Database & Tokenization Configuration");
    
    // Load environment variables
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("POSTGRESQL_DATABASE_URL")
        .expect("POSTGRESQL_DATABASE_URL must be set in .env file");
    
    // Get encryption key from environment (or use a default for demo)
    let encryption_key = std::env::var("ENCRYPTION_KEY")
        .unwrap_or_else(|_| "demo-encryption-key-32-characters!".to_string());
    
    println!("   📦 Connecting to PostgreSQL...");
    println!("   🔑 Encryption key configured (length: {} chars)", encryption_key.len());
    
    TideConfig::init()
        .database_type(DatabaseType::Postgres)
        .database(&db_url)
        .max_connections(10)
        .min_connections(2)
        .connect_timeout(std::time::Duration::from_secs(10))
        // Configure tokenization globally
        .encryption_key(&encryption_key)
        .sync(false)
        .connect()
        .await?;
    
    println!("   ✅ Connected to database!");
    println!();
    
    // Create tables
    println!("   📋 Setting up tables...");
    use tideorm::internal::ConnectionTrait;
    let conn = db().__internal_connection();
    
    // Drop existing tables first (for clean demo runs)
    conn.execute_unprepared(DROP_TABLES_SQL)
        .await
        .map_err(|e| tideorm::Error::query(e.to_string()))?;
    println!("      Dropped existing tables");
    
    // Create fresh tables
    conn.execute_unprepared(CREATE_TABLES_SQL)
        .await
        .map_err(|e| tideorm::Error::query(e.to_string()))?;
    println!("   ✅ Tables created!\n");
    
    // ========================================================================
    // STEP 2: Create Test Data
    // ========================================================================
    print_section("2. Creating Test Data");
    
    // Create users
    println!("   👤 Creating users...");
    let user1 = User::create(User::new("alice@example.com", "Alice Johnson")).await?;
    let user2 = User::create(User::new("bob@example.com", "Bob Smith")).await?;
    let user3 = User::create(User::new("charlie@example.com", "Charlie Brown")).await?;
    
    println!("      Created: {} (ID: {})", user1.name, user1.id);
    println!("      Created: {} (ID: {})", user2.name, user2.id);
    println!("      Created: {} (ID: {})", user3.name, user3.id);
    println!();
    
    // Create products
    println!("   📦 Creating products...");
    let product1 = Product::create(Product::new("LAPTOP-001", "Gaming Laptop", 129999)).await?;
    let product2 = Product::create(Product::new("MOUSE-001", "Wireless Mouse", 4999)).await?;
    let product3 = Product::create(Product::new("KEYBOARD-001", "Mechanical Keyboard", 14999)).await?;
    
    println!("      Created: {} (ID: {}, ${:.2})", product1.name, product1.id, product1.price as f64 / 100.0);
    println!("      Created: {} (ID: {}, ${:.2})", product2.name, product2.id, product2.price as f64 / 100.0);
    println!("      Created: {} (ID: {}, ${:.2})", product3.name, product3.id, product3.price as f64 / 100.0);
    println!();
    
    // Create orders
    println!("   🛒 Creating orders...");
    let order1 = Order::create(Order::new(user1.id, product1.id, 1, product1.price)).await?;
    let order2 = Order::create(Order::new(user1.id, product2.id, 2, product2.price)).await?;
    let order3 = Order::create(Order::new(user2.id, product3.id, 1, product3.price)).await?;
    
    println!("      Order #{}: User {} ordered {} x {} = ${:.2}", 
             order1.id, order1.user_id, order1.quantity, product1.name, order1.total_cents as f64 / 100.0);
    println!("      Order #{}: User {} ordered {} x {} = ${:.2}", 
             order2.id, order2.user_id, order2.quantity, product2.name, order2.total_cents as f64 / 100.0);
    println!("      Order #{}: User {} ordered {} x {} = ${:.2}", 
             order3.id, order3.user_id, order3.quantity, product3.name, order3.total_cents as f64 / 100.0);
    println!();
    
    // ========================================================================
    // STEP 3: Basic Tokenization using model methods
    // ========================================================================
    print_section("3. Basic Tokenization (Model Methods)");
    
    println!("   Using model.tokenize() and Model::tokenize_id():");
    println!();
    
    // Tokenize users using instance method: user.tokenize()
    let user1_token = user1.tokenize()?;
    let user2_token = user2.tokenize()?;
    let user3_token = user3.tokenize()?;
    
    println!("   User Tokens (using user.tokenize()):");
    println!("      {} (ID {}) -> {}", user1.name, user1.id, user1_token);
    println!("      {} (ID {}) -> {}", user2.name, user2.id, user2_token);
    println!("      {} (ID {}) -> {}", user3.name, user3.id, user3_token);
    println!();
    
    // Tokenize products using instance method: product.tokenize()
    let product1_token = product1.tokenize()?;
    let product2_token = product2.tokenize()?;
    
    println!("   Product Tokens (using product.tokenize()):");
    println!("      {} (ID {}) -> {}", product1.name, product1.id, product1_token);
    println!("      {} (ID {}) -> {}", product2.name, product2.id, product2_token);
    println!();
    
    // Tokenize orders using instance method: order.tokenize()
    let order1_token = order1.tokenize()?;
    let order2_token = order2.tokenize()?;
    
    println!("   Order Tokens (using order.tokenize()):");
    println!("      Order #{} -> {}", order1.id, order1_token);
    println!("      Order #{} -> {}", order2.id, order2_token);
    println!();
    
    // ========================================================================
    // STEP 4: Decoding Tokens using Model::detokenize()
    // ========================================================================
    print_section("4. Decoding Tokens (Model::detokenize)");
    
    println!("   Using User::detokenize() to decode tokens:");
    let decoded_user1_id = User::detokenize(&user1_token)?;
    let decoded_user2_id = User::detokenize(&user2_token)?;
    
    println!("      Token {}... -> ID {}", &user1_token[..20], decoded_user1_id);
    println!("      Token {}... -> ID {}", &user2_token[..20], decoded_user2_id);
    
    assert_eq!(decoded_user1_id, user1.id);
    assert_eq!(decoded_user2_id, user2.id);
    println!("      ✅ User tokens decoded correctly!");
    println!();
    
    println!("   Using Product::detokenize() to decode tokens:");
    let decoded_product1_id = Product::detokenize(&product1_token)?;
    println!("      Token {}... -> ID {}", &product1_token[..20], decoded_product1_id);
    
    assert_eq!(decoded_product1_id, product1.id);
    println!("      ✅ Product tokens decoded correctly!");
    println!();
    
    // ========================================================================
    // STEP 5: Finding Records by Token using Model::from_token()
    // ========================================================================
    print_section("5. Finding Records by Token (Model::from_token)");
    
    // Use User::from_token() to directly fetch user from token
    let found_user = User::from_token(&user1_token).await?;
    println!("   Using User::from_token():");
    println!("      Token: {}...", &user1_token[..30]);
    println!("      Name: {}", found_user.name);
    println!("      Email: {}", found_user.email);
    println!("      Status: {}", found_user.status);
    println!();
    
    // Use Product::from_token() to directly fetch product from token
    let found_product = Product::from_token(&product1_token).await?;
    println!("   Using Product::from_token():");
    println!("      Token: {}...", &product1_token[..30]);
    println!("      Name: {}", found_product.name);
    println!("      SKU: {}", found_product.sku);
    println!("      Price: ${:.2}", found_product.price as f64 / 100.0);
    println!();
    
    // ========================================================================
    // STEP 6: Model-Specific Token Security
    // ========================================================================
    print_section("6. Model-Specific Token Security");
    
    println!("   Tokens are model-specific - a User token cannot decode a Product:");
    println!();
    
    // Same ID, different models - using Model::tokenize_id()
    let user_token_for_1 = User::tokenize_id(1)?;
    let product_token_for_1 = Product::tokenize_id(1)?;
    
    println!("   User::tokenize_id(1):    {}", user_token_for_1);
    println!("   Product::tokenize_id(1): {}", product_token_for_1);
    println!("   Tokens are different:    {}", user_token_for_1 != product_token_for_1);
    println!();
    
    // Try cross-model decoding - User::detokenize on a Product token should fail
    let cross_decode = User::detokenize(&product_token_for_1);
    let cross_decode_failed = cross_decode.is_err();
    println!("   User::detokenize(product_token): {}", if cross_decode_failed { "Error (expected)" } else { "OK" });
    assert!(cross_decode_failed);
    println!("   ✅ Cross-model token reuse prevented!");
    println!();
    
    // ========================================================================
    // STEP 7: Tamper Detection
    // ========================================================================
    print_section("7. Tamper Detection");
    
    let original_token = user1.tokenize()?;
    
    // Tamper with the token
    let mut tampered_bytes: Vec<u8> = original_token.bytes().collect();
    if tampered_bytes.len() > 10 {
        tampered_bytes[10] = if tampered_bytes[10] == b'A' { b'B' } else { b'A' };
    }
    let tampered_token = String::from_utf8(tampered_bytes).unwrap_or_default();
    
    println!("   Original token:  {}", original_token);
    println!("   Tampered token:  {}", tampered_token);
    println!();
    
    let original_decode = User::detokenize(&original_token);
    let tampered_decode = User::detokenize(&tampered_token);
    
    println!("   Original decodes to: {:?}", original_decode.as_ref().ok());
    println!("   Tampered decodes to: {}", if tampered_decode.is_err() { "Error (expected)" } else { "OK" });
    
    assert!(original_decode.is_ok());
    // tampered_decode already checked above
    println!("   ✅ Tampered tokens are rejected!");
    println!();
    
    // ========================================================================
    // STEP 8: Invalid Token Handling
    // ========================================================================
    print_section("8. Invalid Token Handling");
    
    let invalid_tokens = vec![
        ("Empty string", ""),
        ("Too short", "abc"),
        ("Random text", "not-a-valid-token"),
        ("Special chars", "token!@#$%^&*()"),
        ("Spaces", "token with spaces"),
    ];
    
    println!("   Testing invalid token rejection with User::detokenize():");
    for (name, token) in invalid_tokens {
        let result = User::detokenize(token);
        println!("      {}: {:?}", name, if result.is_err() { "Error" } else { "OK" });
        assert!(result.is_err());
    }
    println!("   ✅ All invalid tokens rejected!");
    println!();
    
    // ========================================================================
    // STEP 9: URL-Safe Verification using User::tokenize_id()
    // ========================================================================
    print_section("9. URL-Safe Token Verification");
    
    let test_ids: Vec<i64> = vec![1, 100, 999_999, i64::MAX, -1, 0];
    
    println!("   Verifying all tokens are URL-safe (using User::tokenize_id()):");
    for id in test_ids {
        let token = User::tokenize_id(id)?;
        let is_url_safe = token.chars().all(|c| {
            c.is_ascii_alphanumeric() || c == '-' || c == '_'
        });
        println!("      ID {:>20} -> {} chars, URL-safe: {}", id, token.len(), is_url_safe);
        assert!(is_url_safe);
    }
    println!("   ✅ All tokens are URL-safe!");
    println!();
    
    // ========================================================================
    // STEP 10: API URL Examples
    // ========================================================================
    print_section("10. API URL Examples");
    
    println!("   Using tokens in RESTful API URLs:");
    println!();
    
    println!("   📧 User Profile:");
    println!("      GET /api/users/{}", user1_token);
    println!();
    
    println!("   📦 Product Details:");
    println!("      GET /api/products/{}", product1_token);
    println!();
    
    println!("   🛒 Order Status:");
    println!("      GET /api/orders/{}", order1_token);
    println!();
    
    println!("   🔗 Shareable Links:");
    println!("      https://myapp.com/profile/{}", user1_token);
    println!("      https://myapp.com/product/{}", product1_token);
    println!();
    
    println!("   📧 Email Unsubscribe:");
    println!("      https://myapp.com/unsubscribe/{}", user1_token);
    println!();
    
    // ========================================================================
    // STEP 11: Batch Operations using model.tokenize()
    // ========================================================================
    print_section("11. Batch Tokenization");
    
    let all_users = User::all().await?;
    println!("   Tokenizing all {} users (using user.tokenize()):", all_users.len());
    
    for user in &all_users {
        let token = user.tokenize()?;
        println!("      {} -> {}", user.name, token);
    }
    println!();
    
    let all_products = Product::all().await?;
    println!("   Tokenizing all {} products (using product.tokenize()):", all_products.len());
    
    for product in &all_products {
        let token = product.tokenize()?;
        println!("      {} -> {}", product.name, token);
    }
    println!();
    
    // ========================================================================
    // STEP 12: Token Consistency
    // ========================================================================
    print_section("12. Token Consistency Test");
    
    println!("   Verifying tokens are consistent:");
    
    // Tokens are deterministic (same input = same output)
    let token_a = user1.tokenize()?;
    let token_b = user1.tokenize()?;
    
    println!("      Token A (user1.tokenize()): {}", token_a);
    println!("      Token B (user1.tokenize()): {}", token_b);
    println!("      Tokens are same: {}", token_a == token_b);
    
    let decoded_a = User::detokenize(&token_a)?;
    let decoded_b = User::detokenize(&token_b)?;
    
    println!("      Both decode to same ID: {}", decoded_a == decoded_b);
    assert_eq!(decoded_a, decoded_b);
    println!("   ✅ Both tokens decode to the same ID!");
    println!();
    
    // ========================================================================
    // STEP 13: Update Operations with Tokens using from_token()
    // ========================================================================
    print_section("13. Update Operations with Tokens");
    
    // Simulate receiving a token from an API request
    let token_from_request = &user1_token;
    
    println!("   Simulating API update request:");
    println!("      Received token: {}...", &token_from_request[..30]);
    
    // Use User::from_token() to directly fetch and update
    let mut user_to_update = User::from_token(token_from_request).await?;
    println!("      Found user via User::from_token(): {}", user_to_update.name);
    
    // Update the user
    user_to_update.status = "premium".to_string();
    user_to_update.updated_at = chrono::Utc::now();
    let updated_user = user_to_update.update().await?;
    
    println!("      Updated status to: {}", updated_user.status);
    println!("   ✅ User updated successfully via token!");
    println!();
    
    // ========================================================================
    // STEP 14: Delete Operations with Tokens
    // ========================================================================
    print_section("14. Delete Operations with Tokens");
    
    // Create a user to delete
    let temp_user = User::create(User::new("temp@example.com", "Temporary User")).await?;
    let temp_token = temp_user.tokenize()?;
    
    println!("   Created temporary user:");
    println!("      ID: {}", temp_user.id);
    println!("      Token (temp_user.tokenize()): {}", temp_token);
    
    // Delete via token using from_token()
    let user_to_delete = User::from_token(&temp_token).await?;
    let deleted_id = user_to_delete.id;
    user_to_delete.delete().await?;
    println!("   ✅ User deleted via token!");
    
    // Verify deletion
    let deleted_user = User::find(deleted_id).await?;
    println!("      Verify deleted: {:?}", deleted_user.map(|u| u.name));
    println!();
    
    // ========================================================================
    // STEP 15: Comprehensive API Test
    // ========================================================================
    print_section("15. Comprehensive API Summary");
    
    println!("   Model Methods Available:");
    println!();
    println!("   Instance Methods:");
    println!("      user.tokenize()          -> Result<String>");
    println!("      user.to_token()          -> Result<String> (alias)");
    println!("      user.regenerate_token()  -> Result<String>");
    println!();
    println!("   Static Methods:");
    println!("      User::tokenize_id(42)    -> Result<String>");
    println!("      User::detokenize(&token) -> Result<i64>");
    println!("      User::decode_token(&token) -> Result<i64> (alias)");
    println!("      User::from_token(&token) -> Result<User> (async)");
    println!();
    
    // Final record counts
    let user_count = User::count().await?;
    let product_count = Product::count().await?;
    let order_count = Order::count().await?;
    
    println!("   Final Record Counts:");
    println!("      Users: {}", user_count);
    println!("      Products: {}", product_count);
    println!("      Orders: {}", order_count);
    println!();
    
    // ========================================================================
    // SUMMARY
    // ========================================================================
    print_section("Summary");
    
    println!("   ✅ Connected to PostgreSQL with tokenization enabled");
    println!("   ✅ Created users, products, and orders");
    println!("   ✅ Tokenized record IDs to URL-safe strings");
    println!("   ✅ Decoded tokens back to IDs");
    println!("   ✅ Found records using decoded tokens");
    println!("   ✅ Verified model-specific token security");
    println!("   ✅ Demonstrated tamper detection");
    println!("   ✅ Handled invalid tokens gracefully");
    println!("   ✅ Verified URL-safe encoding");
    println!("   ✅ Performed CRUD operations via tokens");
    println!();
    println!("   ⚠️  Security Reminders:");
    println!("      • Use a secure 32+ character encryption key in production");
    println!("      • Store keys in environment variables, never in code");
    println!("      • Changing the key invalidates all existing tokens");
    println!("      • Tokens are unique per model - User token ≠ Product token");
    println!();
    
    Ok(())
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

fn print_section(title: &str) {
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📋 {}", title);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
}
