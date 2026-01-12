//! Model Relations System
//!
//! This module provides model relations using SeaORM-style field declarations.
//! Relations are defined as struct fields with attributes, following SeaORM's pattern.
//!
//! ## Supported Relations
//!
//! - `HasOne<E>`: One-to-one relation (e.g., User has_one Profile)
//! - `HasMany<E>`: One-to-many relation (e.g., User has_many Posts)
//! - `BelongsTo<E>`: Inverse of HasOne/HasMany (e.g., Post belongs_to User)
//!
//! ## Defining Relations Inside Models (SeaORM Style)
//!
//! Relations are declared as fields in your model struct using the `#[tide(relation)]` attribute:
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//!
//! #[derive(Model, Clone, Debug, Serialize, Deserialize)]
//! #[tide(table = "users")]
//! pub struct User {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub name: String,
//!     pub email: String,
//!     
//!     // Relations defined as fields
//!     #[tide(has_one = "Profile", foreign_key = "user_id")]
//!     pub profile: HasOne<Profile>,
//!     
//!     #[tide(has_many = "Post", foreign_key = "user_id")]
//!     pub posts: HasMany<Post>,
//! }
//!
//! #[derive(Model)]
//! #[tide(table = "posts")]
//! pub struct Post {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub user_id: i64,
//!     pub title: String,
//!     
//!     #[tide(belongs_to = "User", foreign_key = "user_id")]
//!     pub author: BelongsTo<User>,
//! }
//!
//! #[derive(Model)]
//! #[tide(table = "profiles")]
//! pub struct Profile {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub user_id: i64,
//!     pub bio: String,
//!     
//!     #[tide(belongs_to = "User", foreign_key = "user_id")]
//!     pub user: BelongsTo<User>,
//! }
//!
//! // Loading relations:
//! let user = User::find(1).await?;
//! let posts = user.posts.load().await?;
//! let profile = user.profile.load().await?;
//!
//! let post = Post::find(1).await?;
//! let author = post.author.load().await?;
//! ```
//!
//! ## Many-to-Many Relations
//!
//! ```rust,ignore
//! #[derive(Model)]
//! #[tide(table = "users")]
//! pub struct User {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub name: String,
//!     
//!     #[tide(has_many_through = "Role", pivot = "user_roles", foreign_key = "user_id", related_key = "role_id")]
//!     pub roles: HasManyThrough<Role, UserRole>,
//! }
//!
//! // Load roles for a user
//! let roles = user.roles.load().await?;
//! ```
//!
//! ## Relation Constraints
//!
//! You can add constraints to relation queries:
//!
//! ```rust,ignore
//! // Load only published posts
//! let published_posts = user.posts.load_with(|query| {
//!     query
//!         .where_eq("published", true)
//!         .order_by("created_at", Order::Desc)
//!         .limit(10)
//! }).await?;
//! ```

use async_trait::async_trait;
use serde::{Serialize, Deserialize, Serializer, Deserializer};
use std::collections::HashMap;
use std::marker::PhantomData;

use crate::error::{Error, Result};
use crate::model::Model;
use crate::query::{QueryBuilder, Order};

// =============================================================================
// SELF-REFERENCING RELATIONS (SeaORM 2.0 feature)
// =============================================================================

/// SelfRef relation type - represents a self-referencing relationship (SeaORM 2.0 feature)
///
/// Use this for hierarchical data like org charts, categories, or tree structures
/// where a model references itself.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Model)]
/// #[tide(table = "employees")]
/// pub struct Employee {
///     #[tide(primary_key)]
///     pub id: i64,
///     pub name: String,
///     pub manager_id: Option<i64>,
///     
///     // Self-reference: each employee optionally reports to another employee
///     #[tide(self_ref = "id", foreign_key = "manager_id")]
///     pub manager: SelfRef<Employee>,
///     
///     // Inverse: employees who report to this employee
///     #[tide(self_ref_many = "id", foreign_key = "manager_id")]
///     pub reports: SelfRefMany<Employee>,
/// }
///
/// // Usage:
/// let employee = Employee::find(5).await?;
/// let manager = employee.manager.load().await?;  // Get their manager
/// let reports = employee.reports.load().await?;  // Get their direct reports
/// ```
#[derive(Debug, Clone)]
pub struct SelfRef<E: Model> {
    /// Foreign key column that references the parent (e.g., "manager_id")
    pub foreign_key: &'static str,
    /// Local key being referenced (usually "id")
    pub local_key: &'static str,
    /// Cached related model
    cached: Option<Box<E>>,
    /// The foreign key value for loading (e.g., the manager_id value)
    fk_value: Option<serde_json::Value>,
    _marker: PhantomData<E>,
}

