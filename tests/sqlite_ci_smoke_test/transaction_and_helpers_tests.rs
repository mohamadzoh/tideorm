use std::sync::{Mutex, OnceLock};

use super::*;

fn leaked_transaction_db_slot() -> &'static Mutex<Option<Database>> {
    static LEAKED_TRANSACTION_DB: OnceLock<Mutex<Option<Database>>> = OnceLock::new();
    LEAKED_TRANSACTION_DB.get_or_init(|| Mutex::new(None))
}

#[tokio::test]
async fn sqlite_direct_crud_helpers_remain_unchanged_for_regular_models() {
    fresh_table(CI_USERS_DDL).await;

    for (email, name) in [
        ("first@example.com", "First"),
        ("middle@example.com", "Middle"),
        ("last@example.com", "Last"),
    ] {
        CiUser {
            id: 0,
            email: email.to_string(),
            name: name.to_string(),
            active: true,
        }
        .save()
        .await
        .expect("failed to insert regular user");
    }

    let all_users = CiUser::all()
        .await
        .expect("failed to fetch all regular users");
    assert_eq!(all_users.len(), 3);

    let first_user = CiUser::first()
        .await
        .expect("failed to fetch first regular user")
        .expect("first regular user should exist");
    assert_eq!(first_user.email, "first@example.com");

    let last_user = CiUser::last()
        .await
        .expect("failed to fetch last regular user")
        .expect("last regular user should exist");
    assert_eq!(last_user.email, "last@example.com");

    let count = CiUser::count()
        .await
        .expect("failed to count regular users");
    assert_eq!(count, 3);

    let exists_any = CiUser::exists_any()
        .await
        .expect("failed to check regular users existence");
    assert!(exists_any);

    let page = CiUser::paginate(1, 10)
        .await
        .expect("failed to paginate regular users");
    assert_eq!(page.len(), 3);
}

#[tokio::test]
async fn sqlite_paginate_rejects_zero_page_number() {
    fresh_table(CI_USERS_DDL).await;

    CiUser {
        id: 0,
        email: "first@example.com".to_string(),
        name: "First".to_string(),
        active: true,
    }
    .save()
    .await
    .expect("failed to insert regular user");

    let paginate_err = CiUser::paginate(0, 10)
        .await
        .expect_err("paginate should reject page 0");
    assert!(matches!(paginate_err, Error::Validation { .. }));
    assert!(paginate_err.to_string().contains("page"));
    assert!(paginate_err.to_string().contains("at least 1"));

    let query_err = CiUser::query()
        .page(0, 10)
        .get()
        .await
        .expect_err("query builder should reject page 0");
    assert!(matches!(query_err, Error::Query { .. }));
    assert!(query_err.to_string().contains("page"));
    assert!(query_err.to_string().contains("at least 1"));
}

#[tokio::test]
async fn sqlite_transaction_model_methods_use_transaction_connection() {
    fresh_table(CI_USERS_DDL).await;

    let baseline = CiUser {
        id: 0,
        email: "baseline@example.com".to_string(),
        name: "Baseline".to_string(),
        active: true,
    }
    .save()
    .await
    .expect("failed to insert baseline user");

    let save_result: tideorm::Result<()> = CiUser::transaction(|_tx| {
        Box::pin(async move {
            CiUser {
                id: 0,
                email: "tx-save@example.com".to_string(),
                name: "Transaction Save".to_string(),
                active: true,
            }
            .save()
            .await?;

            Err(tideorm::Error::query("rollback save transaction"))
        })
    })
    .await;
    assert!(save_result.is_err(), "save transaction should roll back");

    let rolled_back_save = CiUser::query()
        .where_eq("email", "tx-save@example.com")
        .first()
        .await
        .expect("failed to query rolled back save");
    assert!(
        rolled_back_save.is_none(),
        "saved row should not persist after rollback"
    );

    let update_result: tideorm::Result<()> = CiUser::transaction(|_tx| {
        let baseline = baseline.clone();
        Box::pin(async move {
            CiUser {
                name: "Updated In Transaction".to_string(),
                ..baseline
            }
            .update()
            .await?;

            Err(tideorm::Error::query("rollback update transaction"))
        })
    })
    .await;
    assert!(
        update_result.is_err(),
        "update transaction should roll back"
    );

    let unchanged = CiUser::find(baseline.id)
        .await
        .expect("failed to reload baseline user")
        .expect("baseline user should still exist");
    assert_eq!(unchanged.name, "Baseline");

    let delete_result: tideorm::Result<()> = CiUser::transaction(|_tx| {
        let baseline = unchanged.clone();
        Box::pin(async move {
            baseline.delete().await?;
            Err(tideorm::Error::query("rollback delete transaction"))
        })
    })
    .await;
    assert!(
        delete_result.is_err(),
        "delete transaction should roll back"
    );

    let still_present = CiUser::find(baseline.id)
        .await
        .expect("failed to check rolled back delete")
        .expect("baseline user should remain after rollback");
    assert_eq!(still_present.email, "baseline@example.com");
}

#[tokio::test]
async fn sqlite_transaction_leak_on_error_returns_transaction_error() {
    connect_global().await;

    leaked_transaction_db_slot()
        .lock()
        .expect("leaked transaction slot lock poisoned")
        .take();

    let err = CiUser::transaction(|_tx| {
        Box::pin(async move {
            let leaked_db = tideorm::database::__current_db()
                .expect("transaction-scoped database should be available inside transaction");
            *leaked_transaction_db_slot()
                .lock()
                .expect("leaked transaction slot lock poisoned") = Some(leaked_db);

            Err::<(), _>(tideorm::Error::query(
                "rollback with leaked transaction handle",
            ))
        })
    })
    .await
    .expect_err("leaked transaction handle should surface as a transaction error");

    assert!(
        err.to_string()
            .contains("transaction handle leaked outside the transaction scope"),
        "unexpected error: {err}"
    );

    leaked_transaction_db_slot()
        .lock()
        .expect("leaked transaction slot lock poisoned")
        .take();
}
