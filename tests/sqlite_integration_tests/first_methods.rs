use super::*;

pub(super) async fn run() {
    // =========================================================================
    // FIRST AND FIRST_OR_FAIL TESTS
    // =========================================================================
    println!("🎯 Testing: First Methods");
    {
        let _ = Database::execute("DELETE FROM test_users").await;

        TestUser {
            id: 0,
            email: "first@example.com".to_string(),
            name: "First".to_string(),
            age: 25,
            active: true,
        }
        .save()
        .await
        .unwrap();

        let first = TestUser::query().first().await.expect("First failed");
        assert!(first.is_some());
        println!("   ✓ first works");

        let first_or_fail = TestUser::query()
            .where_eq("email", "first@example.com")
            .first_or_fail()
            .await;
        assert!(first_or_fail.is_ok());
        println!("   ✓ first_or_fail works for existing");

        let not_found = TestUser::query()
            .where_eq("email", "nonexistent@example.com")
            .first_or_fail()
            .await;
        assert!(not_found.is_err());
        println!("   ✓ first_or_fail returns error for missing");
    }
    println!();
}
