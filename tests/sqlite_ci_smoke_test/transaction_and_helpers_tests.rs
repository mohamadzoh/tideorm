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

struct RollbackProbe;
#[async_trait::async_trait]
impl tideorm::migration::Migration for RollbackProbe {
    fn version(&self) -> &str {
        "001"
    }
    fn name(&self) -> &str {
        "rollback probe"
    }
    async fn up(&self, _: &mut tideorm::migration::Schema) -> tideorm::Result<()> {
        Ok(())
    }
    async fn down(&self, _: &mut tideorm::migration::Schema) -> tideorm::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn missing_latest_migration_is_an_error_and_reset_does_not_silently_succeed() {
    connect_global().await;
    let migrator = tideorm::migration::Migrator::new().add(RollbackProbe);
    migrator.run().await.unwrap();
    Database::execute("INSERT INTO _migrations (version, name) VALUES ('002', 'missing')")
        .await
        .unwrap();
    let error = migrator.rollback().await.unwrap_err().to_string();
    assert!(
        error.contains("002") && error.contains("not registered"),
        "{error}"
    );
    assert!(migrator.rollback_steps(2).await.is_err());
    assert!(migrator.reset().await.is_err());
    assert!(migrator.status().await.unwrap()[0].applied);
}
