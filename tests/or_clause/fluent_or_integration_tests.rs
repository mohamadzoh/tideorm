use std::time::Duration;
use tideorm::prelude::*;
use tideorm::{Database, TideConfig};

use super::test_config::test_database_url;

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

// (name, role, department, age, active, verified); the email is `<first name>@example.com`.
const USERS: [(&str, &str, &str, i32, bool, bool); 14] = [
    ("Alice Admin", "admin", "Engineering", 30, true, true),
    ("Bob Admin", "admin", "Marketing", 35, true, false),
    ("Carl Admin Inactive", "admin", "Sales", 28, false, true),
    ("Diana Mod", "moderator", "Support", 25, true, true),
    ("Eve Mod Inactive", "moderator", "HR", 40, false, true),
    ("Frank Mod", "moderator", "Engineering", 32, true, false),
    ("Grace Editor", "editor", "Marketing", 27, true, true),
    ("Henry Editor Inactive", "editor", "Sales", 45, false, false),
    ("Ivy Editor", "editor", "Engineering", 29, true, true),
    ("Jack User", "user", "Support", 22, true, false),
    ("Kate User Inactive", "user", "HR", 38, false, true),
    ("Leo User", "user", "Engineering", 31, true, true),
    ("Mike Guest", "guest", "Marketing", 24, true, false),
    ("Nancy Guest Inactive", "guest", "Sales", 50, false, false),
];

