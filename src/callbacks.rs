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
//! Implement `Callbacks` on your model type and override only the lifecycle
//! hooks you need.

use crate::error::Result;

/// Trait for model lifecycle callbacks
///
/// Implement this trait on your model to hook into lifecycle events.
/// All methods have default no-op implementations, so you only need
/// to override the ones you care about.
///
/// # Callback Order
///
/// For `save()` on a new model:
/// 1. `before_validation`
/// 2. `Validate::validate()`
/// 3. `after_validation`
/// 4. `before_save`
/// 5. `before_create`
/// 6. (actual INSERT)
/// 7. `after_create`
/// 8. `after_save`
///
/// For `save()` on an existing model, or `update()`:
/// 1. `before_validation`
/// 2. `Validate::validate()`
/// 3. `after_validation`
/// 4. `before_save`
/// 5. `before_update`
/// 6. (actual UPDATE)
/// 7. `after_update`
/// 8. `after_save`
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

#[doc(hidden)]
pub trait BeforeCreateDispatch<T> {
    fn run_before_create(self) -> Result<()>;
}

impl<T: CallbackRunner> BeforeCreateDispatch<T> for &mut T {
    fn run_before_create(self) -> Result<()> {
        self.run_create_callbacks()
    }
}

impl<T> BeforeCreateDispatch<T> for &&mut T {
    fn run_before_create(self) -> Result<()> {
        Ok(())
    }
}

#[doc(hidden)]
pub trait AfterCreateDispatch<T> {
    fn run_after_create(self) -> Result<()>;
}

impl<T: CallbackRunner> AfterCreateDispatch<T> for &T {
    fn run_after_create(self) -> Result<()> {
        self.run_after_create_callbacks()
    }
}

impl<T> AfterCreateDispatch<T> for &&T {
    fn run_after_create(self) -> Result<()> {
        Ok(())
    }
}

#[doc(hidden)]
pub trait BeforeValidationDispatch<T> {
    fn run_before_validation(self) -> Result<()>;
}

impl<T: Callbacks> BeforeValidationDispatch<T> for &mut T {
    fn run_before_validation(self) -> Result<()> {
        self.before_validation()
    }
}

impl<T> BeforeValidationDispatch<T> for &&mut T {
    fn run_before_validation(self) -> Result<()> {
        Ok(())
    }
}

#[doc(hidden)]
pub trait AfterValidationDispatch<T> {
    fn run_after_validation(self) -> Result<()>;
}

impl<T: Callbacks> AfterValidationDispatch<T> for &T {
    fn run_after_validation(self) -> Result<()> {
        self.after_validation()
    }
}

impl<T> AfterValidationDispatch<T> for &&T {
    fn run_after_validation(self) -> Result<()> {
        Ok(())
    }
}

#[doc(hidden)]
pub trait BeforeSaveDispatch<T> {
    fn run_before_save(self) -> Result<()>;
}

impl<T: Callbacks> BeforeSaveDispatch<T> for &mut T {
    fn run_before_save(self) -> Result<()> {
        self.before_save()
    }
}

impl<T> BeforeSaveDispatch<T> for &&mut T {
    fn run_before_save(self) -> Result<()> {
        Ok(())
    }
}

#[doc(hidden)]
pub trait BeforeCreateOnlyDispatch<T> {
    fn run_before_create_only(self) -> Result<()>;
}

impl<T: Callbacks> BeforeCreateOnlyDispatch<T> for &mut T {
    fn run_before_create_only(self) -> Result<()> {
        self.before_create()
    }
}

impl<T> BeforeCreateOnlyDispatch<T> for &&mut T {
    fn run_before_create_only(self) -> Result<()> {
        Ok(())
    }
}

#[doc(hidden)]
pub trait BeforeUpdateDispatch<T> {
    fn run_before_update(self) -> Result<()>;
}

impl<T: CallbackRunner> BeforeUpdateDispatch<T> for &mut T {
    fn run_before_update(self) -> Result<()> {
        self.run_update_callbacks()
    }
}

impl<T> BeforeUpdateDispatch<T> for &&mut T {
    fn run_before_update(self) -> Result<()> {
        Ok(())
    }
}

#[doc(hidden)]
pub trait BeforeUpdateOnlyDispatch<T> {
    fn run_before_update_only(self) -> Result<()>;
}

impl<T: Callbacks> BeforeUpdateOnlyDispatch<T> for &mut T {
    fn run_before_update_only(self) -> Result<()> {
        self.before_update()
    }
}

impl<T> BeforeUpdateOnlyDispatch<T> for &&mut T {
    fn run_before_update_only(self) -> Result<()> {
        Ok(())
    }
}

#[doc(hidden)]
pub trait AfterUpdateDispatch<T> {
    fn run_after_update(self) -> Result<()>;
}

impl<T: CallbackRunner> AfterUpdateDispatch<T> for &T {
    fn run_after_update(self) -> Result<()> {
        self.run_after_update_callbacks()
    }
}

impl<T> AfterUpdateDispatch<T> for &&T {
    fn run_after_update(self) -> Result<()> {
        Ok(())
    }
}

#[doc(hidden)]
pub trait BeforeDeleteDispatch<T> {
    fn run_before_delete(self) -> Result<()>;
}

impl<T: CallbackRunner> BeforeDeleteDispatch<T> for &T {
    fn run_before_delete(self) -> Result<()> {
        self.run_delete_callbacks()
    }
}

impl<T> BeforeDeleteDispatch<T> for &&T {
    fn run_before_delete(self) -> Result<()> {
        Ok(())
    }
}

#[doc(hidden)]
pub trait AfterDeleteDispatch<T> {
    fn run_after_delete(self) -> Result<()>;
}

impl<T: CallbackRunner> AfterDeleteDispatch<T> for &T {
    fn run_after_delete(self) -> Result<()> {
        self.run_after_delete_callbacks()
    }
}

impl<T> AfterDeleteDispatch<T> for &&T {
    fn run_after_delete(self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "testing/callbacks_tests.rs"]
mod tests;
