//! TideORM Application Template
//! 
//! A comprehensive example demonstrating TideORM features:
//! - CRUD operations
//! - Relationships (has_many, belongs_to)
//! - Timestamps (created_at, updated_at)
//! - JSON columns
//! - Soft deletes
//! - Query builder
//! - Pagination
//! - Transactions
//! 
//! Run with: cargo run

mod models;

use crate::models::{User, Post, Comment, Tag};
use tideorm::prelude::*;

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    tracing::info!("🌊 TideORM App Starting...");

    // Load environment variables
    dotenvy::dotenv().ok();

    // Get database URL from environment or use default
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://./data/app.db?mode=rwc".to_string());

    tracing::info!("Connecting to database...");

    // Initialize TideORM with all models registered
    TideConfig::init()
        .database_type(DatabaseType::SQLite)
        .database(&database_url)
        .max_connections(5)
        .min_connections(1)
        .models::<(User, Post, Comment, Tag)>()  // Register all models
        .sync(true) // Auto-sync schema (dev only!)
        .connect()
        .await?;

    tracing::info!("✓ Connected successfully!\n");

    // Run the demo
    run_demo().await?;

    tracing::info!("✅ TideORM App completed successfully!");
    
    Ok(())
}

async fn run_demo() -> tideorm::Result<()> {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           TideORM Feature Demonstration                      ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // ========================================
    // 1. CREATE - Insert new records
    // ========================================
    println!("━━━ 1. CREATE Operations ━━━\n");

    // Create a user with timestamps
    let user = User::new("Alice Johnson", "alice@example.com");
    let user = user.save().await?;
    println!("✓ Created user: {} (ID: {})", user.name, user.id);
    println!("  Created at: {}", user.created_at);

    // Create another user with password
    let admin = User::with_password("Admin", "admin@example.com", "hashed_password_here");
    let admin = admin.save().await?;
    println!("✓ Created admin: {} (ID: {})", admin.name, admin.id);

    // Create posts with tags (JSON)
    let post1 = Post::with_tags(
        user.id,
        "Getting Started with TideORM",
        "TideORM is a powerful ORM for Rust...",
        vec!["rust", "tutorial", "orm"],
    ).with_excerpt(50);
    let post1 = post1.save().await?;
    println!("✓ Created post: {} (ID: {})", post1.title, post1.id);
    println!("  Slug: {}", post1.slug);
    println!("  Tags: {:?}", post1.tags);

    let post2 = Post::new(
        user.id,
        "Advanced Query Patterns",
        "Learn how to build complex queries...",
    );
    let post2 = post2.save().await?;
    println!("✓ Created post: {} (ID: {})", post2.title, post2.id);

    // Create comments with nested replies
    let comment1 = Comment::new(post1.id, admin.id, "Great article! Very helpful.");
    let comment1 = comment1.save().await?;
    let comment1 = comment1.approve().await?;
    println!("✓ Created comment (ID: {})", comment1.id);

    let reply = Comment::reply(&comment1, user.id, "Thanks for the feedback!");
    let reply = reply.save().await?;
    let reply = reply.approve().await?;
    println!("✓ Created reply to comment (ID: {})", reply.id);

    // Create tags
    let tag1 = Tag::new("Rust").save().await?;
    let _tag2 = Tag::new("Database").save().await?;
    let _tag3 = Tag::new("Tutorial").save().await?;
    println!("✓ Created {} tags", 3);

    println!();

    // ========================================
    // 2. READ - Query records
    // ========================================
    println!("━━━ 2. READ Operations ━━━\n");

    // Find by ID
    let found_user = User::find(user.id).await?;
    println!("✓ Found user by ID: {:?}", found_user.map(|u| u.name));

    // Find by email (custom method)
    let found_by_email = User::find_by_email("alice@example.com").await?;
    println!("✓ Found user by email: {:?}", found_by_email.map(|u| u.email));

    // Get all users
    let all_users = User::all().await?;
    println!("✓ Total users: {}", all_users.len());

    // Query with conditions
    let active_users = User::query()
        .where_eq("status", "active")
        .order_by("created_at", Order::Desc)
        .get()
        .await?;
    println!("✓ Active users: {}", active_users.len());

    // Complex query with multiple conditions
    let recent_posts = Post::query()
        .where_eq("user_id", user.id)
        .where_eq("published", false)
        .order_by("created_at", Order::Desc)
        .limit(10)
        .get()
        .await?;
    println!("✓ User's draft posts: {}", recent_posts.len());

    // Pagination
    let page = 1;
    let per_page = 10;
    let paginated = Post::query()
        .order_by("created_at", Order::Desc)
        .limit(per_page)
        .offset((page - 1) * per_page)
        .get()
        .await?;
    println!("✓ Page {} posts: {}", page, paginated.len());

    // Count query
    let post_count = Post::query().count().await?;
    println!("✓ Total posts: {}", post_count);

    // Get posts for user (relationship)
    let user_posts = user.posts().await?;
    println!("✓ User's posts: {}", user_posts.len());

    println!();

    // ========================================
    // 3. UPDATE - Modify records
    // ========================================
    println!("━━━ 3. UPDATE Operations ━━━\n");

    // Update user bio
    let user = User::find(user.id).await?.unwrap();
    let user = user.update_bio("Rust developer and TideORM enthusiast").await?;
    println!("✓ Updated user bio: {:?}", user.bio);
    println!("  Updated at: {}", user.updated_at);

    // Publish a post
    let post1 = Post::find(post1.id).await?.unwrap();
    let post1 = post1.publish().await?;
    println!("✓ Published post: {}", post1.title);
    println!("  Published at: {:?}", post1.published_at);

    // Update tags on post
    let post1 = post1.set_tags(vec!["rust", "tutorial", "orm", "database"]).await?;
    println!("✓ Updated post tags: {:?}", post1.tags);

    // Increment view count
    let post1 = post1.increment_views().await?;
    let post1 = post1.increment_views().await?;
    let post1 = post1.increment_views().await?;
    println!("✓ Post views: {}", post1.view_count);

    // Like a comment
    let comment1 = Comment::find(comment1.id).await?.unwrap();
    let comment1 = comment1.like().await?;
    let comment1 = comment1.like().await?;
    println!("✓ Comment likes: {}", comment1.likes);

    // Increment tag usage
    let tag1 = tag1.increment_usage().await?;
    let tag1 = tag1.increment_usage().await?;
    println!("✓ Tag '{}' usage: {}", tag1.name, tag1.usage_count);

    println!();

    // ========================================
    // 4. QUERY BUILDER - Advanced queries
    // ========================================
    println!("━━━ 4. QUERY BUILDER Examples ━━━\n");

    // WHERE with multiple conditions
    let filtered = Post::query()
        .where_eq("published", true)
        .where_gt("view_count", 0)
        .get()
        .await?;
    println!("✓ Published posts with views: {}", filtered.len());

    // LIKE query
    let search_results = Post::query()
        .where_like("title", "%TideORM%")
        .get()
        .await?;
    println!("✓ Posts matching 'TideORM': {}", search_results.len());

    // ORDER BY multiple columns
    let sorted = User::query()
        .order_by("status", Order::Asc)
        .order_by("created_at", Order::Desc)
        .get()
        .await?;
    println!("✓ Users sorted by status, then date: {}", sorted.len());

    // NULL checks
    let with_bio = User::query()
        .where_not_null("bio")
        .get()
        .await?;
    println!("✓ Users with bio: {}", with_bio.len());

    // First/Last
    let first_user = User::query()
        .order_by("created_at", Order::Asc)
        .first()
        .await?;
    println!("✓ First user: {:?}", first_user.map(|u| u.name));

    // Popular tags
    let popular_tags = Tag::popular(5).await?;
    println!("✓ Popular tags: {:?}", popular_tags.iter().map(|t| &t.name).collect::<Vec<_>>());

    println!();

    // ========================================
    // 5. RELATIONSHIPS
    // ========================================
    println!("━━━ 5. RELATIONSHIP Examples ━━━\n");

    // Get post author (belongs_to)
    let post = Post::find(post1.id).await?.unwrap();
    let author = post.author().await?;
    println!("✓ Post author: {:?}", author.map(|u| u.name));

    // Get post comments (has_many through Post method)
    let comments = post.comments().await?;
    println!("✓ Post comments: {}", comments.len());

    // Get comment replies
    let comment = Comment::find(comment1.id).await?.unwrap();
    let replies = comment.replies().await?;
    println!("✓ Comment replies: {}", replies.len());

    // Get user's published posts
    let user = User::find(user.id).await?.unwrap();
    let published = user.published_posts().await?;
    println!("✓ User's published posts: {}", published.len());

    // Count user's posts
    let count = user.post_count().await?;
    println!("✓ User's total post count: {}", count);

    println!();

    // ========================================
    // 6. SOFT DELETES
    // ========================================
    println!("━━━ 6. SOFT DELETE Examples ━━━\n");

    // Create a post to soft delete
    let temp_post = Post::new(user.id, "Temporary Post", "This will be deleted");
    let temp_post = temp_post.save().await?;
    println!("✓ Created temporary post (ID: {})", temp_post.id);

    // Soft delete (sets deleted_at)
    temp_post.delete().await?;
    println!("✓ Soft deleted post");

    // Regular queries exclude soft-deleted
    let all_posts = Post::all().await?;
    println!("✓ Posts (excluding deleted): {}", all_posts.len());

    println!();

    // ========================================
    // 7. SUMMARY
    // ========================================
    println!("━━━ Summary ━━━\n");
    
    let user_count = User::query().count().await?;
    let post_count = Post::query().count().await?;
    let comment_count = Comment::query().count().await?;
    let tag_count = Tag::query().count().await?;
    
    println!("Database Statistics:");
    println!("  • Users: {}", user_count);
    println!("  • Posts: {}", post_count);
    println!("  • Comments: {}", comment_count);
    println!("  • Tags: {}", tag_count);

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                    Demo Complete! 🎉                         ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    Ok(())
}