impl<E: Model> SelfRef<E> {
    /// Create a new SelfRef relation
    pub fn new(foreign_key: &'static str, local_key: &'static str) -> Self {
        Self {
            foreign_key,
            local_key,
            cached: None,
            fk_value: None,
            _marker: PhantomData,
        }
    }
    
    /// Set the foreign key value for lazy loading
    pub fn with_fk_value(mut self, fk: serde_json::Value) -> Self {
        self.fk_value = Some(fk);
        self
    }
    
    /// Load the referenced model (e.g., load the manager)
    pub async fn load(&self) -> Result<Option<E>> {
        let fk = match &self.fk_value {
            Some(v) if !v.is_null() => v,
            _ => return Ok(None), // No FK set means no relation
        };
        
        E::query()
            .where_eq(self.local_key, fk.clone())
            .first()
            .await
    }
    
    /// Load with custom constraints
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Option<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        let fk = match &self.fk_value {
            Some(v) if !v.is_null() => v,
            _ => return Ok(None),
        };
        
        let query = E::query().where_eq(self.local_key, fk.clone());
        constraint_fn(query).first().await
    }
    
    /// Check if the self-reference exists
    pub async fn exists(&self) -> Result<bool> {
        let fk = match &self.fk_value {
            Some(v) if !v.is_null() => v,
            _ => return Ok(false),
        };
        
        E::query()
            .where_eq(self.local_key, fk.clone())
            .exists()
            .await
    }
    
    /// Get the cached value if already loaded
    pub fn get_cached(&self) -> Option<&E> {
        self.cached.as_deref()
    }
}

impl<E: Model> Default for SelfRef<E> {
    fn default() -> Self {
        Self {
            foreign_key: "parent_id",
            local_key: "id",
            cached: None,
            fk_value: None,
            _marker: PhantomData,
        }
    }
}

impl<E: Model + Serialize> Serialize for SelfRef<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for SelfRef<E> {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::default())
    }
}

/// SelfRefMany relation type - represents the inverse of a self-referencing relationship
///
/// Use this to get all records that reference this record (e.g., all reports of a manager).
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Model)]
/// pub struct Category {
///     pub id: i64,
///     pub name: String,
///     pub parent_id: Option<i64>,
///     
///     // Get the parent category
///     #[tide(self_ref = "id", foreign_key = "parent_id")]
///     pub parent: SelfRef<Category>,
///     
///     // Get all child categories
///     #[tide(self_ref_many = "id", foreign_key = "parent_id")]
///     pub children: SelfRefMany<Category>,
/// }
/// ```
#[derive(Debug, Clone)]
pub struct SelfRefMany<E: Model> {
    /// Foreign key column on the related records (e.g., "parent_id")
    pub foreign_key: &'static str,
    /// Local key being referenced (usually "id")
    pub local_key: &'static str,
    /// Cached related models
    cached: Option<Vec<E>>,
    /// This record's primary key value
    parent_pk: Option<serde_json::Value>,
    _marker: PhantomData<E>,
}

impl<E: Model> SelfRefMany<E> {
    /// Create a new SelfRefMany relation
    pub fn new(foreign_key: &'static str, local_key: &'static str) -> Self {
        Self {
            foreign_key,
            local_key,
            cached: None,
            parent_pk: None,
            _marker: PhantomData,
        }
    }
    
    /// Set the parent primary key for lazy loading
    pub fn with_parent_pk(mut self, pk: serde_json::Value) -> Self {
        self.parent_pk = Some(pk);
        self
    }
    
