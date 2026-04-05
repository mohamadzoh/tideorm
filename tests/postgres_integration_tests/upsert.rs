use super::*;

pub(super) async fn run() {
    // UPSERT / ON-CONFLICT TESTS
    // =========================================================================
    println!("Testing: Upsert / On-Conflict");
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;

        let user = TestUser {
            id: 1,
            email: "upsert@example.com".into(),
            name: "Initial Upsert".into(),
            age: 28,
            active: true,
        };
        let inserted = TestUser::insert_or_update(user, vec!["id"])
            .await
            .expect("insert_or_update should insert when missing");
        assert_eq!(
            inserted.id, 1,
            "Insert should respect primary key conflict target"
        );
        assert_eq!(inserted.name, "Initial Upsert");

        let user_update = TestUser {
            id: 1,
            email: "upsert@example.com".into(),
            name: "Updated Upsert".into(),
            age: 29,
            active: false,
        };
        let updated = TestUser::insert_or_update(user_update, vec!["id"])
            .await
            .expect("insert_or_update should update on conflict");
        assert_eq!(updated.id, 1, "Conflict should keep same primary key");
        assert_eq!(updated.name, "Updated Upsert");
        assert_eq!(updated.age, 29);
        assert!(
            !updated.active,
            "Active flag should update when included in update set"
        );

        let selective_model = TestUser {
            id: 1,
            email: "upsert@example.com".into(),
            name: "Selective Update".into(),
            age: 31,
            active: true,
        };
        let selective = TestUser::on_conflict(vec!["id"])
            .update_columns(vec!["name", "age"])
            .insert(selective_model)
            .await
            .expect("on_conflict builder should update chosen columns");
        assert_eq!(selective.id, 1);
        assert_eq!(selective.name, "Selective Update");
        assert_eq!(selective.age, 31);
        assert!(
            !selective.active,
            "Active should remain from previous update when column excluded"
        );

        let reloaded = TestUser::find(1)
            .await
            .expect("Reload failed")
            .expect("User should exist after upsert");
        assert_eq!(reloaded.name, "Selective Update");
        assert_eq!(reloaded.age, 31);
        assert!(
            !reloaded.active,
            "Active should be preserved when not updated"
        );

        let quoted_payload = "Robert'); DROP TABLE test_users; --";
        let injected_like = TestUser {
            id: 1,
            email: "upsert@example.com".into(),
            name: quoted_payload.into(),
            age: 32,
            active: true,
        };
        let quoted = TestUser::insert_or_update(injected_like, vec!["id"])
            .await
            .expect("upsert should treat quoted payload as data");
        assert_eq!(quoted.name, quoted_payload);

        println!("   OK insert_or_update and on_conflict");
    }
    println!();

    println!("Testing: Upsert With Timestamp Columns");
    {
        let _ = Database::execute("TRUNCATE TABLE timestamp_users RESTART IDENTITY CASCADE").await;

        let created_at = chrono::Utc::now();
        let updated_at = created_at + chrono::TimeDelta::minutes(15);

        let inserted = TimestampUser::insert_or_update(
            TimestampUser {
                id: 0,
                email: "typed-upsert@example.com".into(),
                name: "Initial Timestamp User".into(),
                login_count: 1,
                created_at,
                updated_at,
            },
            vec!["email"],
        )
        .await
        .expect("insert_or_update should preserve timestamp parameter types on insert");

        assert!(inserted.id > 0, "Upsert insert should assign a primary key");
        assert_eq!(inserted.email, "typed-upsert@example.com");
        assert_eq!(inserted.login_count, 1);
        assert!(
            inserted.created_at <= inserted.updated_at,
            "Auto-managed timestamps should remain ordered"
        );

        let next_updated_at = updated_at + chrono::TimeDelta::minutes(30);
        let updated = TimestampUser::insert_or_update(
            TimestampUser {
                id: inserted.id,
                email: "typed-upsert@example.com".into(),
                name: "Updated Timestamp User".into(),
                login_count: 2,
                created_at,
                updated_at: next_updated_at,
            },
            vec!["email"],
        )
        .await
        .expect("insert_or_update should preserve timestamp parameter types on conflict update");

        assert_eq!(updated.id, inserted.id);
        assert_eq!(updated.name, "Updated Timestamp User");
        assert_eq!(updated.login_count, 2);
        assert!(
            updated.created_at >= inserted.created_at,
            "Conflict updates should keep a valid created_at timestamp"
        );
        assert!(
            updated.updated_at >= inserted.updated_at,
            "Conflict updates should keep updated_at monotonic"
        );

        println!("   OK insert_or_update preserves timestamp column types");
    }
    println!();

    // =========================================================================
}
