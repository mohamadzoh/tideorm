//! PostgreSQL Integration Tests for TideORM
//!
//! These tests require a running PostgreSQL instance with:
//! - Host: localhost
//! - Port: 5432
//! - User: postgres
//! - Password: postgres
//! - Database: test_tide_orm
//!
//! Run with: cargo test --test postgres_integration_tests

use tideorm::prelude::*;
use tideorm::{TideConfig, Database};

mod test_config;
use test_config::test_database_url;

// =============================================================================
// TEST MODELS
// =============================================================================

#[derive(Model, PartialEq)]
#[tide(table = "test_users")]
pub struct TestUser {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub age: i32,
    pub active: bool,
}

#[tideorm::model]
#[tide(table = "test_posts")]
pub struct TestPost {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub content: String,
    pub published: bool,
}

#[tideorm::model]
#[tide(table = "test_soft_deletes", soft_delete)]
pub struct TestSoftDelete {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl SoftDelete for TestSoftDelete {
    fn deleted_at(&self) -> Option<DateTime<Utc>> {
        self.deleted_at
    }
    
    fn set_deleted_at(&mut self, timestamp: Option<DateTime<Utc>>) {
        self.deleted_at = timestamp;
    }
}

// =============================================================================
// SINGLE INTEGRATION TEST - Runs all scenarios sequentially
// =============================================================================

#[tokio::test]
async fn postgres_integration_tests() {
    // =========================================================================
    // SETUP
    // =========================================================================
    println!(" Starting PostgreSQL Integration Tests...\n");
    
    TideConfig::init()
        .database(test_database_url())
        .max_connections(10)
        .min_connections(2)
        .connect()
        .await
        .expect("Failed to connect to database");
    
    // Create tables
    let _ = Database::execute("DROP TABLE IF EXISTS test_soft_deletes CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_posts CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_users CASCADE").await;
    
    Database::execute(r#"
        CREATE TABLE test_users (
            id BIGSERIAL PRIMARY KEY,
            email VARCHAR(255) NOT NULL,
            name VARCHAR(255) NOT NULL,
            age INTEGER NOT NULL,
            active BOOLEAN NOT NULL DEFAULT true
        )
    "#).await.expect("Failed to create test_users table");
    
    Database::execute(r#"
        CREATE TABLE test_posts (
            id BIGSERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL,
            title VARCHAR(255) NOT NULL,
            content TEXT NOT NULL,
            published BOOLEAN NOT NULL DEFAULT false
        )
    "#).await.expect("Failed to create test_posts table");
    
    Database::execute(r#"
        CREATE TABLE test_soft_deletes (
            id BIGSERIAL PRIMARY KEY,
            name VARCHAR(255) NOT NULL,
            deleted_at TIMESTAMPTZ
        )
    "#).await.expect("Failed to create test_soft_deletes table");
    
    println!(" Database setup complete\n");

    // =========================================================================
    // CONNECTION TESTS
    // =========================================================================
    println!("📡 Testing: Database Connection");
    {
        let db = tideorm::db();
        assert!(db.ping().await.is_ok(), "Database ping failed");
        println!("   ✓ Ping successful");
        
        let result = Database::execute("SELECT 1").await;
        assert!(result.is_ok(), "Raw SQL execution failed");
        println!("   ✓ Raw SQL execution works");
    }
    println!();

    // =========================================================================
    // CRUD TESTS
    // =========================================================================
    println!("📝 Testing: CRUD Operations");
    
    // Create and Find
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        let user = TestUser {
            id: 0,
            email: "test@example.com".to_string(),
            name: "Test User".to_string(),
            age: 25,
            active: true,
        };
        
        let saved_user = user.save().await.expect("Failed to save user");
        assert!(saved_user.id > 0, "User should have an auto-generated ID");
        assert_eq!(saved_user.email, "test@example.com");
        
        let found = TestUser::find(saved_user.id).await.expect("Failed to find user");
        assert!(found.is_some(), "User should be found");
        let found_user = found.unwrap();
        assert_eq!(found_user.email, "test@example.com");
        assert_eq!(found_user.name, "Test User");
        println!("   ✓ Create and Find");
    }
    
    // Update
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        let user = TestUser {
            id: 0,
            email: "update@example.com".to_string(),
            name: "Original Name".to_string(),
            age: 30,
            active: true,
        };
        let mut saved_user = user.save().await.expect("Failed to save user");
        
        saved_user.name = "Updated Name".to_string();
        saved_user.age = 31;
        let updated_user = saved_user.update().await.expect("Failed to update user");
        
        assert_eq!(updated_user.name, "Updated Name");
        assert_eq!(updated_user.age, 31);
        
        let reloaded = TestUser::find(updated_user.id).await.expect("Failed to reload").unwrap();
        assert_eq!(reloaded.name, "Updated Name");
        println!("   ✓ Update");
    }
    
    // Delete
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        let user = TestUser {
            id: 0,
            email: "delete@example.com".to_string(),
            name: "To Delete".to_string(),
            age: 25,
            active: true,
        };
        let saved_user = user.save().await.expect("Failed to save user");
        let user_id = saved_user.id;
        
        let deleted_count = saved_user.delete().await.expect("Failed to delete");
        assert_eq!(deleted_count, 1);
        
        let found = TestUser::find(user_id).await.expect("Find failed");
        assert!(found.is_none(), "User should be deleted");
        println!("   ✓ Delete");
    }
    
    // Destroy by ID
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        let user = TestUser {
            id: 0,
            email: "destroy@example.com".to_string(),
            name: "To Destroy".to_string(),
            age: 25,
            active: true,
        };
        let saved_user = user.save().await.expect("Failed to save user");
        
        let deleted = TestUser::destroy(saved_user.id).await.expect("Failed to destroy");
        assert_eq!(deleted, 1);
        
        let found = TestUser::find(saved_user.id).await.expect("Find failed");
        assert!(found.is_none());
        println!("   ✓ Destroy by ID");
    }
    println!();

