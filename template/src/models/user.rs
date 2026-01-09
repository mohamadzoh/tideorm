//! User Model
//! 
//! Comprehensive example demonstrating TideORM model features.

use serde::{Deserialize, Serialize};
use tideorm::prelude::*;

use super::Post;

/// User model representing the `users` table
/// 
/// Demonstrates:
/// - Primary key with auto increment
/// - Timestamps (created_at, updated_at) via `#[tide(timestamps)]`
/// - Nullable fields
/// - Hidden fields (password_hash won't be serialized)
/// - Indexes for performance
/// - Has-many relationship to posts
#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "users", timestamps, hidden = "password_hash")]
#[has_many(Post, foreign_key = "user_id")]
#[index("email")]
#[unique_index("email")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    
    pub name: String,
    
    pub email: String,
    
    /// User status: active, inactive, banned
    pub status: String,
    
    /// Optional bio/description
    #[tide(nullable)]
    pub bio: Option<String>,
    
    /// Password hash - hidden from serialization
    #[tide(nullable)]
    pub password_hash: Option<String>,
    
    /// Account creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    
    /// Last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[allow(dead_code)]
impl User {
    /// Create a new user instance (not yet saved)
    pub fn new(name: impl Into<String>, email: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: 0,
            name: name.into(),
            email: email.into(),
            status: "active".to_string(),
            bio: None,
            password_hash: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a user with password
    pub fn with_password(name: impl Into<String>, email: impl Into<String>, password_hash: impl Into<String>) -> Self {
        let mut user = Self::new(name, email);
        user.password_hash = Some(password_hash.into());
        user
    }

    /// Find a user by email
    pub async fn find_by_email(email: &str) -> tideorm::Result<Option<Self>> {
        Self::query()
            .where_eq("email", email)
            .first()
            .await
    }

    /// Get all active users
    pub async fn active() -> tideorm::Result<Vec<Self>> {
        Self::query()
            .where_eq("status", "active")
            .order_by("created_at", Order::Desc)
            .get()
            .await
    }

    /// Get all posts for this user
    pub async fn posts(&self) -> tideorm::Result<Vec<super::Post>> {
        super::Post::query()
            .where_eq("user_id", self.id)
            .order_by("created_at", Order::Desc)
            .get()
            .await
    }

    /// Get published posts for this user
    pub async fn published_posts(&self) -> tideorm::Result<Vec<super::Post>> {
        super::Post::query()
            .where_eq("user_id", self.id)
            .where_eq("published", true)
            .order_by("published_at", Order::Desc)
            .get()
            .await
    }

    /// Update the user's bio
    pub async fn update_bio(mut self, bio: impl Into<String>) -> tideorm::Result<Self> {
        self.bio = Some(bio.into());
        self.updated_at = chrono::Utc::now();
        self.save().await
    }

    /// Deactivate the user
    pub async fn deactivate(mut self) -> tideorm::Result<Self> {
        self.status = "inactive".to_string();
        self.updated_at = chrono::Utc::now();
        self.save().await
    }

    /// Count posts by this user
    pub async fn post_count(&self) -> tideorm::Result<u64> {
        Post::query()
            .where_eq("user_id", self.id)
            .count()
            .await
    }
}
