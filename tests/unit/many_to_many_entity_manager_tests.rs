use super::HasManyThrough;
use crate::database::{__in_db_scope, Database};
use crate::entity_manager::EntityManager;
use crate::model::Model as _;
use crate::postgres_test_config::test_database_url;
use serde_json::json;
use std::sync::{Arc, OnceLock};

const POST_TABLE: &str = "many_to_many_entity_manager_test_posts";
const TAG_TABLE: &str = "many_to_many_entity_manager_test_tags";
const PIVOT_TABLE: &str = "many_to_many_entity_manager_test_post_tags";

static MANY_TO_MANY_ENTITY_MANAGER_TEST_MUTEX: OnceLock<Arc<tokio::sync::Mutex<()>>> =
    OnceLock::new();

#[tideorm::model(table = "many_to_many_entity_manager_test_posts")]
struct ManyToManyEntityManagerPost {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    title: String,
    #[tideorm(
        has_many_through = "ManyToManyEntityManagerTag",
        pivot = "many_to_many_entity_manager_test_post_tags",
        foreign_key = "post_id",
        related_key = "tag_id"
    )]
    tags: tideorm::relations::HasManyThrough<
        ManyToManyEntityManagerTag,
        ManyToManyEntityManagerPostTag,
    >,
}

#[tideorm::model(table = "many_to_many_entity_manager_test_tags")]
struct ManyToManyEntityManagerTag {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "many_to_many_entity_manager_test_post_tags")]
struct ManyToManyEntityManagerPostTag {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    post_id: i64,
    tag_id: i64,
}

fn test_lock() -> Arc<tokio::sync::Mutex<()>> {
    MANY_TO_MANY_ENTITY_MANAGER_TEST_MUTEX
        .get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

async fn setup_database() -> crate::error::Result<Arc<Database>> {
    let db = Arc::new(Database::connect(test_database_url()).await?);

    __in_db_scope(db.as_ref(), async {
        Database::execute(&format!("DROP TABLE IF EXISTS {PIVOT_TABLE} CASCADE")).await?;
        Database::execute(&format!("DROP TABLE IF EXISTS {TAG_TABLE} CASCADE")).await?;
        Database::execute(&format!("DROP TABLE IF EXISTS {POST_TABLE} CASCADE")).await?;

        Database::execute(&format!(
            "CREATE TABLE {POST_TABLE} (id BIGSERIAL PRIMARY KEY, title VARCHAR(255) NOT NULL)"
        ))
        .await?;
        Database::execute(&format!(
            "CREATE TABLE {TAG_TABLE} (id BIGSERIAL PRIMARY KEY, name VARCHAR(255) NOT NULL)"
        ))
        .await?;
        Database::execute(&format!(
            "CREATE TABLE {PIVOT_TABLE} (id BIGSERIAL PRIMARY KEY, post_id BIGINT NOT NULL, tag_id BIGINT NOT NULL)"
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
    ManyToManyEntityManagerPost,
    ManyToManyEntityManagerTag,
    ManyToManyEntityManagerPostTag,
)> {
    __in_db_scope(db, async {
        let post = ManyToManyEntityManagerPost {
            id: 0,
            title: "Graph Post".to_string(),
            tags: Default::default(),
        }
        .save()
        .await?;

        let tag = ManyToManyEntityManagerTag {
            id: 0,
            name: "graph-tag".to_string(),
        }
        .save()
        .await?;

        let pivot = ManyToManyEntityManagerPostTag {
            id: 0,
            post_id: post.id,
            tag_id: tag.id,
        }
        .save()
        .await?;

        Ok((post, tag, pivot))
    })
    .await
}

#[tokio::test]
async fn has_many_through_load_queries_with_attached_entity_manager_without_global_db()
-> crate::error::Result<()> {
    let _guard = test_lock().lock_owned().await;
    Database::reset_global();

    let db = setup_database().await?;
    let (post, tag, _pivot) = seed_relations(db.as_ref()).await?;
    let entity_manager = EntityManager::new(db.clone());

    let relation = HasManyThrough::<ManyToManyEntityManagerTag, ManyToManyEntityManagerPostTag> {
        foreign_key: "post_id",
        related_key: "tag_id",
        local_key: "id",
        related_local_key: "id",
        pivot_table: PIVOT_TABLE,
        relation_name: "tags",
        owner_table: POST_TABLE,
        related_table: TAG_TABLE,
        parent_pk: Some(json!(post.id)),
        entity_manager: Some(entity_manager.clone()),
        ..Default::default()
    };

    let loaded = relation.load().await?;

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, tag.id);
    assert_eq!(loaded[0].name, tag.name);

    Ok(())
}

#[tokio::test]
async fn has_many_through_helpers_query_via_parent_entity_manager_database_without_global_db()
-> crate::error::Result<()> {
    let _guard = test_lock().lock_owned().await;
    Database::reset_global();

    let db = setup_database().await?;
    let (post, tag, _pivot) = seed_relations(db.as_ref()).await?;
    let entity_manager = EntityManager::new(db.clone());

    let post = ManyToManyEntityManagerPost::find_in_entity_manager(post.id, &entity_manager)
        .await?
        .expect("entity-manager post should exist");

    assert_eq!(post.tags.count().await?, 1);

    let loaded = post.tags.load().await?;

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, tag.id);
    assert_eq!(loaded[0].name, tag.name);

    Ok(())
}
