use crate::Order;
use crate::database::{__in_db_scope, Database};
use crate::entity_manager::EntityManager;
use crate::model::Model as _;
use crate::postgres_test_config::test_database_url;
use std::sync::{Arc, OnceLock};

const USER_TABLE: &str = "tracked_entity_manager_relation_test_users";
const POST_TABLE: &str = "tracked_entity_manager_relation_test_posts";

static TRACKED_ENTITY_MANAGER_RELATION_TEST_MUTEX: OnceLock<Arc<tokio::sync::Mutex<()>>> =
    OnceLock::new();

#[tideorm::model(table = "tracked_entity_manager_relation_test_users")]
struct TrackedEntityManagerRelationUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    #[tideorm(has_many = "TrackedEntityManagerRelationPost", foreign_key = "user_id")]
    posts: tideorm::relations::HasMany<TrackedEntityManagerRelationPost>,
}

#[tideorm::model(table = "tracked_entity_manager_relation_test_posts")]
struct TrackedEntityManagerRelationPost {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    title: String,
}

fn test_lock() -> Arc<tokio::sync::Mutex<()>> {
    TRACKED_ENTITY_MANAGER_RELATION_TEST_MUTEX
        .get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

async fn setup_database() -> crate::error::Result<Arc<Database>> {
    let db = Arc::new(Database::connect(test_database_url()).await?);

    __in_db_scope(db.as_ref(), async {
        Database::execute(&format!("DROP TABLE IF EXISTS {POST_TABLE} CASCADE")).await?;
        Database::execute(&format!("DROP TABLE IF EXISTS {USER_TABLE} CASCADE")).await?;

        Database::execute(&format!(
            "CREATE TABLE {USER_TABLE} (id BIGSERIAL PRIMARY KEY, name VARCHAR(255) NOT NULL)"
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
    TrackedEntityManagerRelationUser,
    Vec<TrackedEntityManagerRelationPost>,
)> {
    __in_db_scope(db, async {
        let user = TrackedEntityManagerRelationUser {
            id: 0,
            name: "Alice".to_string(),
            posts: Default::default(),
        }
        .save()
        .await?;

        let first = TrackedEntityManagerRelationPost {
            id: 0,
            user_id: user.id,
            title: "First".to_string(),
        }
        .save()
        .await?;
        let second = TrackedEntityManagerRelationPost {
            id: 0,
            user_id: user.id,
            title: "Second".to_string(),
        }
        .save()
        .await?;

        Ok((user, vec![first, second]))
    })
    .await
}

#[tokio::test]
async fn tracked_has_many_read_helpers_query_via_parent_entity_manager_database()
-> crate::error::Result<()> {
    let _guard = test_lock().lock_owned().await;
    Database::reset_global();

    let db = setup_database().await?;
    let (saved_user, saved_posts) = seed_relations(db.as_ref()).await?;
    let entity_manager = EntityManager::new(db.clone());

    let user =
        TrackedEntityManagerRelationUser::find_in_entity_manager(saved_user.id, &entity_manager)
            .await?
            .expect("tracked relation user should exist");

    assert_eq!(user.posts.count().await?, 2);
    assert!(user.posts.exists().await?);

    let loaded = user
        .posts
        .load_with(|query| query.order_by("id", Order::Asc))
        .await?;

    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].id, saved_posts[0].id);
    assert_eq!(loaded[1].id, saved_posts[1].id);

    Ok(())
}

#[tokio::test]
async fn cached_relation_load_keeps_the_registered_identity_map_instance()
-> crate::error::Result<()> {
    let entity_manager = EntityManager::new(Arc::new(Database::disconnected()));

    entity_manager
        .register(TrackedEntityManagerRelationPost {
            id: 7,
            user_id: 1,
            title: "Canonical".to_string(),
        })
        .await;

    let mut relation =
        super::TrackedHasMany::<TrackedEntityManagerRelationPost>::new("user_id", "id")
            .with_metadata("posts", USER_TABLE, POST_TABLE)
            .with_owner_key("1".to_string());
    relation.set_cached(vec![TrackedEntityManagerRelationPost {
        id: 7,
        user_id: 1,
        title: "Stale eager copy".to_string(),
    }]);

    let loaded = relation.load_in_entity_manager(&entity_manager).await?;

    // The eagerly cached copy must not replace the already tracked instance.
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].title, "Canonical");

    let mapped = entity_manager
        .get::<TrackedEntityManagerRelationPost>(&7)?
        .expect("identity map should still hold the registered instance");
    assert_eq!(mapped.title, "Canonical");

    Ok(())
}