    /// Load all records that reference this one
    pub async fn load(&self) -> Result<Vec<E>> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for self-reference")))?;
        
        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .get()
            .await
    }
    
    /// Load with custom constraints
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for self-reference")))?;
        
        let query = E::query().where_eq(self.foreign_key, pk.clone());
        constraint_fn(query).get().await
    }
    
    /// Count records that reference this one
    pub async fn count(&self) -> Result<u64> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for self-reference")))?;
        
        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .count()
            .await
            .map(|c| c as u64)
    }
    
    /// Check if any records reference this one
    pub async fn exists(&self) -> Result<bool> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for self-reference")))?;
        
        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .exists()
            .await
    }
    
    /// Get the cached values if already loaded
    pub fn get_cached(&self) -> Option<&[E]> {
        self.cached.as_deref()
    }
    
    /// Load the full tree of descendants (recursive)
    ///
    /// This loads all descendants recursively up to the specified depth.
    /// Use with caution on large datasets.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let all_subcategories = category.children.load_tree(5).await?;
    /// ```
    pub async fn load_tree(&self, max_depth: usize) -> Result<Vec<E>> {
        if max_depth == 0 {
            return Ok(Vec::new());
        }
        
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for self-reference")))?;
        
        self.load_tree_recursive(pk.clone(), max_depth).await
    }
    
    /// Internal recursive tree loading
    #[async_recursion::async_recursion]
    async fn load_tree_recursive(&self, parent_pk: serde_json::Value, depth: usize) -> Result<Vec<E>> {
        if depth == 0 {
            return Ok(Vec::new());
        }
        
        let children: Vec<E> = E::query()
            .where_eq(self.foreign_key, parent_pk)
            .get()
            .await?;
        
        let mut all = children.clone();
        
        for child in children {
            // Convert primary key to JSON value using Display trait
            let pk_string = format!("{}", child.primary_key());
            let child_pk = if let Ok(num) = pk_string.parse::<i64>() {
                serde_json::Value::Number(num.into())
            } else {
                serde_json::Value::String(pk_string)
            };
            
            if !child_pk.is_null() {
                let descendants = self.load_tree_recursive(child_pk, depth - 1).await?;
                all.extend(descendants);
            }
        }
        
        Ok(all)
    }
}

impl<E: Model> Default for SelfRefMany<E> {
    fn default() -> Self {
        Self {
            foreign_key: "parent_id",
            local_key: "id",
            cached: None,
            parent_pk: None,
            _marker: PhantomData,
        }
    }
}

impl<E: Model + Serialize> Serialize for SelfRefMany<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for SelfRefMany<E> {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::default())
    }
}

// =============================================================================
// RELATION TYPE WRAPPERS (SeaORM-style)
// =============================================================================

/// HasOne relation type - represents a one-to-one relationship
///
/// Use this as a field type in your model to define a has_one relation.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Model)]
/// pub struct User {
///     pub id: i64,
///     pub name: String,
///     
///     #[tide(has_one = "Profile", foreign_key = "user_id")]
///     pub profile: HasOne<Profile>,
/// }
/// ```
#[derive(Debug, Clone)]
pub struct HasOne<E: Model> {
    /// Foreign key column on the related model
    pub foreign_key: &'static str,
    /// Local key on this model (usually the primary key)
    pub local_key: &'static str,
    /// Cached related model (loaded via `.load()`)
    cached: Option<Box<E>>,
    /// Parent model's primary key value for loading
    parent_pk: Option<serde_json::Value>,
    _marker: PhantomData<E>,
}

impl<E: Model> HasOne<E> {
    /// Create a new HasOne relation
    pub fn new(foreign_key: &'static str, local_key: &'static str) -> Self {
        Self {
            foreign_key,
            local_key,
            cached: None,
            parent_pk: None,
            _marker: PhantomData,
        }
    }
    
    /// Set the parent primary key for lazy loading
    pub fn with_parent_pk(mut self, pk: serde_json::Value) -> Self {
        self.parent_pk = Some(pk);
        self
    }
    
    /// Load the related model
    pub async fn load(&self) -> Result<Option<E>> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        
        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .first()
            .await
    }
    
    /// Load the related model with custom constraints
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Option<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        
        let query = E::query().where_eq(self.foreign_key, pk.clone());
        constraint_fn(query).first().await
    }
    
    /// Check if the relation exists
    pub async fn exists(&self) -> Result<bool> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        
        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .exists()
            .await
    }
    
    /// Get the cached value if already loaded
    pub fn get_cached(&self) -> Option<&E> {
        self.cached.as_deref()
    }
}

