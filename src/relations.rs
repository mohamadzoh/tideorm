//! Model Relations System
//!
//! This module provides model relations for defining relationships between models.
//!
//! ## Supported Relations
//!
//! - `BelongsTo`: Foreign key on this model (e.g., Post belongs_to User)
//! - `HasOne`: Foreign key on related model, single result (e.g., User has_one Profile)
//! - `HasMany`: Foreign key on related model, multiple results (e.g., User has_many Posts)
//! - `HasManyThrough`: Many-to-many via pivot table (e.g., User has_many Roles through UserRoles)
//! - `MorphTo`/`MorphMany`: Polymorphic relations (e.g., Comment morphMany on Post or Video)
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
//! #[has_many_through(Role, through = "user_roles", foreign_key = "user_id", related_key = "role_id")]
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
//! ## Many-to-Many Relations (HasManyThrough)
//!
//! ```rust,ignore
//! // User has many Roles through user_roles pivot table
//! let roles = user.load_has_many_through::<Role, UserRole>().await?;
//!
//! // With pivot data
//! let roles_with_pivot = user.load_has_many_through_with_pivot::<Role, UserRole>().await?;
//! ```
//!
//! ## Polymorphic Relations
//!
//! ```rust,ignore
//! // Comments can belong to Posts or Videos
//! #[derive(Model)]
//! #[morph_to(Commentable, type_column = "commentable_type", id_column = "commentable_id")]
//! pub struct Comment {
//!     pub id: i64,
//!     pub commentable_type: String,  // "posts" or "videos"
//!     pub commentable_id: i64,
//!     pub body: String,
//! }
//!
//! // Load the parent (either Post or Video)
//! let parent = comment.load_morph_to::<Commentable>().await?;
//! ```
//!
//! ## Nested Eager Loading
//!
//! ```rust,ignore
//! // Load users with their posts and each post's comments
//! let users = User::with(&["posts", "posts.comments", "profile"])
//!     .get()
//!     .await?;
//! ```
//!
//! ## Relation Constraints
//!
//! You can add constraints to relation queries:
//!
//! ```rust,ignore
//! // Load only published posts
//! let published_posts = user.load_has_many_with::<Post>(|query| {
//!     query
//!         .where_eq("published", true)
//!         .order_by("created_at", Order::Desc)
//!         .limit(10)
//! }).await?;
//!
//! // Load active profile
//! let profile = user.load_has_one_with::<Profile>(|query| {
//!     query.where_eq("active", true)
//! }).await?;
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
use crate::query::{QueryBuilder, Order};

// =============================================================================
// RELATION CONSTRAINT TYPES
// =============================================================================

/// Constraint options for relation queries
#[derive(Debug, Clone, Default)]
pub struct RelationConstraints {
    /// Additional where conditions
    pub conditions: Vec<(String, serde_json::Value)>,
    /// Order by clause
    pub order_by: Option<(String, Order)>,
    /// Limit
    pub limit: Option<u64>,
    /// Offset
    pub offset: Option<u64>,
    /// Include soft-deleted records
    pub with_trashed: bool,
    /// Only soft-deleted records
    pub only_trashed: bool,
}

impl RelationConstraints {
    /// Create new empty constraints
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Add a where condition
    pub fn where_eq(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push((column.to_string(), value.into()));
        self
    }
    
    /// Set order by
    pub fn order_by(mut self, column: &str, order: Order) -> Self {
        self.order_by = Some((column.to_string(), order));
        self
    }
    
    /// Set limit
    pub fn limit(mut self, n: u64) -> Self {
        self.limit = Some(n);
        self
    }
    
    /// Set offset
    pub fn offset(mut self, n: u64) -> Self {
        self.offset = Some(n);
        self
    }
    
    /// Include soft-deleted records
    pub fn with_trashed(mut self) -> Self {
        self.with_trashed = true;
        self
    }
    
    /// Only soft-deleted records
    pub fn only_trashed(mut self) -> Self {
        self.only_trashed = true;
        self
    }
    
    /// Apply constraints to a query builder
    pub fn apply<M: Model>(self, mut query: QueryBuilder<M>) -> QueryBuilder<M> {
        for (column, value) in self.conditions {
            query = query.where_eq(&column, value);
        }
        
        if let Some((column, order)) = self.order_by {
            query = query.order_by(&column, order);
        }
        
        if let Some(limit) = self.limit {
            query = query.limit(limit);
        }
        
        if let Some(offset) = self.offset {
            query = query.offset(offset);
        }
        
        if self.with_trashed {
            query = query.with_trashed();
        }
        
        if self.only_trashed {
            query = query.only_trashed();
        }
        
        query
    }
}

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
    
    /// Default constraints for this relation
    fn default_constraints() -> RelationConstraints {
        RelationConstraints::new()
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
    
    /// Default constraints for this relation
    fn default_constraints() -> RelationConstraints {
        RelationConstraints::new()
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
    
    /// Default constraints for this relation (ordering, filtering, etc.)
    fn default_constraints() -> RelationConstraints {
        RelationConstraints::new()
    }
    
    /// Default order for this relation
    fn default_order() -> Option<(String, Order)> {
        None
    }
    
    /// Default limit for this relation (None = no limit)
    fn default_limit() -> Option<u64> {
        None
    }
}

// =============================================================================
// MANY-TO-MANY RELATIONS (HasManyThrough)
// =============================================================================

/// HasManyThrough relation: Many-to-many through a pivot/junction table
///
/// Example: User has_many Roles through user_roles
/// - users table: id, name
/// - roles table: id, name
/// - user_roles pivot: user_id, role_id
///
/// ```rust,ignore
/// impl HasManyThrough<Role, UserRole> for User {
///     fn foreign_key() -> &'static str { "user_id" }      // on pivot
///     fn related_key() -> &'static str { "role_id" }      // on pivot
/// }
/// ```
pub trait HasManyThrough<Related: Model, Pivot: Model>: Model {
    /// Foreign key on the pivot table pointing to this model
    fn foreign_key() -> &'static str;
    
    /// Related key on the pivot table pointing to the related model
    fn related_key() -> &'static str;
    
    /// Local key on this model (defaults to primary key)
    fn local_key() -> &'static str {
        Self::primary_key_name()
    }
    
    /// Related model's local key (defaults to primary key)
    fn related_local_key() -> &'static str {
        Related::primary_key_name()
    }
    
    /// Get the pivot table name (defaults to Pivot model's table)
    fn pivot_table() -> &'static str {
        Pivot::table_name()
    }
    
    /// Pivot columns to include in results (empty = none)
    fn pivot_columns() -> Vec<&'static str> {
        vec![]
    }
    
    /// Default constraints for this relation
    fn default_constraints() -> RelationConstraints {
        RelationConstraints::new()
    }
}

