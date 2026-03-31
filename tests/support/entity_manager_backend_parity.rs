use std::sync::{Arc, OnceLock};

use tideorm::Database;
use tideorm::entity_manager::{EntityManager, save_with_entity_manager};
use tideorm::prelude::*;
use tideorm::profiling::GlobalProfiler;

use super::backend;

static ENTITY_MANAGER_BACKEND_TEST_MUTEX: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();

#[tideorm::model(table = "entity_manager_backend_users")]
struct BackendEntityManagerUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,

    #[tideorm(has_many = "BackendEntityManagerPost", foreign_key = "user_id")]
    posts: HasMany<BackendEntityManagerPost>,
}

#[tideorm::model(table = "entity_manager_backend_posts")]
struct BackendEntityManagerPost {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    title: String,
}

#[tideorm::model(table = "entity_manager_backend_code_users")]
struct BackendEntityManagerCodeUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    code: String,
    name: String,

    #[tideorm(
        has_many = "BackendEntityManagerCodePost",
        foreign_key = "user_code",
        local_key = "code"
    )]
    posts: HasMany<BackendEntityManagerCodePost>,
}

#[tideorm::model(table = "entity_manager_backend_code_posts")]
struct BackendEntityManagerCodePost {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_code: String,
    title: String,
}

#[tideorm::model(table = "entity_manager_backend_aggregate_users")]
struct BackendEntityManagerAggregateUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,

    #[tideorm(
        has_one = "BackendEntityManagerAggregateProfile",
        foreign_key = "user_id"
    )]
    profile: HasOne<BackendEntityManagerAggregateProfile>,

    #[tideorm(
        has_many = "BackendEntityManagerAggregatePost",
        foreign_key = "user_id"
    )]
    posts: HasMany<BackendEntityManagerAggregatePost>,
}

#[tideorm::model(table = "entity_manager_backend_aggregate_profiles")]
struct BackendEntityManagerAggregateProfile {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    bio: String,
}

#[tideorm::model(table = "entity_manager_backend_aggregate_posts")]
struct BackendEntityManagerAggregatePost {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
    title: String,

    #[tideorm(
        belongs_to = "BackendEntityManagerAggregateUser",
        foreign_key = "user_id"
    )]
    author: BelongsTo<BackendEntityManagerAggregateUser>,

    #[tideorm(
        has_many_through = "BackendEntityManagerAggregateTag",
        pivot = "entity_manager_backend_aggregate_post_tags",
        foreign_key = "post_id",
        related_key = "tag_id"
    )]
    tags: HasManyThrough<BackendEntityManagerAggregateTag, BackendEntityManagerAggregatePostTag>,
}

#[tideorm::model(table = "entity_manager_backend_aggregate_tags")]
struct BackendEntityManagerAggregateTag {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "entity_manager_backend_aggregate_post_tags")]
struct BackendEntityManagerAggregatePostTag {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    post_id: i64,
    tag_id: i64,
}

fn test_lock() -> Arc<tokio::sync::Mutex<()>> {
    ENTITY_MANAGER_BACKEND_TEST_MUTEX
        .get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

async fn setup_database() -> tideorm::Result<Option<Arc<Database>>> {
    backend::setup_database().await
}

async fn seed_user_with_posts(
    count: usize,
) -> tideorm::Result<
    Option<(
        BackendEntityManagerUser,
        Vec<BackendEntityManagerPost>,
        Arc<Database>,
    )>,
> {
    let Some(db) = setup_database().await? else {
        return Ok(None);
    };

    let user = BackendEntityManagerUser {
        id: 0,
        name: "Alice".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;

    let mut posts = Vec::with_capacity(count);
    for index in 0..count {
        let post = BackendEntityManagerPost {
            id: 0,
            user_id: user.id,
            title: format!("post-{index}"),
        }
        .save()
        .await?;
        posts.push(post);
    }

    Ok(Some((user, posts, db)))
}

async fn seed_aggregate_graph() -> tideorm::Result<
    Option<(
        BackendEntityManagerAggregateUser,
        BackendEntityManagerAggregatePost,
        BackendEntityManagerAggregateTag,
        Arc<Database>,
    )>,
> {
    let Some(db) = setup_database().await? else {
        return Ok(None);
    };

    let user = BackendEntityManagerAggregateUser {
        id: 0,
        name: "Graph User".to_string(),
        profile: Default::default(),
        posts: Default::default(),
    }
    .save()
    .await?;
    let post = BackendEntityManagerAggregatePost {
        id: 0,
        user_id: user.id,
        title: "Graph Post".to_string(),
        author: Default::default(),
        tags: Default::default(),
    }
    .save()
    .await?;
    let tag = BackendEntityManagerAggregateTag {
        id: 0,
        name: "old-tag".to_string(),
    }
    .save()
    .await?;
    BackendEntityManagerAggregatePostTag {
        id: 0,
        post_id: post.id,
        tag_id: tag.id,
    }
    .save()
    .await?;

    Ok(Some((user, post, tag, db)))
}

#[tokio::test]
async fn identity_map_no_duplicate_queries() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let Some((saved_user, _, db)) = seed_user_with_posts(0).await? else {
        return Ok(());
    };

    GlobalProfiler::enable();
    GlobalProfiler::reset();
    GlobalProfiler::set_slow_threshold(0);

    let entity_manager = EntityManager::new(db);
    let first = BackendEntityManagerUser::find_in_entity_manager(saved_user.id, &entity_manager)
        .await?
        .expect("first lookup should return a user");
    let second = BackendEntityManagerUser::find_in_entity_manager(saved_user.id, &entity_manager)
        .await?
        .expect("second lookup should return a user");

    let stats = GlobalProfiler::stats();
    assert_eq!(stats.total_queries, 1);
    assert_eq!(first.id, second.id);
    assert_eq!(first.name, second.name);

    GlobalProfiler::disable();
    GlobalProfiler::reset();
    Ok(())
}

#[tokio::test]
async fn tracked_deletion_emits_delete() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let Some((saved_user, posts, db)) = seed_user_with_posts(3).await? else {
        return Ok(());
    };

    let entity_manager = EntityManager::new(db.clone());
    let mut user = BackendEntityManagerUser::find_in_entity_manager(saved_user.id, &entity_manager)
        .await?
        .expect("entity-manager user should exist");
    tideorm::entity_manager::TrackedHasManyEntityManagerExt::load(&mut user.posts, &entity_manager)
        .await?;

    let removed_id = posts[1].id;
    user.posts
        .as_mut()
        .expect("loaded posts should be mutable")
        .retain(|post| post.id != removed_id);

    save_with_entity_manager(&user, &entity_manager).await?;

    let remaining = BackendEntityManagerPost::query_with(db.as_ref())
        .where_eq("user_id", user.id)
        .count()
        .await?;
    assert_eq!(remaining, 2);
    assert!(
        BackendEntityManagerPost::find_with(removed_id, db.as_ref())
            .await?
            .is_none()
    );

    Ok(())
}

