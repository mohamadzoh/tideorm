//! SeaORM 2.0 Features Demo
//!
//! This example demonstrates the new SeaORM 2.0 features implemented in TideORM:
//!
//! 1. **Strongly-Typed Columns** - Compile-time type safety for queries
//! 2. **Nested ActiveModel** - Cascade save operations for related models
//! 3. **Self-Referencing Relations** - Hierarchical data support
//! 4. **Join Result Consolidation** - Nest flat join results
//! 5. **Linked Partial Select** - Select specific columns from related tables
//!
//! Run with: `cargo run --example seaorm2_features_demo`

use tideorm::prelude::*;
use tideorm::columns::{Column, ColumnEq, ColumnOrd, ColumnLike, ColumnNullable, ColumnIn};

// =============================================================================
// MODEL DEFINITIONS
// =============================================================================

// Note: These models would use #[tideorm::model] in a real application
// For this demo, we show the structure and intended usage

/// User model
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct User {
    id: i64,
    name: String,
    email: String,
    age: Option<i32>,
    active: bool,
}

/// Post model - belongs to User
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Post {
    id: i64,
    user_id: i64,
    title: String,
    content: String,
    published: bool,
}

/// Profile model - belongs to User (one-to-one)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Profile {
    id: i64,
    user_id: i64,
    bio: String,
    avatar_url: Option<String>,
}

/// Employee model - self-referencing (org chart)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Employee {
    id: i64,
    name: String,
    title: String,
    manager_id: Option<i64>,
}

/// Order model for join examples
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct Order {
    id: i64,
    customer_name: String,
    total: i64,
}

/// LineItem model - belongs to Order
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct LineItem {
    id: i64,
    order_id: i64,
    product_name: String,
    quantity: i32,
    price: i64,
}

// =============================================================================
// FEATURE 1: STRONGLY-TYPED COLUMNS
// =============================================================================

/// Define typed columns for compile-time safety
mod user_columns {
    use tideorm::columns::Column;
    
    /// User ID column (i64)
    pub const ID: Column<i64> = Column::new("id");
    /// User name column (String)
    pub const NAME: Column<String> = Column::new("name");
    /// User email column (String)
    pub const EMAIL: Column<String> = Column::new("email");
    /// User age column (nullable i32)
    pub const AGE: Column<Option<i32>> = Column::new("age");
    /// User active status column (bool)
    pub const ACTIVE: Column<bool> = Column::new("active");
}

fn demo_strongly_typed_columns() {
    println!("\n=== STRONGLY-TYPED COLUMNS ===\n");
    
    // Import our typed columns
    use user_columns::*;
    
    // Create type-safe conditions
    println!("Creating type-safe query conditions:");
    
    // String operations
    let name_eq = NAME.eq("Alice");
    println!("  NAME.eq(\"Alice\")        -> column: {}, op: {:?}", name_eq.column, name_eq.operator);
    
    let name_like = NAME.contains("test");
    println!("  NAME.contains(\"test\")   -> value: {}", name_like.value);
    
    let name_starts = NAME.starts_with("Mr.");
    println!("  NAME.starts_with(\"Mr.\") -> value: {}", name_starts.value);
    
    // Numeric operations
    let id_gt = ID.gt(100);
    println!("  ID.gt(100)               -> column: {}, op: {:?}", id_gt.column, id_gt.operator);
    
    let id_between = ID.between(1, 1000);
    println!("  ID.between(1, 1000)      -> value: {}", id_between.value);
    
    // Nullable operations
    let age_gt = AGE.gt(18);
    println!("  AGE.gt(18)               -> works on Option<i32>!");
    
    let age_null = AGE.is_null();
    println!("  AGE.is_null()            -> op: {:?}", age_null.operator);
    
    let age_not_null = AGE.is_not_null();
    println!("  AGE.is_not_null()        -> op: {:?}", age_not_null.operator);
    
    // Boolean operations
    let active_true = ACTIVE.eq(true);
    println!("  ACTIVE.eq(true)          -> value: {}", active_true.value);
    
    // IN operations
    let id_in = ID.is_in(vec![1, 2, 3, 4, 5]);
    println!("  ID.is_in([1,2,3,4,5])    -> op: {:?}", id_in.operator);
    
    let name_in = NAME.is_in(vec!["Alice", "Bob", "Charlie"]);
    println!("  NAME.is_in([...])        -> value: {}", name_in.value);
    
    println!("\n  ✅ Compile-time type safety:");
    println!("     - NAME.eq(\"text\")  ✓ String == &str");
    println!("     - ID.gt(100)        ✓ i64 > i64");
    println!("     - AGE.is_null()     ✓ Only on Option<T>");
    println!("     - NAME.like(\"%a%\") ✓ Only on String");
    println!("     - NAME.eq(123)      ✗ COMPILE ERROR! String != i32");
    println!("     - ID.like(\"%a%\")   ✗ COMPILE ERROR! i64 has no .like()");
    
    // Show how to use with QueryBuilder
    println!("\n  Usage with QueryBuilder (pseudo-code):");
    println!("    User::query()");
    println!("        .where_col(NAME.eq(\"Alice\"))");
    println!("        .where_col(AGE.gt(18))");
    println!("        .where_col(ACTIVE.eq(true))");
    println!("        .get()");
    println!("        .await?;");
}

