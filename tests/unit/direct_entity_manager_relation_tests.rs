use super::{BelongsTo, HasMany, HasOne};
use crate::database::{__in_db_scope, Database};
use crate::entity_manager::EntityManager;
use crate::model::Model as _;
use serde_json::json;
use std::sync::Arc;

const USER_TABLE: &str = "direct_entity_manager_relation_test_users";
const PROFILE_TABLE: &str = "direct_entity_manager_relation_test_profiles";
const POST_TABLE: &str = "direct_entity_manager_relation_test_posts";

#[tideorm::model(table = "direct_entity_manager_relation_test_users")]
struct DirectEntityManagerRelationUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    #[tideorm(
        has_one = "DirectEntityManagerRelationProfile",
        foreign_key = "user_id"
    )]
    profile: tideorm::relations::HasOne<DirectEntityManagerRelationProfile>,
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
    #[tideorm(
        belongs_to = "DirectEntityManagerRelationUser",
        foreign_key = "user_id"
    )]
    user: tideorm::relations::BelongsTo<DirectEntityManagerRelationUser>,
}

async fn setup_database() -> crate::error::Result<Option<Arc<Database>>> {
    crate::test_support::fresh_postgres_tables(&[
        (
            USER_TABLE,
            "id BIGSERIAL PRIMARY KEY, name VARCHAR(255) NOT NULL",
        ),
        (
            PROFILE_TABLE,
            "id BIGSERIAL PRIMARY KEY, user_id BIGINT NOT NULL, name VARCHAR(255) NOT NULL",
        ),
        (
            POST_TABLE,
            "id BIGSERIAL PRIMARY KEY, user_id BIGINT NOT NULL, title VARCHAR(255) NOT NULL",
        ),
    ])
    .await
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
            profile: Default::default(),
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
            user: Default::default(),
        }
        .save()
        .await?;

        Ok((user, profile, post))
    })
    .await
}

/// A model an entity manager loads has its relations attached to the
/// manager's database, so they query it with no global database installed.
#[tokio::test]
async fn relations_query_via_parent_entity_manager_database_without_global_db()
-> crate::error::Result<()> {
    Database::reset_global();

    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (user, profile, post) = seed_relations(db.as_ref()).await?;
    let entity_manager = EntityManager::new(db.clone());

    let user = entity_manager
        .find::<DirectEntityManagerRelationUser>(user.id)
        .await?
        .expect("entity-manager user should exist");
    assert!(user.profile.exists().await?);
    let loaded = user
        .profile
        .load()
        .await?
        .expect("has_one relation should query via the parent entity manager database");
    assert_eq!(loaded.id, profile.id);
    assert_eq!(loaded.user_id, user.id);
    assert_eq!(loaded.name, profile.name);

    let post = entity_manager
        .find::<DirectEntityManagerRelationPost>(post.id)
        .await?
        .expect("entity-manager post should exist");
    assert!(post.user.exists().await?);
    let loaded = post
        .user
        .load_with(|query| query)
        .await?
        .expect("belongs_to relation should query via the parent entity manager database");
    assert_eq!(loaded.id, user.id);
    assert_eq!(loaded.name, user.name);

    Ok(())
}

// An eagerly loaded copy can predate edits the manager already tracks for the
// same row; loading the relation into the manager must keep the tracked
// instance rather than overwrite it with that copy.
#[tokio::test]
async fn cached_loads_keep_the_tracked_instance() -> crate::error::Result<()> {
    let entity_manager = EntityManager::new(Arc::new(Database::disconnected()));
    let profile = |name: &str| DirectEntityManagerRelationProfile {
        id: 7,
        user_id: 1,
        name: name.to_string(),
    };
    let user = |name: &str| DirectEntityManagerRelationUser {
        id: 1,
        name: name.to_string(),
        profile: Default::default(),
    };
    entity_manager.register(profile("Edited in memory")).await;
    entity_manager.register(user("Edited in memory")).await;

    let mut has_one = HasOne::<DirectEntityManagerRelationProfile>::new("user_id", "id")
        .with_metadata("profile", USER_TABLE)
        .with_parent_pk(json!(1));
    has_one.set_cached(Some(profile("Stale eager copy")));
    let loaded = entity_manager
        .load(&mut has_one)
        .await?
        .expect("the cached profile should stay loaded");
    assert_eq!(loaded.name, "Edited in memory", "has_one");

    let mut belongs_to =
        BelongsTo::<DirectEntityManagerRelationUser>::new("user_id", "id").with_fk_value(json!(1));
    belongs_to.set_cached(Some(user("Stale eager copy")));
    let loaded = entity_manager
        .load(&mut belongs_to)
        .await?
        .expect("the cached owner should stay loaded");
    assert_eq!(loaded.name, "Edited in memory", "belongs_to");

    let mut has_many = HasMany::<DirectEntityManagerRelationProfile>::new("user_id", "id")
        .with_metadata("profiles", USER_TABLE)
        .with_parent_pk(json!(1));
    has_many.set_cached(vec![profile("Stale eager copy")]);
    let loaded = entity_manager.load(&mut has_many).await?;
    assert_eq!(loaded.len(), 1, "has_many");
    assert_eq!(loaded[0].name, "Edited in memory", "has_many");

    let tracked = entity_manager
        .get::<DirectEntityManagerRelationProfile>(&7)?
        .expect("the identity map should still hold the tracked profile");
    assert_eq!(tracked.name, "Edited in memory");
    let tracked = entity_manager
        .get::<DirectEntityManagerRelationUser>(&1)?
        .expect("the identity map should still hold the tracked owner");
    assert_eq!(tracked.name, "Edited in memory");

    Ok(())
}

/// Without a connection `load` serves the cache, rows edited through
/// `as_mut` included.
#[tokio::test]
async fn rows_edited_through_as_mut_are_the_rows_load_serves_without_a_connection()
-> crate::error::Result<()> {
    Database::reset_global();

    let mut relation = HasMany::<DirectEntityManagerRelationProfile>::new("user_id", "id")
        .with_parent_pk(json!(1));
    relation.set_cached(vec![DirectEntityManagerRelationProfile {
        id: 7,
        user_id: 1,
        name: "Loaded".to_string(),
    }]);
    relation.as_mut().expect("the rows are cached")[0].name = "Edited".to_string();

    let loaded = relation.load().await?;
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "Edited");

    Ok(())
}