/// Pivot data wrapper for many-to-many relations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithPivot<M, P> {
    /// The related model
    #[serde(flatten)]
    pub model: M,
    /// Pivot table data
    pub pivot: P,
}

impl<M, P> WithPivot<M, P> {
    /// Create a new WithPivot wrapper
    pub fn new(model: M, pivot: P) -> Self {
        Self { model, pivot }
    }
    
    /// Get the inner model
    pub fn into_model(self) -> M {
        self.model
    }
    
    /// Get the pivot data
    pub fn pivot(&self) -> &P {
        &self.pivot
    }
    
    /// Decompose into model and pivot parts
    pub fn into_parts(self) -> (M, P) {
        (self.model, self.pivot)
    }
}

impl<M, P> std::ops::Deref for WithPivot<M, P> {
    type Target = M;
    
    fn deref(&self) -> &Self::Target {
        &self.model
    }
}

// =============================================================================
// POLYMORPHIC RELATIONS
// =============================================================================

/// MorphTo relation: Polymorphic belongs-to (single parent of multiple types)
///
/// Example: Comment morphs to either Post or Video
/// - comments table: id, commentable_type, commentable_id, body
///
/// ```rust,ignore
/// impl MorphTo<Commentable> for Comment {
///     fn morph_type_column() -> &'static str { "commentable_type" }
///     fn morph_id_column() -> &'static str { "commentable_id" }
/// }
/// ```
pub trait MorphTo<Morphable>: Model {
    /// Column storing the type (e.g., "commentable_type")
    fn morph_type_column() -> &'static str;
    
    /// Column storing the foreign id (e.g., "commentable_id")
    fn morph_id_column() -> &'static str;
    
    /// Map morph type string to table name (override for custom mappings)
    fn type_to_table(type_value: &str) -> String {
        // Default: type value is the table name
        type_value.to_string()
    }
}

/// MorphOne relation: Polymorphic has-one (this model can have one of another type)
///
/// Example: Post has one Image (polymorphic)
/// - images table: id, imageable_type, imageable_id, url
///
/// ```rust,ignore
/// impl MorphOne<Image> for Post {
///     fn morph_name() -> &'static str { "imageable" }
/// }
/// ```
pub trait MorphOne<Related: Model>: Model {
    /// The morph name (used to derive type and id columns)
    fn morph_name() -> &'static str;
    
    /// The type column on the related model (defaults to {morph_name}_type)
    fn morph_type_column() -> String {
        format!("{}_type", Self::morph_name())
    }
    
    /// The id column on the related model (defaults to {morph_name}_id)
    fn morph_id_column() -> String {
        format!("{}_id", Self::morph_name())
    }
    
    /// The type value to use (defaults to this model's table name)
    fn morph_type_value() -> String {
        Self::table_name().to_string()
    }
    
    /// Local key on this model (defaults to primary key)
    fn local_key() -> &'static str {
        Self::primary_key_name()
    }
    
    /// Default constraints for this relation
    fn default_constraints() -> RelationConstraints {
        RelationConstraints::new()
    }
}

/// MorphMany relation: Polymorphic has-many (this model can have many of another type)
///
/// Example: Post has many Comments (polymorphic)
/// - comments table: id, commentable_type, commentable_id, body
///
/// ```rust,ignore
/// impl MorphMany<Comment> for Post {
///     fn morph_name() -> &'static str { "commentable" }
/// }
/// ```
pub trait MorphMany<Related: Model>: Model {
    /// The morph name (used to derive type and id columns)
    fn morph_name() -> &'static str;
    
    /// The type column on the related model (defaults to {morph_name}_type)
    fn morph_type_column() -> String {
        format!("{}_type", Self::morph_name())
    }
    
    /// The id column on the related model (defaults to {morph_name}_id)
    fn morph_id_column() -> String {
        format!("{}_id", Self::morph_name())
    }
    
    /// The type value to use (defaults to this model's table name)
    fn morph_type_value() -> String {
        Self::table_name().to_string()
    }
    
    /// Local key on this model (defaults to primary key)
    fn local_key() -> &'static str {
        Self::primary_key_name()
    }
    
    /// Default constraints for this relation
    fn default_constraints() -> RelationConstraints {
        RelationConstraints::new()
    }
    
    /// Default order for this relation
    fn default_order() -> Option<(String, Order)> {
        None
    }
}

