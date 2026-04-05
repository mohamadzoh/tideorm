use super::*;

pub(super) async fn run() {
    // =========================================================================
    // AGGREGATION TESTS
    // =========================================================================
    println!("📊 Testing: Aggregations");
    {
        let sum = TestUser::query().sum("age").await.expect("Sum failed");
        // Ages: 21+22+23+24+25+26+27+28+29+30 = 255
        assert_eq!(sum as i64, 255);
        println!("   ✓ sum works");

        let avg = TestUser::query().avg("age").await.expect("Avg failed");
        assert!((avg - 25.5).abs() < 0.01);
        println!("   ✓ avg works");

        let min = TestUser::query().min("age").await.expect("Min failed");
        assert_eq!(min as i64, 21);
        println!("   ✓ min works");

        let max = TestUser::query().max("age").await.expect("Max failed");
        assert_eq!(max as i64, 30);
        println!("   ✓ max works");
    }
    println!();

    // =========================================================================
    // PROFILER INTEGRATION TESTS
    // =========================================================================
    println!("⏱️  Testing: Global Profiler Integration");
    {
        let first_user = TestUser::query()
            .order_by("id", Order::Asc)
            .first()
            .await
            .expect("Query failed")
            .expect("Expected at least one seeded user");

        let _ =
            assert_profiled_operation("TestUser::query().count()", TestUser::query().count()).await;
        let _ = assert_profiled_operation(
            "TestUser::query().count_distinct(\"active\")",
            TestUser::query().count_distinct("active"),
        )
        .await;
        let _ = assert_profiled_operation(
            "TestUser::query().sum(\"age\")",
            TestUser::query().sum("age"),
        )
        .await;
        let _ = assert_profiled_operation(
            "Database::raw::<TestUser>()",
            Database::raw::<TestUser>(
                "SELECT id, email, name, age, active FROM test_users ORDER BY id LIMIT 2",
            ),
        )
        .await;
        let _ = assert_profiled_operation(
            "Database::raw_with_params::<TestUser>()",
            Database::raw_with_params::<TestUser>(
                "SELECT id, email, name, age, active FROM test_users WHERE age > ?",
                vec![25.into()],
            ),
        )
        .await;
        let _ = assert_profiled_operation(
            "Database::execute()",
            Database::execute("UPDATE test_users SET active = active"),
        )
        .await;
        let _ = assert_profiled_operation(
            "Database::execute_with_params()",
            Database::execute_with_params(
                "UPDATE test_users SET name = ? WHERE id = ?",
                vec![first_user.name.clone().into(), first_user.id.into()],
            ),
        )
        .await;

        let updated_user = assert_profiled_operation("TestUser::update()", async move {
            let mut user = first_user;
            user.name = format!("{} (profiled)", user.name);
            user.update().await
        })
        .await;
        assert!(updated_user.name.ends_with("(profiled)"));

        let temp_user = assert_profiled_operation(
            "TestUser::save()",
            TestUser {
                id: 0,
                email: "profile-save@example.com".to_string(),
                name: "Profile Save".to_string(),
                age: 33,
                active: true,
            }
            .save(),
        )
        .await;
        let _ = assert_profiled_operation("TestUser::delete()", temp_user.delete()).await;

        let destroy_target = TestUser {
            id: 0,
            email: "profile-destroy@example.com".to_string(),
            name: "Profile Destroy".to_string(),
            age: 44,
            active: false,
        }
        .save()
        .await
        .expect("Failed to create destroy target");
        let _ =
            assert_profiled_operation("TestUser::destroy()", TestUser::destroy(destroy_target.id))
                .await;

        println!("   ✓ profiler records raw SQL, parameterized SQL, aggregates, and CRUD paths");
    }
    println!();
}
