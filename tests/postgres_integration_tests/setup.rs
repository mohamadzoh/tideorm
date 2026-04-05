use super::*;

pub(super) async fn run() {
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
    let _ = Database::execute("DROP TABLE IF EXISTS test_raw_json_types CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS test_users CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS timestamp_users CASCADE").await;
    let _ = Database::execute("DROP TABLE IF EXISTS callback_users CASCADE").await;

    Database::execute(
        r#"
        CREATE TABLE test_users (
            id BIGSERIAL PRIMARY KEY,
            email VARCHAR(255) NOT NULL,
            name VARCHAR(255) NOT NULL,
            age INTEGER NOT NULL,
            active BOOLEAN NOT NULL DEFAULT true
        )
    "#,
    )
    .await
    .expect("Failed to create test_users table");

    Database::execute(
        r#"
        CREATE TABLE test_posts (
            id BIGSERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL,
            title VARCHAR(255) NOT NULL,
            content TEXT NOT NULL,
            published BOOLEAN NOT NULL DEFAULT false
        )
    "#,
    )
    .await
    .expect("Failed to create test_posts table");

    Database::execute(
        r#"
        CREATE TABLE test_soft_deletes (
            id BIGSERIAL PRIMARY KEY,
            name VARCHAR(255) NOT NULL,
            deleted_at TIMESTAMPTZ
        )
    "#,
    )
    .await
    .expect("Failed to create test_soft_deletes table");

    Database::execute(
        r#"
        CREATE TABLE timestamp_users (
            id BIGSERIAL PRIMARY KEY,
            email VARCHAR(255) NOT NULL UNIQUE,
            name VARCHAR(255) NOT NULL,
            login_count INTEGER NOT NULL DEFAULT 0,
            created_at TIMESTAMPTZ NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL
        )
    "#,
    )
    .await
    .expect("Failed to create timestamp_users table");

    Database::execute(
        r#"
        CREATE TABLE test_raw_json_types (
            id BIGSERIAL PRIMARY KEY,
            enabled BOOLEAN NOT NULL,
            payload JSONB NOT NULL,
            amount NUMERIC(10,2) NOT NULL,
            created_at TIMESTAMPTZ NOT NULL,
            uuid_value UUID NOT NULL
        )
    "#,
    )
    .await
    .expect("Failed to create test_raw_json_types table");

    Database::execute(
        r#"
        CREATE TABLE callback_users (
            id BIGSERIAL PRIMARY KEY,
            email VARCHAR(255) NOT NULL,
            name VARCHAR(255) NOT NULL
        )
    "#,
    )
    .await
    .expect("Failed to create callback_users table");

    println!(" Database setup complete\n");

    // =========================================================================
}