// =============================================================================
// FEATURE 2: NESTED ACTIVE MODEL (CASCADE SAVE)
// =============================================================================

fn demo_nested_active_model() {
    println!("\n=== NESTED ACTIVE MODEL (CASCADE SAVE) ===\n");
    
    // Example data
    let user = User {
        id: 0, // Will be assigned on insert
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        age: Some(30),
        active: true,
    };
    
    let profile = Profile {
        id: 0,
        user_id: 0, // Will be set from user.id after save
        bio: "Software developer".to_string(),
        avatar_url: Some("https://example.com/alice.jpg".to_string()),
    };
    
    let posts = vec![
        Post {
            id: 0,
            user_id: 0, // Will be set from user.id
            title: "First Post".to_string(),
            content: "Hello, World!".to_string(),
            published: true,
        },
        Post {
            id: 0,
            user_id: 0,
            title: "Second Post".to_string(),
            content: "More content...".to_string(),
            published: false,
        },
    ];
    
    println!("NestedSave trait provides cascade save operations:\n");
    
    println!("1. save_with_one() - Save parent with single related model:");
    println!("   let (user, profile) = user.save_with_one(profile, \"user_id\").await?;");
    println!("   // Both saved, profile.user_id = user.id\n");
    
    println!("2. save_with_many() - Save parent with multiple related models:");
    println!("   let (user, posts) = user.save_with_many(posts, \"user_id\").await?;");
    println!("   // All posts saved with post.user_id = user.id\n");
    
    println!("3. update_with_one() - Cascade updates:");
    println!("   let (user, profile) = user.update_with_one(profile).await?;\n");
    
    println!("4. update_with_many() - Cascade update multiple:");
    println!("   let (user, posts) = user.update_with_many(posts).await?;\n");
    
    println!("5. delete_with_many() - Cascade delete (children first):");
    println!("   let deleted_count = user.delete_with_many(posts).await?;");
    println!("   // Deletes posts first, then user\n");
    
    println!("6. NestedSaveBuilder - Fluent API for complex nested saves:");
    println!("   let (user, related_json) = NestedSaveBuilder::new(user)");
    println!("       .with_one(profile, \"user_id\")");
    println!("       .with_many(posts, \"user_id\")");
    println!("       .save()");
    println!("       .await?;");
    
    // Demonstrate the JSON manipulation
    println!("\n  Behind the scenes - Foreign key update:");
    let mut post_json = serde_json::to_value(&posts[0]).unwrap();
    println!("    Before: {}", serde_json::to_string_pretty(&post_json).unwrap());
    
    if let serde_json::Value::Object(ref mut map) = post_json {
        map.insert("user_id".to_string(), serde_json::json!(42));
    }
    println!("    After setting user_id = 42:");
    println!("    {}", serde_json::to_string_pretty(&post_json).unwrap());
}

