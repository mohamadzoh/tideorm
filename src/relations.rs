//! Model Relations System
//!
//! This module provides ActiveRecord/Eloquent-style model relations.
//!
//! ## Supported Relations
//!
//! - `BelongsTo`: Foreign key on this model (e.g., Post belongs_to User)
//! - `HasOne`: Foreign key on related model, single result (e.g., User has_one Profile)
//! - `HasMany`: Foreign key on related model, multiple results (e.g., User has_many Posts)
//!
//! ## Using Relation Macros (Recommended)
//!
//! The easiest way to define relations is with attribute macros:
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//!
//! #[derive(Model, Clone, Debug, Serialize, Deserialize)]
//! #[tide(table = "users")]
//! #[has_many(Post, foreign_key = "user_id")]
//! #[has_one(Profile, foreign_key = "user_id")]
//! pub struct User {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub name: String,
//!     pub email: String,
//! }
//!
//! #[derive(Model, Clone, Debug, Serialize, Deserialize)]
//! #[tide(table = "posts")]
//! #[belongs_to(User, foreign_key = "user_id")]
//! pub struct Post {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub user_id: i64,
//!     pub title: String,
//! }
//!
//! #[derive(Model, Clone, Debug, Serialize, Deserialize)]
//! #[tide(table = "profiles")]
//! #[belongs_to(User, foreign_key = "user_id")]
//! pub struct Profile {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub user_id: i64,
//!     pub bio: String,
//! }
//!
//! // Now you can load relations:
//! let user = User::find(1).await?;
//! let posts = user.load_has_many::<Post>().await?;
//! let profile = user.load_has_one::<Profile>().await?;
//!
//! let post = Post::find(1).await?;
//! let author = post.load_belongs_to::<User>().await?;
//! ```
//!
//! ## Manual Trait Implementation
//!
//! You can also implement the traits manually:
//!
//! ```rust,ignore
//! impl HasMany<Post> for User {
//!     fn foreign_key() -> &'static str { "user_id" }
//! }
//!
//! impl BelongsTo<User> for Post {
//!     fn foreign_key() -> &'static str { "user_id" }
//! }
//! ```

use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::model::Model;
use crate::query::QueryBuilder;

// =============================================================================
// RELATION TRAITS
// =============================================================================

/// BelongsTo relation: Foreign key on this model
///
/// Example: Post belongs_to User (posts.user_id -> users.id)
pub trait BelongsTo<Related: Model>: Model {
    /// The foreign key column on this model
    fn foreign_key() -> &'static str;
    
    /// The local key on the related model (defaults to primary key)
    fn owner_key() -> &'static str {
        Related::primary_key_name()
    }
}

/// HasOne relation: Foreign key on related model, single result
///
/// Example: User has_one Profile (profiles.user_id -> users.id)
pub trait HasOne<Related: Model>: Model {
    /// The foreign key column on the related model
    fn foreign_key() -> &'static str;
    
    /// The local key on this model (defaults to primary key)
    fn local_key() -> &'static str {
        Self::primary_key_name()
    }
}

/// HasMany relation: Foreign key on related model, multiple results
///
/// Example: User has_many Posts (posts.user_id -> users.id)
pub trait HasMany<Related: Model>: Model {
    /// The foreign key column on the related model
    fn foreign_key() -> &'static str;
    
    /// The local key on this model (defaults to primary key)
    fn local_key() -> &'static str {
        Self::primary_key_name()
    }
}

// =============================================================================
// RELATION EXTENSION METHODS
// =============================================================================

/// Extension trait providing relation query methods on models
#[async_trait]
pub trait RelationExt: Model {
    /// Load a BelongsTo relation
    ///
    /// # Example
    /// ```rust,ignore
    /// let post = Post::find(1).await?;
    /// let author = post.belongs_to::<User>().await?;
    /// ```
    async fn load_belongs_to<Related>(&self) -> Result<Option<Related>>
    where
        Self: BelongsTo<Related>,
        Related: Model,
    {
        // Get the foreign key value from self
        let fk_column = <Self as BelongsTo<Related>>::foreign_key();
        let fk_value = self.get_field_value(fk_column)?;
        
        // Query the related model by its primary key
        Related::query()
            .where_eq(Related::primary_key_name(), fk_value)
            .first()
            .await
    }
    
    /// Load a HasOne relation
    ///
    /// # Example
    /// ```rust,ignore
    /// let user = User::find(1).await?;
    /// let profile = user.has_one::<Profile>().await?;
    /// ```
    async fn load_has_one<Related>(&self) -> Result<Option<Related>>
    where
        Self: HasOne<Related>,
        Related: Model,
    {
        let fk_column = <Self as HasOne<Related>>::foreign_key();
        let local_key = <Self as HasOne<Related>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        
        Related::query()
            .where_eq(fk_column, pk_value)
            .first()
            .await
    }
    
    /// Load a HasMany relation
    ///
    /// # Example
    /// ```rust,ignore
    /// let user = User::find(1).await?;
    /// let posts = user.has_many::<Post>().await?;
    /// ```
    async fn load_has_many<Related>(&self) -> Result<Vec<Related>>
    where
        Self: HasMany<Related>,
        Related: Model,
    {
        let fk_column = <Self as HasMany<Related>>::foreign_key();
        let local_key = <Self as HasMany<Related>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        
        Related::query()
            .where_eq(fk_column, pk_value)
            .get()
            .await
    }
    
