use tideorm::prelude::*;

#[derive(Model)]
#[tide(table = "users")]
#[index("email")]
#[index("active")]
#[unique_index("email")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    #[tide(nullable)]
    pub bio: Option<String>,
    pub active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Model)]
#[tide(table = "posts")]
#[index("user_id")]
#[index(name = "idx_user_published", columns = "user_id,published")]
pub struct Post {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub body: String,
    pub published: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("POSTGRESQL_DATABASE_URL").unwrap();
    
    match TideConfig::init()
        .database_type(DatabaseType::Postgres)
        .database(&db_url)
        .max_connections(10)
        .min_connections(2)
        .sync(true)
        .models::<(User, Post)>()
        .languages(&["en", "fr"])
        .connect()
        .await
    {
        Ok(_) => {
            println!("✓ TideORM initialized!");
        }
        Err(e) => {
            println!("Could not connect to database: {}", e);
            println!("\nThis example shows the TideORM API - no database needed to compile!");
            println!("\nExample API usage:\n");
            print_api_examples();
            return Ok(());
        }
    };
    
    let unique_email = format!("john{}@example.com", chrono::Utc::now().timestamp_millis());
    
    let user = User {
        id: 0,
        email: unique_email,
        name: "John Doe".to_string(),
        bio: Some("Hello, I'm John!".to_string()),
        active: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    
    let user = user.save().await?;
    println!("Created user: {:?}", user);
    
    let all_users = User::all().await?;
    println!("Total users: {}", all_users.len());
    
    let active_users = User::query()
        .where_eq("active", true)
        .order_by("name", Order::Asc)
        .limit(10)
        .get()
        .await?;
    
    println!("Active users: {}", active_users.len());
    
    if let Some(mut user) = User::query().first().await? {
        user.name = "John Smith".to_string();
        let _user = user.update().await?;
        println!("Updated user!");
    }
    
    let deleted = User::query()
        .where_eq("active", false)
        .delete()
        .await?;
    
    println!("Deleted {} inactive users", deleted);

    println!("\n✓ Example completed successfully!");
    Ok(())
}

fn print_api_examples() {
    println!(r#"
// Model Definition:
#[derive(Model)]
#[tide(table = "users")]
pub struct User {{
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
}}

// Initialize database (once at startup):
Database::init("postgres://localhost/mydb").await?;

// Create:
let user = User {{ id: 0, email: "test@example.com".into(), name: "Test".into() }};
let user = user.save().await?;

// Read:
let users = User::all().await?;
let user = User::find(1).await?;
let users = User::query().where_eq("active", true).get().await?;

// Update:
user.name = "New Name".into();
let user = user.update().await?;

// Delete:
user.delete().await?;
User::destroy(1).await?;
"#);
}

// =============================================================================
// KEY POINTS
// =============================================================================
//
// 1. NO SeaORM types are visible in user code:
//    - No `Entity`, `Model`, `ActiveModel`
//    - No `DbConn`, `DatabaseConnection`
//    - No `sea_orm::*` imports
//
// 2. Clean, expressive API:
//    - `Database::init()` - initialize once
//    - `User::all()` - get all records
//    - `User::find(1)` - find by id
//    - `user.save()` - insert record
//    - `user.update()` - update record
//    - `user.delete()` - delete record
//
// 3. Fluent query builder:
//    - `User::query().where_eq().order_by().get()`
//
// 4. Type-safe without verbosity
//
// 5. Async-first design
//
// 6. User-friendly errors