impl<E: Model> Default for HasOne<E> {
    fn default() -> Self {
        Self {
            foreign_key: "id",
            local_key: "id",
            cached: None,
            parent_pk: None,
            _marker: PhantomData,
        }
    }
}

impl<E: Model + Serialize> Serialize for HasOne<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize only the cached value if present
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for HasOne<E> {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Relations are not deserialized from JSON - they're set up by the model
        Ok(Self::default())
    }
}

/// HasMany relation type - represents a one-to-many relationship
///
/// Use this as a field type in your model to define a has_many relation.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Model)]
/// pub struct User {
///     pub id: i64,
///     pub name: String,
///     
///     #[tide(has_many = "Post", foreign_key = "user_id")]
///     pub posts: HasMany<Post>,
/// }
/// ```
#[derive(Debug, Clone)]
pub struct HasMany<E: Model> {
    /// Foreign key column on the related model
    pub foreign_key: &'static str,
    /// Local key on this model (usually the primary key)
    pub local_key: &'static str,
    /// Cached related models (loaded via `.load()`)
    cached: Option<Vec<E>>,
    /// Parent model's primary key value for loading
    parent_pk: Option<serde_json::Value>,
    _marker: PhantomData<E>,
}

impl<E: Model> HasMany<E> {
    /// Create a new HasMany relation
    pub fn new(foreign_key: &'static str, local_key: &'static str) -> Self {
        Self {
            foreign_key,
            local_key,
            cached: None,
            parent_pk: None,
            _marker: PhantomData,
        }
    }
    
    /// Set the parent primary key for lazy loading
    pub fn with_parent_pk(mut self, pk: serde_json::Value) -> Self {
        self.parent_pk = Some(pk);
        self
    }
    
    /// Load all related models
    pub async fn load(&self) -> Result<Vec<E>> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        
        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .get()
            .await
    }
    
    /// Load related models with custom constraints
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        
        let query = E::query().where_eq(self.foreign_key, pk.clone());
        constraint_fn(query).get().await
    }
    
    /// Count related models
    pub async fn count(&self) -> Result<u64> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        
        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .count()
            .await
    }
    
    /// Check if any related models exist
    pub async fn exists(&self) -> Result<bool> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        
        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .exists()
            .await
    }
    
    /// Get the cached values if already loaded
    pub fn get_cached(&self) -> Option<&[E]> {
        self.cached.as_deref()
    }
}

impl<E: Model> Default for HasMany<E> {
    fn default() -> Self {
        Self {
            foreign_key: "id",
            local_key: "id",
            cached: None,
            parent_pk: None,
            _marker: PhantomData,
        }
    }
}

impl<E: Model + Serialize> Serialize for HasMany<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for HasMany<E> {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::default())
    }
}

/// BelongsTo relation type - represents the inverse of HasOne/HasMany
///
/// Use this as a field type in your model to define a belongs_to relation.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Model)]
/// pub struct Post {
///     pub id: i64,
///     pub user_id: i64,
///     pub title: String,
///     
///     #[tide(belongs_to = "User", foreign_key = "user_id")]
///     pub author: BelongsTo<User>,
/// }
/// ```
#[derive(Debug, Clone)]
pub struct BelongsTo<E: Model> {
    /// Foreign key column on THIS model
    pub foreign_key: &'static str,
    /// Owner key on the related model (usually the primary key)
    pub owner_key: &'static str,
    /// Cached related model (loaded via `.load()`)
    cached: Option<Box<E>>,
    /// Foreign key value for loading
    fk_value: Option<serde_json::Value>,
    _marker: PhantomData<E>,
}

impl<E: Model> BelongsTo<E> {
    /// Create a new BelongsTo relation
    pub fn new(foreign_key: &'static str, owner_key: &'static str) -> Self {
        Self {
            foreign_key,
            owner_key,
            cached: None,
            fk_value: None,
            _marker: PhantomData,
        }
    }
    