#[tokio::test]
async fn string_local_key_is_used_for_new_children() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let created = BackendEntityManagerCodeUser {
        id: 0,
        code: "user-code-1".to_string(),
        name: "Code User".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let mut user =
        BackendEntityManagerCodeUser::find_in_entity_manager(created.id, &entity_manager)
            .await?
            .expect("code user should exist");
    tideorm::entity_manager::TrackedHasManyEntityManagerExt::load(&mut user.posts, &entity_manager)
        .await?;

    user.posts
        .as_mut()
        .expect("loaded posts should be mutable")
        .push(BackendEntityManagerCodePost {
            id: 0,
            user_code: String::new(),
            title: "uses-code".to_string(),
        });

    let user = save_with_entity_manager(&user, &entity_manager).await?;
    let cached_posts = user.posts.get_cached().expect("posts should stay loaded");
    assert_eq!(cached_posts.len(), 1);
    assert_eq!(cached_posts[0].user_code, "user-code-1");

    let saved_posts = BackendEntityManagerCodePost::query_with(db.as_ref())
        .where_eq("user_code", "user-code-1")
        .order_by("id", Order::Asc)
        .get()
        .await?;

    assert_eq!(saved_posts.len(), 1);
    assert_eq!(saved_posts[0].title, "uses-code");

    Ok(())
}

#[tokio::test]
async fn hasone_insert_update_delete_is_synced() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let Some((created, _post, _tag, db)) = seed_aggregate_graph().await? else {
        return Ok(());
    };

    let entity_manager = EntityManager::new(db.clone());
    let mut user =
        BackendEntityManagerAggregateUser::find_in_entity_manager(created.id, &entity_manager)
            .await?
            .expect("aggregate user should exist");
    user.profile.load_in_entity_manager(&entity_manager).await?;

    user.profile
        .set_cached(Some(BackendEntityManagerAggregateProfile {
            id: 0,
            user_id: 0,
            bio: "Bio One".to_string(),
        }));

    let mut user = save_with_entity_manager(&user, &entity_manager).await?;
    let profile = user
        .profile
        .get_cached()
        .expect("profile should be inserted");
    assert!(profile.id > 0);
    assert_eq!(profile.user_id, user.id);
    assert_eq!(profile.bio, "Bio One");

    user.profile
        .as_mut()
        .expect("profile should stay loaded")
        .bio = "Bio Two".to_string();

    let mut user = save_with_entity_manager(&user, &entity_manager).await?;
    assert_eq!(
        user.profile
            .get_cached()
            .expect("profile should remain loaded")
            .bio,
        "Bio Two"
    );

    user.profile.clear();
    let user = save_with_entity_manager(&user, &entity_manager).await?;
    assert!(user.profile.get_cached().is_none());
    assert!(
        BackendEntityManagerAggregateProfile::query_with(db.as_ref())
            .where_eq("user_id", user.id)
            .first()
            .await?
            .is_none()
    );

    Ok(())
}

