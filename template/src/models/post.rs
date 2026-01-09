//! Post Model
//! 
//! Comprehensive example with relationships, timestamps, and JSON fields.

use serde::{Deserialize, Serialize};
use tideorm::prelude::*;

use super::User;

/// Post model representing the `posts` table
/// 
/// Demonstrates:
/// - Belongs-to relationship
/// - Timestamps via `#[tide(timestamps)]` (created_at, updated_at auto-managed)
/// - JSON columns for flexible data (tags, metadata)
/// - Nullable fields
/// - Boolean flags
/// - Soft delete support via `#[tide(soft_delete)]`
#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "posts", timestamps, soft_delete)]
#[belongs_to(User, foreign_key = "user_id")]
#[index("user_id")]
#[index("slug")]
pub struct Post {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    
    /// Foreign key to users table
    pub user_id: i64,
    
    /// URL-friendly slug
    pub slug: String,
    
    /// Post title
    pub title: String,
    
    /// Post content (can be markdown, HTML, etc.)
    pub content: String,
    
    /// Optional excerpt/summary
    #[tide(nullable)]
    pub excerpt: Option<String>,
    
    /// Publication status
    pub published: bool,
    
    /// Optional publication timestamp
    #[tide(nullable)]
    pub published_at: Option<chrono::DateTime<chrono::Utc>>,
    
    /// View count
    pub view_count: i64,
    
    /// JSON array of tags
    #[tide(nullable)]
    pub tags: Option<serde_json::Value>,
    
    /// JSON object for flexible metadata
    #[tide(nullable)]
    pub metadata: Option<serde_json::Value>,
    
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    
    /// Last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
    
    /// Soft delete timestamp
    #[tide(nullable)]
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[allow(dead_code)]
impl Post {
    /// Create a new post instance
    pub fn new(user_id: i64, title: impl Into<String>, content: impl Into<String>) -> Self {
        let title = title.into();
        let slug = Self::slugify(&title);
        let now = chrono::Utc::now();
        
        Self {
            id: 0,
            user_id,
            slug,
            title,
            content: content.into(),
            excerpt: None,
            published: false,
            published_at: None,
            view_count: 0,
            tags: None,
            metadata: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        }
    }

    /// Create a post with tags
    pub fn with_tags(user_id: i64, title: impl Into<String>, content: impl Into<String>, tags: Vec<&str>) -> Self {
        let mut post = Self::new(user_id, title, content);
        post.tags = Some(serde_json::json!(tags));
        post
    }

    /// Generate URL-friendly slug from title
    fn slugify(title: &str) -> String {
        title
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Get all published posts
    pub async fn published() -> tideorm::Result<Vec<Self>> {
        Self::query()
            .where_eq("published", true)
            .order_by("published_at", Order::Desc)
            .get()
            .await
    }

    /// Get all draft posts
    pub async fn drafts() -> tideorm::Result<Vec<Self>> {
        Self::query()
            .where_eq("published", false)
            .order_by("updated_at", Order::Desc)
            .get()
            .await
    }

    /// Get posts by tag (searches JSON array)
    pub async fn with_tag(tag: &str) -> tideorm::Result<Vec<Self>> {
        Self::query()
            .where_like("tags", &format!("%\"{}%", tag))
            .get()
            .await
    }

    /// Get popular posts by view count
    pub async fn popular(limit: u64) -> tideorm::Result<Vec<Self>> {
        Self::query()
            .where_eq("published", true)
            .order_by("view_count", Order::Desc)
            .limit(limit)
            .get()
            .await
    }

    /// Find a post by slug
    pub async fn find_by_slug(slug: &str) -> tideorm::Result<Option<Self>> {
        Self::query()
            .where_eq("slug", slug)
            .first()
            .await
    }

    /// Get the author of this post
    pub async fn author(&self) -> tideorm::Result<Option<super::User>> {
        super::User::find(self.user_id).await
    }

    /// Get comments for this post
    pub async fn comments(&self) -> tideorm::Result<Vec<super::Comment>> {
        super::Comment::query()
            .where_eq("post_id", self.id)
            .order_by("created_at", Order::Asc)
            .get()
            .await
    }

    /// Publish this post
    pub async fn publish(mut self) -> tideorm::Result<Self> {
        self.published = true;
        self.published_at = Some(chrono::Utc::now());
        self.updated_at = chrono::Utc::now();
        self.save().await
    }

    /// Unpublish this post
    pub async fn unpublish(mut self) -> tideorm::Result<Self> {
        self.published = false;
        self.published_at = None;
        self.updated_at = chrono::Utc::now();
        self.save().await
    }

    /// Increment view count
    pub async fn increment_views(mut self) -> tideorm::Result<Self> {
        self.view_count += 1;
        self.save().await
    }

    /// Update tags
    pub async fn set_tags(mut self, tags: Vec<&str>) -> tideorm::Result<Self> {
        self.tags = Some(serde_json::json!(tags));
        self.updated_at = chrono::Utc::now();
        self.save().await
    }

    /// Set excerpt from content
    pub fn with_excerpt(mut self, max_length: usize) -> Self {
        let excerpt = if self.content.len() > max_length {
            format!("{}...", &self.content[..max_length])
        } else {
            self.content.clone()
        };
        self.excerpt = Some(excerpt);
        self
    }
}
