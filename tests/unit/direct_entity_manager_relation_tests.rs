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

#[tokio::test]
async fn hasone_load_queries_through_the_attached_database_without_global_db()
-> crate::error::Result<()> {
    Database::reset_global();

    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (user, profile, _) = seed_relations(db.as_ref()).await?;
    let entity_manager = EntityManager::new(db.clone());

    let mut relation = HasOne::<DirectEntityManagerRelationProfile>::new("user_id", "id")
        .with_metadata("profile", USER_TABLE)
        .with_parent_pk(json!(user.id));
    relation.attach_query_database(entity_manager.database());

    let loaded = relation
        .load()
        .await?
        .expect("has_one relation should query via the attached database");

    assert_eq!(loaded.id, profile.id);
    assert_eq!(loaded.user_id, user.id);
    assert_eq!(loaded.name, profile.name);

    Ok(())
}

#[tokio::test]
async fn belongsto_load_queries_through_the_attached_database_without_global_db()
-> crate::error::Result<()> {
    Database::reset_global();

    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (user, _, post) = seed_relations(db.as_ref()).await?;
    let entity_manager = EntityManager::new(db.clone());

    let mut relation = BelongsTo::<DirectEntityManagerRelationUser>::new("user_id", "id")
        .with_fk_value(json!(post.user_id));
    relation.attach_query_database(entity_manager.database());

    let loaded = relation
        .load()
        .await?
        .expect("belongs_to relation should query via the attached database");

    assert_eq!(loaded.id, user.id);
    assert_eq!(loaded.name, user.name);

    Ok(())
}

// An eagerly loaded copy can predate edits the manager already tracks for the
// same row; loading the relation into the manager must keep the tracked
// instance rather than overwrite it with that copy.
#[tokio::test]
async fn hasone_cached_load_keeps_the_tracked_instance() -> crate::error::Result<()> {
    let entity_manager = EntityManager::new(Arc::new(Database::disconnected()));
    entity_manager
        .register(DirectEntityManagerRelationProfile {
            id: 7,
            user_id: 1,
            name: "Edited in memory".to_string(),
        })
        .await;

    let mut relation = HasOne::<DirectEntityManagerRelationProfile>::new("user_id", "id")
        .with_metadata("profile", USER_TABLE)
        .with_owner_key("1".to_string())
        .with_parent_pk(json!(1));
    relation.set_cached(Some(DirectEntityManagerRelationProfile {
        id: 7,
        user_id: 1,
        name: "Stale eager copy".to_string(),
    }));

    let loaded = entity_manager
        .load(&mut relation)
        .await?
        .expect("the cached profile should stay loaded");
    assert_eq!(loaded.name, "Edited in memory");

    let tracked = entity_manager
        .get::<DirectEntityManagerRelationProfile>(&7)?
        .expect("the identity map should still hold the tracked profile");
    assert_eq!(tracked.name, "Edited in memory");

    Ok(())
}

#[tokio::test]
async fn belongsto_cached_load_keeps_the_tracked_instance() -> crate::error::Result<()> {
    let entity_manager = EntityManager::new(Arc::new(Database::disconnected()));
    entity_manager
        .register(DirectEntityManagerRelationUser {
            id: 1,
            name: "Edited in memory".to_string(),
            profile: Default::default(),
        })
        .await;

    let mut relation =
        BelongsTo::<DirectEntityManagerRelationUser>::new("user_id", "id").with_fk_value(json!(1));
    relation.set_cached(Some(DirectEntityManagerRelationUser {
        id: 1,
        name: "Stale eager copy".to_string(),
        profile: Default::default(),
    }));

    let loaded = entity_manager
        .load(&mut relation)
        .await?
        .expect("the cached owner should stay loaded");
    assert_eq!(loaded.name, "Edited in memory");

    let tracked = entity_manager
        .get::<DirectEntityManagerRelationUser>(&1)?
        .expect("the identity map should still hold the tracked owner");
    assert_eq!(tracked.name, "Edited in memory");

    Ok(())
}

#[tokio::test]
async fn hasone_helpers_query_via_parent_entity_manager_database_without_global_db()
-> crate::error::Result<()> {
    Database::reset_global();

    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (user, profile, _) = seed_relations(db.as_ref()).await?;
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

    Ok(())
}

#[tokio::test]
async fn belongsto_helpers_query_via_parent_entity_manager_database_without_global_db()
-> crate::error::Result<()> {
    Database::reset_global();

    let Some(db) = setup_database().await? else {
        return Ok(());
    };
    let (user, _, post) = seed_relations(db.as_ref()).await?;
    let entity_manager = EntityManager::new(db.clone());

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

#[tokio::test]
async fn hasmany_cached_load_keeps_the_tracked_instances() -> crate::error::Result<()> {
    let entity_manager = EntityManager::new(Arc::new(Database::disconnected()));
    entity_manager
        .register(DirectEntityManagerRelationProfile {
            id: 7,
            user_id: 1,
            name: "Edited in memory".to_string(),
        })
        .await;

    let mut relation = HasMany::<DirectEntityManagerRelationProfile>::new("user_id", "id")
        .with_metadata("profiles", USER_TABLE)
        .with_owner_key("1".to_string())
        .with_parent_pk(json!(1));
    relation.set_cached(vec![DirectEntityManagerRelationProfile {
        id: 7,
        user_id: 1,
        name: "Stale eager copy".to_string(),
    }]);

    let loaded = entity_manager.load(&mut relation).await?;
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "Edited in memory");

    let tracked = entity_manager
        .get::<DirectEntityManagerRelationProfile>(&7)?
        .expect("the identity map should still hold the tracked profile");
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