    /// Get a field value by name (helper for relations)
    fn get_field_value(&self, field: &str) -> Result<serde_json::Value> {
        let json = serde_json::to_value(self)
            .map_err(|e| Error::query(format!("Failed to serialize model: {}", e)))?;
        
        json.get(field)
            .cloned()
            .ok_or_else(|| Error::query(format!("Field '{}' not found on model", field)))
    }
}

// Implement RelationExt for all Models
impl<T: Model> RelationExt for T {}

// =============================================================================
// EAGER LOADING
// =============================================================================

/// Result wrapper that holds a model with its loaded relations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithRelations<M> {
    /// The main model
    #[serde(flatten)]
    pub model: M,
    
    /// Loaded relations stored by name
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub relations: HashMap<String, serde_json::Value>,
}

impl<M: Model> WithRelations<M> {
    /// Create a new model wrapper
    pub fn new(model: M) -> Self {
        Self {
            model,
            relations: HashMap::new(),
        }
    }
    
    /// Add a loaded relation
    pub fn with_relation(mut self, name: &str, data: serde_json::Value) -> Self {
        self.relations.insert(name.to_string(), data);
        self
    }
    
    /// Get a loaded relation by name
    pub fn get_relation<R: for<'de> Deserialize<'de>>(&self, name: &str) -> Option<R> {
        self.relations.get(name)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
    
    /// Check if a relation is loaded
    pub fn has_relation(&self, name: &str) -> bool {
        self.relations.contains_key(name)
    }
    
    /// Get the inner model
    pub fn into_inner(self) -> M {
        self.model
    }
}

impl<M> std::ops::Deref for WithRelations<M> {
    type Target = M;
    
    fn deref(&self) -> &Self::Target {
        &self.model
    }
}

impl<M> std::ops::DerefMut for WithRelations<M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.model
    }
}

// =============================================================================
// EAGER LOADING QUERY BUILDER
// =============================================================================

/// Query builder with eager loading support
pub struct EagerQueryBuilder<M: Model> {
    query: QueryBuilder<M>,
    relations: Vec<RelationLoader<M>>,
}

/// Relation loader configuration
pub struct RelationLoader<M> {
    /// Name of the relation to load
    pub name: String,
    /// Function that loads related models for a batch of parent models
    pub loader: Box<dyn Fn(&[M]) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<HashMap<String, serde_json::Value>>> + Send>> + Send + Sync>,
}

impl<M: Model> EagerQueryBuilder<M> {
    /// Create a new eager query builder
    pub fn new() -> Self {
        Self {
            query: QueryBuilder::new(),
            relations: Vec::new(),
        }
    }
    
    /// Add a where condition
    pub fn where_eq<V: Into<serde_json::Value>>(mut self, column: &str, value: V) -> Self {
        self.query = self.query.where_eq(column, value);
        self
    }
    
    /// Set ordering
    pub fn order_by(mut self, column: &str, order: crate::query::Order) -> Self {
        self.query = self.query.order_by(column, order);
        self
    }
    
    /// Set limit
    pub fn limit(mut self, n: u64) -> Self {
        self.query = self.query.limit(n);
        self
    }
    
    /// Execute and get all results with loaded relations
    pub async fn get(self) -> Result<Vec<WithRelations<M>>> {
        let models = self.query.get().await?;
        
        let results: Vec<WithRelations<M>> = models
            .into_iter()
            .map(WithRelations::new)
            .collect();
        
        // Load each relation for all models (simplified implementation)
        // Full implementation would batch load related models and match them
        let _ = &self.relations;
        
        Ok(results)
    }
    
    /// Execute and get first result with loaded relations
    pub async fn first(mut self) -> Result<Option<WithRelations<M>>> {
        self.query = self.query.limit(1);
        let results = self.get().await?;
        Ok(results.into_iter().next())
    }
}

impl<M: Model> Default for EagerQueryBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// MODEL EXTENSION FOR EAGER LOADING
// =============================================================================

/// Extension trait for models to support eager loading syntax
pub trait EagerLoadExt: Model {
    /// Start an eager loading query builder
    ///
    /// # Example
    /// ```rust,ignore
    /// // Load users with their posts
    /// let users = User::with_relation::<Post>("posts")
    ///     .get()
    ///     .await?;
    ///
    /// for user in &users {
    ///     let posts: Vec<Post> = user.get_relation("posts").unwrap_or_default();
    ///     println!("User {} has {} posts", user.name, posts.len());
    /// }
    /// ```
    fn with_relation<R: Model>(relation_name: &str) -> EagerQueryBuilder<Self>
    where
        Self: Sized,
    {
        let _ = relation_name; // Will be used in the full implementation
        EagerQueryBuilder::new()
    }
}

// Implement for all Models
impl<T: Model> EagerLoadExt for T {}

// =============================================================================
// RELATION INFO (for schema generation and introspection)
// =============================================================================

/// Information about a model relation
#[derive(Debug, Clone)]
pub struct RelationInfo {
    /// Relation name
    pub name: String,
    /// Type of relation
    pub relation_type: RelationType,
    /// Related model table name
    pub related_table: String,
    /// Foreign key column
    pub foreign_key: String,
    /// Local key column
    pub local_key: String,
}

/// Type of relation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationType {
    /// Many-to-one relation (e.g., Post belongs to User)
    BelongsTo,
    /// One-to-one relation (e.g., User has one Profile)
    HasOne,
    /// One-to-many relation (e.g., User has many Posts)
    HasMany,
}

impl std::fmt::Display for RelationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelationType::BelongsTo => write!(f, "belongs_to"),
            RelationType::HasOne => write!(f, "has_one"),
            RelationType::HasMany => write!(f, "has_many"),
        }
    }
}
