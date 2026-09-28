use super::HasManyThrough;
use crate::database::{__in_db_scope, Database};
use crate::entity_manager::EntityManager;
use crate::model::Model as _;
use serde_json::json;
use std::sync::Arc;

const POST_TABLE: &str = "many_to_many_entity_manager_test_posts";
const TAG_TABLE: &str = "many_to_many_entity_manager_test_tags";
const PIVOT_TABLE: &str = "many_to_many_entity_manager_test_post_tags";

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

async fn setup_database() -> crate::error::Result<Option<Arc<Database>>> {
    crate::test_support::fresh_postgres_tables(&[
        (
            POST_TABLE,
            "id BIGSERIAL PRIMARY KEY, title VARCHAR(255) NOT NULL",
        ),
        (
            TAG_TABLE,
            "id BIGSERIAL PRIMARY KEY, name VARCHAR(255) NOT NULL",
        ),
        (
            PIVOT_TABLE,
            "id BIGSERIAL PRIMARY KEY, post_id BIGINT NOT NULL, tag_id BIGINT NOT NULL",
        ),
    ])
    .await
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

fn tags_relation(
    post_id: i64,
) -> HasManyThrough<ManyToManyEntityManagerTag, ManyToManyEntityManagerPostTag> {
    HasManyThrough::new("post_id", "tag_id", "id", "id", PIVOT_TABLE)
        .with_metadata("tags", POST_TABLE)
        .with_owner_key(post_id.to_string())
        .with_parent_pk(json!(post_id))
}

#[tokio::test]
async fn has_many_through_load_queries_through_the_attached_database_without_global_db()
-> crate::error::Result<()> {
    Database::reset_global();

    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (post, tag, _pivot) = seed_relations(db.as_ref()).await?;
    let entity_manager = EntityManager::new(db.clone());

    let mut relation = tags_relation(post.id);
    relation.attach_query_database(entity_manager.database());

    let loaded = relation.load().await?;

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, tag.id);
    assert_eq!(loaded[0].name, tag.name);

    Ok(())
}

#[tokio::test]
async fn has_many_through_helpers_query_via_parent_entity_manager_database_without_global_db()
-> crate::error::Result<()> {
    Database::reset_global();

    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (post, tag, _pivot) = seed_relations(db.as_ref()).await?;
    let entity_manager = EntityManager::new(db.clone());

    let post = entity_manager
        .find::<ManyToManyEntityManagerPost>(post.id)
        .await?
        .expect("entity-manager post should exist");

    assert_eq!(post.tags.count().await?, 1);

    let loaded = post.tags.load().await?;

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, tag.id);
    assert_eq!(loaded[0].name, tag.name);

    Ok(())
}

#[tokio::test]
async fn has_many_through_entity_manager_load_collapses_duplicate_pivot_rows()
-> crate::error::Result<()> {
    Database::reset_global();

    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (post, tag, _pivot) = seed_relations(db.as_ref()).await?;
    __in_db_scope(db.as_ref(), async {
        ManyToManyEntityManagerPostTag {
            id: 0,
            post_id: post.id,
            tag_id: tag.id,
        }
        .save()
        .await
    })
    .await?;
    let entity_manager = EntityManager::new(db.clone());

    let mut post = entity_manager
        .find::<ManyToManyEntityManagerPost>(post.id)
        .await?
        .expect("entity-manager post should exist");

    let plain: Vec<i64> = post.tags.load().await?.iter().map(|tag| tag.id).collect();
    let tracked: Vec<i64> = entity_manager
        .load(&mut post.tags)
        .await?
        .iter()
        .map(|tag| tag.id)
        .collect();

    assert_eq!(plain, vec![tag.id]);
    assert_eq!(
        tracked, plain,
        "loading through the entity manager must not repeat a tag per duplicate pivot row"
    );

    Ok(())
}

// An eagerly loaded copy can predate edits the manager already tracks for the
// same row; loading the relation into the manager must keep the tracked
// instance rather than overwrite it with that copy.
#[tokio::test]
async fn has_many_through_cached_load_keeps_the_tracked_instance() -> crate::error::Result<()> {
    let entity_manager = EntityManager::new(Arc::new(Database::disconnected()));
    entity_manager
        .register(ManyToManyEntityManagerTag {
            id: 7,
            name: "edited-in-memory".to_string(),
        })
        .await;

    let mut relation = tags_relation(1);
    relation.set_cached(vec![ManyToManyEntityManagerTag {
        id: 7,
        name: "stale-eager-copy".to_string(),
    }]);

    let loaded = entity_manager.load(&mut relation).await?;
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "edited-in-memory");

    let tracked = entity_manager
        .get::<ManyToManyEntityManagerTag>(&7)?
        .expect("the identity map should still hold the tracked tag");
    assert_eq!(tracked.name, "edited-in-memory");

    Ok(())
}