    /// Set the foreign key value for lazy loading
    pub fn with_fk_value(mut self, fk: serde_json::Value) -> Self {
        self.fk_value = Some(fk);
        self
    }
    
    /// Load the related model
    pub async fn load(&self) -> Result<Option<E>> {
        let fk = self.fk_value.as_ref()
            .ok_or_else(|| Error::query(String::from("Foreign key value not set for relation")))?;
        
        E::query()
            .where_eq(self.owner_key, fk.clone())
            .first()
            .await
    }
    
    /// Load the related model with custom constraints
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Option<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        let fk = self.fk_value.as_ref()
            .ok_or_else(|| Error::query(String::from("Foreign key value not set for relation")))?;
        
        let query = E::query().where_eq(self.owner_key, fk.clone());
        constraint_fn(query).first().await
    }
    
    /// Check if the relation exists
    pub async fn exists(&self) -> Result<bool> {
        let fk = self.fk_value.as_ref()
            .ok_or_else(|| Error::query(String::from("Foreign key value not set for relation")))?;
        
        E::query()
            .where_eq(self.owner_key, fk.clone())
            .exists()
            .await
    }
    
    /// Get the cached value if already loaded
    pub fn get_cached(&self) -> Option<&E> {
        self.cached.as_deref()
    }
}

impl<E: Model> Default for BelongsTo<E> {
    fn default() -> Self {
        Self {
            foreign_key: "id",
            owner_key: "id",
            cached: None,
            fk_value: None,
            _marker: PhantomData,
        }
    }
}

impl<E: Model + Serialize> Serialize for BelongsTo<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for BelongsTo<E> {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::default())
    }
}

// =============================================================================
// MANY-TO-MANY RELATIONS
// =============================================================================

/// HasManyThrough relation - many-to-many through a pivot table
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Model)]
/// pub struct User {
///     pub id: i64,
///     pub name: String,
///     
///     #[tide(has_many_through = "Role", pivot = "user_roles", foreign_key = "user_id", related_key = "role_id")]
///     pub roles: HasManyThrough<Role, UserRole>,
/// }
/// ```
#[derive(Debug, Clone)]
pub struct HasManyThrough<Related: Model, Pivot: Model> {
    /// Foreign key on pivot table pointing to this model
    pub foreign_key: &'static str,
    /// Related key on pivot table pointing to related model
    pub related_key: &'static str,
    /// Local key on this model (usually primary key)
    pub local_key: &'static str,
    /// Related model's local key (usually primary key)
    pub related_local_key: &'static str,
    /// Pivot table name
    pub pivot_table: &'static str,
    /// Cached related models
    cached: Option<Vec<Related>>,
    /// Parent model's primary key value
    parent_pk: Option<serde_json::Value>,
    _marker: PhantomData<(Related, Pivot)>,
}

impl<Related: Model, Pivot: Model> HasManyThrough<Related, Pivot> {
    /// Create a new HasManyThrough relation
    pub fn new(
        foreign_key: &'static str,
        related_key: &'static str,
        local_key: &'static str,
        related_local_key: &'static str,
        pivot_table: &'static str,
    ) -> Self {
        Self {
            foreign_key,
            related_key,
            local_key,
            related_local_key,
            pivot_table,
            cached: None,
            parent_pk: None,
            _marker: PhantomData,
        }
    }
    
    /// Set the parent primary key for lazy loading
    pub fn with_parent_pk(mut self, pk: serde_json::Value) -> Self {
        self.parent_pk = Some(pk);
        self
    }
    
