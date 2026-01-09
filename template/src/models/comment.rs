//! Comment Model
//! 
//! Example demonstrating nested relationships and soft deletes.

use serde::{Deserialize, Serialize};
use tideorm::prelude::*;

use super::{Post, User};

/// Comment model representing the `comments` table
/// 
/// Demonstrates:
/// - Multiple belongs-to relationships
/// - Self-referential relationship (parent comment for replies)
/// - Soft delete support
/// - Timestamps
#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "comments", soft_delete)]
#[belongs_to(User, foreign_key = "user_id")]
#[belongs_to(Post, foreign_key = "post_id")]
#[index("post_id")]
#[index("user_id")]
#[index("parent_id")]
pub struct Comment {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    
    /// Foreign key to posts table
    pub post_id: i64,
    
    /// Foreign key to users table
    pub user_id: i64,
    
    /// Parent comment ID for replies (null for top-level comments)
    #[tide(nullable)]
    pub parent_id: Option<i64>,
    
    /// Comment content
    pub content: String,
    
    /// Approval status
    pub approved: bool,
    
    /// Like count
    pub likes: i64,
    
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    
    /// Last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
    
    /// Soft delete timestamp
    #[tide(nullable)]
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[allow(dead_code)]
impl Comment {
    /// Create a new comment
    pub fn new(post_id: i64, user_id: i64, content: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: 0,
            post_id,
            user_id,
            parent_id: None,
            content: content.into(),
            approved: false,
            likes: 0,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        }
    }

    /// Create a reply to another comment
    pub fn reply(parent: &Comment, user_id: i64, content: impl Into<String>) -> Self {
        let mut comment = Self::new(parent.post_id, user_id, content);
        comment.parent_id = Some(parent.id);
        comment
    }

    /// Get approved comments for a post
    pub async fn for_post(post_id: i64) -> tideorm::Result<Vec<Self>> {
        Self::query()
            .where_eq("post_id", post_id)
            .where_eq("approved", true)
            .where_null("parent_id") // Top-level comments only
            .order_by("created_at", Order::Asc)
            .get()
            .await
    }

    /// Get replies to this comment
    pub async fn replies(&self) -> tideorm::Result<Vec<Self>> {
        Self::query()
            .where_eq("parent_id", self.id)
            .where_eq("approved", true)
            .order_by("created_at", Order::Asc)
            .get()
            .await
    }

    /// Get the comment author
    pub async fn author(&self) -> tideorm::Result<Option<super::User>> {
        super::User::find(self.user_id).await
    }

    /// Get the parent post
    pub async fn post(&self) -> tideorm::Result<Option<super::Post>> {
        super::Post::find(self.post_id).await
    }

    /// Approve this comment
    pub async fn approve(mut self) -> tideorm::Result<Self> {
        self.approved = true;
        self.updated_at = chrono::Utc::now();
        self.save().await
    }

    /// Increment like count
    pub async fn like(mut self) -> tideorm::Result<Self> {
        self.likes += 1;
        self.save().await
    }

    /// Get pending (unapproved) comments
    pub async fn pending() -> tideorm::Result<Vec<Self>> {
        Self::query()
            .where_eq("approved", false)
            .order_by("created_at", Order::Asc)
            .get()
            .await
    }

    /// Count comments for a post
    pub async fn count_for_post(post_id: i64) -> tideorm::Result<u64> {
        Self::query()
            .where_eq("post_id", post_id)
            .where_eq("approved", true)
            .count()
            .await
    }
}