#[tokio::test]
async fn belongs_to_load_in_entity_manager_reuses_cached_parent() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let Some((user, first_post, _tag, db)) = seed_aggregate_graph().await? else {
        return Ok(());
    };

    let second_post = BackendEntityManagerAggregatePost {
        id: 0,
        user_id: user.id,
        title: "Post B".to_string(),
        author: Default::default(),
        tags: Default::default(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db);
    let _cached_user =
        BackendEntityManagerAggregateUser::find_in_entity_manager(user.id, &entity_manager)
            .await?
            .expect("cached parent should exist");
    let mut first =
        BackendEntityManagerAggregatePost::find_in_entity_manager(first_post.id, &entity_manager)
            .await?
            .expect("first post should exist");
    let mut second =
        BackendEntityManagerAggregatePost::find_in_entity_manager(second_post.id, &entity_manager)
            .await?
            .expect("second post should exist");

    GlobalProfiler::enable();
    GlobalProfiler::reset();
    GlobalProfiler::set_slow_threshold(0);

    let first_author = first
        .author
        .load_in_entity_manager(&entity_manager)
        .await?
        .expect("first author should load from entity-manager");
    let second_author = second
        .author
        .load_in_entity_manager(&entity_manager)
        .await?
        .expect("second author should load from entity-manager");

    let stats = GlobalProfiler::stats();
    assert_eq!(stats.total_queries, 0);
    assert_eq!(first_author.id, user.id);
    assert_eq!(second_author.id, user.id);

    GlobalProfiler::disable();
    GlobalProfiler::reset();
    Ok(())
}

#[tokio::test]
async fn nested_has_many_through_changes_are_synced_from_root_save() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let Some((user, post, old_tag, db)) = seed_aggregate_graph().await? else {
        return Ok(());
    };

    let entity_manager = EntityManager::new(db.clone());
    let mut user =
        BackendEntityManagerAggregateUser::find_in_entity_manager(user.id, &entity_manager)
            .await?
            .expect("graph user should exist");
    user.posts.load_in_entity_manager(&entity_manager).await?;

    let posts = user.posts.as_mut().expect("posts should be loaded");
    let target_post = posts
        .iter_mut()
        .find(|candidate| candidate.id == post.id)
        .expect("seeded post should be present");
    target_post
        .tags
        .load_in_entity_manager(&entity_manager)
        .await?;

    let tags = target_post.tags.as_mut().expect("tags should be loaded");
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "old-tag");
    tags.clear();
    tags.push(BackendEntityManagerAggregateTag {
        id: 0,
        name: "new-tag".to_string(),
    });

    let user = save_with_entity_manager(&user, &entity_manager).await?;
    let saved_post = user
        .posts
        .get_cached()
        .expect("posts should remain loaded")
        .iter()
        .find(|candidate| candidate.id == post.id)
        .expect("saved post should remain loaded");
    let saved_tags = saved_post
        .tags
        .get_cached()
        .expect("tags should remain loaded");
    assert_eq!(saved_tags.len(), 1);
    assert_eq!(saved_tags[0].name, "new-tag");
    assert!(saved_tags[0].id > 0);

    let pivots = BackendEntityManagerAggregatePostTag::query_with(db.as_ref())
        .where_eq("post_id", saved_post.id)
        .order_by("id", Order::Asc)
        .get()
        .await?;
    assert_eq!(pivots.len(), 1);
    assert_eq!(pivots[0].tag_id, saved_tags[0].id);
    assert_eq!(
        BackendEntityManagerAggregatePostTag::query_with(db.as_ref())
            .where_eq("post_id", saved_post.id)
            .where_eq("tag_id", old_tag.id)
            .count()
            .await?,
        0
    );

    Ok(())
}

#[tokio::test]
async fn entity_manager_clear_drops_cached_identity_and_managed_state() -> tideorm::Result<()> {
    let _guard = test_lock().lock_owned().await;
    let Some(db) = setup_database().await? else {
        return Ok(());
    };

    let saved = BackendEntityManagerUser {
        id: 0,
        name: "Before Clear".to_string(),
        posts: Default::default(),
    }
    .save()
    .await?;

    let entity_manager = EntityManager::new(db.clone());
    let initial = entity_manager
        .find::<BackendEntityManagerUser>(saved.id)
        .await?
        .expect("user should load into the entity manager");
    assert_eq!(initial.name, "Before Clear");

    BackendEntityManagerUser {
        id: saved.id,
        name: "After Clear".to_string(),
        posts: Default::default(),
    }
    .update()
    .await?;

    let stale = entity_manager
        .find::<BackendEntityManagerUser>(saved.id)
        .await?
        .expect("identity map should still return the cached entity before clear");
    assert_eq!(stale.name, "Before Clear");

    let managed = entity_manager
        .find_managed::<BackendEntityManagerUser>(saved.id)
        .await?
        .expect("managed entity should load before clear");
    managed.edit(|user| user.name = "Detached By Clear".to_string());

    entity_manager.clear();
    entity_manager.flush().await?;

    let fresh = entity_manager
        .find::<BackendEntityManagerUser>(saved.id)
        .await?
        .expect("cleared entity manager should reload from the database");
    assert_eq!(fresh.name, "After Clear");

    let persisted = BackendEntityManagerUser::find_with(saved.id, db.as_ref())
        .await?
        .expect("user should still exist after clear");
    assert_eq!(persisted.name, "After Clear");

    Ok(())
}