// =============================================================================
// FEATURE 3: SELF-REFERENCING RELATIONS
// =============================================================================

fn demo_self_referencing_relations() {
    println!("\n=== SELF-REFERENCING RELATIONS ===\n");
    
    // Example org chart
    let employees = vec![
        Employee { id: 1, name: "CEO".to_string(), title: "Chief Executive".to_string(), manager_id: None },
        Employee { id: 2, name: "VP Engineering".to_string(), title: "Vice President".to_string(), manager_id: Some(1) },
        Employee { id: 3, name: "VP Sales".to_string(), title: "Vice President".to_string(), manager_id: Some(1) },
        Employee { id: 4, name: "Tech Lead".to_string(), title: "Lead Engineer".to_string(), manager_id: Some(2) },
        Employee { id: 5, name: "Developer".to_string(), title: "Software Engineer".to_string(), manager_id: Some(4) },
    ];
    
    println!("Self-referencing relationships (like org charts):\n");
    
    println!("Model definition:");
    println!("  #[tideorm::model]");
    println!("  #[tide(table = \"employees\")]");
    println!("  struct Employee {{");
    println!("      #[tide(primary_key)]");
    println!("      id: i64,");
    println!("      name: String,");
    println!("      manager_id: Option<i64>,");
    println!("      ");
    println!("      // Parent reference (manager)");
    println!("      #[tide(self_ref = \"id\", foreign_key = \"manager_id\")]");
    println!("      manager: SelfRef<Employee>,");
    println!("      ");
    println!("      // Children reference (direct reports)");
    println!("      #[tide(self_ref_many = \"id\", foreign_key = \"manager_id\")]");
    println!("      reports: SelfRefMany<Employee>,");
    println!("  }}\n");
    
    println!("Sample Org Chart:");
    for emp in &employees {
        let indent = match emp.manager_id {
            None => "",
            Some(1) => "  └─ ",
            Some(2) => "      └─ ",
            Some(4) => "          └─ ",
            _ => "    ",
        };
        println!("  {}{} ({})", indent, emp.name, emp.title);
    }
    
    println!("\nSelfRef<E> operations:");
    println!("  let emp = Employee::find(5).await?;  // Developer");
    println!("  ");
    println!("  // Load the manager (parent)");
    println!("  let manager = emp.manager.load().await?;");
    println!("  // -> Tech Lead");
    println!("  ");
    println!("  // Check if has manager");
    println!("  let has_manager = emp.manager.exists().await?;");
    println!("  // -> true");
    
    println!("\nSelfRefMany<E> operations:");
    println!("  let vp = Employee::find(2).await?;  // VP Engineering");
    println!("  ");
    println!("  // Load direct reports");
    println!("  let reports = vp.reports.load().await?;");
    println!("  // -> [Tech Lead]");
    println!("  ");
    println!("  // Count direct reports");
    println!("  let count = vp.reports.count().await?;");
    println!("  // -> 1");
    println!("  ");
    println!("  // Load entire subtree (recursive)");
    println!("  let tree = vp.reports.load_tree(3).await?;");
    println!("  // -> [Tech Lead, Developer] (flattened)");
}

// =============================================================================
// FEATURE 4: JOIN RESULT CONSOLIDATION
// =============================================================================