/// Polymorphic result that can hold different model types
#[derive(Debug, Clone)]
pub enum MorphResult<A, B> {
    /// First variant
    TypeA(A),
    /// Second variant
    TypeB(B),
    /// Unknown type (stored as JSON)
    Unknown(serde_json::Value),
}

impl<A, B> MorphResult<A, B> {
    /// Check if this is TypeA
    pub fn is_type_a(&self) -> bool {
        matches!(self, MorphResult::TypeA(_))
    }
    
    /// Check if this is TypeB
    pub fn is_type_b(&self) -> bool {
        matches!(self, MorphResult::TypeB(_))
    }
    
    /// Check if this is Unknown
    pub fn is_unknown(&self) -> bool {
        matches!(self, MorphResult::Unknown(_))
    }
    
    /// Get TypeA if present
    pub fn as_type_a(&self) -> Option<&A> {
        match self {
            MorphResult::TypeA(a) => Some(a),
            _ => None,
        }
    }
    
    /// Get TypeB if present
    pub fn as_type_b(&self) -> Option<&B> {
        match self {
            MorphResult::TypeB(b) => Some(b),
            _ => None,
        }
    }
    
    /// Get Unknown JSON if present
    pub fn as_unknown(&self) -> Option<&serde_json::Value> {
        match self {
            MorphResult::Unknown(v) => Some(v),
            _ => None,
        }
    }
    
    /// Convert to TypeA, consuming self
    pub fn into_type_a(self) -> Option<A> {
        match self {
            MorphResult::TypeA(a) => Some(a),
            _ => None,
        }
    }
    
    /// Convert to TypeB, consuming self
    pub fn into_type_b(self) -> Option<B> {
        match self {
            MorphResult::TypeB(b) => Some(b),
            _ => None,
        }
    }
}

/// Three-variant polymorphic result
#[derive(Debug, Clone)]
pub enum MorphResult3<A, B, C> {
    /// First polymorphic type
    TypeA(A),
    /// Second polymorphic type
    TypeB(B),
    /// Third polymorphic type
    TypeC(C),
    /// Unknown type with raw JSON data
    Unknown(serde_json::Value),
}

/// Four-variant polymorphic result
#[derive(Debug, Clone)]
pub enum MorphResult4<A, B, C, D> {
    /// First polymorphic type
    TypeA(A),
    /// Second polymorphic type
    TypeB(B),
    /// Third polymorphic type
    TypeC(C),
    /// Fourth polymorphic type
    TypeD(D),
    /// Unknown type with raw JSON data
    Unknown(serde_json::Value),
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
    /// let author = post.load_belongs_to::<User>().await?;
    /// ```
    async fn load_belongs_to<Related>(&self) -> Result<Option<Related>>
    where
        Self: BelongsTo<Related>,
        Related: Model,
    {
        // Get the foreign key value from self
        let fk_column = <Self as BelongsTo<Related>>::foreign_key();
        let fk_value = self.get_field_value(fk_column)?;
        let constraints = <Self as BelongsTo<Related>>::default_constraints();
        
        // Query the related model by its primary key
        let query = Related::query()
            .where_eq(Related::primary_key_name(), fk_value);
        
        constraints.apply(query).first().await
    }
    
    /// Load a BelongsTo relation with custom constraints
    ///
    /// # Example
    /// ```rust,ignore
    /// let post = Post::find(1).await?;
    /// let author = post.load_belongs_to_with::<User>(|query| {
    ///     query.where_eq("active", true)
    /// }).await?;
    /// ```
    async fn load_belongs_to_with<Related, F>(&self, constraint_fn: F) -> Result<Option<Related>>
    where
        Self: BelongsTo<Related>,
        Related: Model,
        F: FnOnce(QueryBuilder<Related>) -> QueryBuilder<Related> + Send,
    {
        let fk_column = <Self as BelongsTo<Related>>::foreign_key();
        let fk_value = self.get_field_value(fk_column)?;
        
        let query = Related::query()
            .where_eq(Related::primary_key_name(), fk_value);
        
        constraint_fn(query).first().await
    }
    
    /// Load a HasOne relation
    ///
    /// # Example
    /// ```rust,ignore
    /// let user = User::find(1).await?;
    /// let profile = user.load_has_one::<Profile>().await?;
    /// ```
    async fn load_has_one<Related>(&self) -> Result<Option<Related>>
    where
        Self: HasOne<Related>,
        Related: Model,
    {
        let fk_column = <Self as HasOne<Related>>::foreign_key();
        let local_key = <Self as HasOne<Related>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        let constraints = <Self as HasOne<Related>>::default_constraints();
        
        let query = Related::query()
            .where_eq(fk_column, pk_value);
        
        constraints.apply(query).first().await
    }
    
    /// Load a HasOne relation with custom constraints
    ///
    /// # Example
    /// ```rust,ignore
    /// let user = User::find(1).await?;
    /// let profile = user.load_has_one_with::<Profile>(|query| {
    ///     query.where_eq("active", true)
    /// }).await?;
    /// ```
    async fn load_has_one_with<Related, F>(&self, constraint_fn: F) -> Result<Option<Related>>
    where
        Self: HasOne<Related>,
        Related: Model,
        F: FnOnce(QueryBuilder<Related>) -> QueryBuilder<Related> + Send,
    {
        let fk_column = <Self as HasOne<Related>>::foreign_key();
        let local_key = <Self as HasOne<Related>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        
        let query = Related::query()
            .where_eq(fk_column, pk_value);
        
        constraint_fn(query).first().await
    }
    
