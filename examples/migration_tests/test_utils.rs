//! Migration Test Utilities
//!
//! Helper functions for migration testing.

use tideorm::database::db;
use tideorm::Database;

/// Check if a table exists in the database
pub async fn table_exists(table_name: &str) -> bool {
    use tideorm::internal::{ConnectionTrait, Statement};
    
    let sql = format!(
        r#"
        SELECT EXISTS (
            SELECT FROM information_schema.tables 
            WHERE table_schema = 'public' 
            AND table_name = '{}'
        )
        "#,
        table_name
    );
    
    let database = db();
    let backend = database.__internal_connection().get_database_backend();
    let stmt = Statement::from_string(backend, sql);
    
    match database.__internal_connection().query_one(stmt).await {
        Ok(Some(row)) => {
            row.try_get::<bool>("", "exists").unwrap_or(false)
        }
        _ => false,
    }
}

/// Check if a column exists in a table
pub async fn column_exists(table_name: &str, column_name: &str) -> bool {
    use tideorm::internal::{ConnectionTrait, Statement};
    
    let sql = format!(
        r#"
        SELECT EXISTS (
            SELECT FROM information_schema.columns 
            WHERE table_schema = 'public' 
            AND table_name = '{}' 
            AND column_name = '{}'
        )
        "#,
        table_name, column_name
    );
    
    let database = db();
    let backend = database.__internal_connection().get_database_backend();
    let stmt = Statement::from_string(backend, sql);
    
    match database.__internal_connection().query_one(stmt).await {
        Ok(Some(row)) => {
            row.try_get::<bool>("", "exists").unwrap_or(false)
        }
        _ => false,
    }
}

/// Check if an index exists
#[allow(dead_code)]
pub async fn index_exists(index_name: &str) -> bool {
    use tideorm::internal::{ConnectionTrait, Statement};
    
    let sql = format!(
        r#"
        SELECT EXISTS (
            SELECT FROM pg_indexes 
            WHERE schemaname = 'public' 
            AND indexname = '{}'
        )
        "#,
        index_name
    );
    
    let database = db();
    let backend = database.__internal_connection().get_database_backend();
    let stmt = Statement::from_string(backend, sql);
    
    match database.__internal_connection().query_one(stmt).await {
        Ok(Some(row)) => {
            row.try_get::<bool>("", "exists").unwrap_or(false)
        }
        _ => false,
    }
}

/// Get count of records in migrations table
pub async fn get_migration_count() -> i64 {
    use tideorm::internal::{ConnectionTrait, Statement};
    
    let sql = r#"SELECT COUNT(*) as cnt FROM "_migrations""#;
    
    let database = db();
    let backend = database.__internal_connection().get_database_backend();
    let stmt = Statement::from_string(backend, sql.to_string());
    
    match database.__internal_connection().query_one(stmt).await {
        Ok(Some(row)) => {
            row.try_get::<i64>("", "cnt").unwrap_or(0)
        }
        _ => 0,
    }
}

/// Get all applied migration versions
pub async fn get_applied_versions() -> Vec<String> {
    use tideorm::internal::{ConnectionTrait, Statement};
    
    let sql = r#"SELECT "version" FROM "_migrations" ORDER BY "version" ASC"#;
    
    let database = db();
    let backend = database.__internal_connection().get_database_backend();
    let stmt = Statement::from_string(backend, sql.to_string());
    
    match database.__internal_connection().query_all(stmt).await {
        Ok(rows) => {
            rows.iter()
                .filter_map(|row| row.try_get::<String>("", "version").ok())
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Clean up all test tables
pub async fn cleanup_test_tables() -> tideorm::Result<()> {
    let tables = [
        "test_inventory",
        "test_order_items",
        "test_orders",
        "test_products",
    ];
    
    for table in tables {
        let sql = format!(r#"DROP TABLE IF EXISTS "{}" CASCADE"#, table);
        let _ = Database::execute(&sql).await;
    }
    
    // Also clean up migrations table entries for test migrations
    let _ = Database::execute(
        r#"DELETE FROM "_migrations" WHERE "version" LIKE '20260106_%'"#
    ).await;
    
    Ok(())
}

/// Print test result with formatting
pub fn print_test_result(name: &str, passed: bool) {
    if passed {
        println!("  ✅ {}", name);
    } else {
        println!("  ❌ {}", name);
    }
}

/// Print section header
pub fn print_section(title: &str) {
    println!("\n{}", "=".repeat(60));
    println!("📋 {}", title);
    println!("{}", "=".repeat(60));
}