fn demo_join_consolidation() {
    println!("\n=== JOIN RESULT CONSOLIDATION ===\n");
    
    // Simulate flat join results
    let order1 = Order { id: 1, customer_name: "Alice".to_string(), total: 150 };
    let order2 = Order { id: 2, customer_name: "Bob".to_string(), total: 75 };
    
    let flat_results: Vec<(Order, LineItem)> = vec![
        (order1.clone(), LineItem { id: 1, order_id: 1, product_name: "Widget".to_string(), quantity: 2, price: 50 }),
        (order1.clone(), LineItem { id: 2, order_id: 1, product_name: "Gadget".to_string(), quantity: 1, price: 50 }),
        (order2.clone(), LineItem { id: 3, order_id: 2, product_name: "Gizmo".to_string(), quantity: 3, price: 25 }),
    ];
    
    println!("Problem: JOIN queries return flat results:\n");
    println!("  SELECT * FROM orders JOIN line_items ON ...\n");
    println!("  Flat Results:");
    for (order, item) in &flat_results {
        println!("    (Order #{} '{}', LineItem #{} '{}')", 
            order.id, order.customer_name, item.id, item.product_name);
    }
    
    println!("\nSolution: JoinResultConsolidator nests them:\n");
    
    // Use the consolidator
    let nested = JoinResultConsolidator::consolidate_two(flat_results, |o| o.id);
    
    println!("  JoinResultConsolidator::consolidate_two(flat, |o| o.id)");
    println!("\n  Nested Results:");
    for (order, items) in &nested {
        println!("    Order #{} '{}' (${}):", order.id, order.customer_name, order.total);
        for item in items {
            println!("      └─ {} x {} @ ${}", item.quantity, item.product_name, item.price);
        }
    }
    
    println!("\nAvailable methods:");
    println!("  consolidate_two(flat, key_fn)");
    println!("    Vec<(A, B)> -> Vec<(A, Vec<B>)>\n");
    
    println!("  consolidate_two_optional(flat, key_fn)");
    println!("    Vec<(A, Option<B>)> -> Vec<(A, Vec<B>)>");
    println!("    (handles LEFT JOIN nulls)\n");
    
    println!("  consolidate_three(flat, key_a_fn, key_b_fn)");
    println!("    Vec<(A, B, C)> -> Vec<(A, Vec<(B, Vec<C>)>)>\n");
    
    println!("  consolidate_three_optional(flat, key_a_fn, key_b_fn)");
    println!("    Vec<(A, B, Option<C>)> -> Vec<(A, Vec<(B, Vec<C>)>)>");
}

// =============================================================================
// FEATURE 5: LINKED PARTIAL SELECT
// =============================================================================

fn demo_linked_partial_select() {
    println!("\n=== LINKED PARTIAL SELECT ===\n");
    
    println!("Select specific columns from main + related tables with auto-join:\n");
    
    println!("1. select_with_linked() - Choose columns from both tables:");
    println!("   User::query()");
    println!("       .select_with_linked::<Profile>(");
    println!("           &[\"id\", \"name\"],         // Local columns");
    println!("           &[\"bio\", \"avatar_url\"],  // Linked columns");
    println!("           \"user_id\"                 // Foreign key for join");
    println!("       )");
    println!("       .get::<(i64, String, String, Option<String>)>()");
    println!("       .await?;\n");
    
    println!("2. select_also_linked() - All local + specific linked columns:");
    println!("   User::query()");
    println!("       .select_also_linked::<Profile>(");
    println!("           &[\"bio\"],     // Just the linked columns you need");
    println!("           \"user_id\"");
    println!("       )");
    println!("       .get::<(User, String)>()");
    println!("       .await?;\n");
    
    println!("Benefits:");
    println!("  ✅ Only fetches columns you need (performance)");
    println!("  ✅ Auto-generates the JOIN clause");
    println!("  ✅ Type-safe column selection");
    println!("  ✅ Works with any related model");
}

// =============================================================================
// MAIN
// =============================================================================

fn main() {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║           TideORM - SeaORM 2.0 Features Demo                  ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    
    demo_strongly_typed_columns();
    demo_nested_active_model();
    demo_self_referencing_relations();
    demo_join_consolidation();
    demo_linked_partial_select();
    
    println!("\n╔═══════════════════════════════════════════════════════════════╗");
    println!("║                     Demo Complete!                             ║");
    println!("║                                                                 ║");
    println!("║  All SeaORM 2.0 features are now available in TideORM:        ║");
    println!("║  - Strongly-typed columns with compile-time safety            ║");
    println!("║  - Nested ActiveModel for cascade saves                       ║");
    println!("║  - Self-referencing relations for hierarchical data           ║");
    println!("║  - Join result consolidation                                   ║");
    println!("║  - Linked partial select                                       ║");
    println!("╚═══════════════════════════════════════════════════════════════╝\n");
}