    /// Load all related models
    pub async fn load(&self) -> Result<Vec<Related>> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        
        // Build a query that joins through the pivot table
        Related::query()
            .inner_join(
                self.pivot_table,
                &format!("{}.{}", self.pivot_table, self.related_key),
                &format!("{}.{}", Related::table_name(), self.related_local_key),
            )
            .where_raw(&format!("{}.{} = {}", self.pivot_table, self.foreign_key, pk))
            .get()
            .await
    }
    
    /// Load related models with custom constraints
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<Related>>
    where
        F: FnOnce(QueryBuilder<Related>) -> QueryBuilder<Related> + Send,
    {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        
        let query = Related::query()
            .inner_join(
                self.pivot_table,
                &format!("{}.{}", self.pivot_table, self.related_key),
                &format!("{}.{}", Related::table_name(), self.related_local_key),
            )
            .where_raw(&format!("{}.{} = {}", self.pivot_table, self.foreign_key, pk));
        
        constraint_fn(query).get().await
    }
    
    /// Count related models
    pub async fn count(&self) -> Result<u64> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        
        Pivot::query()
            .where_raw(&format!("{}.{} = {}", self.pivot_table, self.foreign_key, pk))
            .count()
            .await
    }
    
    /// Attach a related model (create pivot entry)
    pub async fn attach(&self, related_id: impl Into<serde_json::Value>) -> Result<()> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        
        let mut data = HashMap::new();
        data.insert(self.foreign_key.to_string(), pk.clone());
        data.insert(self.related_key.to_string(), related_id.into());
        
        // Build INSERT SQL
        let columns: Vec<&str> = data.keys().map(|s| s.as_str()).collect();
        let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("${}", i)).collect();
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            self.pivot_table,
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
    
    /// Detach a related model (remove pivot entry)
    pub async fn detach(&self, related_id: impl Into<serde_json::Value>) -> Result<u64> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        
        Pivot::query()
            .where_eq(self.foreign_key, pk.clone())
            .where_eq(self.related_key, related_id.into())
            .delete()
            .await
    }
    
    /// Sync related models (replace all with new set)
    pub async fn sync(&self, related_ids: Vec<serde_json::Value>) -> Result<()> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        
        // Delete all existing pivot entries
        Pivot::query()
            .where_eq(self.foreign_key, pk.clone())
            .delete()
            .await?;
        
        // Insert new pivot entries
        for id in related_ids {
            self.attach(id).await?;
        }
        
        Ok(())
    }
    
    /// Get the cached values if already loaded
    pub fn get_cached(&self) -> Option<&[Related]> {
        self.cached.as_deref()
    }
}

impl<Related: Model, Pivot: Model> Default for HasManyThrough<Related, Pivot> {
    fn default() -> Self {
        Self {
            foreign_key: "id",
            related_key: "id",
            local_key: "id",
            related_local_key: "id",
            pivot_table: "",
            cached: None,
            parent_pk: None,
            _marker: PhantomData,
        }
    }
}

impl<Related: Model + Serialize, Pivot: Model> Serialize for HasManyThrough<Related, Pivot> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, Related: Model, Pivot: Model> Deserialize<'de> for HasManyThrough<Related, Pivot> {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::default())
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

/// MorphTo relation - polymorphic belongs-to (single parent of multiple types)
#[derive(Debug, Clone)]
pub struct MorphTo<Morphable> {
    /// Column storing the type (e.g., "commentable_type")
    pub type_column: &'static str,
    /// Column storing the foreign id (e.g., "commentable_id")
    pub id_column: &'static str,
    /// Type value (e.g., "posts")
    type_value: Option<String>,
    /// ID value
    id_value: Option<serde_json::Value>,
    _marker: PhantomData<Morphable>,
}

impl<Morphable> MorphTo<Morphable> {
    /// Create a new MorphTo relation
    pub fn new(type_column: &'static str, id_column: &'static str) -> Self {
        Self {
            type_column,
            id_column,
            type_value: None,
            id_value: None,
            _marker: PhantomData,
        }
    }
    
    /// Set the type and id values for loading
    pub fn with_values(mut self, type_value: String, id_value: serde_json::Value) -> Self {
        self.type_value = Some(type_value);
        self.id_value = Some(id_value);
        self
    }
}

impl<Morphable> Default for MorphTo<Morphable> {
    fn default() -> Self {
        Self {
            type_column: "",
            id_column: "",
            type_value: None,
            id_value: None,
            _marker: PhantomData,
        }
    }
}

impl<Morphable: Serialize> Serialize for MorphTo<Morphable> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_none()
    }
}

impl<'de, Morphable> Deserialize<'de> for MorphTo<Morphable> {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::default())
    }
}

