use super::{BelongsTo, HasOne};
use crate::database::{__in_db_scope, Database};
use crate::entity_manager::EntityManager;
use crate::model::Model as _;
use serde_json::json;
use std::marker::PhantomData;
use std::sync::{Arc, OnceLock};

#[path = "../support/postgres_test_config.rs"]
mod test_config;
use test_config::test_database_url;

const USER_TABLE: &str = "direct_entity_manager_relation_test_users";
const PROFILE_TABLE: &str = "direct_entity_manager_relation_test_profiles";
const POST_TABLE: &str = "direct_entity_manager_relation_test_posts";

static DIRECT_ENTITY_MANAGER_RELATION_TEST_MUTEX: OnceLock<Arc<tokio::sync::Mutex<()>>> =
    OnceLock::new();

#[tideorm::model(table = "direct_entity_manager_relation_test_users")]
struct DirectEntityManagerRelationUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "direct_entity_manager_relation_test_profiles")]
struct DirectEntityManagerRelationProfile {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    name: String,
}

#[tideorm::model(table = "direct_entity_manager_relation_test_posts")]
struct DirectEntityManagerRelationPost {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    title: String,
}

fn test_lock() -> Arc<tokio::sync::Mutex<()>> {
    DIRECT_ENTITY_MANAGER_RELATION_TEST_MUTEX
        .get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

async fn setup_database() -> crate::error::Result<Arc<Database>> {
    let db = Arc::new(Database::connect(test_database_url()).await?);

    __in_db_scope(db.as_ref(), async {
        Database::execute(&format!("DROP TABLE IF EXISTS {POST_TABLE} CASCADE")).await?;
        Database::execute(&format!("DROP TABLE IF EXISTS {PROFILE_TABLE} CASCADE")).await?;
        Database::execute(&format!("DROP TABLE IF EXISTS {USER_TABLE} CASCADE")).await?;

        Database::execute(&format!(
            "CREATE TABLE {USER_TABLE} (id BIGSERIAL PRIMARY KEY, name VARCHAR(255) NOT NULL)"
        ))
        .await?;
        Database::execute(&format!(
            "CREATE TABLE {PROFILE_TABLE} (id BIGSERIAL PRIMARY KEY, user_id BIGINT NOT NULL, name VARCHAR(255) NOT NULL)"
        ))
        .await?;
        Database::execute(&format!(
            "CREATE TABLE {POST_TABLE} (id BIGSERIAL PRIMARY KEY, user_id BIGINT NOT NULL, title VARCHAR(255) NOT NULL)"
        ))
        .await?;

        Ok(())
    })
    .await?;

    Ok(db)
}

async fn seed_relations(
    db: &Database,
) -> crate::error::Result<(
    DirectEntityManagerRelationUser,
    DirectEntityManagerRelationProfile,
    DirectEntityManagerRelationPost,
)> {
    __in_db_scope(db, async {
        let user = DirectEntityManagerRelationUser {
            id: 0,
            name: "Alice".to_string(),
        }
        .save()
        .await?;

        let profile = DirectEntityManagerRelationProfile {
            id: 0,
            user_id: user.id,
            name: "Alice Profile".to_string(),
        }
        .save()
        .await?;

        let post = DirectEntityManagerRelationPost {
            id: 0,
            user_id: user.id,
            title: "Alice Post".to_string(),
        }
        .save()
        .await?;

        Ok((user, profile, post))
    })
    .await
}

#[tokio::test]
async fn hasone_load_queries_with_attached_entity_manager_without_global_db(
) -> crate::error::Result<()> {
    let _guard = test_lock().lock_owned().await;
    Database::reset_global();

    let db = setup_database().await?;
    let (user, profile, _) = seed_relations(db.as_ref()).await?;
    let entity_manager = EntityManager::new(db.clone());

    let relation = HasOne::<DirectEntityManagerRelationProfile> {
        foreign_key: "user_id",
        local_key: "id",
        relation_name: "profile",
        owner_table: USER_TABLE,
        child_table: PROFILE_TABLE,
        cached: None,
        loaded: false,
        parent_pk: Some(json!(user.id)),
        owner_key: None,
        entity_manager: Some(entity_manager.clone()),
        _marker: PhantomData,
    };

    let loaded = relation
        .load()
        .await?
        .expect("has_one relation should query via the attached entity manager");

    assert_eq!(loaded.id, profile.id);
    assert_eq!(loaded.user_id, user.id);
    assert_eq!(loaded.name, profile.name);

    Ok(())
}

#[tokio::test]
async fn belongsto_load_queries_with_attached_entity_manager_without_global_db(
) -> crate::error::Result<()> {
    let _guard = test_lock().lock_owned().await;
    Database::reset_global();

    let db = setup_database().await?;
    let (user, _, post) = seed_relations(db.as_ref()).await?;
    let entity_manager = EntityManager::new(db.clone());

    let relation = BelongsTo::<DirectEntityManagerRelationUser> {
        foreign_key: "user_id",
        owner_key: "id",
        cached: None,
        loaded: false,
        fk_value: Some(json!(post.user_id)),
        entity_manager: Some(entity_manager.clone()),
        _marker: PhantomData,
    };

    let loaded = relation
        .load()
        .await?
        .expect("belongs_to relation should query via the attached entity manager");

    assert_eq!(loaded.id, user.id);
    assert_eq!(loaded.name, user.name);

    Ok(())
}