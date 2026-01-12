//! SQLite Demo
//!
//! This example demonstrates TideORM with SQLite database.
//!
//! SQLite is great for:
//! - Development and testing
//! - Single-user applications
//! - Embedded databases
//! - Prototyping
//!
//! ## Running
//!
//! ```bash
//! cargo run --example sqlite_demo --features "sqlite runtime-tokio" --no-default-features
//! ```

use tideorm::prelude::*;

#[tideorm::model]
#[tide(table = "users")]
#[index("email")]
#[unique_index("email")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub active: bool,
    pub age: Option<i32>,
}

#[tideorm::model]
#[tide(table = "notes")]
pub struct Note {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub content: String,
    pub user_id: i64,
    /// JSON column using SQLite JSON1 extension
    #[tide(nullable)]
    pub tags: Option<serde_json::Value>,
}

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    println!("🪶 TideORM SQLite Demo\n");

    // SQLite database file (creates if not exists)
    let database_url = "sqlite://./demo.db?mode=rwc";

    println!("Connecting to SQLite...");

    // Initialize TideORM with SQLite
    TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .database(database_url)
        .max_connections(5) // SQLite works best with fewer connections
        .min_connections(1)
        .sync(true) // Auto-sync schema
        .models::<(User, Note)>()  // Register models for sync
        .connect()
        .await?;

    println!("✓ Connected successfully!\n");

    // Check database features
    let db_type = Database::global().backend();
    println!("Database Features:");
    println!("  - JSON support: {} (JSON1 extension)", db_type.supports_json());
    println!("  - Arrays: {} (use JSON instead)", db_type.supports_arrays());
    println!("  - RETURNING clause: {} (SQLite 3.35+)", db_type.supports_returning());
    println!("  - Upsert: {}", db_type.supports_upsert());
    println!("  - Window functions: {} (SQLite 3.25+)", db_type.supports_window_functions());
    println!("  - CTEs: {} (SQLite 3.8+)", db_type.supports_cte());
    println!("  - Parameter style: {}", db_type.param_style());
    println!("  - Optimal batch size: {}", db_type.optimal_batch_size());
    println!();

    // =====================
    // CRUD Operations
    // =====================

    println!("=== CRUD Operations ===\n");

    // Create user
    let user = User {
        id: 0,
        email: "alice@example.com".to_string(),
        name: "Alice".to_string(),
        active: true,
        age: Some(25),
    };
    println!("Creating user: {:?}", user);
    let user = user.save().await?;
    println!("✓ Created user with ID: {}\n", user.id);

    // Create notes for user
    let note1 = Note {
        id: 0,
        title: "First Note".to_string(),
        content: "This is my first note".to_string(),
        user_id: user.id,
        tags: Some(serde_json::json!(["personal", "important"])),
    };
    let note1 = note1.save().await?;
    println!("✓ Created note: {}\n", note1.title);

    let note2 = Note {
        id: 0,
        title: "Shopping List".to_string(),
        content: "Milk, eggs, bread".to_string(),
        user_id: user.id,
        tags: Some(serde_json::json!(["shopping", "todo"])),
    };
    note2.save().await?;

    let note3 = Note {
        id: 0,
        title: "Work Todo".to_string(),
        content: "Finish the report".to_string(),
        user_id: user.id,
        tags: Some(serde_json::json!(["work", "todo", "important"])),
    };
    note3.save().await?;

    // Read
    let found = User::find(user.id).await?;
    println!("Found user: {:?}\n", found);

    // Query notes
    let user_notes = Note::query()
        .where_eq("user_id", user.id)
        .order_by("title", Order::Asc)
        .get()
        .await?;
    println!("User has {} notes:", user_notes.len());
    for note in &user_notes {
        println!("  - {} (tags: {:?})", note.title, note.tags);
    }
    println!();

    // Update
    let mut user = user;
    user.name = "Alice Smith".to_string();
    user.age = Some(26);
    let user = user.update().await?;
    println!("✓ Updated user: {:?}\n", user);

    // =====================
    // JSON Operations (SQLite JSON1)
    // =====================

    println!("=== JSON Operations ===\n");

    // Query notes with specific tags using JSON1
    // Note: SQLite JSON queries use json_each and json_extract
    println!("Querying notes with 'important' tag...");
    let important_notes = Note::query()
        .where_json_contains("tags", serde_json::json!("important"))
        .get()
        .await?;
    println!("Found {} important notes:", important_notes.len());
    for note in &important_notes {
        println!("  - {}", note.title);
    }
    println!();

    // =====================
    // Pattern Matching
    // =====================

    println!("=== Pattern Matching ===\n");

    let todo_notes = Note::query()
        .where_like("title", "%Todo%")
        .get()
        .await?;
    println!("Notes with 'Todo' in title: {}", todo_notes.len());

    let contains_list = Note::query()
        .where_like("content", "%list%")
        .get()
        .await?;
    println!("Notes with 'list' in content: {}\n", contains_list.len());

    // =====================
    // Aggregations
    // =====================

    println!("=== Aggregations ===\n");

    let count = Note::query().count().await?;
    println!("Total notes: {}", count);

    let user_count = User::query().count().await?;
    println!("Total users: {}\n", user_count);

    // =====================
    // Pagination
    // =====================

    println!("=== Pagination ===\n");

    let page1 = Note::query()
        .order_by("id", Order::Asc)
        .limit(2)
        .offset(0)
        .get()
        .await?;
    println!("First 2 notes:");
    for note in &page1 {
        println!("  - {}", note.title);
    }

    let page2 = Note::query()
        .order_by("id", Order::Asc)
        .limit(2)
        .offset(2)
        .get()
        .await?;
    println!("Next 2 notes:");
    for note in &page2 {
        println!("  - {}", note.title);
    }
    println!();

    // =====================
    // Existence Checks
    // =====================

    println!("=== Existence Checks ===\n");

    let exists = User::query()
        .where_eq("email", "alice@example.com")
        .exists()
        .await?;
    println!("Alice exists: {}", exists);

    let exists = User::query()
        .where_eq("email", "bob@example.com")
        .exists()
        .await?;
    println!("Bob exists: {}\n", exists);

    // =====================
    // First/Last Records
    // =====================

    println!("=== First/Last Records ===\n");

    let first_note = Note::query()
        .order_by("id", Order::Asc)
        .first()
        .await?;
    println!("First note: {:?}", first_note.map(|n| n.title));

    let latest_note = Note::query()
        .order_by("id", Order::Desc)
        .first()
        .await?;
    println!("Latest note: {:?}\n", latest_note.map(|n| n.title));

    // =====================
    // Cleanup
    // =====================

    println!("=== Cleanup ===\n");

    // Bulk delete notes
    let deleted_notes = Note::query().delete().await?;
    println!("Deleted {} notes", deleted_notes);

    // Delete user
    User::destroy(user.id).await?;
    println!("Deleted test user\n");

    // Optionally remove the database file
    println!("Note: SQLite database file 'demo.db' was created.");
    println!("You can delete it manually if desired.\n");

    println!("✓ SQLite demo completed successfully!");

    Ok(())
}