#[tokio::test]
async fn test_all_fluent_or_scenarios() {
    if !super::test_config::should_run_postgres_tests() {
        println!("{}", super::test_config::SKIPPED);
        return;
    }
    TideConfig::init()
        .database(test_database_url())
        .max_connections(10)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(30))
        .connect()
        .await
        .expect("Failed to connect to database");

    Database::execute("DROP TABLE IF EXISTS or_test_users CASCADE")
        .await
        .expect("Failed to drop table");
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

    for (name, role, department, age, active, verified) in USERS {
        let first_name = name.split(' ').next().unwrap_or(name).to_lowercase();
        OrTestUser::create(OrTestUser {
            id: 0,
            name: name.to_string(),
            email: format!("{first_name}@example.com"),
            role: role.to_string(),
            department: department.to_string(),
            age,
            active,
            verified,
        })
        .await
        .expect("Failed to seed user");
    }

    // Simple OR across one column.
    let results = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .or_where_eq("role", "moderator")
        .or_where_eq("role", "editor")
        .end_or()
        .get()
        .await
        .expect("Query failed");
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

    // Branches carry their own AND conditions; the moderator branch contradicts
    // the outer `active = true` and so contributes nothing.
    let results = OrTestUser::query()
        .where_eq("active", true)
        .begin_or()
        .or_where_eq("role", "admin")
        .and_where_eq("active", true)
        .or_where_eq("role", "moderator")
        .and_where_eq("active", false)
        .or_where_eq("role", "editor")
        .end_or()
        .get()
        .await
        .expect("Query failed");
    assert_eq!(results.len(), 4, "active admins and active editors");
    for user in &results {
        assert!(user.active, "User {} should be active", user.name);
        assert!(
            user.role == "admin" || user.role == "editor",
            "User {} has unexpected role {}",
            user.name,
            user.role
        );
    }

    let results = OrTestUser::query()
        .where_eq("active", true)
        .begin_or()
        .or_where_eq("role", "admin")
        .and_where_eq("verified", true)
        .or_where_eq("role", "moderator")
        .and_where_eq("department", "Engineering")
        .or_where_eq("role", "editor")
        .end_or()
        .get()
        .await
        .expect("Query failed");
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

    let results = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .and_where_lt("age", 30)
        .or_where_eq("role", "moderator")
        .and_where_gt("age", 35)
        .or_where_eq("role", "editor")
        .and_where_eq("verified", true)
        .end_or()
        .get()
        .await
        .expect("Query failed");
    assert_eq!(results.len(), 4, "Carl, Eve, Grace and Ivy");
    for user in &results {
        let matches = (user.role == "admin" && user.age < 30)
            || (user.role == "moderator" && user.age > 35)
            || (user.role == "editor" && user.verified);
        assert!(matches, "User {} doesn't match age criteria", user.name);
    }

    let results = OrTestUser::query()
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
        .end_or()
        .get()
        .await
        .expect("Query failed");
    assert_eq!(results.len(), 4, "Alice, Frank, Grace and Ivy");
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

    let results = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .and_where_in("department", vec!["Engineering", "Marketing"])
        .or_where_in("department", vec!["HR", "Support"])
        .end_or()
        .get()
        .await
        .expect("Query failed");
    assert_eq!(results.len(), 6, "2 admins plus 4 HR/Support users");
    for user in &results {
        let matches = (user.role == "admin"
            && (user.department == "Engineering" || user.department == "Marketing"))
            || (user.department == "HR" || user.department == "Support");
        assert!(matches, "User {} doesn't match IN criteria", user.name);
    }

    let results = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .and_where_between("age", 25, 35)
        .or_where_eq("role", "moderator")
        .and_where_between("age", 30, 45)
        .end_or()
        .get()
        .await
        .expect("Query failed");
    assert_eq!(results.len(), 5, "3 admins aged 25-35 plus Eve and Frank");
    for user in &results {
        let matches = (user.role == "admin" && user.age >= 25 && user.age <= 35)
            || (user.role == "moderator" && user.age >= 30 && user.age <= 45);
        assert!(
            matches,
            "User {} (age: {}) doesn't match BETWEEN criteria",
            user.name, user.age
        );
    }

    let count = OrTestUser::query()
        .where_eq("active", true)
        .begin_or()
        .or_where_eq("role", "admin")
        .or_where_eq("role", "moderator")
        .end_or()
        .count()
        .await
        .expect("Count failed");
    assert_eq!(count, 4, "Alice, Bob, Diana and Frank");

    let first = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .or_where_eq("role", "moderator")
        .end_or()
        .order_by("name", Order::Asc)
        .first()
        .await
        .expect("First failed")
        .expect("Should find at least one user");
    assert_eq!(first.name, "Alice Admin");

    let results = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .end_or()
        .get()
        .await
        .expect("Query failed");
    assert_eq!(results.len(), 3, "Should find 3 admins");
    for user in &results {
        assert_eq!(user.role, "admin", "User should be admin");
    }

    let results = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .or_where_eq("role", "moderator")
        .or_where_eq("role", "editor")
        .end_or()
        .order_by("age", Order::Desc)
        .limit(5)
        .get()
        .await
        .expect("Query failed");
    let ages: Vec<i32> = results.iter().map(|user| user.age).collect();
    assert_eq!(ages, [45, 40, 35, 32, 30]);

    // An empty OR group must not constrain the query.
    let results = OrTestUser::query()
        .where_eq("active", true)
        .begin_or()
        .end_or()
        .get()
        .await
        .expect("Query failed");
    assert_eq!(results.len(), 9, "all active users");

    let sql = OrTestUser::query()
        .where_eq("active", true)
        .begin_or()
        .or_where_eq("role", "admin")
        .and_where_eq("verified", true)
        .or_where_eq("role", "moderator")
        .and_where_gt("age", 30)
        .end_or()
        .build_sql_preview();
    let sql_lower = sql.to_lowercase();
    assert!(
        sql_lower.contains("where"),
        "SQL should contain WHERE: {sql}"
    );
    assert!(sql_lower.contains(" or "), "SQL should contain OR: {sql}");
    assert!(
        sql_lower.contains("active") && sql_lower.contains("role"),
        "SQL should reference both filtered columns: {sql}"
    );

    let results = OrTestUser::query()
        .begin_or()
        .or_where_eq("role", "admin")
        .and_where_like("name", "A%")
        .or_where_like("email", "%example%")
        .end_or()
        .get()
        .await
        .expect("Query failed");
    assert_eq!(results.len(), USERS.len(), "every email matches %example%");

    Database::execute("DROP TABLE IF EXISTS or_test_users CASCADE")
        .await
        .expect("Failed to drop table");
}
