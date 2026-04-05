// ============================================================================
// Integration tests for Fluent OR API with database queries
// ============================================================================

use std::time::Duration;
use tideorm::prelude::*;
use tideorm::{Database, TideConfig};

fn test_database_url() -> &'static str {
    let _ = dotenvy::dotenv();
    Box::leak(
        std::env::var("POSTGRESQL_DATABASE_URL")
            .unwrap_or_else(|_| {
                "postgres://postgres:postgres@localhost:5432/test_tide_orm".to_string()
            })
            .into_boxed_str(),
    )
}

#[derive(Model, PartialEq)]
#[tideorm(table = "or_test_users")]
pub struct OrTestUser {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub email: String,
    pub role: String,
    pub department: String,
    pub age: i32,
    pub active: bool,
    pub verified: bool,
}

#[tokio::test]
async fn test_all_fluent_or_scenarios() {
    println!("\n========================================");
    println!(" Fluent OR API Integration Tests");
    println!("========================================\n");

    TideConfig::init()
        .database(test_database_url())
        .max_connections(10)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(30))
        .connect()
        .await
        .expect("Failed to connect to database");

    let _ = Database::execute("DROP TABLE IF EXISTS or_test_users CASCADE").await;

    Database::execute(
        r#"
        CREATE TABLE or_test_users (
            id BIGSERIAL PRIMARY KEY,
            name VARCHAR(255) NOT NULL,
            email VARCHAR(255) NOT NULL,
            role VARCHAR(50) NOT NULL,
            department VARCHAR(100) NOT NULL,
            age INTEGER NOT NULL,
            active BOOLEAN NOT NULL DEFAULT true,
            verified BOOLEAN NOT NULL DEFAULT false
        )
    "#,
    )
    .await
    .expect("Failed to create table");

    let test_users = vec![
        (
            "Alice Admin",
            "alice@example.com",
            "admin",
            "Engineering",
            30,
            true,
            true,
        ),
        (
            "Bob Admin",
            "bob@example.com",
            "admin",
            "Marketing",
            35,
            true,
            false,
        ),
        (
            "Carl Admin Inactive",
            "carl@example.com",
            "admin",
            "Sales",
            28,
            false,
            true,
        ),
        (
            "Diana Mod",
            "diana@example.com",
            "moderator",
            "Support",
            25,
            true,
            true,
        ),
        (
            "Eve Mod Inactive",
            "eve@example.com",
            "moderator",
            "HR",
            40,
            false,
            true,
        ),
        (
            "Frank Mod",
            "frank@example.com",
            "moderator",
            "Engineering",
            32,
            true,
            false,
        ),
        (
            "Grace Editor",
            "grace@example.com",
            "editor",
            "Marketing",
            27,
            true,
            true,
        ),
        (
            "Henry Editor Inactive",
            "henry@example.com",
            "editor",
            "Sales",
            45,
            false,
            false,
        ),
        (
            "Ivy Editor",
            "ivy@example.com",
            "editor",
            "Engineering",
            29,
            true,
            true,
        ),
        (
            "Jack User",
            "jack@example.com",
            "user",
            "Support",
            22,
            true,
            false,
        ),
        (
            "Kate User Inactive",
            "kate@example.com",
            "user",
            "HR",
            38,
            false,
            true,
        ),
        (
            "Leo User",
            "leo@example.com",
            "user",
            "Engineering",
            31,
            true,
            true,
        ),
        (
            "Mike Guest",
            "mike@example.com",
            "guest",
            "Marketing",
            24,
            true,
            false,
        ),
        (
            "Nancy Guest Inactive",
            "nancy@example.com",
            "guest",
            "Sales",
            50,
            false,
            false,
        ),
    ];

    for (name, email, role, dept, age, active, verified) in test_users {
        let user = OrTestUser {
            id: 0,
            name: name.to_string(),
            email: email.to_string(),
            role: role.to_string(),
            department: dept.to_string(),
            age,
            active,
            verified,
        };
        let _ = OrTestUser::create(user).await;
    }

    println!(" Setup complete: 14 test users created\n");

    println!("TEST 1: Simple OR with multiple roles");
    println!("--------------------------------------");

    let query = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .or_where_eq("role", "moderator")
        .or_where_eq("role", "editor")
        .end_or();

    let sql = query.build_sql_preview();
    println!("SQL: {}", sql);

    let results = query.get().await.expect("Query failed");
    println!("Results: {} users", results.len());

    assert_eq!(
        results.len(),
        9,
        "Expected 9 users (3 admin + 3 moderator + 3 editor)"
    );

    for user in &results {
        assert!(
            user.role == "admin" || user.role == "moderator" || user.role == "editor",
            "Unexpected role: {}",
            user.role
        );
    }
    println!(" PASSED\n");

    println!("TEST 2: User's exact example - OR with AND conditions");
    println!("------------------------------------------------------");
    println!("Pattern: where_eq(active, true)");
    println!("         .begin_or()");
    println!("           .or_where_eq(role, admin).and_where_eq(active, true)");
    println!("           .or_where_eq(role, moderator).and_where_eq(active, false)");
    println!("           .or_where_eq(role, editor)");
    println!("         .end_or()");

    let query = OrTestUser::query()
        .where_eq("active", true)
        .begin_or()
        .or_where_eq("role", "admin")
        .and_where_eq("active", true)
        .or_where_eq("role", "moderator")
        .and_where_eq("active", false)
        .or_where_eq("role", "editor")
        .end_or();

    let sql = query.build_sql_preview();
    println!("SQL: {}", sql);

    let results = query.get().await.expect("Query failed");
    println!("Results: {} users", results.len());

    for user in &results {
        println!(
            "  - {} (role: {}, active: {})",
            user.name, user.role, user.active
        );
    }

    for user in &results {
        assert!(user.active, "User {} should be active", user.name);
        assert!(
            user.role == "admin" || user.role == "editor",
            "User {} has unexpected role {}",
            user.name,
            user.role
        );
    }
    println!(" PASSED\n");

    println!("TEST 3: Privileged active users - business logic");
    println!("-------------------------------------------------");

    let query = OrTestUser::query()
        .where_eq("active", true)
        .begin_or()
        .or_where_eq("role", "admin")
        .and_where_eq("verified", true)
        .or_where_eq("role", "moderator")
        .and_where_eq("department", "Engineering")
        .or_where_eq("role", "editor")
        .end_or();

    let sql = query.build_sql_preview();
    println!("SQL: {}", sql);

    let results = query.get().await.expect("Query failed");
    println!("Results: {} users", results.len());

    for user in &results {
        println!(
            "  - {} (role: {}, dept: {}, verified: {})",
            user.name, user.role, user.department, user.verified
        );
    }

    assert_eq!(results.len(), 4, "Expected 4 privileged active users");

    for user in &results {
        assert!(user.active, "Must be active");
        let matches_criteria = (user.role == "admin" && user.verified)
            || (user.role == "moderator" && user.department == "Engineering")
            || user.role == "editor";
        assert!(
            matches_criteria,
            "User {} doesn't match criteria",
            user.name
        );
    }
    println!(" PASSED\n");

    println!("TEST 4: Age-based OR conditions");
    println!("--------------------------------");

    let query = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .and_where_lt("age", 30)
        .or_where_eq("role", "moderator")
        .and_where_gt("age", 35)
        .or_where_eq("role", "editor")
        .and_where_eq("verified", true)
        .end_or();

    let sql = query.build_sql_preview();
    println!("SQL: {}", sql);

    let results = query.get().await.expect("Query failed");
    println!("Results: {} users", results.len());

    for user in &results {
        println!(
            "  - {} (role: {}, age: {}, verified: {})",
            user.name, user.role, user.age, user.verified
        );
    }

    for user in &results {
        let matches = (user.role == "admin" && user.age < 30)
            || (user.role == "moderator" && user.age > 35)
            || (user.role == "editor" && user.verified);
        assert!(matches, "User {} doesn't match age criteria", user.name);
    }
    println!(" PASSED\n");

    println!("TEST 5: Multiple AND conditions per branch");
    println!("-------------------------------------------");

    let query = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .and_where_eq("active", true)
        .and_where_eq("verified", true)
        .and_where_gt("age", 25)
        .or_where_eq("role", "moderator")
        .and_where_eq("active", true)
        .and_where_eq("department", "Engineering")
        .or_where_eq("role", "editor")
        .and_where_eq("verified", true)
        .end_or();

    let sql = query.build_sql_preview();
    println!("SQL: {}", sql);

    let results = query.get().await.expect("Query failed");
    println!("Results: {} users", results.len());

    for user in &results {
        println!(
            "  - {} (role: {}, active: {}, verified: {}, age: {}, dept: {})",
            user.name, user.role, user.active, user.verified, user.age, user.department
        );
    }

    for user in &results {
        let branch1 = user.role == "admin" && user.active && user.verified && user.age > 25;
        let branch2 = user.role == "moderator" && user.active && user.department == "Engineering";
        let branch3 = user.role == "editor" && user.verified;
        assert!(
            branch1 || branch2 || branch3,
            "User {} doesn't match any branch",
            user.name
        );
    }
    println!(" PASSED\n");

    println!("TEST 6: OR with IN conditions");
    println!("------------------------------");

    let query = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .and_where_in("department", vec!["Engineering", "Marketing"])
        .or_where_in("department", vec!["HR", "Support"])
        .end_or();

    let sql = query.build_sql_preview();
    println!("SQL: {}", sql);

    let results = query.get().await.expect("Query failed");
    println!("Results: {} users", results.len());

    for user in &results {
        println!(
            "  - {} (role: {}, dept: {})",
            user.name, user.role, user.department
        );
    }

    for user in &results {
        let matches = (user.role == "admin"
            && (user.department == "Engineering" || user.department == "Marketing"))
            || (user.department == "HR" || user.department == "Support");
        assert!(matches, "User {} doesn't match IN criteria", user.name);
    }
    println!(" PASSED\n");

    println!("TEST 7: OR with BETWEEN conditions");
    println!("-----------------------------------");

    let query = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .and_where_between("age", 25, 35)
        .or_where_eq("role", "moderator")
        .and_where_between("age", 30, 45)
        .end_or();

    let sql = query.build_sql_preview();
    println!("SQL: {}", sql);

    let results = query.get().await.expect("Query failed");
    println!("Results: {} users", results.len());

    for user in &results {
        println!("  - {} (role: {}, age: {})", user.name, user.role, user.age);
    }

    for user in &results {
        let matches = (user.role == "admin" && user.age >= 25 && user.age <= 35)
            || (user.role == "moderator" && user.age >= 30 && user.age <= 45);
        assert!(
            matches,
            "User {} (age: {}) doesn't match BETWEEN criteria",
            user.name, user.age
        );
    }
    println!(" PASSED\n");

    println!("TEST 8: Count with OR conditions");
    println!("---------------------------------");

    let query = OrTestUser::query()
        .where_eq("active", true)
        .begin_or()
        .or_where_eq("role", "admin")
        .or_where_eq("role", "moderator")
        .end_or();

    let sql = query.build_sql_preview();
    println!("SQL: {}", sql);

    let count = query.count().await.expect("Count failed");
    println!("Count: {}", count);

    let verify_query = OrTestUser::query()
        .where_eq("active", true)
        .begin_or()
        .or_where_eq("role", "admin")
        .or_where_eq("role", "moderator")
        .end_or();

    let results = verify_query.get().await.expect("Query failed");
    assert_eq!(
        count as usize,
        results.len(),
        "Count should match actual results"
    );
    println!(" PASSED\n");

    println!("TEST 9: First with OR conditions");
    println!("---------------------------------");

    let query = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .or_where_eq("role", "moderator")
        .end_or()
        .order_by("name", Order::Asc);

    let sql = query.build_sql_preview();
    println!("SQL: {}", sql);

    let first = query.first().await.expect("First failed");
    assert!(first.is_some(), "Should find at least one user");

    if let Some(user) = first {
        println!("First user: {} (role: {})", user.name, user.role);
        assert!(user.role == "admin" || user.role == "moderator");
    }
    println!(" PASSED\n");

    println!("TEST 10: Single branch OR");
    println!("--------------------------");

    let query = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .end_or();

    let sql = query.build_sql_preview();
    println!("SQL: {}", sql);

    let results = query.get().await.expect("Query failed");
    println!("Results: {} users", results.len());

    assert_eq!(results.len(), 3, "Should find 3 admins");
    for user in &results {
        assert_eq!(user.role, "admin", "User should be admin");
    }
    println!(" PASSED\n");

    println!("TEST 11: OR with ORDER BY and LIMIT");
    println!("------------------------------------");

    let query = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .or_where_eq("role", "moderator")
        .or_where_eq("role", "editor")
        .end_or()
        .order_by("age", Order::Desc)
        .limit(5);

    let sql = query.build_sql_preview();
    println!("SQL: {}", sql);

    let results = query.get().await.expect("Query failed");
    println!("Results: {} users (limited to 5)", results.len());

    assert!(results.len() <= 5, "Should return at most 5 users");

    for i in 1..results.len() {
        assert!(
            results[i - 1].age >= results[i].age,
            "Results should be ordered by age descending"
        );
    }

    for user in &results {
        println!("  - {} (role: {}, age: {})", user.name, user.role, user.age);
    }
    println!(" PASSED\n");

    println!("TEST 12: Empty begin_or().end_or()");
    println!("-----------------------------------");

    let query = OrTestUser::query()
        .where_eq("active", true)
        .begin_or()
        .end_or();

    let sql = query.build_sql_preview();
    println!("SQL: {}", sql);

    let results = query.get().await.expect("Query failed");
    println!("Results: {} users", results.len());

    for user in &results {
        assert!(user.active, "User should be active");
    }
    println!(" PASSED\n");

    println!("TEST 13: SQL structure verification");
    println!("------------------------------------");

    let query = OrTestUser::query()
        .where_eq("active", true)
        .begin_or()
        .or_where_eq("role", "admin")
        .and_where_eq("verified", true)
        .or_where_eq("role", "moderator")
        .and_where_gt("age", 30)
        .end_or();

    let sql = query.build_sql_preview();
    println!("Full SQL: {}", sql);

    let sql_lower = sql.to_lowercase();

    assert!(sql_lower.contains("select"), "SQL should contain SELECT");
    assert!(sql_lower.contains("from"), "SQL should contain FROM");
    assert!(sql_lower.contains("where"), "SQL should contain WHERE");
    assert!(
        sql_lower.contains("active"),
        "SQL should contain active column"
    );
    assert!(sql_lower.contains("role"), "SQL should contain role column");
    println!(" PASSED\n");

    println!("TEST 14: OR with LIKE conditions");
    println!("---------------------------------");

    let query = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .and_where_like("name", "A%")
        .or_where_like("email", "%example%")
        .end_or();

    let sql = query.build_sql_preview();
    println!("SQL: {}", sql);

    let results = query.get().await.expect("Query failed");
    println!("Results: {} users", results.len());

    assert!(!results.is_empty(), "Should find at least one user");

    for user in &results {
        let matches =
            (user.role == "admin" && user.name.starts_with("A")) || user.email.contains("example");
        assert!(matches, "User {} doesn't match LIKE criteria", user.name);
    }
    println!(" PASSED\n");

    println!("========================================");
    println!(" ALL 14 TESTS PASSED!");
    println!("========================================\n");

    let _ = Database::execute("DROP TABLE IF EXISTS or_test_users CASCADE").await;
}