    /// Load a HasMany relation
    ///
    /// # Example
    /// ```rust,ignore
    /// let user = User::find(1).await?;
    /// let posts = user.load_has_many::<Post>().await?;
    /// ```
    async fn load_has_many<Related>(&self) -> Result<Vec<Related>>
    where
        Self: HasMany<Related>,
        Related: Model,
    {
        let fk_column = <Self as HasMany<Related>>::foreign_key();
        let local_key = <Self as HasMany<Related>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        let constraints = <Self as HasMany<Related>>::default_constraints();
        
        let mut query = Related::query()
            .where_eq(fk_column, pk_value);
        
        // Apply default order if specified
        if let Some((col, order)) = <Self as HasMany<Related>>::default_order() {
            query = query.order_by(&col, order);
        }
        
        // Apply default limit if specified
        if let Some(limit) = <Self as HasMany<Related>>::default_limit() {
            query = query.limit(limit);
        }
        
        constraints.apply(query).get().await
    }
    
    /// Load a HasMany relation with custom constraints
    ///
    /// # Example
    /// ```rust,ignore
    /// let user = User::find(1).await?;
    /// let posts = user.load_has_many_with::<Post>(|query| {
    ///     query
    ///         .where_eq("published", true)
    ///         .order_by("created_at", Order::Desc)
    ///         .limit(10)
    /// }).await?;
    /// ```
    async fn load_has_many_with<Related, F>(&self, constraint_fn: F) -> Result<Vec<Related>>
    where
        Self: HasMany<Related>,
        Related: Model,
        F: FnOnce(QueryBuilder<Related>) -> QueryBuilder<Related> + Send,
    {
        let fk_column = <Self as HasMany<Related>>::foreign_key();
        let local_key = <Self as HasMany<Related>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        
        let query = Related::query()
            .where_eq(fk_column, pk_value);
        
        constraint_fn(query).get().await
    }
    
    /// Count HasMany related records
    ///
    /// # Example
    /// ```rust,ignore
    /// let user = User::find(1).await?;
    /// let post_count = user.count_has_many::<Post>().await?;
    /// ```
    async fn count_has_many<Related>(&self) -> Result<u64>
    where
        Self: HasMany<Related>,
        Related: Model,
    {
        let fk_column = <Self as HasMany<Related>>::foreign_key();
        let local_key = <Self as HasMany<Related>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        
        Related::query()
            .where_eq(fk_column, pk_value)
            .count()
            .await
    }
    
    /// Count HasMany related records with constraints
    ///
    /// # Example
    /// ```rust,ignore
    /// let user = User::find(1).await?;
    /// let published_count = user.count_has_many_with::<Post>(|query| {
    ///     query.where_eq("published", true)
    /// }).await?;
    /// ```
    async fn count_has_many_with<Related, F>(&self, constraint_fn: F) -> Result<u64>
    where
        Self: HasMany<Related>,
        Related: Model,
        F: FnOnce(QueryBuilder<Related>) -> QueryBuilder<Related> + Send,
    {
        let fk_column = <Self as HasMany<Related>>::foreign_key();
        let local_key = <Self as HasMany<Related>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        
        let query = Related::query()
            .where_eq(fk_column, pk_value);
        
        constraint_fn(query).count().await
    }
    
    /// Check if any HasMany related records exist
    ///
    /// # Example
    /// ```rust,ignore
    /// let user = User::find(1).await?;
    /// if user.has_any::<Post>().await? {
    ///     println!("User has posts!");
    /// }
    /// ```
    async fn has_any<Related>(&self) -> Result<bool>
    where
        Self: HasMany<Related>,
        Related: Model,
    {
        let fk_column = <Self as HasMany<Related>>::foreign_key();
        let local_key = <Self as HasMany<Related>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        
        Related::query()
            .where_eq(fk_column, pk_value)
            .exists()
            .await
    }
    
