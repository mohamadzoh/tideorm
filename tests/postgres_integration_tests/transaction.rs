use super::*;

pub(super) async fn run() {
    // TRANSACTION TESTS
    // =========================================================================
    println!("Testing: Transactions");
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        // Transaction commit
        let result = TestUser::transaction(|_tx| {
            Box::pin(async move {
                let user = TestUser {
                    id: 0,
                    email: "tx_commit@example.com".to_string(),
                    name: "Transaction User".to_string(),
                    age: 25,
                    active: true,
                };
                let saved = user.save().await?;
                Ok(saved.id)
            })
        })
        .await;

        assert!(result.is_ok(), "Transaction should succeed");

        let found = TestUser::query()
            .where_eq("email", "tx_commit@example.com")
            .first()
            .await
            .expect("Query failed");
        assert!(found.is_some(), "User should exist after commit");
        println!("   OK transaction commit");

        // Transaction rollback
        let db = tideorm::require_db().unwrap();
        let result: tideorm::Result<i64> = TestUser::transaction(|_tx| {
            Box::pin(async move { Err(tideorm::Error::query("Intentional rollback")) })
        })
        .await;
        assert!(result.is_err(), "Transaction should fail");

        let result2: tideorm::Result<()> = db.transaction(|tx| Box::pin(async move {
            use sea_orm::ConnectionTrait;
            tx.__internal_transaction()
                .execute_unprepared("INSERT INTO test_users (email, name, age, active) VALUES ('tx_test@example.com', 'TX User', 30, true)")
                .await
                .map_err(|e| tideorm::Error::query(e.to_string()))?;
            Err(tideorm::Error::query("Intentional rollback"))
        })).await;
        assert!(result2.is_err(), "Transaction should fail");

        let found = TestUser::query()
            .where_eq("email", "tx_test@example.com")
            .first()
            .await
            .expect("Query failed");
        assert!(found.is_none(), "User should not exist after rollback");
        println!("   OK transaction rollback");

        let baseline = TestUser {
            id: 0,
            email: "tx_baseline@example.com".to_string(),
            name: "Baseline User".to_string(),
            age: 41,
            active: true,
        }
        .save()
        .await
        .expect("Failed to save baseline transaction user");

        let save_result: tideorm::Result<()> = TestUser::transaction(|_tx| {
            Box::pin(async move {
                TestUser {
                    id: 0,
                    email: "tx_model_save@example.com".to_string(),
                    name: "Transaction Save".to_string(),
                    age: 22,
                    active: true,
                }
                .save()
                .await?;

                Err(tideorm::Error::query("Intentional rollback after save"))
            })
        })
        .await;
        assert!(save_result.is_err(), "save transaction should roll back");

        let rolled_back_save = TestUser::query()
            .where_eq("email", "tx_model_save@example.com")
            .first()
            .await
            .expect("Failed to query rolled back save");
        assert!(
            rolled_back_save.is_none(),
            "saved model row should not persist after rollback"
        );
        println!("   OK transaction rollback via model save");

        let update_result: tideorm::Result<()> = TestUser::transaction(|_tx| {
            let baseline = TestUser {
                id: baseline.id,
                email: baseline.email.clone(),
                name: baseline.name.clone(),
                age: baseline.age,
                active: baseline.active,
            };
            Box::pin(async move {
                TestUser {
                    name: "Updated In Transaction".to_string(),
                    age: 99,
                    ..baseline
                }
                .update()
                .await?;

                Err(tideorm::Error::query("Intentional rollback after update"))
            })
        })
        .await;
        assert!(
            update_result.is_err(),
            "update transaction should roll back"
        );

        let unchanged = TestUser::find(baseline.id)
            .await
            .expect("Failed to reload baseline user")
            .expect("Baseline user should still exist");
        assert_eq!(unchanged.name, "Baseline User");
        assert_eq!(unchanged.age, 41);
        println!("   OK transaction rollback via model update");

        let delete_result: tideorm::Result<()> = TestUser::transaction(|_tx| {
            let baseline = TestUser {
                id: unchanged.id,
                email: unchanged.email.clone(),
                name: unchanged.name.clone(),
                age: unchanged.age,
                active: unchanged.active,
            };
            Box::pin(async move {
                baseline.delete().await?;
                Err(tideorm::Error::query("Intentional rollback after delete"))
            })
        })
        .await;
        assert!(
            delete_result.is_err(),
            "delete transaction should roll back"
        );

        let still_present = TestUser::find(baseline.id)
            .await
            .expect("Failed to reload baseline user after delete rollback")
            .expect("Baseline user should remain after delete rollback");
        assert_eq!(still_present.email, "tx_baseline@example.com");
        println!("   OK transaction rollback via model delete");
    }
    println!();

    // =========================================================================
}