    // =========================================================================
    // QUERY BUILDER TESTS
    // =========================================================================
    println!("🔍 Testing: Query Builder");
    
    // Where Equal
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        for i in 1..=5 {
            let user = TestUser {
                id: 0,
                email: format!("user{i}@example.com"),
                name: format!("User {i}"),
                age: 20 + i,
                active: i % 2 == 0,
            };
            user.save().await.expect("Failed to save");
        }
        
        let active_users = TestUser::query()
            .where_eq("active", true)
            .get()
            .await
            .expect("Query failed");
        
        assert_eq!(active_users.len(), 2, "Should have 2 active users");
        for user in &active_users {
            assert!(user.active, "All users should be active");
        }
        println!("   ✓ where_eq");
    }
    
    // Where Greater Than / Less Than
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        for i in 1..=5 {
            let user = TestUser {
                id: 0,
                email: format!("age{i}@example.com"),
                name: format!("User {i}"),
                age: 20 + (i * 5), // Ages: 25, 30, 35, 40, 45
                active: true,
            };
            user.save().await.expect("Failed to save");
        }
        
        let older_users = TestUser::query()
            .where_gt("age", 30)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(older_users.len(), 3, "Should have 3 users with age > 30");
        
        let younger_users = TestUser::query()
            .where_lt("age", 35)
            .get()
            .await
            .expect("Query failed");
        assert_eq!(younger_users.len(), 2, "Should have 2 users with age < 35");
        println!("   ✓ where_gt / where_lt");
    }
    
    // Where Like
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        TestUser { id: 0, email: "john@gmail.com".into(), name: "John Doe".into(), age: 25, active: true }.save().await.ok();
        TestUser { id: 0, email: "jane@gmail.com".into(), name: "Jane Doe".into(), age: 30, active: true }.save().await.ok();
        TestUser { id: 0, email: "bob@yahoo.com".into(), name: "Bob Smith".into(), age: 35, active: true }.save().await.ok();
        
        let gmail_users = TestUser::query()
            .where_like("email", "%gmail%")
            .get()
            .await
            .expect("Query failed");
        assert_eq!(gmail_users.len(), 2, "Should have 2 gmail users");
        
        let doe_users = TestUser::query()
            .where_like("name", "%Doe%")
            .get()
            .await
            .expect("Query failed");
        assert_eq!(doe_users.len(), 2, "Should have 2 Doe users");
        println!("   ✓ where_like");
    }
    
    // Where In
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        for i in 1..=5 {
            let user = TestUser {
                id: 0,
                email: format!("in_test{i}@example.com"),
                name: format!("User {i}"),
                age: 20 + i,
                active: true,
            };
            user.save().await.expect("Failed to save");
        }
        
        let users = TestUser::query()
            .where_in("age", vec![21, 23, 25])
            .get()
            .await
            .expect("Query failed");
        assert_eq!(users.len(), 3, "Should have 3 users with ages 21, 23, 25");
        println!("   ✓ where_in");
    }
    
    // Order and Limit
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        for i in 1..=10 {
            let user = TestUser {
                id: 0,
                email: format!("order{i}@example.com"),
                name: format!("User {i:02}"),
                age: 20 + i,
                active: true,
            };
            user.save().await.expect("Failed to save");
        }
        
        let users = TestUser::query()
            .order_by("age", Order::Desc)
            .limit(3)
            .get()
            .await
            .expect("Query failed");
        
        assert_eq!(users.len(), 3, "Should have 3 users");
        assert_eq!(users[0].age, 30, "First user should be oldest");
        assert_eq!(users[1].age, 29);
        assert_eq!(users[2].age, 28);
        println!("   ✓ order_by / limit");
    }
    
    // Pagination
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        for i in 1..=20 {
            let user = TestUser {
                id: 0,
                email: format!("page{i}@example.com"),
                name: format!("User {i:02}"),
                age: 20 + i,
                active: true,
            };
            user.save().await.expect("Failed to save");
        }
        
        let page2 = TestUser::query()
            .order_by("age", Order::Asc)
            .page(2, 5)
            .get()
            .await
            .expect("Query failed");
        
        assert_eq!(page2.len(), 5, "Should have 5 users on page 2");
        assert_eq!(page2[0].age, 26, "First user on page 2 should have age 26");
        println!("   ✓ pagination");
    }
    
    // Count
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        for i in 1..=10 {
            let user = TestUser {
                id: 0,
                email: format!("count{i}@example.com"),
                name: format!("User {i}"),
                age: 20 + i,
                active: i <= 6,
            };
            user.save().await.expect("Failed to save");
        }
        
        let total = TestUser::count().await.expect("Count failed");
        assert_eq!(total, 10, "Should have 10 total users");
        
        let active_count = TestUser::query()
            .where_eq("active", true)
            .count()
            .await
            .expect("Count failed");
        assert_eq!(active_count, 6, "Should have 6 active users");
        println!("   ✓ count");
    }
    
    // First
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        for i in 1..=5 {
            let user = TestUser {
                id: 0,
                email: format!("first{i}@example.com"),
                name: format!("User {i}"),
                age: 20 + i,
                active: true,
            };
            user.save().await.expect("Failed to save");
        }
        
        let first = TestUser::query()
            .where_gt("age", 22)
            .order_by("age", Order::Asc)
            .first()
            .await
            .expect("Query failed");
        
        assert!(first.is_some());
        assert_eq!(first.unwrap().age, 23, "First matching user should have age 23");
        println!("   ✓ first");
    }
    
    // Bulk Delete
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        for i in 1..=10 {
            let user = TestUser {
                id: 0,
                email: format!("bulk{i}@example.com"),
                name: format!("User {i}"),
                age: 20 + i,
                active: i <= 5,
            };
            user.save().await.expect("Failed to save");
        }
        
        let deleted = TestUser::query()
            .where_eq("active", false)
            .delete()
            .await
            .expect("Delete failed");
        assert_eq!(deleted, 5, "Should have deleted 5 inactive users");
        
        let remaining = TestUser::count().await.expect("Count failed");
        assert_eq!(remaining, 5, "Should have 5 remaining users");
        println!("   ✓ bulk delete");
    }
    println!();

    // =========================================================================
    // SOFT DELETE TESTS
    // =========================================================================
    println!("🗑️  Testing: Soft Delete");
    {
        let _ = Database::execute("TRUNCATE TABLE test_soft_deletes RESTART IDENTITY CASCADE").await;
        
        assert!(TestSoftDelete::soft_delete_enabled(), "soft_delete should be enabled");
        
        let record1 = TestSoftDelete { id: 0, name: "Record 1".into(), deleted_at: None }
            .save().await.expect("Failed to save");
        let record2 = TestSoftDelete { id: 0, name: "Record 2".into(), deleted_at: None }
            .save().await.expect("Failed to save");
        let _record3 = TestSoftDelete { id: 0, name: "Record 3".into(), deleted_at: None }
            .save().await.expect("Failed to save");
        
        // Soft delete
        let deleted_record = record1.soft_delete().await.expect("Failed to soft delete");
        assert!(deleted_record.deleted_at.is_some(), "deleted_at should be set");
        println!("   ✓ soft_delete sets deleted_at");
        
        // Query without trashed
        let active = TestSoftDelete::query().get().await.expect("Query failed");
        assert_eq!(active.len(), 2, "Should have 2 active records");
        println!("   ✓ default query excludes soft deleted");
        
        // Query with trashed
        let all = TestSoftDelete::query().with_trashed().get().await.expect("Query failed");
        assert_eq!(all.len(), 3, "Should have 3 total records");
        println!("   ✓ with_trashed includes all");
        
        // Query only trashed
        let trashed = TestSoftDelete::query().only_trashed().get().await.expect("Query failed");
        assert_eq!(trashed.len(), 1, "Should have 1 trashed record");
        assert_eq!(trashed[0].name, "Record 1");
        println!("   ✓ only_trashed works");
        
        // Restore
        let restored = deleted_record.restore().await.expect("Failed to restore");
        assert!(restored.deleted_at.is_none(), "deleted_at should be cleared");
        
        let active_after_restore = TestSoftDelete::query().get().await.expect("Query failed");
        assert_eq!(active_after_restore.len(), 3, "Should have 3 active records after restore");
        println!("   ✓ restore works");
        
        // Force delete
        record2.force_delete().await.expect("Failed to force delete");
        
        let final_count = TestSoftDelete::query().with_trashed().count().await.expect("Count failed");
        assert_eq!(final_count, 2, "Should have 2 records after force delete");
        println!("   ✓ force_delete works");
    }
    println!();

    // =========================================================================
    // TRANSACTION TESTS
    // =========================================================================
    println!("💳 Testing: Transactions");
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        // Transaction commit
        let result = TestUser::transaction(|_tx| async move {
            let user = TestUser {
                id: 0,
                email: "tx_commit@example.com".to_string(),
                name: "Transaction User".to_string(),
                age: 25,
                active: true,
            };
            let saved = user.save().await?;
            Ok(saved.id)
        }).await;
        
        assert!(result.is_ok(), "Transaction should succeed");
        
        let found = TestUser::query()
            .where_eq("email", "tx_commit@example.com")
            .first()
            .await
            .expect("Query failed");
        assert!(found.is_some(), "User should exist after commit");
        println!("   ✓ transaction commit");
        
        // Transaction rollback
        let db = tideorm::db();
        let result: tideorm::Result<i64> = TestUser::transaction(|_tx| async move {
            Err(tideorm::Error::query("Intentional rollback"))
        }).await;
        assert!(result.is_err(), "Transaction should fail");
        
        let result2: tideorm::Result<()> = db.transaction(|tx| async move {
            use sea_orm::ConnectionTrait;
            tx.__internal_transaction()
                .execute_unprepared("INSERT INTO test_users (email, name, age, active) VALUES ('tx_test@example.com', 'TX User', 30, true)")
                .await
                .map_err(|e| tideorm::Error::query(e.to_string()))?;
            Err(tideorm::Error::query("Intentional rollback"))
        }).await;
        assert!(result2.is_err(), "Transaction should fail");
        
        let found = TestUser::query()
            .where_eq("email", "tx_test@example.com")
            .first()
            .await
            .expect("Query failed");
        assert!(found.is_none(), "User should not exist after rollback");
        println!("   ✓ transaction rollback");
    }
    println!();

    // =========================================================================
    // RAW SQL TESTS
    // =========================================================================
    println!("📜 Testing: Raw SQL");
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        for i in 1..=3 {
            let user = TestUser {
                id: 0,
                email: format!("raw{i}@example.com"),
                name: format!("Raw User {i}"),
                age: 20 + i,
                active: true,
            };
            user.save().await.expect("Failed to save");
        }
        
        let users: Vec<TestUser> = Database::raw_with_params::<TestUser>(
            "SELECT * FROM test_users WHERE age > $1 ORDER BY age",
            vec![21.into()]
        ).await.expect("Raw query failed");
        assert_eq!(users.len(), 2, "Should have 2 users with age > 21");
        println!("   ✓ raw_with_params query");
    }
    
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        for i in 1..=5 {
            let user = TestUser {
                id: 0,
                email: format!("exec{i}@example.com"),
                name: format!("Exec User {i}"),
                age: 20 + i,
                active: true,
            };
            user.save().await.expect("Failed to save");
        }
        
        let affected = Database::execute_with_params(
            "UPDATE test_users SET active = false WHERE age > $1",
            vec![23.into()]
        ).await.expect("Execute failed");
        assert_eq!(affected, 2, "Should have updated 2 users");
        
        let inactive = TestUser::query()
            .where_eq("active", false)
            .count()
            .await
            .expect("Count failed");
        assert_eq!(inactive, 2);
        println!("   ✓ execute_with_params");
    }
    println!();

    // =========================================================================
    // BATCH OPERATIONS TESTS
    // =========================================================================
    println!(" Testing: Batch Operations");
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        let users = vec![
            TestUser { id: 0, email: "batch1@example.com".into(), name: "Batch 1".into(), age: 25, active: true },
            TestUser { id: 0, email: "batch2@example.com".into(), name: "Batch 2".into(), age: 30, active: true },
            TestUser { id: 0, email: "batch3@example.com".into(), name: "Batch 3".into(), age: 35, active: false },
        ];
        
        let inserted = TestUser::insert_all(users).await.expect("Insert all failed");
        
        assert_eq!(inserted.len(), 3, "Should have inserted 3 users");
        for user in &inserted {
            assert!(user.id > 0, "Each user should have an ID");
        }
        
        let count = TestUser::count().await.expect("Count failed");
        assert_eq!(count, 3, "Should have 3 users in database");
        println!("   ✓ insert_all");
    }
    println!();

    // =========================================================================
    // UPSERT / ON-CONFLICT TESTS
    // =========================================================================
    println!("♻️  Testing: Upsert / On-Conflict");
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        let user = TestUser {
            id: 1,
            email: "upsert@example.com".into(),
            name: "Initial Upsert".into(),
            age: 28,
            active: true,
        };
        let inserted = TestUser::insert_or_update(user, vec!["id"]).await.expect("insert_or_update should insert when missing");
        assert_eq!(inserted.id, 1, "Insert should respect primary key conflict target");
        assert_eq!(inserted.name, "Initial Upsert");
        
        let user_update = TestUser {
            id: 1,
            email: "upsert@example.com".into(),
            name: "Updated Upsert".into(),
            age: 29,
            active: false,
        };
        let updated = TestUser::insert_or_update(user_update, vec!["id"]).await.expect("insert_or_update should update on conflict");
        assert_eq!(updated.id, 1, "Conflict should keep same primary key");
        assert_eq!(updated.name, "Updated Upsert");
        assert_eq!(updated.age, 29);
        assert!(!updated.active, "Active flag should update when included in update set");
        
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
        assert!(!selective.active, "Active should remain from previous update when column excluded");
        
        let reloaded = TestUser::find(1).await.expect("Reload failed").expect("User should exist after upsert");
        assert_eq!(reloaded.name, "Selective Update");
        assert_eq!(reloaded.age, 31);
        assert!(!reloaded.active, "Active should be preserved when not updated");
        println!("   ✓ insert_or_update and on_conflict");
    }
    println!();

    // =========================================================================
    // BATCH UPDATE TESTS
    // =========================================================================
    println!("🛠️  Testing: Batch Update");
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        for i in 0..5 {
            let user = TestUser {
                id: 0,
                email: format!("batch-update-{i}@example.com"),
                name: format!("Batch Update {i}"),
                age: 24 + (i * 3), // 24, 27, 30, 33, 36
                active: true,
            };
            user.save().await.expect("Failed to seed user for batch update");
        }
        
        let affected = TestUser::update_all()
            .set("active", false)
            .where_gt("age", 30)
            .execute()
            .await
            .expect("Batch update should succeed");
        assert_eq!(affected, 2, "Two users have age > 30");
        
        let inactive = TestUser::query()
            .where_eq("active", false)
            .count()
            .await
            .expect("Count inactive failed");
        assert_eq!(inactive, 2, "Two users should now be inactive");
        
        let active = TestUser::query()
            .where_eq("active", true)
            .count()
            .await
            .expect("Count active failed");
        assert_eq!(active, 3, "Three users should remain active");
        println!("   ✓ batch update builder");
    }
    println!();

    // =========================================================================
    // SCOPES TESTS
    // =========================================================================
    println!("🎯 Testing: Scopes");
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        for i in 1..=10 {
            let user = TestUser {
                id: 0,
                email: format!("scope{i}@example.com"),
                name: format!("Scope User {i}"),
                age: 20 + i,
                active: i <= 5,
            };
            user.save().await.expect("Failed to save");
        }
        
        fn active_scope(q: QueryBuilder<TestUser>) -> QueryBuilder<TestUser> {
            q.where_eq("active", true)
        }
        
        fn adult_scope(q: QueryBuilder<TestUser>) -> QueryBuilder<TestUser> {
            q.where_gte("age", 25)
        }
        
        let users = TestUser::query()
            .scope(active_scope)
            .scope(adult_scope)
            .get()
            .await
            .expect("Query failed");
        
        assert_eq!(users.len(), 1, "Should have 1 user matching both scopes");
        println!("   ✓ scope chaining");
    }
    
    {
        let _ = Database::execute("TRUNCATE TABLE test_users RESTART IDENTITY CASCADE").await;
        
        for i in 1..=5 {
            let user = TestUser {
                id: 0,
                email: format!("cond{i}@example.com"),
                name: format!("Conditional User {i}"),
                age: 20 + i,
                active: i <= 3,
            };
            user.save().await.expect("Failed to save");
        }
        
        let filter_active = true;
        let users = TestUser::query()
            .when(filter_active, |q| q.where_eq("active", true))
            .get()
            .await
            .expect("Query failed");
        assert_eq!(users.len(), 3, "Should have 3 active users when filter is true");
        
        let filter_active = false;
        let users = TestUser::query()
            .when(filter_active, |q| q.where_eq("active", true))
            .get()
            .await
            .expect("Query failed");
        assert_eq!(users.len(), 5, "Should have 5 users when filter is false");
        
        let min_age: Option<i32> = Some(23);
        let users = TestUser::query()
            .when_some(min_age, |q, age| q.where_gte("age", age))
            .get()
            .await
            .expect("Query failed");
        assert_eq!(users.len(), 3, "Should have 3 users with age >= 23");
        println!("   ✓ conditional scopes (when/when_some)");
    }
    println!();

    // =========================================================================
    // CLEANUP
    // =========================================================================
    println!("🧹 Cleaning up...");
    let _ = Database::execute("DROP TABLE IF EXISTS test_soft_deletes CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_posts CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_users CASCADE").await;
    
    println!("\n All PostgreSQL integration tests passed!\n");
}
