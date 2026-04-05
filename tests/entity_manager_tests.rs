#![cfg(feature = "entity-manager")]

use std::sync::{Arc, OnceLock};

use tideorm::Database;
use tideorm::prelude::*;

#[path = "support/postgres_test_config.rs"]
mod test_config;
use test_config::test_database_url;

#[cfg(feature = "entity-manager")]
use tideorm::entity_manager::{EntityManager, save_with_entity_manager};

static ENTITY_MANAGER_TEST_MUTEX: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();

#[tideorm::model(table = "entity_manager_test_users")]
struct EntityManagerUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,

    #[tideorm(has_many = "EntityManagerPost", foreign_key = "user_id")]
    posts: HasMany<EntityManagerPost>,
}

#[tideorm::model(table = "entity_manager_test_posts")]
struct EntityManagerPost {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    title: String,
}

#[tideorm::model(table = "entity_manager_code_users")]
struct EntityManagerCodeUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    code: String,
    name: String,

    #[tideorm(
        has_many = "EntityManagerCodePost",
        foreign_key = "user_code",
        local_key = "code"
    )]
    posts: HasMany<EntityManagerCodePost>,
}

#[tideorm::model(table = "entity_manager_code_posts")]
struct EntityManagerCodePost {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_code: String,
    title: String,
}

#[tideorm::model(table = "entity_manager_slug_users")]
struct EntityManagerSlugUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,

    #[tideorm(has_many = "EntityManagerSlugPost", foreign_key = "user_id")]
    posts: HasMany<EntityManagerSlugPost>,
}

#[tideorm::model(table = "entity_manager_slug_posts")]
struct EntityManagerSlugPost {
    #[tideorm(primary_key)]
    slug: String,
    user_id: i64,
    title: String,
}

#[tideorm::model(table = "entity_manager_api_keys")]
struct EntityManagerApiKey {
    #[tideorm(primary_key)]
    key: String,
    label: String,
    active: bool,
}

#[tideorm::model(table = "entity_manager_team_memberships")]
struct EntityManagerTeamMembership {
    #[tideorm(primary_key)]
    team_id: i64,
    #[tideorm(primary_key)]
    member_id: i64,
    role: String,
}

#[tideorm::model(table = "entity_manager_composite_users")]
struct EntityManagerCompositeUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,

    #[tideorm(has_many = "EntityManagerCompositePost", foreign_key = "user_id")]
    posts: HasMany<EntityManagerCompositePost>,
}

#[tideorm::model(table = "entity_manager_composite_posts")]
struct EntityManagerCompositePost {
    #[tideorm(primary_key)]
    user_id: i64,
    #[tideorm(primary_key)]
    slug: String,
    title: String,
}

#[tideorm::model(table = "entity_manager_aggregate_users")]
struct EntityManagerAggregateUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,

    #[tideorm(has_one = "EntityManagerAggregateProfile", foreign_key = "user_id")]
    profile: HasOne<EntityManagerAggregateProfile>,

    #[tideorm(has_many = "EntityManagerAggregatePost", foreign_key = "user_id")]
    posts: HasMany<EntityManagerAggregatePost>,
}

#[tideorm::model(table = "entity_manager_aggregate_profiles")]
struct EntityManagerAggregateProfile {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    bio: String,
}

#[tideorm::model(table = "entity_manager_aggregate_posts")]
struct EntityManagerAggregatePost {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    title: String,

    #[tideorm(belongs_to = "EntityManagerAggregateUser", foreign_key = "user_id")]
    author: BelongsTo<EntityManagerAggregateUser>,

    #[tideorm(
        has_many_through = "EntityManagerAggregateTag",
        pivot = "entity_manager_aggregate_post_tags",
        foreign_key = "post_id",
        related_key = "tag_id"
    )]
    tags: HasManyThrough<EntityManagerAggregateTag, EntityManagerAggregatePostTag>,
}

#[tideorm::model(table = "entity_manager_aggregate_tags")]
struct EntityManagerAggregateTag {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "entity_manager_aggregate_post_tags")]
struct EntityManagerAggregatePostTag {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    post_id: i64,
    tag_id: i64,
}