/// MorphOne relation - polymorphic has-one
#[derive(Debug, Clone)]
pub struct MorphOne<Related: Model> {
    /// The morph name (e.g., "imageable")
    pub morph_name: &'static str,
    /// Local key on this model
    pub local_key: &'static str,
    /// Cached related model
    cached: Option<Box<Related>>,
    /// Parent model's primary key value
    parent_pk: Option<serde_json::Value>,
    /// Parent model's table name (for type value)
    parent_table: Option<String>,
    _marker: PhantomData<Related>,
}

impl<Related: Model> MorphOne<Related> {
    /// Create a new MorphOne relation
    pub fn new(morph_name: &'static str, local_key: &'static str) -> Self {
        Self {
            morph_name,
            local_key,
            cached: None,
            parent_pk: None,
            parent_table: None,
            _marker: PhantomData,
        }
    }
    
    /// Set the parent values for loading
    pub fn with_parent(mut self, pk: serde_json::Value, table: String) -> Self {
        self.parent_pk = Some(pk);
        self.parent_table = Some(table);
        self
    }
    
    /// Load the related model
    pub async fn load(&self) -> Result<Option<Related>> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let table = self.parent_table.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent table not set for relation")))?;
        
        let type_column = format!("{}_type", self.morph_name);
        let id_column = format!("{}_id", self.morph_name);
        
        Related::query()
            .where_eq(&type_column, table.clone())
            .where_eq(&id_column, pk.clone())
            .first()
            .await
    }
    
    /// Get the cached value if already loaded
    pub fn get_cached(&self) -> Option<&Related> {
        self.cached.as_deref()
    }
}

impl<Related: Model> Default for MorphOne<Related> {
    fn default() -> Self {
        Self {
            morph_name: "",
            local_key: "id",
            cached: None,
            parent_pk: None,
            parent_table: None,
            _marker: PhantomData,
        }
    }
}

impl<Related: Model + Serialize> Serialize for MorphOne<Related> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, Related: Model> Deserialize<'de> for MorphOne<Related> {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::default())
    }
}

/// MorphMany relation - polymorphic has-many
#[derive(Debug, Clone)]
pub struct MorphMany<Related: Model> {
    /// The morph name (e.g., "commentable")
    pub morph_name: &'static str,
    /// Local key on this model
    pub local_key: &'static str,
    /// Cached related models
    cached: Option<Vec<Related>>,
    /// Parent model's primary key value
    parent_pk: Option<serde_json::Value>,
    /// Parent model's table name (for type value)
    parent_table: Option<String>,
    _marker: PhantomData<Related>,
}

impl<Related: Model> MorphMany<Related> {
    /// Create a new MorphMany relation
    pub fn new(morph_name: &'static str, local_key: &'static str) -> Self {
        Self {
            morph_name,
            local_key,
            cached: None,
            parent_pk: None,
            parent_table: None,
            _marker: PhantomData,
        }
    }
    
    /// Set the parent values for loading
    pub fn with_parent(mut self, pk: serde_json::Value, table: String) -> Self {
        self.parent_pk = Some(pk);
        self.parent_table = Some(table);
        self
    }
    
    /// Load all related models
    pub async fn load(&self) -> Result<Vec<Related>> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let table = self.parent_table.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent table not set for relation")))?;
        
        let type_column = format!("{}_type", self.morph_name);
        let id_column = format!("{}_id", self.morph_name);
        
        Related::query()
            .where_eq(&type_column, table.clone())
            .where_eq(&id_column, pk.clone())
            .get()
            .await
    }
    
    /// Load related models with custom constraints
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<Related>>
    where
        F: FnOnce(QueryBuilder<Related>) -> QueryBuilder<Related> + Send,
    {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let table = self.parent_table.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent table not set for relation")))?;
        
        let type_column = format!("{}_type", self.morph_name);
        let id_column = format!("{}_id", self.morph_name);
        
        let query = Related::query()
            .where_eq(&type_column, table.clone())
            .where_eq(&id_column, pk.clone());
        
        constraint_fn(query).get().await
    }
    
    /// Count related models
    pub async fn count(&self) -> Result<u64> {
        let pk = self.parent_pk.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let table = self.parent_table.as_ref()
            .ok_or_else(|| Error::query(String::from("Parent table not set for relation")))?;
        
        let type_column = format!("{}_type", self.morph_name);
        let id_column = format!("{}_id", self.morph_name);
        
        Related::query()
            .where_eq(&type_column, table.clone())
            .where_eq(&id_column, pk.clone())
            .count()
            .await
    }
    
    /// Get the cached values if already loaded
    pub fn get_cached(&self) -> Option<&[Related]> {
        self.cached.as_deref()
    }
}

