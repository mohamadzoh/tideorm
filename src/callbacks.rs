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
//! #[derive(Model)]
//! #[tideorm(table = "audit_logs")]
//! pub struct AuditLog {
//!     #[tideorm(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub action: String,
//!     pub entity_type: String,
//!     pub entity_id: i64,
//!     pub created_at: DateTime<Utc>,
//! }
//!
//! #[derive(Model)]
//! #[tideorm(table = "users")]
//! pub struct User {
//!     #[tideorm(primary_key, auto_increment)]
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
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct PlainModel;

    struct HookedModel {
        events: RefCell<Vec<&'static str>>,
    }

    impl HookedModel {
        fn new() -> Self {
            Self {
                events: RefCell::new(Vec::new()),
            }
        }

        fn events(&self) -> Vec<&'static str> {
            self.events.borrow().clone()
        }
    }

    impl Callbacks for HookedModel {
        fn before_validation(&mut self) -> Result<()> {
            self.events.borrow_mut().push("before_validation");
            Ok(())
        }

        fn after_validation(&self) -> Result<()> {
            self.events.borrow_mut().push("after_validation");
            Ok(())
        }

        fn before_save(&mut self) -> Result<()> {
            self.events.borrow_mut().push("before_save");
            Ok(())
        }

        fn after_save(&self) -> Result<()> {
            self.events.borrow_mut().push("after_save");
            Ok(())
        }

        fn before_create(&mut self) -> Result<()> {
            self.events.borrow_mut().push("before_create");
            Ok(())
        }

        fn after_create(&self) -> Result<()> {
            self.events.borrow_mut().push("after_create");
            Ok(())
        }

        fn before_update(&mut self) -> Result<()> {
            self.events.borrow_mut().push("before_update");
            Ok(())
        }

        fn after_update(&self) -> Result<()> {
            self.events.borrow_mut().push("after_update");
            Ok(())
        }

        fn before_delete(&self) -> Result<()> {
            self.events.borrow_mut().push("before_delete");
            Ok(())
        }

        fn after_delete(&self) -> Result<()> {
            self.events.borrow_mut().push("after_delete");
            Ok(())
        }
    }

    #[test]
    fn callback_dispatch_is_noop_for_models_without_callbacks() {
        let mut model = PlainModel;
        assert!((&mut &mut model).run_before_create().is_ok());
        assert!((&model).run_after_create().is_ok());
        assert!((&mut &mut model).run_before_update().is_ok());
        assert!((&model).run_after_update().is_ok());
        assert!((&model).run_before_delete().is_ok());
        assert!((&model).run_after_delete().is_ok());
    }

    #[test]
    fn callback_dispatch_runs_create_chain_in_order() {
        let mut model = HookedModel::new();
        (&mut model).run_before_create().unwrap();
        (&model).run_after_create().unwrap();

        assert_eq!(
            model.events(),
            vec![
                "before_validation",
                "after_validation",
                "before_save",
                "before_create",
                "after_create",
                "after_save"
            ]
        );
    }

    #[test]
    fn callback_dispatch_runs_update_and_delete_chains() {
        let mut model = HookedModel::new();
        (&mut model).run_before_update().unwrap();
        (&model).run_after_update().unwrap();
        (&model).run_before_delete().unwrap();
        (&model).run_after_delete().unwrap();

        assert_eq!(
            model.events(),
            vec![
                "before_validation",
                "after_validation",
                "before_save",
                "before_update",
                "after_update",
                "after_save",
                "before_delete",
                "after_delete"
            ]
        );
    }
}
