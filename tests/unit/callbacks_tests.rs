use super::*;
use crate::validation::{Validate, ValidationErrors};
use std::cell::RefCell;

// The tests below call the dispatchers with the same call shapes, in the same
// order, as the code the model derive emits for `create`, `update` and
// `delete`: `(&mut model).run_*()` before the write, `(&model).run_*()` after.

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
    assert!((&mut model).run_before_validation().is_ok());
    assert!((&model).run_after_validation().is_ok());
    assert!((&mut model).run_before_save().is_ok());
    assert!((&mut model).run_before_create_only().is_ok());
    assert!((&model).run_after_create().is_ok());
    assert!((&mut model).run_before_update_only().is_ok());
    assert!((&model).run_after_update().is_ok());
    assert!((&model).run_before_delete().is_ok());
    assert!((&model).run_after_delete().is_ok());
}

#[test]
fn callback_dispatch_runs_create_chain_in_order() {
    let mut model = HookedModel::new();
    (&mut model).run_before_validation().unwrap();
    Validate::validate(&model).unwrap();
    (&model).run_after_validation().unwrap();
    (&mut model).run_before_save().unwrap();
    (&mut model).run_before_create_only().unwrap();
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
    (&mut model).run_before_validation().unwrap();
    Validate::validate(&model).unwrap();
    (&model).run_after_validation().unwrap();
    (&mut model).run_before_save().unwrap();
    (&mut model).run_before_update_only().unwrap();
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

mod derived_model_dispatch {
    use crate::callbacks::Callbacks;
    use crate::model::Model;
    use std::sync::Mutex;

    static EVENTS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

    fn record(event: &'static str) -> crate::Result<()> {
        EVENTS.lock().unwrap().push(event);
        Ok(())
    }

    #[tideorm::model(table = "callback_dispatch_users")]
    struct RejectedUser {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        #[validate(min_length = 3)]
        name: String,
    }

    impl Callbacks for RejectedUser {
        fn before_validation(&mut self) -> crate::Result<()> {
            record("before_validation")
        }

        fn after_validation(&self) -> crate::Result<()> {
            record("after_validation")
        }

        fn before_save(&mut self) -> crate::Result<()> {
            record("before_save")
        }

        fn before_create(&mut self) -> crate::Result<()> {
            record("before_create")
        }
    }

    #[tokio::test]
    async fn a_failed_validation_stops_create_before_any_later_hook() {
        EVENTS.lock().unwrap().clear();

        // Validation fails before `create` asks for a connection, so no
        // database is needed.
        let error = RejectedUser::create(RejectedUser {
            id: 0,
            name: "x".to_string(),
        })
        .await
        .expect_err("a too-short name must fail validation");

        assert!(
            matches!(error, crate::Error::Validation { .. }),
            "{error:?}"
        );
        assert_eq!(*EVENTS.lock().unwrap(), vec!["before_validation"]);
    }
}