impl<Related: Model> Default for MorphMany<Related> {
    fn default() -> Self {
        Self {
            morph_name: "",
            local_key: "id",
            cached: None,
            parent_pk: None,
            parent_table: None,
            _marker: PhantomData,
        }
    }
}

impl<Related: Model + Serialize> Serialize for MorphMany<Related> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, Related: Model> Deserialize<'de> for MorphMany<Related> {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::default())
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
    /// First variant
    TypeA(A),
    /// Second variant
    TypeB(B),
    /// Third variant
    TypeC(C),
    /// Unknown type (stored as JSON)
    Unknown(serde_json::Value),
}

/// Four-variant polymorphic result
#[derive(Debug, Clone)]
pub enum MorphResult4<A, B, C, D> {
    /// First variant
    TypeA(A),
    /// Second variant
    TypeB(B),
    /// Third variant
    TypeC(C),
    /// Fourth variant
    TypeD(D),
    /// Unknown type (stored as JSON)
    Unknown(serde_json::Value),
}

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
    
    /// Get depth of nesting
    pub fn depth(&self) -> usize {
        self.segments.len()
    }
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

/// Query builder with eager loading support
pub struct EagerQueryBuilder<M: Model> {
    query: QueryBuilder<M>,
    relation_tree: RelationTree,
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
    pub fn with(mut self, relation: &str) -> Self {
        let path = RelationPath::parse(relation);
        self.relation_tree.add_path(&path);
        self
    }
    
    /// Add multiple relations at once
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
    pub fn order_by(mut self, column: &str, order: Order) -> Self {
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
    
    /// Get the relation tree
    pub fn get_relation_tree(&self) -> &RelationTree {
        &self.relation_tree
    }
    
    /// Execute and get all results with loaded relations
    pub async fn get(self) -> Result<Vec<WithRelations<M>>> {
        let models = self.query.get().await?;
        
        let results: Vec<WithRelations<M>> = models
            .into_iter()
            .map(WithRelations::new)
            .collect();
        
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

/// Relation loader configuration
pub struct RelationLoader<M> {
    /// Name of the relation to load
    pub name: String,
    /// Loader function
    pub loader: Box<dyn Fn(&[M]) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<HashMap<String, serde_json::Value>>> + Send>> + Send + Sync>,
}

// =============================================================================
// RELATION INFO (for schema and introspection)
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
    
    /// Create a HasManyThrough relation info
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
            morph_type_column: Some(related_key.to_string()),
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
    /// Many-to-one relation
    BelongsTo,
    /// One-to-one relation
    HasOne,
    /// One-to-many relation
    HasMany,
    /// Many-to-many through pivot table
    HasManyThrough,
    /// Polymorphic many-to-one
    MorphTo,
    /// Polymorphic one-to-one
    MorphOne,
    /// Polymorphic one-to-many
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

// =============================================================================
// EXTENSION TRAITS FOR MODELS
// =============================================================================

/// Extension trait for models to support eager loading syntax
pub trait EagerLoadExt: Model {
    /// Start an eager loading query builder
    fn eager() -> EagerQueryBuilder<Self>
    where
        Self: Sized,
    {
        EagerQueryBuilder::new()
    }
    
    /// Start with a specific relation
    fn with_relation(relation_name: &str) -> EagerQueryBuilder<Self>
    where
        Self: Sized,
    {
        EagerQueryBuilder::new().with(relation_name)
    }
    
    /// Start with multiple relations
    fn with_relations(relations: &[&str]) -> EagerQueryBuilder<Self>
    where
        Self: Sized,
    {
        EagerQueryBuilder::new().with_many(relations)
    }
}

// Implement for all Models
impl<T: Model> EagerLoadExt for T {}

/// Extension trait providing relation query methods on models (for backward compatibility)
#[async_trait]
pub trait RelationExt: Model {
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