fn test_lock() -> Arc<tokio::sync::Mutex<()>> {
    ENTITY_MANAGER_TEST_MUTEX
        .get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

async fn setup_database() -> tideorm::Result<Arc<Database>> {
    let db = Arc::new(Database::connect(test_database_url()).await?);
    Database::set_global(db.as_ref().clone())?;

    Database::execute("DROP TABLE IF EXISTS entity_manager_test_posts CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS entity_manager_test_users CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS entity_manager_code_posts CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS entity_manager_code_users CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS entity_manager_slug_posts CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS entity_manager_slug_users CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS entity_manager_api_keys CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS entity_manager_team_memberships CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS entity_manager_composite_posts CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS entity_manager_composite_users CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS entity_manager_aggregate_post_tags CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS entity_manager_aggregate_tags CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS entity_manager_aggregate_profiles CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS entity_manager_aggregate_posts CASCADE").await?;
    Database::execute("DROP TABLE IF EXISTS entity_manager_aggregate_users CASCADE").await?;

    Database::execute(
        r#"
        CREATE TABLE entity_manager_test_users (
            id BIGSERIAL PRIMARY KEY,
            name VARCHAR(255) NOT NULL
        )
        "#,
    )
    .await?;

    Database::execute(
        r#"
        CREATE TABLE entity_manager_test_posts (
            id BIGSERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL,
            title VARCHAR(255) NOT NULL
        )
        "#,
    )
    .await?;

    Database::execute(
        r#"
        CREATE TABLE entity_manager_code_users (
            id BIGSERIAL PRIMARY KEY,
            code VARCHAR(255) NOT NULL UNIQUE,
            name VARCHAR(255) NOT NULL
        )
        "#,
    )
    .await?;

    Database::execute(
        r#"
        CREATE TABLE entity_manager_code_posts (
            id BIGSERIAL PRIMARY KEY,
            user_code VARCHAR(255) NOT NULL,
            title VARCHAR(255) NOT NULL
        )
        "#,
    )
    .await?;

    Database::execute(
        r#"
        CREATE TABLE entity_manager_slug_users (
            id BIGSERIAL PRIMARY KEY,
            name VARCHAR(255) NOT NULL
        )
        "#,
    )
    .await?;

    Database::execute(
        r#"
        CREATE TABLE entity_manager_slug_posts (
            slug VARCHAR(255) PRIMARY KEY,
            user_id BIGINT NOT NULL,
            title VARCHAR(255) NOT NULL
        )
        "#,
    )
    .await?;

    Database::execute(
        r#"
        CREATE TABLE entity_manager_api_keys (
            key VARCHAR(255) PRIMARY KEY,
            label VARCHAR(255) NOT NULL,
            active BOOLEAN NOT NULL
        )
        "#,
    )
    .await?;

    Database::execute(
        r#"
        CREATE TABLE entity_manager_team_memberships (
            team_id BIGINT NOT NULL,
            member_id BIGINT NOT NULL,
            role VARCHAR(255) NOT NULL,
            PRIMARY KEY (team_id, member_id)
        )
        "#,
    )
    .await?;

    Database::execute(
        r#"
        CREATE TABLE entity_manager_composite_users (
            id BIGSERIAL PRIMARY KEY,
            name VARCHAR(255) NOT NULL
        )
        "#,
    )
    .await?;

    Database::execute(
        r#"
        CREATE TABLE entity_manager_composite_posts (
            user_id BIGINT NOT NULL,
            slug VARCHAR(255) NOT NULL,
            title VARCHAR(255) NOT NULL,
            PRIMARY KEY (user_id, slug)
        )
        "#,
    )
    .await?;

    Database::execute(
        r#"
        CREATE TABLE entity_manager_aggregate_users (
            id BIGSERIAL PRIMARY KEY,
            name VARCHAR(255) NOT NULL
        )
        "#,
    )
    .await?;

    Database::execute(
        r#"
        CREATE TABLE entity_manager_aggregate_posts (
            id BIGSERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL,
            title VARCHAR(255) NOT NULL
        )
        "#,
    )
    .await?;

    Database::execute(
        r#"
        CREATE TABLE entity_manager_aggregate_profiles (
            id BIGSERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL UNIQUE,
            bio VARCHAR(255) NOT NULL
        )
        "#,
    )
    .await?;

    Database::execute(
        r#"
        CREATE TABLE entity_manager_aggregate_tags (
            id BIGSERIAL PRIMARY KEY,
            name VARCHAR(255) NOT NULL
        )
        "#,
    )
    .await?;

    Database::execute(
        r#"
        CREATE TABLE entity_manager_aggregate_post_tags (
            id BIGSERIAL PRIMARY KEY,
            post_id BIGINT NOT NULL,
            tag_id BIGINT NOT NULL
        )
        "#,
    )
    .await?;

    Ok(db)
}

async fn seed_user_with_posts(
    count: usize,
) -> tideorm::Result<(EntityManagerUser, Vec<EntityManagerPost>)> {
    let user = EntityManagerUser {
        id: 0,
        name: "Alice".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;

    let mut posts = Vec::with_capacity(count);
    for index in 0..count {
        let post = EntityManagerPost {
            id: 0,
            user_id: user.id,
            title: format!("post-{index}"),
        }
        .save()
        .await?;
        posts.push(post);
    }

    Ok((user, posts))
}

#[cfg(feature = "entity-manager")]
#[path = "entity_manager_tests/core_identity_tests.rs"]
mod core_identity_tests;

#[cfg(feature = "entity-manager")]
#[path = "entity_manager_tests/natural_and_composite_key_tests.rs"]
mod natural_and_composite_key_tests;

#[cfg(feature = "entity-manager")]
#[path = "entity_manager_tests/relation_sync_tests.rs"]
mod relation_sync_tests;

#[cfg(feature = "entity-manager")]
#[path = "entity_manager_tests/lifecycle_tests.rs"]
mod lifecycle_tests;
