//! Callbacks and Hooks for Model Lifecycle Events
//!
//! This module provides a trait-based callback system for model lifecycle events.
//!
//! ## Available Callbacks
//!
//! - `before_save` - Called before both create and update
//! - `after_save` - Called after both create and update
//! - `before_create` - Called before inserting a new record
//! - `after_create` - Called after inserting a new record
//! - `before_update` - Called before updating an existing record
//! - `after_update` - Called after updating an existing record
//! - `before_delete` - Called before deleting a record
//! - `after_delete` - Called after deleting a record
//! - `before_validation` - Called before validation runs
//! - `after_validation` - Called after validation passes
//!
//! ## Usage
//!
//! ```ignore
//! use tideorm::prelude::*;
//! use tideorm::callbacks::Callbacks;
//!
//! #[derive(Model, Clone, Debug, Serialize, Deserialize)]
//! #[tide(table = "audit_logs")]
//! pub struct AuditLog {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub action: String,
//!     pub entity_type: String,
//!     pub entity_id: i64,
//!     pub created_at: DateTime<Utc>,
//! }
//!
//! #[derive(Model, Clone, Debug, Serialize, Deserialize)]
//! #[tide(table = "users")]
//! pub struct User {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub name: String,
//!     pub email: String,
//!     pub password_hash: String,
//! }
//!
//! impl Callbacks for User {
//!     fn before_save(&mut self) -> tideorm::Result<()> {
//!         // Normalize email before saving
//!         self.email = self.email.to_lowercase();
//!         Ok(())
//!     }
//!     
//!     fn after_create(&self) -> tideorm::Result<()> {
//!         // Log the creation
//!         println!("User {} created with id {}", self.name, self.id);
//!         Ok(())
//!     }
//! }
//! ```

use crate::error::Result;

/// Trait for model lifecycle callbacks
///
/// Implement this trait on your model to hook into lifecycle events.
/// All methods have default no-op implementations, so you only need
/// to override the ones you care about.
///
/// # Callback Order
///
/// For `save()` (insert):
/// 1. `before_validation`
/// 2. `after_validation`
/// 3. `before_save`
/// 4. `before_create`
/// 5. (actual INSERT)
/// 6. `after_create`
/// 7. `after_save`
///
/// For `update()`:
/// 1. `before_validation`
/// 2. `after_validation`
/// 3. `before_save`
/// 4. `before_update`
/// 5. (actual UPDATE)
/// 6. `after_update`
/// 7. `after_save`
///
/// For `delete()`:
/// 1. `before_delete`
/// 2. (actual DELETE)
/// 3. `after_delete`
///
/// # Stopping the Chain
///
/// If any `before_*` callback returns `Err`, the operation is aborted
/// and the error is returned to the caller.
pub trait Callbacks: Sized {
    /// Called before validation runs
    /// 
    /// Use this to prepare data for validation.
    fn before_validation(&mut self) -> Result<()> {
        Ok(())
    }
    
    /// Called after validation passes
    ///
    /// Use this to perform actions that depend on valid data.
    fn after_validation(&self) -> Result<()> {
        Ok(())
    }
    
    /// Called before both create and update operations
    ///
    /// Use this for common pre-save logic like setting timestamps
    /// or normalizing data.
    fn before_save(&mut self) -> Result<()> {
        Ok(())
    }
    
    /// Called after both create and update operations complete
    ///
    /// Use this for post-save actions like sending notifications
    /// or updating caches.
    fn after_save(&self) -> Result<()> {
        Ok(())
    }
    
    /// Called before inserting a new record
    ///
    /// Use this for create-specific logic like generating UUIDs
    /// or setting default values.
    fn before_create(&mut self) -> Result<()> {
        Ok(())
    }
    
    /// Called after inserting a new record
    ///
    /// Use this for post-create actions like creating related records
    /// or triggering welcome emails.
    fn after_create(&self) -> Result<()> {
        Ok(())
    }
    
    /// Called before updating an existing record
    ///
    /// Use this for update-specific logic like tracking changes
    /// or incrementing version numbers.
    fn before_update(&mut self) -> Result<()> {
        Ok(())
    }
    
    /// Called after updating an existing record
    ///
    /// Use this for post-update actions like audit logging
    /// or cache invalidation.
    fn after_update(&self) -> Result<()> {
        Ok(())
    }
    
    /// Called before deleting a record
    ///
    /// Use this to prevent deletion or clean up related data.
    fn before_delete(&self) -> Result<()> {
        Ok(())
    }
    
    /// Called after deleting a record
    ///
    /// Use this for post-delete cleanup like removing files
    /// or updating statistics.
    fn after_delete(&self) -> Result<()> {
        Ok(())
    }
}

/// Helper trait to run callbacks around model operations
///
/// This is used internally by TideORM to execute callbacks.
/// You typically don't need to use this directly.
pub trait CallbackRunner: Callbacks {
    /// Run the full save (create) callback chain
    fn run_create_callbacks(&mut self) -> Result<()> {
        self.before_validation()?;
        self.after_validation()?;
        self.before_save()?;
        self.before_create()?;
        Ok(())
    }
    
    /// Run the post-create callbacks
    fn run_after_create_callbacks(&self) -> Result<()> {
        self.after_create()?;
        self.after_save()?;
        Ok(())
    }
    
    /// Run the full update callback chain
    fn run_update_callbacks(&mut self) -> Result<()> {
        self.before_validation()?;
        self.after_validation()?;
        self.before_save()?;
        self.before_update()?;
        Ok(())
    }
    
    /// Run the post-update callbacks
    fn run_after_update_callbacks(&self) -> Result<()> {
        self.after_update()?;
        self.after_save()?;
        Ok(())
    }
    
    /// Run the delete callback chain
    fn run_delete_callbacks(&self) -> Result<()> {
        self.before_delete()?;
        Ok(())
    }
    
    /// Run post-delete callbacks
    fn run_after_delete_callbacks(&self) -> Result<()> {
        self.after_delete()?;
        Ok(())
    }
}

// Automatically implement CallbackRunner for anything that implements Callbacks
impl<T: Callbacks> CallbackRunner for T {}

/// Blanket implementation of Callbacks for all types (no-op by default)
/// 
/// This allows models to work without explicitly implementing Callbacks.
/// Models that want custom callbacks should implement the trait themselves.
impl<T> Callbacks for T where T: Sized {}
