//! Tag Model
//! 
//! Example demonstrating simple lookup table patterns.

use serde::{Deserialize, Serialize};
use tideorm::prelude::*;

/// Tag model representing the `tags` table
/// 
/// Demonstrates:
/// - Simple lookup table
/// - Unique constraints
/// - Slug generation
#[derive(Model, Clone, Debug, Serialize, Deserialize)]
#[tide(table = "tags")]
#[unique_index("name")]
#[unique_index("slug")]
pub struct Tag {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    
    /// Tag name (display)
    pub name: String,
    
    /// URL-friendly slug
    pub slug: String,
    
    /// Optional description
    #[tide(nullable)]
    pub description: Option<String>,
    
    /// Usage count (denormalized for performance)
    pub usage_count: i64,
    
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[allow(dead_code)]
impl Tag {
    /// Create a new tag
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let slug = Self::slugify(&name);
        Self {
            id: 0,
            name,
            slug,
            description: None,
            usage_count: 0,
            created_at: chrono::Utc::now(),
        }
    }

    /// Generate slug from name
    fn slugify(name: &str) -> String {
        name.to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Find or create a tag by name
    pub async fn find_or_create(name: &str) -> tideorm::Result<Self> {
        if let Some(tag) = Self::find_by_name(name).await? {
            Ok(tag)
        } else {
            Self::new(name).save().await
        }
    }

    /// Find a tag by name
    pub async fn find_by_name(name: &str) -> tideorm::Result<Option<Self>> {
        Self::query()
            .where_eq("name", name)
            .first()
            .await
    }

    /// Find a tag by slug
    pub async fn find_by_slug(slug: &str) -> tideorm::Result<Option<Self>> {
        Self::query()
            .where_eq("slug", slug)
            .first()
            .await
    }

    /// Get popular tags
    pub async fn popular(limit: u64) -> tideorm::Result<Vec<Self>> {
        Self::query()
            .order_by("usage_count", Order::Desc)
            .limit(limit)
            .get()
            .await
    }

    /// Get all tags ordered by name
    pub async fn all_sorted() -> tideorm::Result<Vec<Self>> {
        Self::query()
            .order_by("name", Order::Asc)
            .get()
            .await
    }

    /// Increment usage count
    pub async fn increment_usage(mut self) -> tideorm::Result<Self> {
        self.usage_count += 1;
        self.save().await
    }

    /// Decrement usage count
    pub async fn decrement_usage(mut self) -> tideorm::Result<Self> {
        if self.usage_count > 0 {
            self.usage_count -= 1;
        }
        self.save().await
    }

    /// Search tags by name prefix
    pub async fn search(query: &str) -> tideorm::Result<Vec<Self>> {
        Self::query()
            .where_like("name", &format!("{}%", query))
            .order_by("usage_count", Order::Desc)
            .limit(10)
            .get()
            .await
    }
}