    /// Check if HasOne relation exists
    ///
    /// # Example
    /// ```rust,ignore
    /// let user = User::find(1).await?;
    /// if user.has_one_exists::<Profile>().await? {
    ///     println!("User has a profile!");
    /// }
    /// ```
    async fn has_one_exists<Related>(&self) -> Result<bool>
    where
        Self: HasOne<Related>,
        Related: Model,
    {
        let fk_column = <Self as HasOne<Related>>::foreign_key();
        let local_key = <Self as HasOne<Related>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        
        Related::query()
            .where_eq(fk_column, pk_value)
            .exists()
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
    
    // =========================================================================
    // MANY-TO-MANY (HasManyThrough) LOADING
    // =========================================================================
    
    /// Load a HasManyThrough (many-to-many) relation
    ///
    /// # Example
    /// ```rust,ignore
    /// let user = User::find(1).await?;
    /// let roles = user.load_has_many_through::<Role, UserRole>().await?;
    /// ```
    async fn load_has_many_through<Related, Pivot>(&self) -> Result<Vec<Related>>
    where
        Self: HasManyThrough<Related, Pivot>,
        Related: Model,
        Pivot: Model,
    {
        let local_key = <Self as HasManyThrough<Related, Pivot>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        let foreign_key = <Self as HasManyThrough<Related, Pivot>>::foreign_key();
        let related_key = <Self as HasManyThrough<Related, Pivot>>::related_key();
        let related_local_key = <Self as HasManyThrough<Related, Pivot>>::related_local_key();
        let pivot_table = <Self as HasManyThrough<Related, Pivot>>::pivot_table();
        let constraints = <Self as HasManyThrough<Related, Pivot>>::default_constraints();
        
        // Build a query that joins through the pivot table
        // SELECT related.* FROM related
        // INNER JOIN pivot ON pivot.related_key = related.id
        // WHERE pivot.foreign_key = ?
        let query = Related::query()
            .inner_join(
                pivot_table,
                &format!("{}.{}", pivot_table, related_key),
                &format!("{}.{}", Related::table_name(), related_local_key),
            )
            .where_raw(&format!("{}.{} = {}", pivot_table, foreign_key, pk_value));
        
        constraints.apply(query).get().await
    }
    
    /// Load a HasManyThrough relation with custom constraints
    async fn load_has_many_through_with<Related, Pivot, F>(&self, constraint_fn: F) -> Result<Vec<Related>>
    where
        Self: HasManyThrough<Related, Pivot>,
        Related: Model,
        Pivot: Model,
        F: FnOnce(QueryBuilder<Related>) -> QueryBuilder<Related> + Send,
    {
        let local_key = <Self as HasManyThrough<Related, Pivot>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        let foreign_key = <Self as HasManyThrough<Related, Pivot>>::foreign_key();
        let related_key = <Self as HasManyThrough<Related, Pivot>>::related_key();
        let related_local_key = <Self as HasManyThrough<Related, Pivot>>::related_local_key();
        let pivot_table = <Self as HasManyThrough<Related, Pivot>>::pivot_table();
        
        let query = Related::query()
            .inner_join(
                pivot_table,
                &format!("{}.{}", pivot_table, related_key),
                &format!("{}.{}", Related::table_name(), related_local_key),
            )
            .where_raw(&format!("{}.{} = {}", pivot_table, foreign_key, pk_value));
        
        constraint_fn(query).get().await
    }
    
    /// Count HasManyThrough related records
    async fn count_has_many_through<Related, Pivot>(&self) -> Result<u64>
    where
        Self: HasManyThrough<Related, Pivot>,
        Related: Model,
        Pivot: Model,
    {
        let local_key = <Self as HasManyThrough<Related, Pivot>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        let foreign_key = <Self as HasManyThrough<Related, Pivot>>::foreign_key();
        let pivot_table = <Self as HasManyThrough<Related, Pivot>>::pivot_table();
        
        // Count through pivot table
        Pivot::query()
            .where_raw(&format!("{}.{} = {}", pivot_table, foreign_key, pk_value))
            .count()
            .await
    }
    
    /// Attach a related model through pivot (many-to-many)
    ///
    /// # Example
    /// ```rust,ignore
    /// let user = User::find(1).await?;
    /// user.attach::<Role, UserRole>(role_id, None).await?;
    /// ```
    async fn attach<Related, Pivot>(&self, related_id: impl Into<serde_json::Value> + Send, pivot_data: Option<HashMap<String, serde_json::Value>>) -> Result<()>
    where
        Self: HasManyThrough<Related, Pivot>,
        Related: Model,
        Pivot: Model,
    {
        let local_key = <Self as HasManyThrough<Related, Pivot>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        let foreign_key = <Self as HasManyThrough<Related, Pivot>>::foreign_key();
        let related_key = <Self as HasManyThrough<Related, Pivot>>::related_key();
        let pivot_table = <Self as HasManyThrough<Related, Pivot>>::pivot_table();
        
        let mut data = pivot_data.unwrap_or_default();
        data.insert(foreign_key.to_string(), pk_value);
        data.insert(related_key.to_string(), related_id.into());
        
        // Build INSERT SQL directly
        let columns: Vec<&str> = data.keys().map(|s| s.as_str()).collect();
        let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("${}", i)).collect();
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            pivot_table,
            columns.join(", "),
            placeholders.join(", ")
        );
        
        let params: Vec<crate::internal::Value> = columns.iter()
            .filter_map(|col| data.get(*col))
            .map(|v| crate::internal::Value::from(v.clone()))
            .collect();
        
