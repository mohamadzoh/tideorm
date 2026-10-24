// The order the hooks of a create, update and delete run in is checked end to
// end, on every backend, by `callbacks_run_in_order` in
// `tests/support/integration_parity.rs`.

use super::Callbacks;
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
