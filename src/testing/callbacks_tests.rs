use super::*;
use crate::validation::{Validate, ValidationErrors};
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

impl Validate for HookedModel {
    fn validate(&self) -> std::result::Result<(), ValidationErrors> {
        self.events.borrow_mut().push("validate");
        Ok(())
    }
}

struct InvalidHookedModel {
    events: RefCell<Vec<&'static str>>,
}

impl InvalidHookedModel {
    fn new() -> Self {
        Self {
            events: RefCell::new(Vec::new()),
        }
    }

    fn events(&self) -> Vec<&'static str> {
        self.events.borrow().clone()
    }
}

impl Callbacks for InvalidHookedModel {
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

    fn before_create(&mut self) -> Result<()> {
        self.events.borrow_mut().push("before_create");
        Ok(())
    }
}

impl Validate for InvalidHookedModel {
    fn validate(&self) -> std::result::Result<(), ValidationErrors> {
        self.events.borrow_mut().push("validate");
        let mut errors = ValidationErrors::new();
        errors.add("name", "is invalid");
        Err(errors)
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
#[allow(clippy::unnecessary_mut_passed)]
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
            "validate",
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
            "validate",
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

#[test]
fn callback_dispatch_stops_create_chain_when_validation_fails() {
    let mut model = InvalidHookedModel::new();

    let err = (&mut model).run_before_create().unwrap_err();

    assert!(matches!(err, crate::Error::Validation { .. }));
    assert_eq!(model.events(), vec!["before_validation", "validate"]);
}