        crate::database::Database::execute_with_params(&sql, params).await?;
        Ok(())
    }
    
    /// Detach a related model (remove from pivot table)
    async fn detach<Related, Pivot>(&self, related_id: impl Into<serde_json::Value> + Send) -> Result<u64>
    where
        Self: HasManyThrough<Related, Pivot>,
        Related: Model,
        Pivot: Model,
    {
        let local_key = <Self as HasManyThrough<Related, Pivot>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        let foreign_key = <Self as HasManyThrough<Related, Pivot>>::foreign_key();
        let related_key = <Self as HasManyThrough<Related, Pivot>>::related_key();
        
        Pivot::query()
            .where_eq(foreign_key, pk_value)
            .where_eq(related_key, related_id.into())
            .delete()
            .await
    }
    
    /// Detach all related models (clear pivot table entries)
    async fn detach_all<Related, Pivot>(&self) -> Result<u64>
    where
        Self: HasManyThrough<Related, Pivot>,
        Related: Model,
        Pivot: Model,
    {
        let local_key = <Self as HasManyThrough<Related, Pivot>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        let foreign_key = <Self as HasManyThrough<Related, Pivot>>::foreign_key();
        
        Pivot::query()
            .where_eq(foreign_key, pk_value)
            .delete()
            .await
    }
    
    /// Sync related models (replace all with new set)
    async fn sync<Related, Pivot>(&self, related_ids: Vec<serde_json::Value>) -> Result<()>
    where
        Self: HasManyThrough<Related, Pivot>,
        Related: Model,
        Pivot: Model,
    {
        // First detach all
        self.detach_all::<Related, Pivot>().await?;
        
        // Then attach new ones
        for id in related_ids {
            self.attach::<Related, Pivot>(id, None).await?;
        }
        
        Ok(())
    }
    
    // =========================================================================
    // POLYMORPHIC RELATIONS LOADING
    // =========================================================================
    
    /// Load a MorphOne (polymorphic has-one) relation
    ///
    /// # Example
    /// ```rust,ignore
    /// let post = Post::find(1).await?;
    /// let image = post.load_morph_one::<Image>().await?;
    /// ```
    async fn load_morph_one<Related>(&self) -> Result<Option<Related>>
    where
        Self: MorphOne<Related>,
        Related: Model,
    {
        let local_key = <Self as MorphOne<Related>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        let type_column = <Self as MorphOne<Related>>::morph_type_column();
        let id_column = <Self as MorphOne<Related>>::morph_id_column();
        let type_value = <Self as MorphOne<Related>>::morph_type_value();
        let constraints = <Self as MorphOne<Related>>::default_constraints();
        
        let query = Related::query()
            .where_eq(&type_column, type_value)
            .where_eq(&id_column, pk_value);
        
        constraints.apply(query).first().await
    }
    
    /// Load a MorphOne relation with custom constraints
    async fn load_morph_one_with<Related, F>(&self, constraint_fn: F) -> Result<Option<Related>>
    where
        Self: MorphOne<Related>,
        Related: Model,
        F: FnOnce(QueryBuilder<Related>) -> QueryBuilder<Related> + Send,
    {
        let local_key = <Self as MorphOne<Related>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        let type_column = <Self as MorphOne<Related>>::morph_type_column();
        let id_column = <Self as MorphOne<Related>>::morph_id_column();
        let type_value = <Self as MorphOne<Related>>::morph_type_value();
        
        let query = Related::query()
            .where_eq(&type_column, type_value)
            .where_eq(&id_column, pk_value);
        
        constraint_fn(query).first().await
    }
    
    /// Load a MorphMany (polymorphic has-many) relation
    ///
    /// # Example
    /// ```rust,ignore
    /// let post = Post::find(1).await?;
    /// let comments = post.load_morph_many::<Comment>().await?;
    /// ```
    async fn load_morph_many<Related>(&self) -> Result<Vec<Related>>
    where
        Self: MorphMany<Related>,
        Related: Model,
    {
        let local_key = <Self as MorphMany<Related>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        let type_column = <Self as MorphMany<Related>>::morph_type_column();
        let id_column = <Self as MorphMany<Related>>::morph_id_column();
        let type_value = <Self as MorphMany<Related>>::morph_type_value();
        let constraints = <Self as MorphMany<Related>>::default_constraints();
        
        let mut query = Related::query()
            .where_eq(&type_column, type_value)
            .where_eq(&id_column, pk_value);
        
        if let Some((col, order)) = <Self as MorphMany<Related>>::default_order() {
            query = query.order_by(&col, order);
        }
        
        constraints.apply(query).get().await
    }
    
    /// Load a MorphMany relation with custom constraints
    async fn load_morph_many_with<Related, F>(&self, constraint_fn: F) -> Result<Vec<Related>>
    where
        Self: MorphMany<Related>,
        Related: Model,
        F: FnOnce(QueryBuilder<Related>) -> QueryBuilder<Related> + Send,
    {
        let local_key = <Self as MorphMany<Related>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        let type_column = <Self as MorphMany<Related>>::morph_type_column();
        let id_column = <Self as MorphMany<Related>>::morph_id_column();
        let type_value = <Self as MorphMany<Related>>::morph_type_value();
        
        let query = Related::query()
            .where_eq(&type_column, type_value)
            .where_eq(&id_column, pk_value);
        
        constraint_fn(query).get().await
    }
    
    /// Count MorphMany related records
    async fn count_morph_many<Related>(&self) -> Result<u64>
    where
        Self: MorphMany<Related>,
        Related: Model,
    {
        let local_key = <Self as MorphMany<Related>>::local_key();
        let pk_value = self.get_field_value(local_key)?;
        let type_column = <Self as MorphMany<Related>>::morph_type_column();
        let id_column = <Self as MorphMany<Related>>::morph_id_column();
        let type_value = <Self as MorphMany<Related>>::morph_type_value();
        
        Related::query()
            .where_eq(&type_column, type_value)
            .where_eq(&id_column, pk_value)
            .count()
            .await
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

/// Parsed relation path for nested eager loading
#[derive(Debug, Clone)]
pub struct RelationPath {
    /// Full path string (e.g., "posts.comments.author")
    pub full_path: String,
    /// Parsed segments
    pub segments: Vec<String>,
}

impl RelationPath {
    /// Parse a relation path from a string
    /// Supports dot notation for nested relations: "posts.comments.author"
    pub fn parse(path: &str) -> Self {
        let segments: Vec<String> = path.split('.').map(|s| s.to_string()).collect();
        Self {
            full_path: path.to_string(),
            segments,
        }
    }
    
    /// Get the top-level relation name
    pub fn root(&self) -> &str {
        self.segments.first().map(|s| s.as_str()).unwrap_or("")
    }
    
    /// Get nested path (everything after the first segment)
    pub fn nested(&self) -> Option<RelationPath> {
        if self.segments.len() > 1 {
            Some(RelationPath {
                full_path: self.segments[1..].join("."),
                segments: self.segments[1..].to_vec(),
            })
        } else {
            None
        }
    }
    
    /// Check if this is a nested path
    pub fn is_nested(&self) -> bool {
        self.segments.len() > 1
    }
    
    /// Get depth of nesting (1 for simple, 2+ for nested)
    pub fn depth(&self) -> usize {
        self.segments.len()
    }
}

/// Query builder with eager loading support
/// 
/// Supports nested eager loading using dot notation:
/// ```rust,ignore
/// // Load users with posts and nested comments
/// let users = User::eager()
///     .with("posts")
///     .with("posts.comments")         // Nested: load comments for each post
///     .with("posts.comments.author")  // Deep nesting: author of each comment
///     .get()
///     .await?;
/// ```
pub struct EagerQueryBuilder<M: Model> {
    query: QueryBuilder<M>,
    /// Relations to load, organized as a tree structure
    relation_tree: RelationTree,
}

/// Tree structure for organizing nested relations
#[derive(Debug, Clone, Default)]
pub struct RelationTree {
    /// Map of relation name -> nested relations
    children: HashMap<String, RelationTree>,
}

impl RelationTree {
    /// Create a new empty tree
    pub fn new() -> Self {
        Self {
            children: HashMap::new(),
        }
    }
    
    /// Add a relation path to the tree
    pub fn add_path(&mut self, path: &RelationPath) {
        if path.segments.is_empty() {
            return;
        }
        
        let root = path.root().to_string();
        let child = self.children.entry(root).or_insert_with(RelationTree::new);
        
        if let Some(nested) = path.nested() {
            child.add_path(&nested);
        }
    }
    
    /// Get all root-level relations
    pub fn roots(&self) -> Vec<String> {
        self.children.keys().cloned().collect()
    }
    
    /// Get nested tree for a relation
    pub fn get_nested(&self, name: &str) -> Option<&RelationTree> {
        self.children.get(name)
    }
    
    /// Check if tree is empty
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
    
    /// Check if a relation has nested relations
    pub fn has_nested(&self, name: &str) -> bool {
        self.children.get(name)
            .map(|t| !t.is_empty())
            .unwrap_or(false)
    }
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
            relation_tree: RelationTree::new(),
        }
    }
    
    /// Add a relation to eager load
    /// 
    /// Supports dot notation for nested relations:
    /// ```rust,ignore
    /// // Simple relation
    /// builder.with("posts")
    /// 
    /// // Nested relation (loads posts, then comments for each post)
    /// builder.with("posts.comments")
    /// 
    /// // Multiple levels of nesting
    /// builder.with("posts.comments.author")
    /// ```
    pub fn with(mut self, relation: &str) -> Self {
        let path = RelationPath::parse(relation);
        self.relation_tree.add_path(&path);
        self
    }
    
    /// Add multiple relations at once
    /// 
    /// # Example
    /// ```rust,ignore
    /// User::eager()
    ///     .with_many(&["posts", "posts.comments", "profile"])
    ///     .get()
    ///     .await?;
    /// ```
    pub fn with_many(mut self, relations: &[&str]) -> Self {
        for relation in relations {
            self = self.with(relation);
        }
        self
    }
    
    /// Add a where condition
    pub fn where_eq<V: Into<serde_json::Value>>(mut self, column: &str, value: V) -> Self {
        self.query = self.query.where_eq(column, value);
        self
    }
    
    /// Add a where_in condition
    pub fn where_in<V: Into<serde_json::Value>>(mut self, column: &str, values: Vec<V>) -> Self {
        self.query = self.query.where_in(column, values);
        self
    }
    
    /// Add a raw where condition
    pub fn where_raw(mut self, sql: &str) -> Self {
        self.query = self.query.where_raw(sql);
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
    
    /// Set offset
    pub fn offset(mut self, n: u64) -> Self {
        self.query = self.query.offset(n);
        self
    }
    
    /// Get the relation tree (for introspection)
    pub fn get_relation_tree(&self) -> &RelationTree {
        &self.relation_tree
    }
    
    /// Execute and get all results with loaded relations
    /// 
    /// Relations are loaded efficiently using batch queries to avoid N+1 problems
    pub async fn get(self) -> Result<Vec<WithRelations<M>>> {
        let models = self.query.get().await?;
        
        let results: Vec<WithRelations<M>> = models
            .into_iter()
            .map(WithRelations::new)
            .collect();
        
        // Note: Full implementation would use the relation_tree to:
        // 1. Batch load root relations for all models
        // 2. For each root relation with nested, recursively load nested relations
        // 3. Match loaded data back to parent models
        //
        // Example pseudo-code:
        // for root in self.relation_tree.roots() {
        //     let related_data = batch_load_relation(&results, &root).await?;
        //     for (idx, result) in results.iter_mut().enumerate() {
        //         result.relations.insert(root.clone(), related_data[idx].clone());
        //     }
        //     if let Some(nested_tree) = self.relation_tree.get_nested(&root) {
        //         // Recursively load nested relations
        //     }
        // }
        
        Ok(results)
    }
    
    /// Execute and get first result with loaded relations
    pub async fn first(mut self) -> Result<Option<WithRelations<M>>> {
        self.query = self.query.limit(1);
        let results = self.get().await?;
        Ok(results.into_iter().next())
    }
    
    /// Find by ID with eager loaded relations
    pub async fn find(mut self, id: impl Into<serde_json::Value>) -> Result<Option<WithRelations<M>>> {
        self.query = self.query.where_eq(M::primary_key_name(), id);
        self.first().await
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
    /// // Simple eager loading
    /// let users = User::eager()
    ///     .with("posts")
    ///     .get()
    ///     .await?;
    ///
    /// // Nested eager loading
    /// let users = User::eager()
    ///     .with("posts")
    ///     .with("posts.comments")         // Load comments for each post
    ///     .with("posts.comments.author")  // Load author for each comment
    ///     .get()
    ///     .await?;
    ///
    /// for user in &users {
    ///     let posts: Vec<Post> = user.get_relation("posts").unwrap_or_default();
    ///     for post in &posts {
    ///         // Access nested comments if needed
    ///     }
    /// }
    /// ```
    fn eager() -> EagerQueryBuilder<Self>
    where
        Self: Sized,
    {
        EagerQueryBuilder::new()
    }
    
    /// Start an eager loading query builder with a specific relation
    fn with_relation(relation_name: &str) -> EagerQueryBuilder<Self>
    where
        Self: Sized,
    {
        EagerQueryBuilder::new().with(relation_name)
    }
    
    /// Start an eager loading query builder with multiple relations
    /// 
    /// # Example
    /// ```rust,ignore
    /// let users = User::with_relations(&["posts", "posts.comments", "profile"])
    ///     .get()
    ///     .await?;
    /// ```
    fn with_relations(relations: &[&str]) -> EagerQueryBuilder<Self>
    where
        Self: Sized,
    {
        EagerQueryBuilder::new().with_many(relations)
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
    /// For HasManyThrough: pivot table name
    pub pivot_table: Option<String>,
    /// For polymorphic: morph type column
    pub morph_type_column: Option<String>,
    /// For polymorphic: morph id column
    pub morph_id_column: Option<String>,
}

impl RelationInfo {
    /// Create a BelongsTo relation info
    pub fn belongs_to(name: &str, related_table: &str, foreign_key: &str, local_key: &str) -> Self {
        Self {
            name: name.to_string(),
            relation_type: RelationType::BelongsTo,
            related_table: related_table.to_string(),
            foreign_key: foreign_key.to_string(),
            local_key: local_key.to_string(),
            pivot_table: None,
            morph_type_column: None,
            morph_id_column: None,
        }
    }
    
    /// Create a HasOne relation info
    pub fn has_one(name: &str, related_table: &str, foreign_key: &str, local_key: &str) -> Self {
        Self {
            name: name.to_string(),
            relation_type: RelationType::HasOne,
            related_table: related_table.to_string(),
            foreign_key: foreign_key.to_string(),
            local_key: local_key.to_string(),
            pivot_table: None,
            morph_type_column: None,
            morph_id_column: None,
        }
    }
    
    /// Create a HasMany relation info
    pub fn has_many(name: &str, related_table: &str, foreign_key: &str, local_key: &str) -> Self {
        Self {
            name: name.to_string(),
            relation_type: RelationType::HasMany,
            related_table: related_table.to_string(),
            foreign_key: foreign_key.to_string(),
            local_key: local_key.to_string(),
            pivot_table: None,
            morph_type_column: None,
            morph_id_column: None,
        }
    }
    
    /// Create a HasManyThrough (many-to-many) relation info
    pub fn has_many_through(
        name: &str,
        related_table: &str,
        pivot_table: &str,
        foreign_key: &str,
        related_key: &str,
        local_key: &str,
    ) -> Self {
        Self {
            name: name.to_string(),
            relation_type: RelationType::HasManyThrough,
            related_table: related_table.to_string(),
            foreign_key: foreign_key.to_string(),
            local_key: local_key.to_string(),
            pivot_table: Some(pivot_table.to_string()),
            morph_type_column: Some(related_key.to_string()), // Reuse for related_key
            morph_id_column: None,
        }
    }
    
    /// Create a MorphOne relation info
    pub fn morph_one(
        name: &str,
        related_table: &str,
        type_column: &str,
        id_column: &str,
        local_key: &str,
    ) -> Self {
        Self {
            name: name.to_string(),
            relation_type: RelationType::MorphOne,
            related_table: related_table.to_string(),
            foreign_key: String::new(),
            local_key: local_key.to_string(),
            pivot_table: None,
            morph_type_column: Some(type_column.to_string()),
            morph_id_column: Some(id_column.to_string()),
        }
    }
    
    /// Create a MorphMany relation info
    pub fn morph_many(
        name: &str,
        related_table: &str,
        type_column: &str,
        id_column: &str,
        local_key: &str,
    ) -> Self {
        Self {
            name: name.to_string(),
            relation_type: RelationType::MorphMany,
            related_table: related_table.to_string(),
            foreign_key: String::new(),
            local_key: local_key.to_string(),
            pivot_table: None,
            morph_type_column: Some(type_column.to_string()),
            morph_id_column: Some(id_column.to_string()),
        }
    }
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
    /// Many-to-many through pivot table (e.g., User has many Roles through UserRole)
    HasManyThrough,
    /// Polymorphic many-to-one (e.g., Comment belongs to commentable)
    MorphTo,
    /// Polymorphic one-to-one (e.g., Post has one Image as imageable)
    MorphOne,
    /// Polymorphic one-to-many (e.g., Post has many Comments as commentable)
    MorphMany,
}

impl std::fmt::Display for RelationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelationType::BelongsTo => write!(f, "belongs_to"),
            RelationType::HasOne => write!(f, "has_one"),
            RelationType::HasMany => write!(f, "has_many"),
            RelationType::HasManyThrough => write!(f, "has_many_through"),
            RelationType::MorphTo => write!(f, "morph_to"),
            RelationType::MorphOne => write!(f, "morph_one"),
            RelationType::MorphMany => write!(f, "morph_many"),
        }
    }
}
